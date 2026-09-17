//! Minimal kit-owned compatibility shim between Codex 0.154 and api.x.ai.
//!
//! Codex 0.154 echoes Responses `reasoning` input items with an explicit
//! `content: null` field; api.x.ai rejects exactly that shape on every
//! follow-up request of a tool turn (see the recorded interop evidence in
//! the `retire-opencodex-keep-xai-profile` change). The shim removes only
//! that field from Responses request bodies and forwards everything else
//! byte-transparently, including streamed responses. It stores no
//! credentials (Authorization passes through), rewrites no protocol,
//! registers no scheduled task, and exits once no `codex.exe` process
//! remains on the host.

use crate::{
    broker_state::BrokerRoot,
    process::{Cancellation, Deadline},
    task_forward::{ForwardRequest, Forwarder},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub const DEFAULT_PORT: u16 = 56122;
pub const DEFAULT_UPSTREAM: &str = "https://api.x.ai";
/// Local control endpoints: the launcher verifies which build owns the port
/// before reusing a running shim, and retires a shim from another build so the
/// selected build (with its fixes) actually serves new sessions.
pub const IDENTITY_PATH: &str = "/__harness/xai-shim/identity";
pub const RETIRE_PATH: &str = "/__harness/xai-shim/retire";
const HEAD_LIMIT: usize = 16 * 1024;
const BODY_LIMIT: usize = 64 * 1024 * 1024;
const MAX_ACTIVE_CONNECTIONS: usize = 16;
const IO_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_DEADLINE: Duration = Duration::from_secs(30 * 60);
const DEFAULT_POLL: Duration = Duration::from_secs(10);
const DEFAULT_GRACE: Duration = Duration::from_secs(15);
const MAX_SSE_LINE: usize = 4 * 1024 * 1024;
/// A retired shim stops accepting immediately and drains in-flight streams for
/// at most this long, so both the port and the process are released promptly.
const RETIRE_DRAIN: Duration = Duration::from_secs(60);

pub struct Options {
    pub port: u16,
    pub upstream: String,
    pub poll_interval: Duration,
    pub startup_grace: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            upstream: DEFAULT_UPSTREAM.to_string(),
            poll_interval: DEFAULT_POLL,
            startup_grace: DEFAULT_GRACE,
        }
    }
}

pub fn run(options: &Options) -> io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", options.port)).map_err(|_| {
        io::Error::other("xai responses shim port unavailable; another shim may be running")
    })?;
    serve(
        listener,
        &options.upstream,
        Arc::new(codex_process_present),
        options.poll_interval,
        options.startup_grace,
    )
}

/// Serve accepted connections until the host no longer needs the shim.
/// `keep_running` is polled only while no request is in flight, so an active
/// stream is never cut by the lifecycle check.
pub(crate) fn serve(
    listener: TcpListener,
    upstream: &str,
    keep_running: Arc<dyn Fn() -> bool + Send + Sync>,
    poll_interval: Duration,
    startup_grace: Duration,
) -> io::Result<()> {
    serve_with_retire(
        listener,
        upstream,
        keep_running,
        poll_interval,
        startup_grace,
        Arc::new(AtomicBool::new(false)),
    )
}

pub(crate) fn serve_with_retire(
    listener: TcpListener,
    upstream: &str,
    keep_running: Arc<dyn Fn() -> bool + Send + Sync>,
    poll_interval: Duration,
    startup_grace: Duration,
    retire: Arc<AtomicBool>,
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let mut listener = Some(listener);
    let active = Arc::new(AtomicUsize::new(0));
    let started = Instant::now();
    let mut next_poll = started + startup_grace;
    loop {
        if retire.load(Ordering::SeqCst) {
            // Release the port at once so the replacing build can bind, then
            // let in-flight streams drain within a bounded wait.
            drop(listener.take());
            let drain_deadline = Instant::now() + RETIRE_DRAIN;
            while active.load(Ordering::SeqCst) > 0 && Instant::now() < drain_deadline {
                thread::sleep(Duration::from_millis(20));
            }
            return Ok(());
        }
        let now = Instant::now();
        if now >= next_poll {
            next_poll = now + poll_interval;
            if active.load(Ordering::SeqCst) == 0 && !keep_running() {
                return Ok(());
            }
        }
        let Some(accepting) = listener.as_ref() else {
            return Ok(());
        };
        match accepting.accept() {
            Ok((stream, _)) => {
                if active.load(Ordering::SeqCst) >= MAX_ACTIVE_CONNECTIONS {
                    let mut stream = stream;
                    let _ = write_simple_response(&mut stream, "503 Service Unavailable");
                    continue;
                }
                active.fetch_add(1, Ordering::SeqCst);
                let upstream = upstream.to_string();
                let active = Arc::clone(&active);
                let retire = Arc::clone(&retire);
                thread::Builder::new()
                    .name("xai-responses-shim".to_string())
                    .spawn(move || {
                        handle_connection(stream, &upstream, &retire);
                        active.fetch_sub(1, Ordering::SeqCst);
                    })?;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

enum ReadOutcome {
    Request(Box<Request>),
    Reject(&'static str),
    Drop,
}

fn handle_connection(mut stream: TcpStream, upstream: &str, retire: &AtomicBool) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let request = match read_request(&mut stream) {
        Ok(ReadOutcome::Request(request)) => request,
        Ok(ReadOutcome::Reject(status)) => {
            let _ = write_simple_response(&mut stream, status);
            return;
        }
        _ => return,
    };
    // Local build-identity control plane: the launcher uses it to reuse only a
    // shim from the selected build and to retire one from a superseded build.
    // Handling happens before the upstream hop so it never reaches api.x.ai.
    if request.method == "GET" && request.target == IDENTITY_PATH {
        let _ = write_json_response(&mut stream, &identity_body());
        return;
    }
    if request.method == "POST" && request.target == RETIRE_PATH {
        retire.store(true, Ordering::SeqCst);
        let body = serde_json::json!({"retiring": true}).to_string();
        let _ = write_json_response(&mut stream, &body);
        return;
    }
    let mut body = request.body;
    let mut adaptation = ToolAdaptation::default();
    if request.method == "POST" && request.target.contains("/responses") {
        adaptation = ToolAdaptation::parse(&body);
        if let Some(adapted) = adapt_request_body(&body, &adaptation) {
            body = adapted;
        }
    }
    let url = format!("{}{}", upstream, request.target);
    let deadline = match Deadline::after(REQUEST_DEADLINE) {
        Ok(deadline) => deadline,
        Err(_) => {
            let _ = write_simple_response(&mut stream, "503 Service Unavailable");
            return;
        }
    };
    let cancel = Cancellation::default();
    let root = BrokerRoot::prepare();
    let forwarder = Forwarder::new();
    let rewriting = adaptation.needs_stream_rewrite();
    let headers = if rewriting {
        request
            .headers
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("accept-encoding"))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        request.headers
    };
    let mut wrote = false;
    let mut sse = SseAdapter::new(adaptation.custom_names(), adaptation.namespace_names());
    let mut framer = ResponseFramer::new(rewriting);
    let mut trace = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::var_os("CODEX_HARNESS_SHIM_TRACE").unwrap_or_default())
        .ok();
    if let Some(trace) = trace.as_mut() {
        let _ = writeln!(
            trace,
            "\n=== request ===\n{}",
            String::from_utf8_lossy(&body)
        );
    }
    let result = match (root, forwarder) {
        (Ok(root), Ok(forwarder)) => forwarder.forward(
            root.root(),
            ForwardRequest {
                url: &url,
                headers: &headers,
                body: &body,
            },
            deadline,
            &cancel,
            |bytes| {
                wrote = true;
                let relayed = framer.feed(bytes);
                let outgoing = sse.feed(&relayed.body);
                let framed = if relayed.raw.starts_with(b"HTTP/") {
                    let mut framed = relayed.raw.clone();
                    framed.extend_from_slice(&framer.reframe(&outgoing));
                    framed
                } else {
                    let mut framed = framer.reframe(&outgoing);
                    framed.extend_from_slice(&relayed.raw);
                    framed
                };
                if let (Some(trace), false) = (trace.as_mut(), framed.is_empty()) {
                    let _ = trace.write_all(&framed);
                }
                if framed.is_empty() {
                    Ok(())
                } else {
                    stream.write_all(&framed)
                }
            },
        ),
        (Err(error), _) | (_, Err(error)) => Err(error),
    };
    if result.is_err() && !wrote {
        let _ = write_simple_response(&mut stream, "502 Bad Gateway");
    }
    let tail = {
        let mut tail = framer.reframe(&sse.finish());
        tail.extend_from_slice(&framer.finish());
        tail
    };
    if !tail.is_empty() {
        let _ = stream.write_all(&tail);
    }
}

fn read_request(stream: &mut TcpStream) -> io::Result<ReadOutcome> {
    let mut head = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(position) = find_double_crlf(&head) {
            break position;
        }
        if head.len() > HEAD_LIMIT {
            return Ok(ReadOutcome::Reject("431 Request Header Fields Too Large"));
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(ReadOutcome::Drop),
            Ok(size) => head.extend_from_slice(&chunk[..size]),
            Err(_) => return Ok(ReadOutcome::Drop),
        }
    };
    let head_text = String::from_utf8_lossy(&head[..head_end]).into_owned();
    let mut lines = head_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Ok(ReadOutcome::Reject("400 Bad Request"));
    };
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Ok(ReadOutcome::Reject("400 Bad Request"));
        };
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }
    let mut content_length = None;
    for (name, value) in &headers {
        let lowered = name.to_ascii_lowercase();
        if lowered == "transfer-encoding" {
            return Ok(ReadOutcome::Reject("411 Length Required"));
        }
        if lowered == "content-length" {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(content_length) = content_length else {
        return Ok(ReadOutcome::Reject("411 Length Required"));
    };
    if content_length > BODY_LIMIT {
        return Ok(ReadOutcome::Reject("413 Content Too Large"));
    }
    let mut body = head[head_end + 4..].to_vec();
    while body.len() < content_length {
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(ReadOutcome::Drop),
            Ok(size) => body.extend_from_slice(&chunk[..size]),
            Err(_) => return Ok(ReadOutcome::Drop),
        }
    }
    if body.len() != content_length {
        return Ok(ReadOutcome::Reject("400 Bad Request"));
    }
    Ok(ReadOutcome::Request(Box::new(Request {
        method: method.to_string(),
        target: target.to_string(),
        headers,
        body,
    })))
}

