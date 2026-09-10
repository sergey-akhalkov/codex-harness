//! In-memory CodeGraph response shaping and private detail paging.
//!
//! One [`Responses`] instance belongs to one MCP client. [`Responses::shape`]
//! bounds an upstream envelope before model delivery; [`Responses::detail`]
//! pages retained bytes without repeating a backend query. Use
//! [`validate_budget`] / [`validate_budget_len`] for `max_response_bytes`.
//! [`Responses::refuse`] retains the original once and returns a bounded
//! refusal even when the payload would fit the caller's budget.
use serde_json::{Map, Value, json};
use std::{
    collections::VecDeque,
    io,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_BUDGET: usize = 4096;
pub const MIN_BUDGET: usize = 1024;
pub const MAX_BUDGET: usize = 16384;
const MAX_CAPTURES: usize = 32;
const MAX_CAPTURE_BYTES: usize = 256 * 1024;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const TTL: Duration = Duration::from_secs(30 * 60);
const LIST_KEYS: &[&str] = &[
    "results",
    "matches",
    "nodes",
    "symbols",
    "callers",
    "callees",
    "files",
    "items",
    "edges",
    "records",
    "hits",
    "definitions",
    "references",
];
const SOURCE_KEYS: &[&str] = &["code", "body", "source", "snippet", "fileContent"];
const IDENTITY_KEYS: &[&str] = &[
    "root",
    "generation",
    "provider",
    "version",
    "freshness",
    "coverage",
    "worker_lease",
];
const KEEP_KEYS: &[&str] = &[
    "root",
    "generation",
    "provider",
    "version",
    "freshness",
    "coverage",
    "worker_lease",
    "error",
    "warnings",
    "truncated",
    "source_partial",
    "list_partial",
    "omitted_records",
    "detail_id",
    "next_offset",
    "hint",
    "duplicate_payload_omitted",
    "capture_truncated",
    "original_bytes",
    "retained_bytes",
    "id",
    "offset",
    "page_partial",
    "rerun",
    "diagnostics",
];
const HINT: &str = "codegraph_detail(id, offset=next_offset) reads retained bytes without rerunning; prefer an exact file/symbol.";

fn invalid_budget() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "max_response_bytes must be an integer in 1024..=16384",
    )
}

/// Accepts a JSON integer or an arguments object with `max_response_bytes`.
pub fn validate_budget(value: &Value) -> io::Result<usize> {
    let number = match value {
        Value::Number(_) => value,
        Value::Object(map) => map.get("max_response_bytes").ok_or_else(invalid_budget)?,
        _ => return Err(invalid_budget()),
    };
    let Some(bits) = number.as_u64() else {
        return Err(invalid_budget());
    };
    validate_budget_len(usize::try_from(bits).map_err(|_| invalid_budget())?)
}

pub fn validate_budget_len(max_bytes: usize) -> io::Result<usize> {
    (MIN_BUDGET..=MAX_BUDGET)
        .contains(&max_bytes)
        .then_some(max_bytes)
        .ok_or_else(invalid_budget)
}

fn encoded(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|_| {
        br#"{"content":[{"type":"text","text":"encode-failed"}],"isError":true}"#.to_vec()
    })
}

fn encoded_len(value: &Value) -> usize {
    encoded(value).len()
}

fn boundary(bytes: &[u8], offset: usize) -> bool {
    offset == 0 || offset == bytes.len() || bytes.get(offset).is_some_and(|b| b & 0xC0 != 0x80)
}

fn utf8_end(bytes: &[u8], mut end: usize) -> usize {
    end = end.min(bytes.len());
    while end > 0 && !boundary(bytes, end) {
        end -= 1;
    }
    end
}

fn mcp(is_error: bool, text: impl Into<String>, structured: Map<String, Value>) -> Value {
    json!({
        "content": [{"type": "text", "text": text.into()}],
        "isError": is_error,
        "structuredContent": structured
    })
}

fn looks_like_source(text: &str) -> bool {
    let trimmed = text.trim_start();
    text.len() > 256
        || trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.contains("fn ")
        || text.lines().any(|line| line.len() > 120)
}

struct Capture {
    id: String,
    created: Instant,
    bytes: Vec<u8>,
    original_bytes: usize,
    capture_truncated: bool,
    identity: Value,
    is_error: bool,
    storage_bytes: usize,
}

struct Tombstone {
    id: String,
    category: &'static str,
    message: String,
}

pub struct Responses {
    prefix: u64,
    next: u64,
    origin: Instant,
    elapsed: Duration,
    captures: VecDeque<Capture>,
    tombstones: VecDeque<Tombstone>,
    total: usize,
}

