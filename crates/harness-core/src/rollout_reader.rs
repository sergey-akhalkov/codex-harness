//! Tolerant reader for Codex rollout session JSONL files.
//!
//! Owns the recognized rollout event vocabulary (`session_meta`,
//! `turn_context`, `event_msg`, `token_usage_record`, `response_item`,
//! `compacted`), both recorded usage formats (older `event_msg` token-count
//! snapshots and newer `token_usage_record` per-response entries with turn and
//! thread captures), response-identity deduplication, turn association,
//! instruction byte capture and per-file coverage counters for malformed,
//! unknown or skipped records.
//!
//! Only allowlisted visible telemetry is parsed; reasoning and encrypted state
//! are ignored. Instruction texts are measured but never retained: consumers
//! receive byte counts, identities and counters, not instruction or transcript
//! content.
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

/// Recorded token counters of one response, turn or thread snapshot.
pub const TOKEN_FIELDS: [&str; 5] = [
    "input_tokens",
    "cached_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
    "total_tokens",
];
pub type Usage = BTreeMap<String, Option<u64>>;

const MAX_RECORD_BYTES: u64 = 32 * 1024 * 1024;
const EVENT_TYPES: [&str; 6] = [
    "session_meta",
    "turn_context",
    "event_msg",
    "token_usage_record",
    "response_item",
    "compacted",
];

/// Per-file counters for malformed, unknown or skipped records.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Coverage {
    /// Non-blank lines read from the file.
    pub lines: u64,
    /// Lines parsed as JSON values (`recognized_events` plus `unrecognized_events`).
    pub events: u64,
    /// Parsed events with a recognized top-level event type.
    pub recognized_events: u64,
    /// Parsed JSON lines that are not recognized rollout events.
    pub unrecognized_events: u64,
    /// Lines that are not valid JSON.
    pub corrupt_lines: u64,
    /// Lines skipped because they exceed the per-record size bound.
    pub oversized_lines: u64,
}

/// Recorded instruction sizes; texts are measured and never retained.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InstructionBytes {
    /// Bytes of recorded `session_meta` base instructions.
    pub base_bytes: u64,
    /// Bytes of recorded per-turn developer instruction messages.
    pub developer_bytes: u64,
}

/// One deduplicated model response with its recorded turn association.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TurnUsage {
    /// Recorded turn identity; `None` when the record carries none.
    pub turn_id: Option<String>,
    /// Stable response identity used for deduplication.
    pub response_id: Option<String>,
    /// Usage recorded for this response, the counted delta.
    pub usage: Usage,
    /// Cumulative turn usage when the record carries it.
    pub turn_usage: Option<Usage>,
    /// Cumulative thread usage when the record carries it.
    pub thread_usage: Option<Usage>,
}

/// One parsed rollout session file.
#[derive(Debug)]
pub struct SessionSummary {
    /// Accounted thread row; consumed by delegation accounting as-is.
    pub row: Value,
    /// Duplicate-detection fingerprint of the accounted identity and usage.
    pub fingerprint: Value,
    /// Coverage warning codes observed while reading.
    pub warnings: BTreeSet<String>,
    /// Malformed, unknown and skipped record counters.
    pub coverage: Coverage,
    /// Sizes of recorded instructions.
    pub instructions: InstructionBytes,
    /// Deduplicated responses in recorded order with turn association.
    pub turns: Vec<TurnUsage>,
    /// Open source handle retained for caller-side identity checks.
    pub source: Option<File>,
}

/// Recorded token counters of a value; missing or unusable fields stay unknown.
pub fn usage(raw: &Value) -> Usage {
    TOKEN_FIELDS
        .iter()
        .map(|key| ((*key).to_owned(), raw.get(key).and_then(Value::as_u64)))
        .collect()
}

pub fn unknown_usage() -> Usage {
    usage(&Value::Null)
}