fn find_double_crlf(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

#[derive(Default)]
pub(crate) struct ToolAdaptation {
    custom: HashSet<String>,
    namespaces: HashMap<String, (String, String)>,
    rewrite_stream: bool,
}

impl ToolAdaptation {
    pub(crate) fn parse(body: &[u8]) -> Self {
        let mut custom = HashSet::new();
        let mut namespaces = HashMap::new();
        let mut rewrite_stream = false;
        let Ok(value) = serde_json::from_slice::<Value>(body) else {
            return Self {
                custom,
                namespaces,
                rewrite_stream,
            };
        };
        if let Some(tools) = value.get("tools").and_then(Value::as_array) {
            rewrite_stream = !tools.is_empty();
            for tool in tools {
                match tool.get("type").and_then(Value::as_str) {
                    Some("custom") => {
                        if let Some(name) = tool.get("name").and_then(Value::as_str) {
                            custom.insert(name.to_string());
                        }
                    }
                    Some("namespace") => {
                        let Some(namespace) = tool.get("name").and_then(Value::as_str) else {
                            continue;
                        };
                        if let Some(subtools) = tool.get("tools").and_then(Value::as_array) {
                            for subtool in subtools {
                                let Some(subname) = subtool.get("name").and_then(Value::as_str)
                                else {
                                    continue;
                                };
                                namespaces.insert(
                                    format!("{namespace}__{subname}"),
                                    (namespace.to_string(), subname.to_string()),
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Self {
            custom,
            namespaces,
            rewrite_stream,
        }
    }

    pub(crate) fn custom_names(&self) -> HashSet<String> {
        self.custom.clone()
    }

    pub(crate) fn namespace_names(&self) -> HashMap<String, (String, String)> {
        self.namespaces.clone()
    }

    pub(crate) fn needs_stream_rewrite(&self) -> bool {
        self.rewrite_stream
    }
}

fn function_description(tool: &Value) -> String {
    let original = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let guidance =
        "Provide the freeform input as the `input` string field of the JSON arguments object.";
    let patch_format = "Patch format (follow exactly): the first line must be exactly `*** Begin Patch` with nothing else on that line, then hunks such as `*** Add File: <path>` followed by `+` lines, or `*** Update File: <path>` with `@@` context and `+`/`-`/space lines, then the final line `*** End Patch`. No extra decoration, no markdown fences.";
    let replaced = original
        .replace(
            "This is a FREEFORM tool, so do not wrap the patch in JSON.",
            &format!("This is a JSON function tool: pass the complete patch text as the `input` string field of the JSON arguments. {patch_format}"),
        )
        .replace(
            "Accepts raw JavaScript source text, not JSON, quoted strings, or markdown code fences.",
            "Pass the raw JavaScript source text as the `input` string field of the JSON arguments.",
        );
    if replaced == original && !original.is_empty() {
        format!("{original}\n{guidance}")
    } else {
        replaced
    }
}

fn custom_tool_to_function(tool: &Value) -> Value {
    serde_json::json!({
        "type": "function",
        "name": tool.get("name").cloned().unwrap_or(Value::Null),
        "description": function_description(tool),
        "strict": false,
        "parameters": {
            "type": "object",
            "properties": {
                "input": { "type": "string", "description": "Freeform tool input text." }
            },
            "required": ["input"],
            "additionalProperties": false
        }
    })
}

/// Apply the known Codex 0.154 <-> api.x.ai request adaptations:
/// - remove `content: null` from echoed reasoning items;
/// - remove `external_web_access` from web_search tool declarations;
/// - translate custom tool declarations and echoed custom call items into the
///   function forms that api.x.ai accepts.
pub(crate) fn adapt_request_body(body: &[u8], adaptation: &ToolAdaptation) -> Option<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(body).ok()?;
    let mut changed = false;
    let mut flattened = Vec::new();
    // Auxiliary Codex requests (compaction, titles) may set tool_choice with
    // an empty tool list; api.x.ai rejects that combination outright.
    let tools_empty = value
        .get("tools")
        .and_then(Value::as_array)
        .is_none_or(|tools| tools.is_empty());
    if tools_empty
        && value.get("tool_choice").is_some()
        && let Some(object) = value.as_object_mut()
        && object.remove("tool_choice").is_some()
    {
        changed = true;
    }
    if let Some(tools) = value.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools.iter_mut() {
            match tool.get("type").and_then(Value::as_str) {
                Some("custom") => {
                    *tool = custom_tool_to_function(tool);
                    changed = true;
                }
                Some("web_search") => {
                    if let Some(object) = tool.as_object_mut()
                        && object.remove("external_web_access").is_some()
                    {
                        changed = true;
                    }
                }
                Some("namespace") => {
                    if let Some(namespace) = tool.get("name").and_then(Value::as_str)
                        && let Some(subtools) = tool.get("tools").and_then(Value::as_array)
                    {
                        for subtool in subtools {
                            let mut flat = subtool.clone();
                            if let Some(subname) = subtool.get("name").and_then(Value::as_str) {
                                flat["name"] = Value::String(format!("{namespace}__{subname}"));
                            }
                            flattened.push(flat);
                        }
                        *tool = Value::Null;
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        if changed {
            tools.retain(|tool| !tool.is_null());
            tools.extend(flattened);
        }
    }
    if let Some(items) = value.get_mut("input").and_then(Value::as_array_mut) {
        for item in items.iter_mut() {
            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                let namespace = item.get("namespace").and_then(Value::as_str);
                if let Some(namespace) = namespace {
                    let namespace = namespace.to_string();
                    let subname = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if let Some(object) = item.as_object_mut() {
                        object.remove("namespace");
                    }
                    item["name"] = Value::String(format!("{namespace}__{subname}"));
                    changed = true;
                }
            }
        }
    }
    if let Some(items) = value.get_mut("input").and_then(Value::as_array_mut) {
        for item in items.iter_mut() {
            match item.get("type").and_then(Value::as_str) {
                Some("reasoning") => {
                    if item.get("content").is_some_and(Value::is_null)
                        && item
                            .as_object_mut()
                            .is_some_and(|object| object.remove("content").is_some())
                    {
                        changed = true;
                    }
                }
                Some("custom_tool_call") => {
                    let input = item
                        .get("input")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let arguments = serde_json::json!({ "input": input }).to_string();
                    let mut rewritten = serde_json::json!({
                        "type": "function_call",
                        "call_id": item.get("call_id").cloned().unwrap_or(Value::Null),
                        "arguments": arguments,
                    });
                    if let Some(name) = item.get("name") {
                        rewritten["name"] = name.clone();
                    }
                    if let Some(id) = item.get("id") {
                        rewritten["id"] = id.clone();
                    }
                    *item = rewritten;
                    changed = true;
                }
                Some("custom_tool_call_output") => {
                    let mut rewritten = serde_json::json!({
                        "type": "function_call_output",
                        "call_id": item.get("call_id").cloned().unwrap_or(Value::Null),
                        "output": item
                            .get("output")
                            .cloned()
                            .unwrap_or(Value::String(String::new())),
                    });
                    if let Some(id) = item.get("id") {
                        rewritten["id"] = id.clone();
                    }
                    *item = rewritten;
                    changed = true;
                }
                _ => {}
            }
        }
    }
    let _ = adaptation;
    if changed {
        serde_json::to_vec(&value).ok()
    } else {
        None
    }
}

fn function_call_to_custom(item: &Value) -> Value {
    let arguments = item.get("arguments").and_then(Value::as_str).unwrap_or("");
    let mut input = serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|value| {
            value
                .get("input")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| arguments.to_string());
    if item.get("name").and_then(Value::as_str) == Some("apply_patch")
        && let Some(normalized) = normalize_patch_markers(&input)
    {
        input = normalized;
    }
    let mut rewritten = serde_json::json!({
        "type": "custom_tool_call",
        "call_id": item.get("call_id").cloned().unwrap_or(Value::Null),
        "input": input,
    });
    if let Some(name) = item.get("name") {
        rewritten["name"] = name.clone();
    }
    if let Some(id) = item.get("id") {
        rewritten["id"] = id.clone();
    }
    rewritten
}

/// Grok frequently decorates the patch markers (`*** Begin Patch ***`,
/// `*** End Patch ***`, `*** End of File ***`); Codex's validator accepts only
/// the undecorated marker lines and otherwise rejects the whole call, costing
/// a full-context retry. Rewrite only those marker lines, keeping the patch
/// body byte-identical; anything else is left untouched.
fn normalize_patch_markers(text: &str) -> Option<String> {
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for (index, raw) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let (line, carriage) = match raw.strip_suffix('\r') {
            Some(line) => (line, "\r"),
            None => (raw, ""),
        };
        let trimmed = line.trim_end();
        let canonical = ["*** Begin Patch", "*** End Patch", "*** End of File"]
            .iter()
            .find(|marker| {
                trimmed.strip_prefix(**marker).is_some_and(|decoration| {
                    !decoration.is_empty() && decoration.chars().all(|c| c == '*' || c == ' ')
                })
            });
        match canonical {
            Some(marker) => {
                changed = true;
                out.push_str(marker);
                out.push_str(carriage);
            }
            None => {
                out.push_str(raw);
            }
        }
    }
    changed.then_some(out)
}

fn serialize_data_line(value: &Value) -> Vec<u8> {
    let mut bytes = b"data: ".to_vec();
    match serde_json::to_vec(value) {
        Ok(encoded) => bytes.extend_from_slice(&encoded),
        Err(_) => bytes.extend_from_slice(b"null"),
    }
    bytes
}

/// Codex 0.154 deserializes integer tool fields with serde integers, which
/// reject JSON numbers written as `30000.0`. Grok emits that shape for
/// timeouts, hwnd values and similar fields; coerce whole numbers so the
/// call is executed instead of retried.
fn coerce_whole_floats(text: &str) -> Option<String> {
    let mut value: Value = serde_json::from_str(text).ok()?;
    if !coerce_whole_float_value(&mut value) {
        return None;
    }
    serde_json::to_string(&value).ok()
}

fn coerce_whole_float_value(value: &mut Value) -> bool {
    match value {
        Value::Number(number) => coerce_whole_float_number(number),
        Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= coerce_whole_float_value(item);
            }
            changed
        }
        Value::Object(fields) => {
            let mut changed = false;
            for item in fields.values_mut() {
                changed |= coerce_whole_float_value(item);
            }
            changed
        }
        _ => false,
    }
}

fn coerce_whole_float_number(number: &mut serde_json::Number) -> bool {
    if number.as_i64().is_some() || number.as_u64().is_some() {
        return false;
    }
    let Some(float) = number.as_f64() else {
        return false;
    };
    if !float.is_finite() || float.fract() != 0.0 {
        return false;
    }
    let coerced = if float >= 0.0 && float <= u64::MAX as f64 {
        serde_json::Number::from(float as u64)
    } else if (i64::MIN as f64..=i64::MAX as f64).contains(&float) {
        serde_json::Number::from(float as i64)
    } else {
        return false;
    };
    *number = coerced;
    true
}

fn coerce_item_arguments(item: &mut Value) -> bool {
    let Some(arguments) = item.get("arguments").and_then(Value::as_str) else {
        return false;
    };
    let Some(coerced) = coerce_whole_floats(arguments) else {
        return false;
    };
    item["arguments"] = Value::String(coerced);
    true
}

/// Decode the curl `--raw` relay just enough to rewrite event lines: response
/// heads pass through untouched; when rewriting is active, chunked framing is
/// decoded before SSE parsing and re-encoded for the client. Non-rewritten
/// streams keep the original framing byte-for-byte.
pub(crate) struct ResponseFramer {
    rewriting: bool,
    pending: Vec<u8>,
    head_done: bool,
    chunked: bool,
    chunk_remaining: usize,
    chunk_started: bool,
    body_passthrough: bool,
    terminal_seen: bool,
    terminal_emitted: bool,
    tail: Vec<u8>,
}

pub(crate) struct FramerOutput {
    pub body: Vec<u8>,
    pub raw: Vec<u8>,
}

impl ResponseFramer {
    pub(crate) fn new(rewriting: bool) -> Self {
        Self {
            rewriting,
            pending: Vec::new(),
            head_done: false,
            chunked: false,
            chunk_remaining: 0,
            chunk_started: false,
            body_passthrough: false,
            terminal_seen: false,
            terminal_emitted: false,
            tail: Vec::new(),
        }
    }

    /// Feed raw relay bytes and return decoded body bytes (empty unless the
    /// stream is being rewritten).
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> FramerOutput {
        self.pending.extend_from_slice(bytes);
        let mut raw = Vec::new();
        if !self.head_done {
            let Some(position) = find_double_crlf(&self.pending) else {
                return FramerOutput {
                    body: Vec::new(),
                    raw: Vec::new(),
                };
            };
            self.head_done = true;
            self.chunked = head_line_chunked(&self.pending[..position]);
            if !self.rewriting || !self.chunked {
                self.body_passthrough = true;
            }
            if self.body_passthrough {
                let drained = std::mem::take(&mut self.pending);
                return FramerOutput {
                    body: Vec::new(),
                    raw: drained,
                };
            }
            raw.extend_from_slice(&self.pending[..position + 4]);
            self.pending.drain(..position + 4);
        }
        if self.body_passthrough {
            return FramerOutput {
                body: std::mem::take(&mut self.pending),
                raw,
            };
        }
        let mut decoded = Vec::new();
        loop {
            if self.pending.is_empty() && self.chunk_started {
                raw.extend_from_slice(&std::mem::take(&mut self.tail));
                return FramerOutput { body: decoded, raw };
            }
            if self.terminal_seen {
                if !self.terminal_emitted {
                    self.terminal_emitted = true;
                    self.tail.extend_from_slice(b"0\r\n");
                }
                self.tail
                    .extend_from_slice(&std::mem::take(&mut self.pending));
                if !raw.starts_with(b"HTTP/") {
                    raw.extend_from_slice(&std::mem::take(&mut self.tail));
                }
                return FramerOutput { body: decoded, raw };
            }
            if !self.chunk_started {
                let Some(line_end) = self.pending.iter().position(|byte| *byte == b'\n') else {
                    return FramerOutput { body: decoded, raw };
                };
                let size_text = String::from_utf8_lossy(&self.pending[..line_end])
                    .trim()
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .to_string();
                let size = match usize::from_str_radix(&size_text, 16) {
                    Ok(size) => size,
                    Err(_) => {
                        self.body_passthrough = true;
                        decoded.extend_from_slice(&std::mem::take(&mut self.pending));
                        return FramerOutput { body: decoded, raw };
                    }
                };
                self.chunk_remaining = size;
                self.chunk_started = true;
                self.pending.drain(..line_end + 1);
                if size == 0 {
                    self.terminal_seen = true;
                }
                continue;
            }
            if self.chunk_remaining > 0 {
                let take = self.chunk_remaining.min(self.pending.len());
                decoded.extend_from_slice(&self.pending[..take]);
                self.pending.drain(..take);
                self.chunk_remaining -= take;
            }
            if self.chunk_remaining == 0 && self.chunk_started {
                if self.pending.len() >= 2 {
                    if self.pending[..2] != *b"\r\n" {
                        self.body_passthrough = true;
                        decoded.extend_from_slice(&std::mem::take(&mut self.pending));
                        return FramerOutput { body: decoded, raw };
                    }
                    self.pending.drain(..2);
                    self.chunk_started = false;
                } else {
                    return FramerOutput { body: decoded, raw };
                }
            }
        }
    }

    /// Flush any remaining buffered bytes (head tail, partial framing).
    pub(crate) fn finish(&mut self) -> Vec<u8> {
        let mut raw = std::mem::take(&mut self.tail);
        raw.extend_from_slice(&std::mem::take(&mut self.pending));
        raw
    }

    /// Re-encode rewritten body bytes with chunked framing when the original
    /// response used it.
    pub(crate) fn reframe(&self, body: &[u8]) -> Vec<u8> {
        if body.is_empty() {
            return Vec::new();
        }
        if self.body_passthrough || !self.rewriting {
            return body.to_vec();
        }
        let mut framed = format!("{:x}\r\n", body.len()).into_bytes();
        framed.extend_from_slice(body);
        framed.extend_from_slice(b"\r\n");
        framed
    }
}

fn head_line_chunked(head: &[u8]) -> bool {
    String::from_utf8_lossy(head)
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("transfer-encoding")
                .then_some(value.trim().to_ascii_lowercase())
        })
        .is_some_and(|value| value.split(',').any(|part| part.trim() == "chunked"))
}

/// Line-buffered SSE rewriter: function calls for tools that Codex declared
/// as custom are turned back into `custom_tool_call` items and their
/// argument-delta events are suppressed; every unrelated line keeps its
/// original bytes.
pub(crate) struct SseAdapter {
    custom: HashSet<String>,
    namespaces: HashMap<String, (String, String)>,
    custom_indexes: HashMap<u64, bool>,
    pending: Vec<u8>,
    held_event: Option<Vec<u8>>,
    swallow_blank: bool,
}

impl SseAdapter {
    pub(crate) fn new(
        custom: HashSet<String>,
        namespaces: HashMap<String, (String, String)>,
    ) -> Self {
        Self {
            custom,
            namespaces,
            custom_indexes: HashMap::new(),
            pending: Vec::new(),
            held_event: None,
            swallow_blank: false,
        }
    }

    fn item_is_custom(&self, item: &Value) -> bool {
        item.get("type").and_then(Value::as_str) == Some("function_call")
            && item
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| self.custom.contains(name))
    }

    fn rewrite_item(&self, item: &Value) -> Option<Value> {
        self.item_is_custom(item)
            .then(|| function_call_to_custom(item))
    }

    fn rewrite_namespaced(&self, item: &Value) -> Option<Value> {
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            return None;
        }
        let name = item.get("name").and_then(Value::as_str)?;
        let (namespace, subname) = self.namespaces.get(name)?;
        let mut rewritten = item.clone();
        rewritten["name"] = Value::String(subname.clone());
        rewritten["namespace"] = Value::String(namespace.clone());
        Some(rewritten)
    }

    fn rewrite_event(&mut self, original: &[u8], event: &str) -> Option<Vec<u8>> {
        let Ok(mut value) = serde_json::from_str::<Value>(event) else {
            return Some(original.to_vec());
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "response.output_item.added" | "response.output_item.done" => {
                let Some(item) = value.get("item").cloned() else {
                    return Some(original.to_vec());
                };
                let index = value
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let mut item = item;
                let mut changed = false;
                if let Some(rewritten) = self.rewrite_namespaced(&item) {
                    item = rewritten;
                    changed = true;
                } else if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    let is_custom = self.item_is_custom(&item);
                    self.custom_indexes.insert(index, is_custom);
                    if is_custom && let Some(rewritten) = self.rewrite_item(&item) {
                        item = rewritten;
                        changed = true;
                    }
                }
                if coerce_item_arguments(&mut item) {
                    changed = true;
                }
                if changed {
                    value["item"] = item;
                    Some(serialize_data_line(&value))
                } else {
                    Some(original.to_vec())
                }
            }
            "response.function_call_arguments.delta" | "response.function_call_arguments.done" => {
                let index = value
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if self.custom_indexes.get(&index).copied().unwrap_or(false) {
                    None
                } else if kind == "response.function_call_arguments.done"
                    && let Some(arguments) = value.get("arguments").and_then(Value::as_str)
                    && let Some(coerced) = coerce_whole_floats(arguments)
                {
                    value["arguments"] = Value::String(coerced);
                    Some(serialize_data_line(&value))
                } else {
                    Some(original.to_vec())
                }
            }
            "response.completed" => {
                let mut changed = false;
                if let Some(output) = value
                    .get_mut("response")
                    .and_then(|response| response.get_mut("output"))
                    .and_then(Value::as_array_mut)
                {
                    for item in output.iter_mut() {
                        if let Some(rewritten) = self
                            .rewrite_item(item)
                            .or_else(|| self.rewrite_namespaced(item))
                        {
                            *item = rewritten;
                            changed = true;
                        }
                        if coerce_item_arguments(item) {
                            changed = true;
                        }
                    }
                }
                if changed {
                    Some(serialize_data_line(&value))
                } else {
                    Some(original.to_vec())
                }
            }
            _ => Some(original.to_vec()),
        }
    }

    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut outgoing = Vec::new();
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > MAX_SSE_LINE && !self.pending.contains(&b'\n') {
            return std::mem::take(&mut self.pending);
        }
        while let Some(position) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = self.pending.split_off(position + 1);
            std::mem::swap(&mut line, &mut self.pending);
            let complete = &line[..line.len() - 1];
            if complete.starts_with(b"event:") {
                self.held_event = Some(complete.to_vec());
                continue;
            }
            if self.swallow_blank && complete.is_empty() {
                self.swallow_blank = false;
                continue;
            }
            let event = complete
                .strip_prefix(b"data: ")
                .map(|data| String::from_utf8_lossy(data).into_owned());
            let rewritten = match event {
                Some(event) => self.rewrite_event(complete, &event),
                None => Some(complete.to_vec()),
            };
            match rewritten {
                Some(mut bytes) => {
                    if let Some(mut held) = self.held_event.take() {
                        held.push(b'\n');
                        outgoing.extend_from_slice(&held);
                    }
                    bytes.push(b'\n');
                    outgoing.extend_from_slice(&bytes);
                }
                None => {
                    self.held_event = None;
                    self.swallow_blank = true;
                }
            }
        }
        outgoing
    }

    pub(crate) fn finish(&mut self) -> Vec<u8> {
        let mut remaining = std::mem::take(&mut self.pending);
        if let Some(mut held) = self.held_event.take() {
            held.push(b'\n');
            let mut combined = held;
            combined.extend_from_slice(&remaining);
            remaining = combined;
        }
        remaining
    }
}