impl Responses {
    pub fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let addr = std::ptr::addr_of!(nanos) as u64;
        Self {
            prefix: nanos ^ addr.rotate_left(17) ^ (std::process::id() as u64).rotate_left(9),
            next: 0,
            origin: Instant::now(),
            elapsed: Duration::ZERO,
            captures: VecDeque::new(),
            tombstones: VecDeque::new(),
            total: 0,
        }
    }

    fn now(&self) -> Instant {
        self.origin + self.elapsed
    }

    fn purge(&mut self) {
        let now = self.now();
        while let Some(front) = self.captures.front() {
            if now.saturating_duration_since(front.created) < TTL {
                break;
            }
            self.forget(
                "expired",
                "Retained CodeGraph detail expired; it is not rerun.",
            );
        }
    }

    fn forget(&mut self, category: &'static str, message: &str) {
        if let Some(capture) = self.captures.pop_front() {
            self.total = self.total.saturating_sub(capture.storage_bytes);
            if self.tombstones.len() == MAX_CAPTURES {
                self.tombstones.pop_front();
            }
            self.tombstones.push_back(Tombstone {
                id: capture.id,
                category,
                message: message.into(),
            });
        }
    }

    fn retain(
        &mut self,
        upstream: &Value,
        identity: &Value,
        is_error: bool,
    ) -> (String, usize, bool) {
        self.purge();
        let mut bytes = serde_json::to_vec(upstream).unwrap_or_default();
        let original_bytes = bytes.len();
        // Count the retained identity and ID as well as the original payload.
        let metadata_bytes = encoded_len(identity).saturating_add(32);
        let payload_limit = MAX_CAPTURE_BYTES.saturating_sub(metadata_bytes);
        if bytes.len() > payload_limit {
            bytes.truncate(utf8_end(&bytes, payload_limit));
        }
        let storage_bytes = bytes.len().saturating_add(metadata_bytes);
        let capture_truncated = bytes.len() < original_bytes;
        while self.captures.len() >= MAX_CAPTURES
            || self.total.saturating_add(storage_bytes) > MAX_TOTAL_BYTES
        {
            if self.captures.is_empty() {
                break;
            }
            self.forget(
                "evicted",
                "Retained CodeGraph detail was evicted by the 32-response or 8 MiB limit; it is not rerun.",
            );
        }
        self.next += 1;
        let id = format!("cg{:016x}{:08x}", self.prefix, self.next);
        self.total = self.total.saturating_add(storage_bytes);
        self.captures.push_back(Capture {
            id: id.clone(),
            created: self.now(),
            bytes,
            original_bytes,
            capture_truncated,
            identity: identity.clone(),
            is_error,
            storage_bytes,
        });
        (id, original_bytes, capture_truncated)
    }

    fn lookup<'a>(&'a mut self, id: &str) -> Result<&'a Capture, (&'static str, String)> {
        self.purge();
        if let Some(index) = self.captures.iter().position(|capture| capture.id == id) {
            if self
                .now()
                .saturating_duration_since(self.captures[index].created)
                >= TTL
            {
                let capture = self.captures.remove(index).expect("indexed capture");
                self.total = self.total.saturating_sub(capture.storage_bytes);
                return Err((
                    "expired",
                    "Retained CodeGraph detail expired; it is not rerun.".into(),
                ));
            }
            return Ok(&self.captures[index]);
        }
        if let Some(tomb) = self.tombstones.iter().rev().find(|tomb| tomb.id == id) {
            return Err((tomb.category, tomb.message.clone()));
        }
        Err((
            "unknown_id",
            "Unknown CodeGraph detail id for this client; it is not rerun.".into(),
        ))
    }

    fn budget_or_default(max_bytes: usize) -> usize {
        validate_budget_len(max_bytes).unwrap_or(DEFAULT_BUDGET)
    }

    pub fn shape(&mut self, upstream: Value, identity: &Value, max_bytes: usize) -> Value {
        let budget = match validate_budget_len(max_bytes) {
            Ok(budget) => budget,
            Err(error) => {
                return bound(
                    mcp(
                        true,
                        error.to_string(),
                        identity_map(
                            identity,
                            json!({"category": "invalid_budget", "message": error.to_string()}),
                        ),
                    ),
                    DEFAULT_BUDGET,
                );
            }
        };
        let extracted = extract(&upstream);
        let full = assemble(&extracted, identity, None, Reduce::Keep, 0, false, None);
        if encoded_len(&full) <= budget {
            return bound(full, budget);
        }
        let (detail_id, original_bytes, capture_truncated) =
            self.retain(&upstream, identity, extracted.is_error);
        let reduced = assemble(
            &extracted,
            identity,
            Some(&detail_id),
            Reduce::Partial,
            original_bytes,
            capture_truncated,
            None,
        );
        bound(reduced, budget)
    }

    /// Retain the original payload once and return a bounded refusal. Parent
    /// detects fan-out/ambiguity and supplies `reason`; this never reruns a query.
    pub fn refuse(
        &mut self,
        upstream: Value,
        identity: &Value,
        reason: &str,
        max_bytes: usize,
    ) -> Value {
        let budget = Self::budget_or_default(max_bytes);
        let extracted = extract(&upstream);
        let (detail_id, original_bytes, capture_truncated) = self.retain(&upstream, identity, true);
        let reduced = assemble(
            &extracted,
            identity,
            Some(&detail_id),
            Reduce::Refuse,
            original_bytes,
            capture_truncated,
            Some(reason),
        );
        bound(reduced, budget)
    }

    pub fn detail(&mut self, id: &str, offset: usize, max_bytes: usize) -> Value {
        let budget = match validate_budget_len(max_bytes) {
            Ok(budget) => budget,
            Err(error) => {
                return bound(
                    mcp(
                        true,
                        error.to_string(),
                        error_map("invalid_budget", &error.to_string(), None),
                    ),
                    DEFAULT_BUDGET,
                );
            }
        };
        match self.lookup(id) {
            Err((category, message)) => bound(
                mcp(
                    true,
                    message.clone(),
                    error_map(category, &message, Some(id)),
                ),
                budget,
            ),
            Ok(capture) => {
                if offset > capture.bytes.len() || !boundary(&capture.bytes, offset) {
                    let message = "CodeGraph detail offset is past the retained bytes or not a UTF-8 boundary; it is not rerun.";
                    return bound(
                        mcp(true, message, {
                            let mut map = error_map("invalid_offset", message, Some(id));
                            map.insert("offset".into(), json!(offset));
                            map.insert("retained_bytes".into(), json!(capture.bytes.len()));
                            map
                        }),
                        budget,
                    );
                }
                page(capture, offset, budget)
            }
        }
    }
}