pub fn list(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

pub fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    let text = value.as_str()?;
    if text.len() > 128 {
        return None;
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Some(parsed.with_timezone(&Utc));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y%m%dT%H%M%S%.f",
    ] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(text, format) {
            return Some(parsed.and_utc());
        }
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)
        .map(|v| v.and_utc())
}

fn usage_snapshot(value: &Value) -> Option<Usage> {
    value.is_object().then(|| usage(value))
}

fn identifier(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    (text.len() <= 128
        && text
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./:-".contains(&b)))
    .then(|| text.to_owned())
}

/// Extracts recorded text from a string or a list of text parts.
fn text_parts(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    let mut result = String::new();
    for item in list(value) {
        for key in ["text", "input_text"] {
            if let Some(text) = item[key].as_str() {
                result.push_str(text);
            }
        }
    }
    result
}

fn message_text(payload: &Value) -> String {
    text_parts(&payload["content"])
}

fn prefix(text: &str, count: usize) -> String {
    text.trim_start()
        .chars()
        .take(count)
        .collect::<String>()
        .to_lowercase()
}

fn continuations(triggers: &[Value], turns: &[Value], users: &[Value]) -> usize {
    let triggers: BTreeSet<_> = triggers.iter().filter_map(timestamp).collect();
    let turns: BTreeSet<_> = turns.iter().filter_map(timestamp).collect();
    let users: BTreeSet<_> = users.iter().filter_map(timestamp).collect();
    let mut claimed = BTreeSet::new();
    for trigger in triggers {
        let Some(turn) = turns
            .range((
                std::ops::Bound::Excluded(trigger),
                std::ops::Bound::Unbounded,
            ))
            .next()
        else {
            continue;
        };
        let user = users
            .range((
                std::ops::Bound::Excluded(trigger),
                std::ops::Bound::Unbounded,
            ))
            .next();
        if user.is_none_or(|user| user >= turn) {
            claimed.insert(*turn);
        }
    }
    claimed.len()
}

fn project_label(value: &str) -> Option<String> {
    if value.trim().is_empty() {
        return None;
    }
    Some(
        Path::new(value)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("workspace")
            .to_owned(),
    )
}

fn only(values: &BTreeSet<String>) -> Option<String> {
    (values.len() == 1).then(|| values.first().unwrap().clone())
}

#[derive(Default)]
struct Reader {
    thread_ids: Vec<String>,
    parents: BTreeSet<String>,
    models: BTreeSet<String>,
    efforts: BTreeSet<String>,
    session_ids: BTreeSet<String>,
    forked: BTreeSet<String>,
    versions: BTreeSet<String>,
    projects: BTreeSet<String>,
    warnings: BTreeSet<String>,
    counts: BTreeMap<&'static str, u64>,
    responses: BTreeMap<String, Usage>,
    conflicts: BTreeSet<String>,
    children: BTreeSet<String>,
    durations: Vec<u64>,
    stamps: Vec<Value>,
    triggers: Vec<Value>,
    turns: Vec<Value>,
    users: Vec<Value>,
    usage: Usage,
    previous: Option<Usage>,
    saw_meta: bool,
    coverage: Coverage,
    instructions: InstructionBytes,
    turn_usages: Vec<TurnUsage>,
}

impl Reader {
    fn bump(&mut self, key: &'static str) {
        *self.counts.entry(key).or_default() += 1;
    }
    fn warn(&mut self, code: &str) {
        self.warnings.insert(code.to_owned());
    }
    fn stamp(target: &mut Vec<Value>, event: &Value) {
        if event["timestamp"].as_str().is_some_and(|s| s.len() <= 128) {
            target.push(event["timestamp"].clone());
        }
    }
    fn check_usage(&mut self, value: &Usage) {
        if value.values().any(Option::is_none) {
            self.warn("invalid_or_missing_token_fields");
        }
        if value["cached_input_tokens"]
            .zip(value["input_tokens"])
            .is_some_and(|(c, i)| c > i)
        {
            self.warn("cached_input_exceeds_input");
        }
    }

    fn event(&mut self, event: Value) {
        if !event.is_object() {
            self.warn("invalid_event");
            self.coverage.unrecognized_events += 1;
            return;
        }
        let Some(kind) = event["type"].as_str() else {
            self.warn("invalid_event_type");
            self.coverage.unrecognized_events += 1;
            return;
        };
        if !EVENT_TYPES.contains(&kind) {
            self.coverage.unrecognized_events += 1;
            return;
        }
        self.coverage.recognized_events += 1;
        let p = &event["payload"];
        if !p.is_object() {
            self.warn("invalid_payload");
            return;
        }
        if kind == "session_meta" {
            self.saw_meta = true;
            self.instructions.base_bytes = self
                .instructions
                .base_bytes
                .saturating_add(text_parts(&p["base_instructions"]).len() as u64);
            for field in ["id", "session_id"] {
                if !p[field].is_null() {
                    if let Some(id) = identifier(&p[field]) {
                        if field == "id" {
                            if !self.thread_ids.contains(&id) {
                                self.thread_ids.push(id);
                            }
                        } else {
                            self.session_ids.insert(id);
                        }
                    } else {
                        self.warn("invalid_thread_id");
                    }
                }
            }
            if let Some(v) = identifier(&p["cli_version"]) {
                self.versions.insert(v);
            }
            if let Some(v) = identifier(&p["forked_from_id"]) {
                self.forked.insert(v);
                self.warn("forked_or_compacted_history");
            }
            for candidate in [
                &p["parent_thread_id"],
                &p["source"]["subagent"]["thread_spawn"]["parent_thread_id"],
                &p["thread_source"]["subagent"]["thread_spawn"]["parent_thread_id"],
            ] {
                if !candidate.is_null() {
                    if let Some(parent) = identifier(candidate) {
                        self.parents.insert(parent);
                    } else {
                        self.warn("invalid_parent_id");
                    }
                }
            }
        } else if kind == "turn_context" {
            if let Some(model) = identifier(&p["model"]) {
                self.models.insert(model);
            } else {
                self.warn("missing_model_context");
            }
            let effort = p.get("effort").unwrap_or(&p["reasoning_effort"]);
            if let Some(e) = effort.as_str().filter(|v| {
                ["none", "minimal", "low", "medium", "high", "xhigh", "max"].contains(v)
            }) {
                self.efforts.insert(e.to_owned());
            } else {
                self.warn("missing_reasoning_context");
            }
            for value in std::iter::once(&p["cwd"]).chain(list(&p["workspace_roots"])) {
                if let Some(label) = value.as_str().and_then(project_label) {
                    self.projects.insert(label);
                }
            }
        } else if p["type"] == "token_count" {
            let Some(raw) = p["info"].get("total_token_usage") else {
                return;
            };
            let next = usage(raw);
            self.check_usage(&next);
            if self.previous.as_ref().is_some_and(|previous| {
                TOKEN_FIELDS
                    .iter()
                    .any(|key| next[*key].zip(previous[*key]).is_some_and(|(a, b)| a < b))
            }) {
                self.warn("cumulative_usage_decreased");
            }
            self.previous = Some(next.clone());
            self.usage = next;
        } else if kind == "event_msg" && p["type"] == "task_started" {
            self.bump("task_started");
            Self::stamp(&mut self.turns, &event);
        } else if kind == "event_msg" && p["type"] == "task_complete" {
            self.bump("task_complete");
            if let Some(ms) = p["duration_ms"].as_u64() {
                self.durations.push(ms);
            }
        } else if kind == "event_msg" && p["type"] == "turn_aborted" {
            self.bump("turn_aborted");
        } else if kind == "token_usage_record" {
            let next = usage(&p["usage"]);
            if let Some(id) = identifier(&p["response_id"]) {
                if let Some(previous) = self.responses.get(&id) {
                    if previous != &next {
                        self.responses.insert(id.clone(), unknown_usage());
                        self.conflicts.insert(id);
                        self.warn("conflicting_response_id");
                    }
                } else {
                    self.responses.insert(id.clone(), next.clone());
                    Self::stamp(&mut self.turns, &event);
                    self.turn_usages.push(TurnUsage {
                        turn_id: identifier(&p["turn_id"]),
                        response_id: Some(id),
                        usage: next.clone(),
                        turn_usage: usage_snapshot(&p["turn_token_usage"]),
                        thread_usage: usage_snapshot(&p["thread_token_usage"]),
                    });
                }
            } else {
                self.warn("missing_response_id");
            }
            self.check_usage(&next);
            if next["reasoning_output_tokens"]
                .zip(next["output_tokens"])
                .is_some_and(|(r, o)| r > o)
            {
                self.warn("reasoning_not_included_in_output");
            }
        } else if kind == "response_item" {
            if p["type"] == "message" {
                let text = message_text(p);
                if p["role"] == "developer" {
                    self.instructions.developer_bytes = self
                        .instructions
                        .developer_bytes
                        .saturating_add(text.len() as u64);
                }
                if p["role"] == "user" {
                    let head = prefix(&text, 96);
                    let continuation = prefix(&text, 160);
                    if [
                        "<hook_prompt",
                        "<subagent_notification",
                        "<codex_internal_context",
                    ]
                    .iter()
                    .any(|m| head.starts_with(m))
                    {
                        self.bump("hook_messages");
                        *self.counts.entry("hook_chars").or_default() +=
                            text.chars().count() as u64;
                    } else if head.starts_with("<turn_aborted")
                        || head.starts_with("the user interrupted")
                        || [
                            "resume after tool-host restart",
                            "resume user goal",
                            "your turn ended",
                        ]
                        .iter()
                        .any(|phrase| continuation.contains(phrase))
                    {
                        self.bump("continuation_notices");
                        Self::stamp(&mut self.triggers, &event);
                    } else {
                        self.bump("user_messages");
                        Self::stamp(&mut self.users, &event);
                    }
                } else if p["phase"] == "commentary" {
                    self.bump("commentary_messages");
                } else if p["phase"] == "final_answer" {
                    self.bump("final_messages");
                }
            } else if p["type"] == "function_call" && p["name"] == "spawn_agent" {
                self.bump("spawn_calls");
            }
        } else if kind == "compacted" {
            self.bump("compacted_windows");
        }
        Self::stamp(&mut self.stamps, &event);
        if kind == "event_msg" && p["type"] == "item_completed" {
            for child in list(&p["item"]["receiver_thread_ids"]) {
                if let Some(id) = identifier(child) {
                    self.children.insert(id);
                }
            }
        }
    }

    fn finish(mut self) -> SessionSummary {
        let mut id = self.thread_ids.first().cloned();
        if self.thread_ids.len() > 1 {
            let inherited = |id: &String| {
                self.session_ids.contains(id)
                    || self.parents.contains(id)
                    || self.forked.contains(id)
            };
            if (!self.parents.is_empty() || !self.forked.is_empty())
                && self.thread_ids[1..].iter().all(inherited)
            {
                self.warn("forked_or_compacted_history");
            } else {
                self.warn("conflicting_thread_ids");
                id = None;
            }
        }
        if !self.saw_meta || self.thread_ids.is_empty() {
            self.warn("missing_thread_id");
        }
        if id.is_none() {
            self.usage = unknown_usage();
            self.responses.clear();
            self.children.clear();
            self.turn_usages.clear();
        }
        if self.parents.len() > 1 {
            self.warn("conflicting_parent_ids");
        }
        let model = only(&self.models);
        let mut provider = match model.as_deref() {
            Some("gpt-6-astra" | "openai/gpt-6-astra") => Some("OpenAI"),
            Some("xai/grok-4.6" | "grok-4.6") => Some("xai"),
            _ => None,
        };
        if self.models.len() > 1 {
            self.warn("mixed_model_attribution");
        } else if provider.is_none() {
            self.warn("unsupported_or_missing_model");
        }
        if self.efforts.len() > 1 {
            self.warn("mixed_reasoning");
        }
        if self.efforts.is_empty() {
            self.warn("missing_reasoning");
        }
        if self.usage.values().any(Option::is_none) {
            self.warn("missing_usage");
        }
        if self.warnings.contains("missing_model_context") {
            provider = None;
        }
        let project = if self.projects.len() > 1 {
            self.warn("mixed_project");
            Some("mixed".to_owned())
        } else {
            only(&self.projects)
        };
        if self.counts.get("compacted_windows").is_some_and(|n| *n > 0) {
            self.warn("forked_or_compacted_history");
        }
        if self.counts.get("turn_aborted").is_some_and(|n| *n > 0) {
            self.warn("interrupted_turn");
        }
        let parsed: Vec<_> = self
            .stamps
            .iter()
            .filter_map(|v| timestamp(v).map(|t| (t, v)))
            .collect();
        let first = parsed.iter().min_by_key(|(t, _)| *t);
        let last = parsed.iter().max_by_key(|(t, _)| *t);
        let elapsed =
            (parsed.len() >= 2).then(|| (last.unwrap().0 - first.unwrap().0).num_seconds().max(0));
        let mut row = json!({"id":id,"parent_id":only(&self.parents),"model":model,"reasoning":only(&self.efforts),
            "provider":provider,"missing_usage":self.usage.values().any(Option::is_none),"partial":!self.warnings.is_empty(),
            "project":project,"cli_version":only(&self.versions),"elapsed_seconds":elapsed,
            "response_count":self.responses.len(),"response_ids":self.responses.keys().collect::<Vec<_>>(),
            "response_usages":self.responses,"response_conflict_ids":self.conflicts,"spawned_child_ids":self.children,
            "actual_continuations":continuations(&self.triggers,&self.turns,&self.users),
            "turn_duration_ms_sum":if self.durations.is_empty() { None } else { self.durations.iter().try_fold(0u64,|a,b|a.checked_add(*b)) },
            "first_timestamp":first.map(|(_,v)| *v),"last_timestamp":last.map(|(_,v)| *v)});
        for (key, value) in self.usage {
            row[&key] = json!(value);
        }
        for key in [
            "spawn_calls",
            "hook_messages",
            "hook_chars",
            "continuation_notices",
            "task_started",
            "task_complete",
            "turn_aborted",
            "commentary_messages",
            "final_messages",
            "user_messages",
            "compacted_windows",
        ] {
            row[key] = json!(self.counts.get(key).copied().unwrap_or(0));
        }
        let selected: BTreeMap<_, _> = ["id", "parent_id", "model", "reasoning", "provider"]
            .into_iter()
            .chain(TOKEN_FIELDS)
            .map(|key| (key, row[key].clone()))
            .collect();
        let fingerprint = json!([
            selected,
            self.models,
            self.efforts,
            self.parents,
            self.warnings,
            self.responses
        ]);
        SessionSummary {
            row,
            fingerprint,
            warnings: self.warnings,
            coverage: self.coverage,
            instructions: self.instructions,
            turns: self.turn_usages,
            source: None,
        }
    }
}

/// Reads one rollout session file tolerantly; unreadable input yields warnings.
pub fn read(path: &Path) -> SessionSummary {
    let mut reader = Reader {
        usage: unknown_usage(),
        ..Reader::default()
    };
    let mut held = None;
    match File::open(path) {
        Ok(file) => {
            let mut stream = BufReader::new(file);
            let mut line = Vec::new();
            loop {
                line.clear();
                // A malformed giant line cannot force unbounded allocation.
                let count = (&mut stream)
                    .take(MAX_RECORD_BYTES + 1)
                    .read_until(b'\n', &mut line);
                match count {
                    Ok(0) => break,
                    Ok(_) if line.len() as u64 > MAX_RECORD_BYTES => {
                        reader.warn("oversized_jsonl_record");
                        reader.coverage.oversized_lines += 1;
                        if line.last() != Some(&b'\n') && stream.skip_until(b'\n').is_err() {
                            reader.warn("unreadable_input");
                            break;
                        }
                        continue;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        reader.warn("unreadable_input");
                        break;
                    }
                }
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                reader.coverage.lines += 1;
                match serde_json::from_slice(&line) {
                    Ok(event) => {
                        reader.coverage.events += 1;
                        reader.event(event);
                    }
                    Err(_) => {
                        reader.warn("corrupt_jsonl");
                        reader.coverage.corrupt_lines += 1;
                    }
                }
            }
            held = Some(stream.into_inner());
        }
        Err(_) => reader.warn("unreadable_input"),
    }
    let mut summary = reader.finish();
    summary.source = held;
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write(dir: &Path, name: &str, events: &[Value]) -> PathBuf {
        let path = dir.join(name);
        let text: String = events.iter().map(|event| format!("{event}\n")).collect();
        std::fs::write(&path, text).unwrap();
        path
    }
    fn meta(id: &str) -> Value {
        json!({"type":"session_meta","payload":{"id":id,"session_id":id}})
    }
    fn context() -> Value {
        json!({"type":"turn_context","payload":{"model":"fixture-model","effort":"high"}})
    }
    fn counts(amount: u64) -> Value {
        json!({"input_tokens":amount,"cached_input_tokens":amount/2,"output_tokens":amount/2,"reasoning_output_tokens":amount/5,"total_tokens":amount+amount/2})
    }
    fn record(turn: &str, response: &str, delta: u64, cumulative: u64) -> Value {
        json!({"type":"token_usage_record","payload":{"turn_id":turn,"response_id":response,
            "usage":counts(delta),"turn_token_usage":counts(cumulative),"thread_token_usage":counts(cumulative)}})
    }

    #[test]
    fn older_token_count_format_keeps_the_last_cumulative_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let path = write(
            root.path(),
            "older.jsonl",
            &[
                meta("thread_one"),
                context(),
                json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":counts(10)}}}),
                json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":counts(20)}}}),
            ],
        );
        let session = read(&path);
        assert_eq!(session.row["id"], "thread_one");
        assert_eq!(session.row["input_tokens"], 20);
        assert_eq!(session.row["total_tokens"], 30);
        assert_eq!(session.row["missing_usage"], false);
        assert_eq!(session.coverage.recognized_events, 4);
        assert_eq!(session.coverage.unrecognized_events, 0);
        assert_eq!(session.coverage.corrupt_lines, 0);
        assert!(session.turns.is_empty());
        assert!(session.source.is_some());
    }

    #[test]
    fn newer_usage_records_deduplicate_responses_and_associate_turns() {
        let root = tempfile::tempdir().unwrap();
        let path = write(
            root.path(),
            "newer.jsonl",
            &[
                meta("thread_two"),
                context(),
                record("turn_one", "resp_one", 10, 10),
                record("turn_one", "resp_one", 10, 10),
                record("turn_two", "resp_two", 20, 30),
            ],
        );
        let session = read(&path);
        assert_eq!(session.row["response_count"], 2);
        assert_eq!(
            session.row["response_usages"]["resp_one"]["total_tokens"],
            15
        );
        assert_eq!(
            session.row["response_usages"]["resp_two"]["total_tokens"],
            30
        );
        assert!(
            session.row["response_conflict_ids"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(session.turns.len(), 2);
        assert_eq!(session.turns[0].turn_id.as_deref(), Some("turn_one"));
        assert_eq!(session.turns[0].response_id.as_deref(), Some("resp_one"));
        assert_eq!(
            session.turns[1].turn_usage.as_ref().unwrap()["total_tokens"],
            Some(45)
        );
        assert_eq!(
            session.turns[1].thread_usage.as_ref().unwrap()["input_tokens"],
            Some(30)
        );
        // The newer format carries no thread cumulative snapshot for the row.
        assert_eq!(session.row["missing_usage"], true);
    }

    #[test]
    fn conflicting_response_identity_invalidates_and_missing_turn_identity_stays_explicit() {
        let root = tempfile::tempdir().unwrap();
        let mut no_turn = record("turn_unused", "resp_no_turn", 5, 5);
        no_turn["payload"]
            .as_object_mut()
            .unwrap()
            .remove("turn_id");
        no_turn["payload"]
            .as_object_mut()
            .unwrap()
            .remove("turn_token_usage");
        let mut unnamed = record("turn_unused", "resp_unused", 5, 5);
        unnamed["payload"]
            .as_object_mut()
            .unwrap()
            .remove("response_id");
        let path = write(
            root.path(),
            "conflicts.jsonl",
            &[
                meta("thread_three"),
                context(),
                record("turn_one", "resp_dup", 10, 10),
                record("turn_one", "resp_dup", 20, 20),
                no_turn,
                unnamed,
            ],
        );
        let session = read(&path);
        assert!(
            session.row["response_usages"]["resp_dup"]["total_tokens"].is_null(),
            "conflicting response usage must stay unknown"
        );
        assert!(
            session.row["response_conflict_ids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|id| id == "resp_dup")
        );
        assert!(session.warnings.contains("conflicting_response_id"));
        assert!(session.warnings.contains("missing_response_id"));
        assert_eq!(session.turns.len(), 2);
        assert_eq!(session.turns[1].turn_id, None);
        assert_eq!(session.turns[1].turn_usage, None);
        assert_eq!(session.turns[1].usage["total_tokens"], Some(7));
    }

    #[test]
    fn instruction_bytes_are_measured_without_retaining_text() {
        let root = tempfile::tempdir().unwrap();
        let mut meta = meta("thread_four");
        meta["payload"]["base_instructions"] = "base prompt".into();
        let path = write(
            root.path(),
            "instructions.jsonl",
            &[
                meta,
                context(),
                json!({"type":"response_item","payload":{"type":"message","role":"developer",
                    "content":[{"type":"input_text","text":"developer block"}]}}),
                json!({"type":"response_item","payload":{"type":"message","role":"user",
                    "content":[{"type":"input_text","text":"user request"}]}}),
            ],
        );
        let session = read(&path);
        assert_eq!(session.instructions.base_bytes, 11);
        assert_eq!(session.instructions.developer_bytes, 15);
        assert!(!format!("{:?}", session.row).contains("developer block"));
    }

    #[test]
    fn unknown_and_malformed_lines_count_as_coverage_not_new_warnings() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("coverage.jsonl");
        let text = [
            meta("thread_five").to_string(),
            context().to_string(),
            json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":counts(10)}}}).to_string(),
            json!({"type":"world_state","payload":{"full":true}}).to_string(),
            "not json".to_owned(),
            json!([]).to_string(),
        ]
        .join("\n");
        std::fs::write(&path, text).unwrap();
        let session = read(&path);
        assert_eq!(session.coverage.lines, 6);
        assert_eq!(session.coverage.events, 5);
        assert_eq!(session.coverage.recognized_events, 3);
        assert_eq!(session.coverage.unrecognized_events, 2);
        assert_eq!(session.coverage.corrupt_lines, 1);
        assert_eq!(session.coverage.oversized_lines, 0);
        assert!(session.warnings.contains("corrupt_jsonl"));
        assert!(session.warnings.contains("invalid_event"));
        assert!(!session.warnings.contains("unrecognized_event"));
        assert_eq!(session.row["total_tokens"], 15);
    }

    #[test]
    fn unreadable_input_yields_a_warning_without_a_source_handle() {
        let root = tempfile::tempdir().unwrap();
        let session = read(&root.path().join("missing.jsonl"));
        assert!(session.source.is_none());
        assert!(session.warnings.contains("unreadable_input"));
        assert_eq!(session.coverage, Coverage::default());
        assert_eq!(session.row["id"], Value::Null);
    }
}