fn write_simple_response(stream: &mut TcpStream, status: &'static str) -> io::Result<()> {
    stream.write_all(
        format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
    )
}

fn write_json_response(stream: &mut TcpStream, body: &str) -> io::Result<()> {
    stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
}

/// Build identity of this shim process: the launcher compares `exe` with the
/// manager executable of the selected build. No credentials or request data.
fn identity_body() -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    serde_json::json!({
        "harness": "xai-responses-shim",
        "schema": 1,
        "pid": std::process::id(),
        "exe": exe,
    })
    .to_string()
}

#[cfg(windows)]
fn codex_process_present() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::K32EnumProcesses;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    const MAX_IDS: usize = 16384;
    let mut ids = vec![0u32; MAX_IDS];
    let mut needed = 0u32;
    let enumerated = unsafe {
        K32EnumProcesses(
            ids.as_mut_ptr(),
            (ids.len() * std::mem::size_of::<u32>()) as u32,
            &mut needed,
        )
    };
    if enumerated == 0 {
        // Fail open: an observation failure must never stop the shim while a
        // Codex session may still depend on it.
        return true;
    }
    let count = (needed as usize / std::mem::size_of::<u32>()).min(MAX_IDS);
    for &pid in &ids[..count] {
        if pid == 0 {
            continue;
        }
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            continue;
        }
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let found =
            unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) } != 0;
        unsafe { CloseHandle(handle) };
        if found {
            let path = String::from_utf16_lossy(&buffer[..length as usize]);
            if image_is_codex(&path) {
                return true;
            }
        }
    }
    false
}