impl Default for Responses {
    fn default() -> Self {
        Self::new()
    }
}

fn error_map(category: &str, message: &str, id: Option<&str>) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(
        "error".into(),
        json!({"category": category, "message": message, "rerun": false}),
    );
    if let Some(id) = id {
        map.insert("id".into(), json!(id));
        map.insert("detail_id".into(), json!(id));
    }
    map
}

fn identity_map(identity: &Value, error: Value) -> Map<String, Value> {
    let mut map = Map::new();
    overlay(&mut map, identity);
    map.insert("error".into(), error);
    map
}

fn overlay(map: &mut Map<String, Value>, identity: &Value) {
    if let Some(object) = identity.as_object() {
        for key in IDENTITY_KEYS {
            if let Some(value) = object.get(*key) {
                map.insert((*key).into(), value.clone());
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reduce {
    Keep,
    Partial,
    Refuse,
}

struct Extracted {
    is_error: bool,
    error: Option<Value>,
    warnings: Option<Value>,
    diagnostics: Option<Value>,
    extras: Map<String, Value>,
    structured: Option<Value>,
    texts: Vec<String>,
    duplicate: bool,
    list_key: Option<String>,
    records: Vec<Value>,
}

// A blank line inside source is not a record boundary. Keep fenced source as
// one indivisible record, including an unterminated fence from upstream.
fn markdown_records(text: &str) -> Vec<Value> {
    let mut records = Vec::new();
    let mut block = String::new();
    let mut fence: Option<(char, usize)> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some((marker, width)) = fence {
            if trimmed.chars().take_while(|c| *c == marker).count() >= width
                && trimmed.chars().all(|c| c == marker)
            {
                fence = None;
            }
        } else {
            let marker = trimmed.chars().next().unwrap_or(' ');
            let width = trimmed.chars().take_while(|c| *c == marker).count();
            if matches!(marker, '`' | '~') && width >= 3 {
                fence = Some((marker, width));
            } else if trimmed.is_empty() {
                if !block.is_empty() {
                    records.push(Value::String(std::mem::take(&mut block)));
                }
                continue;
            }
        }
        block.push_str(line);
    }
    if !block.is_empty() {
        records.push(Value::String(block));
    }
    records
}

fn extract(upstream: &Value) -> Extracted {
    let Some(object) = upstream.as_object() else {
        return Extracted {
            is_error: true,
            error: Some(json!({
                "category": "malformed",
                "message": "CodeGraph upstream result is not an object"
            })),
            warnings: None,
            diagnostics: None,
            extras: Map::new(),
            structured: None,
            texts: Vec::new(),
            duplicate: false,
            list_key: None,
            records: Vec::new(),
        };
    };
    let mut texts = Vec::new();
    if let Some(content) = object.get("content").and_then(Value::as_array) {
        for item in content {
            if item["type"] == "text"
                && let Some(text) = item["text"].as_str()
                && texts.last().map(String::as_str) != Some(text)
            {
                texts.push(text.to_owned());
            }
        }
    }
    let structured = object.get("structuredContent").cloned();
    let duplicate = structured
        .as_ref()
        .is_some_and(|value| texts.iter().any(|text| same_payload(text, value)));
    if duplicate {
        texts.clear();
    }
    let mut records = Vec::new();
    let mut list_key = None;
    match structured.as_ref() {
        Some(Value::Array(items)) => records = items.clone(),
        Some(Value::Object(map)) => {
            for key in LIST_KEYS {
                if let Some(Value::Array(items)) = map.get(*key) {
                    list_key = Some((*key).into());
                    records = items.clone();
                    break;
                }
            }
        }
        _ => {}
    }
    if records.is_empty() && texts.len() == 1 {
        if let Ok(Value::Array(items)) = serde_json::from_str(&texts[0]) {
            records = items;
        } else if texts[0].contains("\n\n") {
            let chunks = markdown_records(&texts[0]);
            if chunks.len() > 1 {
                records = chunks;
            }
        }
    }
    let is_error = object
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || object.get("error").is_some();
    let mut extras = Map::new();
    for (key, value) in object {
        if !matches!(
            key.as_str(),
            "content" | "isError" | "structuredContent" | "error" | "warnings" | "diagnostics"
        ) {
            extras.insert(key.clone(), value.clone());
        }
    }
    Extracted {
        is_error,
        error: object.get("error").cloned().or_else(|| {
            is_error.then(|| json!({"category": "error", "message": "CodeGraph tool failed"}))
        }),
        warnings: object.get("warnings").cloned(),
        diagnostics: object.get("diagnostics").cloned(),
        extras,
        structured,
        texts,
        duplicate,
        list_key,
        records,
    }
}

fn same_payload(text: &str, structured: &Value) -> bool {
    if structured.as_str() == Some(text) {
        return true;
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(text)
        && parsed == *structured
    {
        return true;
    }
    serde_json::to_string(structured).is_ok_and(|encoded| encoded == text)
}

fn strip_source(mut value: Value) -> (Value, bool) {
    let mut stripped = false;
    match &mut value {
        Value::Object(object) => {
            for key in SOURCE_KEYS {
                if object.remove(*key).is_some() {
                    stripped = true;
                }
            }
            if object
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(looks_like_source)
            {
                object.remove("text");
                stripped = true;
            }
        }
        Value::String(body) if looks_like_source(body) => {
            return (Value::Null, true);
        }
        _ => {}
    }
    (value, stripped)
}

fn apply_records(structured: &mut Map<String, Value>, key: Option<&str>, records: Vec<Value>) {
    structured.insert(key.unwrap_or("records").into(), Value::Array(records));
}

fn merge_error(existing: Option<Value>, category: &str, message: &str) -> Value {
    match existing {
        Some(Value::Object(mut map)) => {
            map.entry("category").or_insert_with(|| json!(category));
            map.insert("message".into(), json!(message));
            map.insert("rerun".into(), json!(false));
            Value::Object(map)
        }
        Some(Value::String(old)) => json!({
            "category": category,
            "message": message,
            "original": old,
            "rerun": false
        }),
        Some(other) => json!({
            "category": category,
            "message": message,
            "original": other,
            "rerun": false
        }),
        None => json!({"category": category, "message": message, "rerun": false}),
    }
}

fn assemble(
    extracted: &Extracted,
    identity: &Value,
    detail_id: Option<&str>,
    reduce: Reduce,
    original_bytes: usize,
    capture_truncated: bool,
    reason: Option<&str>,
) -> Value {
    let mut structured = match extracted.structured.clone() {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    overlay(&mut structured, identity);
    for (key, value) in &extracted.extras {
        structured
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
    if let Some(error) = extracted.error.clone() {
        structured.insert("error".into(), error);
    }
    if let Some(warnings) = extracted.warnings.clone() {
        structured.insert("warnings".into(), warnings);
    }
    if let Some(diagnostics) = extracted.diagnostics.clone() {
        structured.insert("diagnostics".into(), diagnostics);
    }
    if extracted.duplicate {
        structured.insert("duplicate_payload_omitted".into(), json!(true));
    }
    if capture_truncated {
        structured.insert("capture_truncated".into(), json!(true));
        structured.insert("original_bytes".into(), json!(original_bytes));
        structured.insert(
            "retained_bytes".into(),
            json!(MAX_CAPTURE_BYTES.saturating_sub(encoded_len(identity).saturating_add(32))),
        );
    }
    let mut text = if extracted.texts.is_empty() {
        String::new()
    } else {
        extracted.texts.join("\n")
    };
    let mut source_partial = false;
    let mut list_partial = false;
    let mut omitted = 0usize;
    let mut kept = extracted.records.clone();
    if reduce != Reduce::Keep {
        kept.clear();
        for record in &extracted.records {
            match reduce {
                Reduce::Refuse => {
                    omitted += 1;
                    list_partial = true;
                    if matches!(record, Value::String(body) if looks_like_source(body))
                        || strip_source(record.clone()).1
                    {
                        source_partial = true;
                    }
                }
                Reduce::Partial => {
                    if matches!(record, Value::String(body) if looks_like_source(body)) {
                        omitted += 1;
                        source_partial = true;
                        list_partial = true;
                        continue;
                    }
                    let (stripped, did_strip) = strip_source(record.clone());
                    if did_strip {
                        source_partial = true;
                        list_partial = true;
                        if stripped.is_null() {
                            omitted += 1;
                        } else {
                            kept.push(stripped);
                        }
                    } else {
                        kept.push(record.clone());
                    }
                }
                Reduce::Keep => {}
            }
        }
        for key in SOURCE_KEYS {
            if structured.remove(*key).is_some() {
                source_partial = true;
                list_partial = true;
            }
        }
        if looks_like_source(&text) || reduce == Reduce::Refuse {
            if !text.is_empty() {
                source_partial = true;
                list_partial = true;
                omitted = omitted.max(1);
            }
            text.clear();
        }
    }
    // Text-derived records are an alternative representation for reduction,
    // not a second copy of the same successful text payload.
    if !extracted.records.is_empty() && (reduce != Reduce::Keep || extracted.structured.is_some()) {
        apply_records(&mut structured, extracted.list_key.as_deref(), kept.clone());
        if omitted > 0 || kept.len() != extracted.records.len() {
            list_partial = true;
        }
    }
    let is_error = extracted.is_error || reduce == Reduce::Refuse;
    let truncated = reduce != Reduce::Keep || capture_truncated || list_partial || source_partial;
    if truncated {
        structured.insert("truncated".into(), json!(true));
    }
    if list_partial {
        structured.insert("list_partial".into(), json!(true));
        structured.insert(
            "omitted_records".into(),
            json!(omitted.max(extracted.records.len().saturating_sub(kept.len()))),
        );
    }
    if source_partial {
        structured.insert("source_partial".into(), json!(true));
    }
    if let Some(id) = detail_id {
        structured.insert("detail_id".into(), json!(id));
        structured.insert("next_offset".into(), json!(0));
        structured.insert("hint".into(), json!(HINT));
        structured.insert("rerun".into(), json!(false));
    }
    if reduce == Reduce::Refuse {
        let message = reason.unwrap_or("CodeGraph result refused; narrow by exact file or symbol.");
        let merged = merge_error(structured.remove("error"), "refused", message);
        structured.insert("error".into(), merged);
    }
    if text.is_empty() || truncated && looks_like_source(&text) {
        text = summary(
            is_error,
            structured.get("error"),
            truncated,
            kept.len(),
            extracted.records.len(),
            detail_id,
            reason,
        );
    } else if truncated {
        text = format!(
            "{}\n{}",
            text,
            summary(
                is_error,
                structured.get("error"),
                true,
                kept.len(),
                extracted.records.len(),
                detail_id,
                reason
            )
        );
    }
    mcp(is_error, text, structured)
}

fn summary(
    is_error: bool,
    error: Option<&Value>,
    truncated: bool,
    kept: usize,
    total: usize,
    detail_id: Option<&str>,
    reason: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(reason) = reason {
        parts.push(reason.to_owned());
    }
    if is_error {
        let message = error
            .and_then(|value| value["message"].as_str())
            .or_else(|| error.and_then(Value::as_str))
            .unwrap_or("CodeGraph tool failed");
        let category = error
            .and_then(|value| value["category"].as_str())
            .unwrap_or("error");
        parts.push(format!("{category}: {message}"));
    }
    if truncated {
        parts.push(format!(
            "Partial: {kept}/{total} records; omitted source is available in retained detail."
        ));
        if let Some(id) = detail_id {
            parts.push(format!("detail_id={id}"));
        }
    } else if parts.is_empty() {
        parts.push("CodeGraph managed result.".into());
    }
    parts.join("\n")
}

fn shrink_structured(map: &mut Map<String, Value>) -> bool {
    let mut removable: Vec<String> = map
        .keys()
        .filter(|key| !KEEP_KEYS.contains(&key.as_str()))
        .cloned()
        .collect();
    if let Some(key) = removable.pop() {
        map.remove(&key);
        return true;
    }
    for key in [
        "diagnostics",
        "hint",
        "coverage",
        "records",
        "results",
        "matches",
        "nodes",
        "symbols",
        "callers",
        "callees",
        "files",
        "items",
        "edges",
        "hits",
        "definitions",
        "references",
        "page",
    ] {
        if map.remove(key).is_some() {
            return true;
        }
    }
    false
}

fn set_text(value: &mut Value, text: String) {
    if let Some(slot) = value.pointer_mut("/content/0/text") {
        *slot = Value::String(text);
    }
}

fn mark_partial(value: &mut Value) {
    if let Some(object) = value
        .get_mut("structuredContent")
        .and_then(Value::as_object_mut)
    {
        object.insert("truncated".into(), json!(true));
        object.insert("source_partial".into(), json!(true));
        object.insert("list_partial".into(), json!(true));
    }
}

fn bound(value: Value, budget: usize) -> Value {
    bound_with(value, budget, false)
}

fn bound_with(mut value: Value, budget: usize, slice_text: bool) -> Value {
    if encoded_len(&value) <= budget {
        return value;
    }
    let text = value
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if !slice_text && looks_like_source(&text) {
        let replacement = value
            .get("structuredContent")
            .and_then(|structured| structured.get("error"))
            .map(|error| {
                summary(
                    value["isError"] == true,
                    Some(error),
                    true,
                    0,
                    0,
                    value["structuredContent"]["detail_id"].as_str(),
                    None,
                )
            })
            .unwrap_or_else(|| {
                "truncated: source/JSON records were not sliced; use detail_id or narrow the query."
                    .into()
            });
        set_text(&mut value, replacement);
        mark_partial(&mut value);
        if encoded_len(&value) <= budget {
            return value;
        }
    } else {
        let bytes = text.as_bytes();
        let mut end = utf8_end(bytes, bytes.len().min(budget.saturating_sub(256)));
        while end > 0 {
            set_text(&mut value, text[..end].to_owned());
            mark_partial(&mut value);
            if encoded_len(&value) <= budget {
                return value;
            }
            end = utf8_end(bytes, end.saturating_sub(1));
        }
        set_text(&mut value, "truncated".into());
        mark_partial(&mut value);
    }
    while encoded_len(&value) > budget {
        let Some(object) = value
            .get_mut("structuredContent")
            .and_then(Value::as_object_mut)
        else {
            break;
        };
        if !shrink_structured(object) {
            let error = object.get("error").cloned();
            let detail = object.get("detail_id").cloned();
            let warnings = object.get("warnings").cloned();
            let root = object.get("root").cloned();
            let generation = object.get("generation").cloned();
            object.clear();
            object.insert("truncated".into(), json!(true));
            object.insert("source_partial".into(), json!(true));
            object.insert("list_partial".into(), json!(true));
            if let Some(error) = error {
                object.insert("error".into(), error);
            }
            if let Some(detail) = detail {
                object.insert("detail_id".into(), detail);
            }
            if let Some(warnings) = warnings {
                object.insert("warnings".into(), warnings);
            }
            if let Some(root) = root {
                object.insert("root".into(), root);
            }
            if let Some(generation) = generation {
                object.insert("generation".into(), generation);
            }
            break;
        }
        object.insert("truncated".into(), json!(true));
    }
    if encoded_len(&value) > budget {
        let error = value.pointer("/structuredContent/error").cloned();
        let detail = value.pointer("/structuredContent/detail_id").cloned();
        let warnings = value.pointer("/structuredContent/warnings").cloned();
        let mut map = Map::new();
        map.insert("truncated".into(), json!(true));
        map.insert("source_partial".into(), json!(true));
        map.insert("list_partial".into(), json!(true));
        if let Some(error) = error {
            map.insert("error".into(), error);
        }
        if let Some(detail) = detail {
            map.insert("detail_id".into(), detail);
        }
        if let Some(warnings) = warnings {
            map.insert("warnings".into(), warnings);
        }
        mcp(true, "truncated", map)
    } else {
        value
    }
}

fn page(capture: &Capture, offset: usize, budget: usize) -> Value {
    let mut structured = Map::new();
    overlay(&mut structured, &capture.identity);
    structured.insert("id".into(), json!(capture.id));
    structured.insert("detail_id".into(), json!(capture.id));
    structured.insert("offset".into(), json!(offset));
    structured.insert("retained_bytes".into(), json!(capture.bytes.len()));
    structured.insert("original_bytes".into(), json!(capture.original_bytes));
    structured.insert("capture_truncated".into(), json!(capture.capture_truncated));
    structured.insert("hint".into(), json!(HINT));
    structured.insert("rerun".into(), json!(false));
    let mut end = utf8_end(
        &capture.bytes,
        capture.bytes.len().min(offset.saturating_add(budget)),
    );
    if end < offset {
        end = offset;
    }
    loop {
        let slice = std::str::from_utf8(&capture.bytes[offset..end]).unwrap_or("");
        structured.insert("next_offset".into(), json!(end));
        let remaining = end < capture.bytes.len() || capture.capture_truncated;
        structured.insert("page_partial".into(), json!(remaining));
        structured.insert("list_partial".into(), json!(remaining));
        structured.insert("source_partial".into(), json!(remaining));
        structured.insert("truncated".into(), json!(remaining));
        let value = mcp(capture.is_error, slice, structured.clone());
        if encoded_len(&value) <= budget || end == offset {
            return bound_with(value, budget, true);
        }
        let next = utf8_end(&capture.bytes, end.saturating_sub(1));
        end = if next <= offset { offset } else { next };
    }
}

#[cfg(test)]
impl Responses {
    fn advance(&mut self, duration: Duration) {
        self.elapsed += duration;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Value {
        json!({
            "provider": "colbymchenry/codegraph",
            "version": "1.6.0",
            "root": "D:/repo",
            "generation": null,
            "worker_lease": "lease",
            "freshness": "unverified",
            "coverage": "unverified coverage"
        })
    }

    fn text_of(value: &Value) -> &str {
        value["content"][0]["text"].as_str().unwrap()
    }

    fn assert_budget(value: &Value, budget: usize) {
        let bytes = encoded(value);
        assert!(
            bytes.len() <= budget,
            "envelope {} exceeded {budget}: {}",
            bytes.len(),
            String::from_utf8_lossy(&bytes)
        );
        serde_json::from_slice::<Value>(&bytes).unwrap();
    }

    #[test]
    fn validate_budget_rejects_non_integers_and_out_of_range() {
        for value in [
            json!(1023),
            json!(16385),
            json!(4096.5),
            json!(-1),
            json!(null),
            json!("4096"),
            json!({"max_response_bytes": 0}),
        ] {
            assert!(validate_budget(&value).is_err(), "{value}");
        }
        assert_eq!(validate_budget(&json!(1024)).unwrap(), 1024);
        assert_eq!(validate_budget(&json!(4096)).unwrap(), 4096);
        assert_eq!(validate_budget(&json!(16384)).unwrap(), 16384);
        assert_eq!(
            validate_budget(&json!({"max_response_bytes": 4096})).unwrap(),
            4096
        );
        let mut responses = Responses::new();
        let shaped = responses.shape(json!({"content":[]}), &identity(), 8);
        assert_eq!(shaped["isError"], true);
        assert_eq!(
            shaped["structuredContent"]["error"]["category"],
            "invalid_budget"
        );
        assert_budget(&shaped, DEFAULT_BUDGET);
    }

    #[test]
    fn unicode_escaping_and_page_boundaries() {
        let mut responses = Responses::new();
        let quotes = "\"".repeat(2000);
        let shaped = responses.shape(
            json!({
                "content": [{"type": "text", "text": quotes}],
                "isError": false
            }),
            &identity(),
            1024,
        );
        assert_budget(&shaped, 1024);
        let needle = "日本😀кириллица";
        let body = format!("{}{}{}", "a".repeat(3000), needle, "b".repeat(3000));
        let shaped = responses.shape(
            json!({
                "content": [{"type": "text", "text": body}],
                "structuredContent": {"code": body.clone()},
                "isError": false
            }),
            &identity(),
            1024,
        );
        assert_budget(&shaped, 1024);
        assert_eq!(shaped["structuredContent"]["source_partial"], true);
        assert_eq!(shaped["structuredContent"]["truncated"], true);
        assert!(
            !encoded(&shaped)
                .windows(needle.len())
                .any(|window| window == needle.as_bytes()),
            "source needle leaked into the bounded answer"
        );
        let id = shaped["structuredContent"]["detail_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut offset = 0;
        let mut recovered = String::new();
        for _ in 0..16 {
            let page = responses.detail(&id, offset, 1024);
            assert_budget(&page, 1024);
            assert!(boundary(text_of(&page).as_bytes(), 0));
            recovered.push_str(text_of(&page));
            let next = page["structuredContent"]["next_offset"].as_u64().unwrap() as usize;
            assert!(next >= offset);
            if next == offset {
                break;
            }
            offset = next;
            if page["structuredContent"]["page_partial"] != true {
                break;
            }
        }
        assert!(recovered.contains(needle), "{recovered}");
        let mid = recovered.find('😀').unwrap();
        let invalid = responses.detail(&id, mid + 1, 1024);
        assert_eq!(invalid["isError"], true);
        assert_eq!(
            invalid["structuredContent"]["error"]["category"],
            "invalid_offset"
        );
        assert_eq!(invalid["structuredContent"]["error"]["rerun"], false);
        assert_budget(&invalid, 1024);
    }

    #[test]
    fn errors_warnings_and_identity_survive_oversized_bodies() {
        let mut responses = Responses::new();
        let body = "Ω".repeat(4000);
        let shaped = responses.shape(
            json!({
                "content": [{"type": "text", "text": body}],
                "isError": true,
                "error": {"category": "index_failed", "code": -32000, "message": "parser exploded"},
                "warnings": [{"category": "stale", "message": "generation unverified"}],
                "diagnostics": {"stderr": "note"}
            }),
            &identity(),
            1024,
        );
        assert_budget(&shaped, 1024);
        assert_eq!(shaped["isError"], true);
        assert_eq!(
            shaped["structuredContent"]["error"]["category"],
            "index_failed"
        );
        assert_eq!(
            shaped["structuredContent"]["error"]["message"],
            "parser exploded"
        );
        assert_eq!(
            shaped["structuredContent"]["warnings"][0]["category"],
            "stale"
        );
        assert_eq!(shaped["structuredContent"]["root"], "D:/repo");
        assert!(shaped["structuredContent"]["generation"].is_null());
        assert_eq!(shaped["structuredContent"]["truncated"], true);
        assert!(text_of(&shaped).contains("index_failed"));
        assert!(text_of(&shaped).contains("parser exploded"));
    }

    #[test]
    fn complete_small_records_kept_and_duplicate_payload_dropped() {
        let mut responses = Responses::new();
        let unique = "UNIQUE_TOKEN_XYZ";
        let payload = json!({"name": unique, "file": "src/a.rs"});
        let shaped = responses.shape(
            json!({
                "content": [{"type": "text", "text": serde_json::to_string(&payload).unwrap()}],
                "structuredContent": payload.clone(),
                "isError": false
            }),
            &identity(),
            4096,
        );
        assert_budget(&shaped, 4096);
        let encoded = encoded(&shaped);
        let hits = encoded
            .windows(unique.len())
            .filter(|window| *window == unique.as_bytes())
            .count();
        assert_eq!(hits, 1, "{}", String::from_utf8_lossy(&encoded));
        assert_eq!(
            shaped["structuredContent"]["duplicate_payload_omitted"],
            true
        );
        assert_eq!(shaped["structuredContent"]["name"], unique);

        let huge = format!("HEAD{}TAIL", "x".repeat(5000));
        let shaped = responses.shape(
            json!({
                "content": [{"type": "text", "text": "list"}],
                "structuredContent": {
                    "callers": [
                        {"name": "one", "file": "a.rs"},
                        {"name": "two", "file": "b.rs", "code": huge.clone()},
                        {"name": "three", "file": "c.rs"}
                    ]
                },
                "isError": false
            }),
            &identity(),
            1024,
        );
        assert_budget(&shaped, 1024);
        let callers = shaped["structuredContent"]["callers"].as_array().unwrap();
        assert!(callers.iter().any(|row| row["name"] == "one"));
        assert!(callers.iter().any(|row| row["name"] == "three"));
        assert!(callers.iter().all(|row| row.get("code").is_none()));
        assert_eq!(shaped["structuredContent"]["source_partial"], true);
        assert_eq!(shaped["structuredContent"]["list_partial"], true);
        assert!(
            !text_of(&shaped).contains("HEAD") || !text_of(&shaped).contains(&"x".repeat(64)),
            "{}",
            text_of(&shaped)
        );
        let id = shaped["structuredContent"]["detail_id"].as_str().unwrap();
        let page = responses.detail(id, 0, 16384);
        assert_budget(&page, 16384);
        assert!(text_of(&page).contains("HEAD") || page.to_string().contains("two"));
    }

    #[test]
    fn markdown_payload_is_not_duplicated_or_split_inside_source_fences() {
        let mut responses = Responses::new();
        let text = "**Search Results**\n\n**single_source_oracle**\nsrc/lib.rs:4\n";
        let answer = responses.shape(
            json!({"content":[{"type":"text","text":text}]}),
            &identity(),
            4096,
        );
        assert_eq!(
            answer.to_string().matches("single_source_oracle").count(),
            1
        );
        let source = format!(
            "**Source**\n\n```rust\npub fn fence_start() {{\n\n    {}\n\n}} // fence_end\n```\n\n**Next result**\n",
            "x".repeat(5000)
        );
        let records = markdown_records(&source);
        let body = records
            .iter()
            .find_map(|v| v.as_str().filter(|s| s.contains("fence_start")))
            .unwrap();
        assert!(body.contains("fence_end") && body.trim_end().ends_with("```"));
        let answer = responses.shape(
            json!({"content":[{"type":"text","text":source}]}),
            &identity(),
            1024,
        );
        assert_budget(&answer, 1024);
        assert!(!answer.to_string().contains("fence_start"));
        assert!(!answer.to_string().contains("fence_end"));
        let id = answer["structuredContent"]["detail_id"].as_str().unwrap();
        let recovered = responses.detail(id, 0, 16384);
        assert!(
            text_of(&recovered).contains("fence_start")
                && text_of(&recovered).contains("fence_end")
        );
    }

    #[test]
    fn long_source_line_is_refused_instead_of_sliced() {
        let mut responses = Responses::new();
        let line = format!("fn entry({}) {{}}", "x".repeat(1800));
        let shaped = responses.shape(
            json!({
                "content": [{"type": "text", "text": line}],
                "isError": false
            }),
            &identity(),
            1024,
        );
        assert_budget(&shaped, 1024);
        assert_eq!(shaped["structuredContent"]["source_partial"], true);
        assert_eq!(shaped["structuredContent"]["list_partial"], true);
        assert!(shaped["structuredContent"]["detail_id"].is_string());
        assert!(
            !text_of(&shaped).contains("fn entry") && !text_of(&shaped).contains(&"x".repeat(80)),
            "{}",
            text_of(&shaped)
        );
        assert!(text_of(&shaped).contains("omitted source"));
    }

    #[test]
    fn refuse_retains_original_and_preserves_warning_category() {
        let mut responses = Responses::new();
        let upstream = json!({
            "content": [{"type": "text", "text": "Showing 15 callers across 3 distinct definitions"}],
            "warnings": [{"category": "ambiguous", "message": "distinct definitions"}],
            "structuredContent": {
                "callers": [
                    {"name": "a", "file": "one.rs"},
                    {"name": "b", "file": "two.rs"}
                ]
            },
            "isError": false
        });
        let refused = responses.refuse(
            upstream,
            &identity(),
            "Callers span multiple definitions; narrow by exact file instead of treating limit as global.",
            4096,
        );
        assert_budget(&refused, 4096);
        assert_eq!(refused["isError"], true);
        assert_eq!(refused["structuredContent"]["truncated"], true);
        assert_eq!(refused["structuredContent"]["error"]["category"], "refused");
        assert_eq!(
            refused["structuredContent"]["warnings"][0]["category"],
            "ambiguous"
        );
        assert!(refused["structuredContent"]["detail_id"].is_string());
        assert_eq!(refused["structuredContent"]["rerun"], false);
        let id = refused["structuredContent"]["detail_id"].as_str().unwrap();
        let page = responses.detail(id, 0, 4096);
        assert!(
            text_of(&page).contains("distinct definitions") || page.to_string().contains("one.rs")
        );
    }

    #[test]
    fn ttl_count_isolation_and_unknown_id() {
        let mut first = Responses::new();
        let mut last_id = String::new();
        let mut oldest = String::new();
        for index in 0..33 {
            let body = format!("capture-{index}-{}", "z".repeat(1800));
            let shaped = first.shape(
                json!({"content":[{"type":"text","text":body}],"isError":false}),
                &identity(),
                1024,
            );
            let id = shaped["structuredContent"]["detail_id"]
                .as_str()
                .unwrap()
                .to_owned();
            if index == 0 {
                oldest = id.clone();
            }
            last_id = id;
        }
        let evicted = first.detail(&oldest, 0, 1024);
        assert_eq!(evicted["isError"], true);
        assert_eq!(evicted["structuredContent"]["error"]["rerun"], false);
        assert!(
            evicted["structuredContent"]["error"]["category"] == "evicted"
                || evicted["structuredContent"]["error"]["category"] == "unknown_id",
            "{evicted}"
        );
        assert_budget(&evicted, 1024);
        assert_eq!(first.detail(&last_id, 0, 1024)["isError"], false);

        let mut second = Responses::new();
        let foreign = second.detail(&last_id, 0, 1024);
        assert_eq!(
            foreign["structuredContent"]["error"]["category"],
            "unknown_id"
        );
        assert_eq!(foreign["isError"], true);

        let live = first.shape(
            json!({"content":[{"type":"text","text": format!("ttl-{}", "q".repeat(2000))}],"isError":false}),
            &identity(),
            1024,
        );
        let ttl_id = live["structuredContent"]["detail_id"]
            .as_str()
            .unwrap()
            .to_owned();
        first.advance(TTL + Duration::from_secs(1));
        let expired = first.detail(&ttl_id, 0, 1024);
        assert_eq!(expired["structuredContent"]["error"]["category"], "expired");
        assert_eq!(expired["structuredContent"]["error"]["rerun"], false);
        assert_budget(&expired, 1024);
        assert_eq!(
            first.detail("cgdeadbeef", 0, 1024)["structuredContent"]["error"]["category"],
            "unknown_id"
        );
    }

    #[test]
    fn capture_truncation_never_claims_full_recovery() {
        let mut responses = Responses::new();
        let body = format!("START{}NEEDLE{}", "n".repeat(300_000), "m".repeat(8_000));
        let shaped = responses.shape(
            json!({
                "content": [{"type": "text", "text": body}],
                "isError": false
            }),
            &identity(),
            4096,
        );
        assert_budget(&shaped, 4096);
        let id = shaped["structuredContent"]["detail_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut offset = 0;
        let mut recovered = 0usize;
        let mut saw_truncation = false;
        let mut original = 0usize;
        for _ in 0..64 {
            let page = responses.detail(&id, offset, 16384);
            assert_budget(&page, 16384);
            recovered += text_of(&page).len();
            original = page["structuredContent"]["original_bytes"]
                .as_u64()
                .unwrap() as usize;
            saw_truncation |= page["structuredContent"]["capture_truncated"] == true;
            let next = page["structuredContent"]["next_offset"].as_u64().unwrap() as usize;
            if next == offset {
                break;
            }
            offset = next;
        }
        assert!(saw_truncation);
        assert!(original > MAX_CAPTURE_BYTES, "{original}");
        assert!(recovered <= MAX_CAPTURE_BYTES);
        assert!(recovered < original);
        assert!(!{
            let mut all = String::new();
            let mut offset = 0;
            for _ in 0..64 {
                let page = responses.detail(&id, offset, 16384);
                all.push_str(text_of(&page));
                let next = page["structuredContent"]["next_offset"].as_u64().unwrap() as usize;
                if next == offset {
                    break;
                }
                offset = next;
            }
            all.contains("NEEDLE")
        });
    }
}