#[cfg(not(windows))]
fn codex_process_present() -> bool {
    // The delivered host is Windows; other platforms keep the shim running
    // rather than silently removing a transport a session may depend on.
    true
}

fn image_is_codex(path: &str) -> bool {
    path.rsplit(['\\', '/'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("codex.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{net::TcpListener, sync::atomic::AtomicBool, sync::mpsc, time::Duration};

    #[test]
    fn sanitize_removes_only_null_reasoning_content() {
        let body = json!({
            "model": "grok-4.6",
            "input": [
                {"type": "message", "role": "user", "content": "hi"},
                {"type": "reasoning", "id": "rs_1", "summary": [], "content": null, "encrypted_content": "blob"},
                {"type": "reasoning", "id": "rs_2", "content": ["kept"], "encrypted_content": "other"},
                {"type": "function_call", "call_id": "c1", "content": null}
            ]
        });
        let adapted = adapt_request_body(
            body.to_string().as_bytes(),
            &ToolAdaptation::parse(body.to_string().as_bytes()),
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&adapted).unwrap();
        let items = value["input"].as_array().unwrap();
        assert!(items[1].get("content").is_none());
        assert_eq!(items[1]["encrypted_content"], "blob");
        assert_eq!(items[1]["id"], "rs_1");
        assert_eq!(items[2]["content"], json!(["kept"]));
        assert!(items[3].get("content").is_some());
    }

    #[test]
    fn sanitize_leaves_unrelated_and_malformed_bodies_untouched() {
        assert!(adapt_request_body(b"not json", &ToolAdaptation::default()).is_none());
        assert!(
            adapt_request_body(
                br#"{"input": {"nested": true}}"#,
                &ToolAdaptation::default()
            )
            .is_none()
        );
        assert!(adapt_request_body(br#"{"input": []}"#, &ToolAdaptation::default()).is_none());
        assert!(
            adapt_request_body(
                br#"{"input": [{"type": "message", "content": null}]}"#,
                &ToolAdaptation::default()
            )
            .is_none()
        );
    }

    #[test]
    fn adapt_removes_tool_choice_when_no_tools_remain() {
        let body = br#"{"model":"grok-4.6","input":[],"tool_choice":"auto"}"#;
        let adapted = adapt_request_body(body, &ToolAdaptation::default()).unwrap();
        let value: Value = serde_json::from_slice(&adapted).unwrap();
        assert!(value.get("tool_choice").is_none());

        let body =
            br#"{"model":"grok-4.6","input":[],"tool_choice":"auto","tools":[{"type":"function","name":"exec_command"}]}"#;
        assert!(
            adapt_request_body(body, &ToolAdaptation::default()).is_none(),
            "tool_choice with tools must pass through unchanged"
        );
    }
    #[test]
    fn custom_tools_and_calls_are_translated_to_function_forms() {
        let body = json!({
            "tools": [
                {"type": "custom", "name": "apply_patch", "description": "The `apply_patch` tool can be used to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON.", "format": {"type": "grammar"}},
                {"type": "web_search", "external_web_access": true},
                {"type": "function", "name": "exec_command"}
            ],
            "input": [
                {"type": "custom_tool_call", "id": "fc_1", "call_id": "c1", "name": "apply_patch", "input": "*** Begin Patch\n*** End Patch\n"},
                {"type": "custom_tool_call_output", "id": "o1", "call_id": "c1", "output": "Done!"}
            ]
        })
        .to_string();
        let adapted =
            adapt_request_body(body.as_bytes(), &ToolAdaptation::parse(body.as_bytes())).unwrap();
        let value: Value = serde_json::from_slice(&adapted).unwrap();
        let tools = value["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "apply_patch");
        assert!(tools[0].get("format").is_none());
        assert!(
            tools[0]["description"]
                .as_str()
                .unwrap()
                .contains("JSON function tool")
        );
        assert_eq!(tools[0]["parameters"]["required"], json!(["input"]));
        assert!(tools[1].get("external_web_access").is_none());
        assert_eq!(tools[2]["type"], "function");
        let items = value["input"].as_array().unwrap();
        assert_eq!(items[0]["type"], "function_call");
        assert_eq!(items[0]["call_id"], "c1");
        let arguments: Value =
            serde_json::from_str(items[0]["arguments"].as_str().unwrap()).unwrap();
        assert!(
            arguments["input"]
                .as_str()
                .unwrap()
                .starts_with("*** Begin Patch")
        );
        assert_eq!(items[1]["type"], "function_call_output");
        assert_eq!(items[1]["output"], "Done!");
    }

    #[test]
    fn sse_adapter_rewrites_custom_calls_and_suppresses_deltas() {
        let mut adapter =
            SseAdapter::new(HashSet::from(["apply_patch".to_string()]), HashMap::new());
        let added = b"event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"c1\",\"name\":\"apply_patch\",\"arguments\":\"\"}}\n\n";
        let delta = b"event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"delta\":\"chunk\"}\n\n";
        let done = b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"c1\",\"name\":\"apply_patch\",\"arguments\":\"{\\\"input\\\":\\\"patch text\\\"}\"}}\n\n";
        let unrelated = b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n";
        let out_added = adapter.feed(added);
        let out_delta = adapter.feed(delta);
        let out_done = adapter.feed(done);
        let out_unrelated = adapter.feed(unrelated);
        assert!(out_delta.is_empty(), "delta must be suppressed");
        assert!(
            String::from_utf8(out_added)
                .unwrap()
                .contains("\"type\":\"custom_tool_call\"")
        );
        let done_text = String::from_utf8(out_done).unwrap();
        assert!(done_text.contains("\"type\":\"custom_tool_call\""));
        assert!(done_text.contains("\"input\":\"patch text\""));
        assert!(!done_text.contains("arguments"));
        assert_eq!(
            String::from_utf8(out_unrelated).unwrap(),
            String::from_utf8(unrelated.to_vec()).unwrap()
        );
    }

    #[test]
    fn sse_adapter_buffers_partial_lines() {
        let mut adapter =
            SseAdapter::new(HashSet::from(["apply_patch".to_string()]), HashMap::new());
        let first = adapter.feed(b"event: x\ndata: {\"type\":\"other\"}\n");
        assert!(
            adapter
                .feed(b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.d")
                .is_empty()
        );
        let rest = adapter.feed(
            br#"one","output_index":0,"item":{"type":"function_call","call_id":"c1","name":"apply_patch","arguments":"{\"input\":\"x\"}"}}

"#,
        );
        assert!(
            String::from_utf8(rest)
                .unwrap()
                .contains("custom_tool_call")
        );
        assert_eq!(
            String::from_utf8(first).unwrap(),
            "event: x\ndata: {\"type\":\"other\"}\n"
        );
    }

    #[test]
    fn namespace_tools_are_flattened_and_calls_are_rewired() {
        let body = json!({
            "tools": [
                {"type": "namespace", "name": "mcp__nuphus", "description": "Desktop automation.", "tools": [
                    {"type": "function", "name": "desktop_screen_size", "description": "Get the screen size.", "strict": false, "parameters": {"type": "object", "properties": {}, "additionalProperties": false}}
                ]},
                {"type": "namespace", "name": "multi_agent_v1", "description": "Agents.", "tools": [
                    {"type": "function", "name": "close_agent", "description": "Close an agent.", "strict": false, "parameters": {"type": "object", "properties": {"target": {"type": "string"}}, "required": ["target"], "additionalProperties": false}}
                ]}
            ],
            "input": [
                {"type": "function_call", "id": "fc_1", "call_id": "c1", "name": "desktop_screen_size", "namespace": "mcp__nuphus", "arguments": "{}"}
            ]
        })
        .to_string();
        let adaptation = ToolAdaptation::parse(body.as_bytes());
        let adapted = adapt_request_body(body.as_bytes(), &adaptation).unwrap();
        let value: Value = serde_json::from_slice(&adapted).unwrap();
        let tools = value["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "mcp__nuphus__desktop_screen_size");
        assert_eq!(tools[0]["parameters"]["additionalProperties"], json!(false));
        assert_eq!(tools[1]["name"], "multi_agent_v1__close_agent");
        let items = value["input"].as_array().unwrap();
        assert_eq!(items[0]["name"], "mcp__nuphus__desktop_screen_size");
        assert!(items[0].get("namespace").is_none());

        let mut adapter = SseAdapter::new(
            HashSet::new(),
            HashMap::from([(
                "mcp__nuphus__desktop_screen_size".to_string(),
                ("mcp__nuphus".to_string(), "desktop_screen_size".to_string()),
            )]),
        );
        let done = b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_9\",\"call_id\":\"c9\",\"name\":\"mcp__nuphus__desktop_screen_size\",\"arguments\":\"{}\"}}\n\n";
        let delta = b"event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"delta\":\"{}\"}\n\n";
        let out_done = adapter.feed(done);
        let out_delta = adapter.feed(delta);
        assert!(!out_delta.is_empty(), "namespaced deltas must pass through");
        let text = String::from_utf8(out_done).unwrap();
        assert!(text.contains("\"name\":\"desktop_screen_size\""));
        assert!(text.contains("\"namespace\":\"mcp__nuphus\""));
    }

    #[test]
    fn namespace_only_tools_enable_stream_rewrite() {
        let body = json!({
            "tools": [
                {"type": "namespace", "name": "mcp__serena", "tools": [
                    {"type": "function", "name": "find_symbol", "parameters": {"type": "object"}}
                ]}
            ]
        })
        .to_string();
        let adaptation = ToolAdaptation::parse(body.as_bytes());
        assert!(adaptation.custom_names().is_empty());
        assert!(adaptation.needs_stream_rewrite());
        assert!(
            adaptation
                .namespace_names()
                .contains_key("mcp__serena__find_symbol")
        );
    }

    #[test]
    fn namespace_only_chunked_stream_rewrites_calls() {
        let body = json!({
            "tools": [
                {"type": "namespace", "name": "mcp__serena", "tools": [
                    {"type": "function", "name": "find_symbol"}
                ]}
            ]
        })
        .to_string();
        let adaptation = ToolAdaptation::parse(body.as_bytes());
        let mut framer = ResponseFramer::new(adaptation.needs_stream_rewrite());
        let mut sse = SseAdapter::new(adaptation.custom_names(), adaptation.namespace_names());
        let event = b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"c1\",\"name\":\"mcp__serena__find_symbol\",\"arguments\":\"{}\"}}\n\n";
        let mut wire =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
        wire.extend_from_slice(format!("{:x}\r\n", event.len()).as_bytes());
        wire.extend_from_slice(event);
        wire.extend_from_slice(b"\r\n0\r\n\r\n");
        let relayed = framer.feed(&wire);
        let outgoing = sse.feed(&relayed.body);
        let mut client = framer.reframe(&outgoing);
        client.extend_from_slice(&relayed.raw);
        client.extend_from_slice(&framer.reframe(&sse.finish()));
        client.extend_from_slice(&framer.finish());
        let text = String::from_utf8(client).unwrap();
        assert!(text.contains("\"namespace\":\"mcp__serena\""));
        assert!(text.contains("\"name\":\"find_symbol\""));
        assert!(!text.contains("mcp__serena__find_symbol"));
    }

    #[test]
    fn whole_number_floats_in_function_arguments_are_coerced() {
        let mut adapter = SseAdapter::new(HashSet::new(), HashMap::new());
        let done = b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"c1\",\"name\":\"exec_command\",\"arguments\":\"{\\\"yield_time_ms\\\":30000.0,\\\"hwnd\\\":78611.0,\\\"ratio\\\":1.5}\"}}\n\n";
        let text = String::from_utf8(adapter.feed(done)).unwrap();
        assert!(
            text.contains("yield_time_ms") && text.contains("30000") && !text.contains("30000.0")
        );
        assert!(text.contains("hwnd") && text.contains("78611") && !text.contains("78611.0"));
        assert!(text.contains("1.5"));

        let mut adapter = SseAdapter::new(
            HashSet::new(),
            HashMap::from([(
                "mcp__nuphus__desktop_window_activate".to_string(),
                (
                    "mcp__nuphus".to_string(),
                    "desktop_window_activate".to_string(),
                ),
            )]),
        );
        let namespaced = b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_2\",\"call_id\":\"c2\",\"name\":\"mcp__nuphus__desktop_window_activate\",\"arguments\":\"{\\\"hwnd\\\":50925.0}\"}}\n\n";
        let text = String::from_utf8(adapter.feed(namespaced)).unwrap();
        assert!(text.contains("\"namespace\":\"mcp__nuphus\""));
        assert!(text.contains("\"name\":\"desktop_window_activate\""));
        assert!(text.contains("50925") && !text.contains("50925.0"));

        let done_args = b"event: response.function_call_arguments.done\ndata: {\"type\":\"response.function_call_arguments.done\",\"output_index\":0,\"arguments\":\"{\\\"timeout_ms\\\":60000.0}\"}\n\n";
        let text = String::from_utf8(adapter.feed(done_args)).unwrap();
        assert!(text.contains("timeout_ms") && text.contains("60000") && !text.contains("60000.0"));
    }

    #[test]
    fn decorated_patch_markers_are_normalized_and_other_lines_kept() {
        let decorated = "*** Begin Patch ***\r\n*** Add File: a.txt\r\n+*** End Patch ***\r\n*** End Patch ***\r\n*** End of File ***";
        let normalized = normalize_patch_markers(decorated).unwrap();
        assert_eq!(
            normalized,
            "*** Begin Patch\r\n*** Add File: a.txt\r\n+*** End Patch ***\r\n*** End Patch\r\n*** End of File"
        );
        assert!(normalize_patch_markers("*** Begin Patch\n*** End Patch").is_none());
        assert!(normalize_patch_markers("*** Add File: one ***\n+text").is_none());
        assert!(
            normalize_patch_markers("*** Begin Patch # oops\n*** End Patch").is_none(),
            "only star/space decoration may be normalized"
        );
        assert!(
            normalize_patch_markers("+*** Begin Patch ***").is_none(),
            "patch body lines must stay untouched"
        );
    }

    #[test]
    fn streamed_patch_with_decorated_markers_reaches_codex_canonical() {
        let mut adapter =
            SseAdapter::new(HashSet::from(["apply_patch".to_string()]), HashMap::new());
        let done = b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"c1\",\"name\":\"apply_patch\",\"arguments\":\"{\\\"input\\\":\\\"*** Begin Patch ***\\\\n*** Add File: a.txt\\\\n+ok\\\\n*** End Patch ***\\\"}\"}}\n\n";
        let text = String::from_utf8(adapter.feed(done)).unwrap();
        let data = text
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("rewritten data line");
        let value: Value = serde_json::from_str(data).unwrap();
        let input = value["item"]["input"].as_str().unwrap();
        assert_eq!(
            input,
            "*** Begin Patch\n*** Add File: a.txt\n+ok\n*** End Patch"
        );
    }

    #[test]
    fn response_framer_decodes_and_reframes_split_chunks() {
        let mut framer = ResponseFramer::new(true);
        let mut sse = SseAdapter::new(HashSet::from(["apply_patch".to_string()]), HashMap::new());
        let done_json = br#"{"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"c1","name":"apply_patch","arguments":"{\"input\":\"x\"}"}}"#;
        let head = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
        let first = [
            b"event: response.output_item.done\ndata: ".as_slice(),
            &done_json[..40],
        ]
        .concat();
        let second = done_json[40..].to_vec();
        let body = [
            first.as_slice(),
            second.as_slice(),
            b"\n\n".as_slice(),
            b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n".as_slice(),
        ]
        .concat();
        // Frame the body so the split point falls inside the JSON line.
        let mut wire = head.to_vec();
        let mut body = body;
        while !body.is_empty() {
            let take = body.len().min(37);
            let chunk = body[..take].to_vec();
            body.drain(..take);
            wire.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
            wire.extend_from_slice(&chunk);
            wire.extend_from_slice(b"\r\n");
        }
        wire.extend_from_slice(b"0\r\n\r\n");
        // Feed in awkward slices: head alone, then mid-chunk boundaries.
        let mut client_stream = Vec::new();
        for size in [head.len(), 13, 61, 7, wire.len()] {
            let take = size.min(wire.len());
            let piece = wire[..take].to_vec();
            wire.drain(..take);
            let relayed = framer.feed(&piece);
            let outgoing = sse.feed(&relayed.body);
            let mut framed = framer.reframe(&outgoing);
            framed.extend_from_slice(&relayed.raw);
            client_stream.extend_from_slice(&framed);
        }
        client_stream.extend_from_slice(&framer.reframe(&sse.finish()));
        client_stream.extend_from_slice(&framer.finish());
        let text = String::from_utf8(client_stream.clone()).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Transfer-Encoding: chunked"));
        assert!(
            text.contains("\"type\":\"custom_tool_call\""),
            "client stream: {text}"
        );
        assert!(text.contains("\"input\":\"x\""));
        // No chunk-size tokens may leak into the JSON lines.
        assert!(!text.contains("\ndata: 25\n"));
        // Decode the re-framed body and validate SSE integrity.
        let body_start = client_stream
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        let mut body = &client_stream[body_start..];
        let mut decoded = Vec::new();
        loop {
            let line_end = body.iter().position(|byte| *byte == b'\n').unwrap();
            let size = usize::from_str_radix(String::from_utf8_lossy(&body[..line_end]).trim(), 16)
                .unwrap();
            body = &body[line_end + 1..];
            if size == 0 {
                break;
            }
            decoded.extend_from_slice(&body[..size]);
            body = &body[size + 2..];
        }
        let decoded_text = String::from_utf8(decoded).unwrap();
        assert!(decoded_text.contains("\"type\":\"custom_tool_call\""));
        assert!(decoded_text.contains("\"input\":\"x\""));
        assert!(decoded_text.ends_with("\n\n"));
    }
    #[test]
    fn codex_image_names_are_matched_without_path_resolution() {
        assert!(image_is_codex(r"C:\tools\codex.exe"));
        assert!(image_is_codex("/usr/bin/CODEX.EXE"));
        assert!(!image_is_codex(r"C:\tools\codex-code-mode-host.exe"));
        assert!(!image_is_codex("codex"));
    }

    fn control_request(port: u16, method: &str, path: &str) -> (String, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .write_all(
                format!(
                    "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .unwrap();
        let mut received = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(size) => received.extend_from_slice(&chunk[..size]),
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&received).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
        (head.to_string(), body.to_string())
    }

    #[test]
    fn control_endpoints_identify_and_retire_without_an_upstream_hop() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let retire = Arc::new(AtomicBool::new(false));
        let worker = {
            let retire = Arc::clone(&retire);
            thread::spawn(move || {
                serve_with_retire(
                    listener,
                    // An upstream that cannot answer proves control requests
                    // never leave the local shim.
                    "http://127.0.0.1:1",
                    Arc::new(|| true),
                    Duration::from_millis(20),
                    Duration::from_secs(30),
                    retire,
                )
                .unwrap();
            })
        };
        let (head, body) = control_request(port, "GET", IDENTITY_PATH);
        assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
        let identity: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(identity["harness"], "xai-responses-shim");
        assert_eq!(identity["schema"], 1);
        let exe = std::env::current_exe()
            .unwrap()
            .canonicalize()
            .unwrap()
            .display()
            .to_string();
        assert_eq!(identity["exe"], exe.as_str());
        assert!(identity["pid"].as_u64().unwrap() > 0);

        let (head, body) = control_request(port, "POST", RETIRE_PATH);
        assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
        assert_eq!(body, "{\"retiring\":true}");
        // The listen socket must be released before the process finishes
        // draining, so the replacing build can bind the port at once.
        let mut released = false;
        for _ in 0..200 {
            if TcpListener::bind(("127.0.0.1", port)).is_ok() {
                released = true;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(released, "retired shim must release its port");
        worker.join().unwrap();
        assert!(retire.load(Ordering::SeqCst));
    }

    #[test]
    fn serve_exits_after_grace_when_no_keeper_and_no_work() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let started = Instant::now();
        serve(
            listener,
            "https://api.x.ai",
            Arc::new(|| false),
            Duration::from_millis(20),
            Duration::from_millis(30),
        )
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn serve_stays_while_keeper_is_alive() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let keeper = Arc::new(AtomicBool::new(true));
        let worker_keeper = Arc::clone(&keeper);
        let worker = thread::spawn(move || {
            serve(
                listener,
                "https://api.x.ai",
                Arc::new(move || worker_keeper.load(Ordering::SeqCst)),
                Duration::from_millis(20),
                Duration::from_millis(30),
            )
            .unwrap();
        });
        thread::sleep(Duration::from_millis(150));
        assert!(!worker.is_finished());
        keeper.store(false, Ordering::SeqCst);
        worker.join().unwrap();
    }

    #[test]
    fn shim_sanitizes_and_streams_a_full_exchange() {
        let upstream = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        let shim = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let shim_port = shim.local_addr().unwrap().port();
        let (release, wait) = mpsc::channel::<()>();
        let server = thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 65536);
            }
            let head = String::from_utf8_lossy(&request).into_owned();
            let length: usize = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap();
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            let value: Value = serde_json::from_slice(&body).unwrap();
            let reasoning = value["input"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["type"] == "reasoning")
                .unwrap();
            assert!(reasoning.get("content").is_none());
            assert_eq!(reasoning["encrypted_content"], "blob");
            let tools = value["tools"].as_array().unwrap();
            assert_eq!(tools[0]["type"], "function");
            assert_eq!(tools[0]["name"], "apply_patch");
            assert!(tools[0].get("format").is_none());
            assert_eq!(tools[0]["parameters"]["required"], json!(["input"]));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            let added = b"event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"c1\",\"name\":\"apply_patch\",\"arguments\":\"\"}}\n\n";
            let delta = b"event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"delta\":\"chunk\"}\n\n";
            let done = b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"c1\",\"name\":\"apply_patch\",\"arguments\":\"{\\\"input\\\":\\\"first-\\\"}\"}}\n\n";
            for event in [&added[..], &delta[..], &done[..]] {
                write!(stream, "{:x}\r\n", event.len()).unwrap();
                stream.write_all(event).unwrap();
                stream.write_all(b"\r\n").unwrap();
            }
            stream.flush().unwrap();
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            let completed = b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_fixture\",\"output\":[{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"c1\",\"name\":\"apply_patch\",\"arguments\":\"{\\\"input\\\":\\\"first-\\\"}\"}]}}\n\n";
            write!(stream, "{:x}\r\n", completed.len()).unwrap();
            stream.write_all(completed).unwrap();
            stream.write_all(b"\r\n").unwrap();
            stream.write_all(b"0\r\n\r\n").unwrap();
        });
        let _worker = thread::spawn(move || {
            serve(
                shim,
                &format!("http://127.0.0.1:{upstream_port}"),
                Arc::new(|| true),
                Duration::from_millis(20),
                Duration::from_secs(30),
            )
            .unwrap();
        });
        let request_body = json!({
            "model": "grok-4.6",
            "tools": [
                {"type": "custom", "name": "apply_patch", "description": "The `apply_patch` tool can be used to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON.", "format": {"type": "grammar"}}
            ],
            "input": [
                {"type": "message", "role": "user", "content": "hi"},
                {"type": "reasoning", "id": "rs_1", "summary": [], "content": null, "encrypted_content": "blob"}
            ]
        })
        .to_string();
        let mut client = TcpStream::connect(("127.0.0.1", shim_port)).unwrap();
        client
            .write_all(
                format!(
                    "POST /v1/responses HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer owned-test-token\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    request_body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        client.write_all(request_body.as_bytes()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut received = Vec::new();
        let mut released = false;
        loop {
            let mut chunk = [0; 256];
            match client.read(&mut chunk) {
                Ok(0) => break,
                Ok(size) => {
                    received.extend_from_slice(&chunk[..size]);
                    if !released
                        && String::from_utf8_lossy(&received).contains("\"custom_tool_call\"")
                    {
                        release.send(()).unwrap();
                        released = true;
                    }
                }
                Err(_) => break,
            }
        }
        assert!(received.starts_with(b"HTTP/1.1 200 OK\r\n"));
        let text = String::from_utf8_lossy(&received);
        assert!(text.contains("\"type\":\"custom_tool_call\""));
        assert!(text.contains("\"input\":\"first-\""));
        assert!(!text.contains("response.function_call_arguments.delta"));
        assert!(text.ends_with("0\r\n\r\n"));
        drop(client);
        server.join().unwrap();
    }

    #[test]
    fn shim_rewrites_a_one_shot_namespaced_call_with_whole_floats() {
        let upstream = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        let shim = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let shim_port = shim.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 65536);
            }
            let head = String::from_utf8_lossy(&request).into_owned();
            let length: usize = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap();
            let already = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap()
                + 4;
            let mut body = request[already..].to_vec();
            while body.len() < length {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                body.push(byte[0]);
            }
            let value: Value = serde_json::from_slice(&body[..length]).unwrap();
            assert_eq!(value["tools"][0]["name"], "mcp__serena__find_symbol");
            let event = b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"c1\",\"name\":\"mcp__serena__find_symbol\",\"arguments\":\"{\\\"hwnd\\\":50925.0}\"}}\n\n";
            let mut wire =
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
                    .to_vec();
            write!(wire, "{:x}\r\n", event.len()).unwrap();
            wire.extend_from_slice(event);
            wire.extend_from_slice(b"\r\n0\r\n\r\n");
            stream.write_all(&wire).unwrap();
        });
        let _worker = thread::spawn(move || {
            serve(
                shim,
                &format!("http://127.0.0.1:{upstream_port}"),
                Arc::new(|| true),
                Duration::from_millis(20),
                Duration::from_secs(30),
            )
            .unwrap();
        });
        let request_body = json!({
            "model": "grok-4.6",
            "tools": [{
                "type": "namespace",
                "name": "mcp__serena",
                "tools": [{"type": "function", "name": "find_symbol", "parameters": {"type": "object"}}]
            }],
            "input": [{"type": "message", "role": "user", "content": "hi"}]
        })
        .to_string();
        let mut client = TcpStream::connect(("127.0.0.1", shim_port)).unwrap();
        client
            .write_all(
                format!(
                    "POST /v1/responses HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    request_body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        client.write_all(request_body.as_bytes()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut received = Vec::new();
        loop {
            let mut chunk = [0; 256];
            match client.read(&mut chunk) {
                Ok(0) => break,
                Ok(size) => received.extend_from_slice(&chunk[..size]),
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&received);
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        assert!(text.contains("\"namespace\":\"mcp__serena\""), "{text}");
        assert!(text.contains("\"name\":\"find_symbol\""), "{text}");
        assert!(!text.contains("mcp__serena__find_symbol"), "{text}");
        assert!(
            text.contains("50925") && !text.contains("50925.0"),
            "{text}"
        );
        drop(client);
        server.join().unwrap();
    }
}
