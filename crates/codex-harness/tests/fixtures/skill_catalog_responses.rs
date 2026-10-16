//! Model-free canned Responses HTTP transport for the skill-catalog TUI probe.
//! Binds 127.0.0.1 with an OS-assigned port. This is a local substitute for the
//! reserved openai provider route, not a live proxy or billed model. It also
//! owns the shared probe-home and recorded-install helpers the catalogue and
//! native lead checks run their owned sessions with.
#![allow(dead_code)] // Each test binary compiles the subset its own cases use.
use harness_core::build_identity;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// `tool_catalogue` is a shared leaf fixture; `control_responses` holds its one
// declaration per test target (clippy::duplicate_mod). Every target including
// this fixture also includes `control_responses`.
pub use crate::control_responses::tool_catalogue;

pub const PROVIDER_ID: &str = "canned_skill_catalog";
pub const API_KEY: &str = "canned-skill-catalog-fixture-key";
pub const API_KEY_ENV: &str = "SKILL_CATALOG_CANNED_API_KEY";
pub const COMPACT_PROMPT: &str = "CANNED_SKILL_CATALOG_COMPACT_PROMPT_DO_NOT_ECHO_LATE_SKILL";
pub const SUMMARY_TEXT: &str = "CANNED_SKILL_CATALOG_SUMMARY_WITHOUT_LATE_SKILL";
pub const SUMMARY_PREFIX: &str = "Another language model started to solve this problem and produced a summary of its thinking process. You also have access to the state of the tools that were used by that language model. Use this to build on the work that has already been done and avoid duplicating work. Here is the summary produced by the other language model, use the information in this summary to assist with your own analysis:";
pub const CALL_ID: &str = "skill-catalog-refresh-exec-1";
pub const USER_PROMPT: &str =
    "Run the outstanding inspection command. Do not create files. Do not mention skill names.";
pub const NEXT_TURN_PROMPT: &str =
    "Next-turn catalogue control. Reply with SKILL_CATALOG_NEXT_TURN_OK.";
pub const UNRELATED_PROMPT: &str =
    "Unrelated request. Reply with SKILL_CATALOG_UNRELATED_OK. Do not mention skills.";
pub const RETIRE_PROMPT: &str = "Retire catalogue control. Reply with SKILL_CATALOG_RETIRE_OK.";
pub const RESUME_PROMPT: &str = "Resume catalogue control. Reply with SKILL_CATALOG_RESUME_OK.";
pub const CHILD_PROMPT: &str =
    "Child catalogue control. Reply with SKILL_CATALOG_CHILD_OK. Do not spawn other agents.";
pub const EARLY_SKILL: &str = "skill_catalog_early_probe";
pub const CHILD_CALL_ID: &str = "skill-catalog-child-exec-1";
/// The probe turn prompt: a non-special prompt so the turn classifies outside
/// the canned turn kinds and can be repeated for bounded settle rounds.
pub const ALL_TOOLS_PROBE_PROMPT: &str = "Canned all-tools settle probe: report the nested tool catalogue. Reply with CANNED_ALL_TOOLS_SETTLE_OK.";
/// The installed CLI asks the model for a thread title after a turn starts;
/// the probe answers that request with a plain message so a title turn never
/// consumes a probe call or masks a probe turn.
const TITLE_PROMPT: &str = "Generate a concise, single-line task title";

const MAX_BODY: usize = 2 * 1024 * 1024;
/// Idle keep-alive bound for one accepted connection: the client may hold
/// a pooled connection across probe rounds, so the bound only stops a
/// socket from outliving the run; it stays longer than any probe.
const CONNECTION_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Debug)]
pub struct CapturedRequest {
    pub seq: usize,
    pub method: String,
    pub path: String,
    pub body: String,
    pub unix_ms: u128,
    pub kind: RequestKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind {
    Models,
    FirstSampling,
    Compact,
    Continuation,
    NextTurn,
    Resume,
    ChildSampling,
    ChildContinuation,
    Unrelated,
    RetireTurn,
    SkillBody,
    Other,
}

struct HttpRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

struct Inner {
    stop: AtomicBool,
    next_seq: AtomicUsize,
    requests: Mutex<Vec<CapturedRequest>>,
    root: PathBuf,
    short_exec: AtomicBool,
    spawn_child: AtomicBool,
    all_tools_probe: AtomicBool,
    probe_seq: AtomicUsize,
    pending_probe: Mutex<Option<String>>,
    skill_read: Mutex<Option<String>>,
    skill_exec: Mutex<Option<String>>,
}

pub struct CannedResponses {
    pub addr: SocketAddr,
    inner: Arc<Inner>,
    thread: Option<JoinHandle<()>>,
}

impl CannedResponses {
    pub fn spawn(root: &Path) -> io::Result<Self> {
        Self::spawn_mode(root, false, false)
    }

    pub fn spawn_short(root: &Path) -> io::Result<Self> {
        Self::spawn_mode(root, true, false)
    }

    pub fn spawn_child(root: &Path) -> io::Result<Self> {
        Self::spawn_mode(root, false, true)
    }

    fn spawn_mode(root: &Path, short_exec: bool, spawn_child: bool) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        std::fs::create_dir_all(root.join("http"))?;
        let inner = Arc::new(Inner {
            stop: AtomicBool::new(false),
            next_seq: AtomicUsize::new(1),
            requests: Mutex::new(Vec::new()),
            root: root.to_path_buf(),
            short_exec: AtomicBool::new(short_exec),
            spawn_child: AtomicBool::new(spawn_child),
            all_tools_probe: AtomicBool::new(false),
            probe_seq: AtomicUsize::new(0),
            pending_probe: Mutex::new(None),
            skill_read: Mutex::new(None),
            skill_exec: Mutex::new(None),
        });
        let worker = Arc::clone(&inner);
        let thread = thread::spawn(move || accept_loop(listener, worker));
        Ok(Self {
            addr,
            inner,
            thread: Some(thread),
        })
    }

    /// Turns every probe turn into one code-mode `exec` call that reports the
    /// runtime `ALL_TOOLS` names between markers, so the caller observes the
    /// effective nested catalogue at the session's registration boundary
    /// instead of the static first-request schemas. The follow-up request
    /// that carries the exec output is answered with a plain message.
    pub fn spawn_all_tools(root: &Path) -> io::Result<Self> {
        let canned = Self::spawn_mode(root, true, false)?;
        canned.inner.all_tools_probe.store(true, Ordering::SeqCst);
        Ok(canned)
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.addr.port())
    }

    pub fn requests(&self) -> Vec<CapturedRequest> {
        self.inner.requests.lock().unwrap().clone()
    }

    pub fn wait_for(&self, timeout: Duration, pred: impl Fn(&[CapturedRequest]) -> bool) -> bool {
        let until = Instant::now() + timeout;
        while Instant::now() < until {
            if pred(&self.requests()) {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        pred(&self.requests())
    }

    pub fn set_skill_read(&self, path: &Path) {
        *self.inner.skill_read.lock().unwrap() = Some(path.to_string_lossy().into_owned());
    }

    pub fn set_exec_cmd(&self, cmd: impl Into<String>) {
        *self.inner.skill_exec.lock().unwrap() = Some(cmd.into());
    }
}

impl Drop for CannedResponses {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        if let Ok(stream) = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200)) {
            let _ = stream.shutdown(Shutdown::Both);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One line per accepted connection, request, response and read error,
/// appended to the fixture root's `connections.log`: the local socket
/// exchange is the only place where a client-side send failure can be told
/// apart from a request the fixture never received.
fn connection_log(inner: &Inner, message: &str) {
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(inner.root.join("connections.log"))
    {
        let _ = writeln!(file, "{unix_ms} {message}");
    }
}

fn accept_loop(listener: TcpListener, inner: Arc<Inner>) {
    while !inner.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => {
                connection_log(&inner, &format!("accept peer={peer}"));
                let inner = Arc::clone(&inner);
                thread::spawn(move || {
                    let _ = handle_connection(stream, inner);
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                // A transient accept failure, such as a client aborting
                // during the handshake, must not end the listener while the
                // session still runs: the next sampling request would then
                // fail as rejected instead of reaching this fixture. Service
                // continues until the explicit stop; the error stays visible
                // for diagnosis.
                eprintln!("canned responses accept error: {error}");
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

fn handle_connection(mut stream: TcpStream, inner: Arc<Inner>) -> io::Result<()> {
    let peer = stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "unknown".to_owned());
    // A Windows accept inherits the non-blocking listener mode; in that
    // mode every read after the first response returns immediately and the
    // keep-alive loop below would close each connection after one request.
    // Blocking mode lets the timeouts below bound idle waits instead.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(CONNECTION_IDLE_TIMEOUT))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    loop {
        let request = match read_http(&mut stream) {
            Ok(request) => request,
            Err(error) => {
                connection_log(
                    &inner,
                    &format!(
                        "read-error peer={peer} kind={:?} error={error}",
                        error.kind()
                    ),
                );
                return Ok(());
            }
        };
        let method = request.method;
        let path = request.path;
        let headers = request.headers;
        let body = request.body;
        // HTTP/1.1 keeps one connection alive unless the client asks to
        // close. A client that reuses it for its next request is then served
        // here instead of writing into a closed peer.
        let close = headers
            .get("connection")
            .is_some_and(|value| value.eq_ignore_ascii_case("close"));
        connection_log(
            &inner,
            &format!("request peer={peer} method={method} path={path} close={close}"),
        );
        if method == "POST"
            && headers
                .get("expect")
                .is_some_and(|value| value.eq_ignore_ascii_case("100-continue"))
        {
            stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        }
        let body_text = String::from_utf8_lossy(&body).into_owned();
        let kind = classify(&method, &path, &body_text, &inner);
        let seq = inner.next_seq.fetch_add(1, Ordering::SeqCst);
        let captured = CapturedRequest {
            seq,
            method: method.clone(),
            path: path.clone(),
            body: if body_text.len() > MAX_BODY {
                body_text[..MAX_BODY].to_string()
            } else {
                body_text.clone()
            },
            unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            kind,
        };
        persist_request(&inner.root, &captured);
        inner.requests.lock().unwrap().push(captured);
        let (status, content_type, payload) =
            response_for(&method, &path, kind, &body_text, &inner);
        let result = write_response(&mut stream, status, content_type, payload.as_bytes(), close);
        connection_log(
            &inner,
            &format!(
                "response peer={peer} status={status} bytes={} result={:?}",
                payload.len(),
                result.as_ref().err().map(io::Error::kind)
            ),
        );
        result?;
        if close {
            return Ok(());
        }
    }
}

fn classify(method: &str, path: &str, body: &str, inner: &Inner) -> RequestKind {
    let path = path.split('?').next().unwrap_or(path);
    if method == "GET" && path.ends_with("/models") {
        return RequestKind::Models;
    }
    if method != "POST" || !path.contains("responses") {
        return RequestKind::Other;
    }
    if body.contains(COMPACT_PROMPT) {
        RequestKind::Compact
    } else if body.contains(RETIRE_PROMPT) {
        RequestKind::RetireTurn
    } else if body.contains(UNRELATED_PROMPT) {
        RequestKind::Unrelated
    } else if body.contains(RESUME_PROMPT) {
        RequestKind::Resume
    } else if body.contains(NEXT_TURN_PROMPT) {
        let already = inner
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|req| req.kind == RequestKind::NextTurn);
        if already {
            RequestKind::SkillBody
        } else {
            RequestKind::NextTurn
        }
    } else if body.contains(CHILD_PROMPT) {
        let requests = inner.requests.lock().unwrap();
        let had_sample = requests
            .iter()
            .any(|req| req.kind == RequestKind::ChildSampling);
        let had_cont = requests
            .iter()
            .any(|req| req.kind == RequestKind::ChildContinuation);
        if had_cont {
            RequestKind::SkillBody
        } else if had_sample {
            RequestKind::ChildContinuation
        } else {
            RequestKind::ChildSampling
        }
    } else if body.contains(SUMMARY_TEXT) || body.contains(SUMMARY_PREFIX) {
        let already = inner
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|req| req.kind == RequestKind::Continuation);
        if already {
            RequestKind::SkillBody
        } else {
            RequestKind::Continuation
        }
    } else if body.contains(USER_PROMPT) {
        let requests = inner.requests.lock().unwrap();
        let had_first = requests
            .iter()
            .any(|req| req.kind == RequestKind::FirstSampling);
        let had_cont = requests
            .iter()
            .any(|req| req.kind == RequestKind::Continuation);
        if had_cont {
            RequestKind::SkillBody
        } else if had_first {
            RequestKind::Continuation
        } else {
            RequestKind::FirstSampling
        }
    } else {
        RequestKind::Other
    }
}

fn response_for(
    method: &str,
    path: &str,
    kind: RequestKind,
    body: &str,
    inner: &Inner,
) -> (&'static str, &'static str, String) {
    let path = path.split('?').next().unwrap_or(path);
    if method == "GET" && path.ends_with("/models") {
        return (
            "200 OK",
            "application/json",
            json!({
                "object": "list",
                "data": [{"id": "gpt-6-astra", "object": "model", "owned_by": "canned"}]
            })
            .to_string(),
        );
    }
    if method != "POST" || !path.contains("responses") {
        return (
            "404 Not Found",
            "application/json",
            json!({"error": {"message": "canned skill catalog fixture has no such route"}})
                .to_string(),
        );
    }
    if inner.all_tools_probe.load(Ordering::SeqCst) && body.contains(TITLE_PROMPT) {
        return (
            "200 OK",
            "text/event-stream",
            message_sse("all-tools-title", "All-tools probe session", 200),
        );
    }
    let probing = inner.all_tools_probe.load(Ordering::SeqCst)
        && (kind == RequestKind::FirstSampling
            || (kind == RequestKind::Other && body.contains(ALL_TOOLS_PROBE_PROMPT)));
    if probing {
        let mut pending = inner.pending_probe.lock().unwrap();
        if let Some(call_id) = pending.clone()
            && probe_output_present(body, &call_id)
        {
            *pending = None;
            return (
                "200 OK",
                "text/event-stream",
                message_sse(
                    &format!("all-tools-done-{call_id}"),
                    "CANNED_ALL_TOOLS_DONE",
                    400,
                ),
            );
        }
        let call_id = format!(
            "{}{}",
            tool_catalogue::PROBE_CALL_PREFIX,
            inner.probe_seq.fetch_add(1, Ordering::SeqCst) + 1
        );
        *pending = Some(call_id.clone());
        return ("200 OK", "text/event-stream", all_tools_probe_sse(&call_id));
    }
    let sse = match kind {
        RequestKind::Compact => compact_sse(),
        RequestKind::Continuation => {
            live_read_or_message(inner, "cont-1", "SKILL_CATALOG_CONTINUATION_OK", 1200)
        }
        RequestKind::NextTurn => {
            live_read_or_message(inner, "next-1", "SKILL_CATALOG_NEXT_TURN_OK", 800)
        }
        RequestKind::SkillBody => message_sse("skill-body-1", "SKILL_CATALOG_SKILL_BODY_OK", 800),
        RequestKind::Unrelated => message_sse("unrelated-1", "SKILL_CATALOG_UNRELATED_OK", 400),
        RequestKind::RetireTurn => message_sse("retire-1", "SKILL_CATALOG_RETIRE_OK", 400),
        RequestKind::Resume => message_sse("resume-1", "SKILL_CATALOG_RESUME_OK", 800),
        RequestKind::ChildSampling => child_sampling_sse(),
        RequestKind::ChildContinuation => {
            live_read_or_message(inner, "child-cont-1", "SKILL_CATALOG_CHILD_OK", 800)
        }
        RequestKind::FirstSampling | RequestKind::Other => first_sampling_sse(
            inner.short_exec.load(Ordering::SeqCst),
            inner.spawn_child.load(Ordering::SeqCst),
        ),
        RequestKind::Models => unreachable!(),
    };
    ("200 OK", "text/event-stream", sse)
}

fn first_sampling_sse(short_exec: bool, spawn_child: bool) -> String {
    if spawn_child {
        let arguments = serde_json::to_string(&json!({
            "fork_context": false,
            "message": CHILD_PROMPT
        }))
        .unwrap();
        return sse(&[
            json!({"type": "response.created", "response": {"id": "resp-first"}}),
            json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "function_call",
                    "call_id": "skill-catalog-spawn-1",
                    "namespace": "multi_agent_v1",
                    "name": "spawn_agent",
                    "arguments": arguments
                }
            }),
            completed("resp-first", 1_200),
        ]);
    }
    let (cmd, yield_ms, tokens) = if short_exec {
        ("ping -n 4 127.0.0.1", 3000, 1_200)
    } else {
        ("ping -n 40 127.0.0.1", 30000, 250_000)
    };
    sse(&[
        json!({"type": "response.created", "response": {"id": "resp-first"}}),
        json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "call_id": CALL_ID,
                "name": "exec_command",
                "arguments": format!("{{\"cmd\":\"{cmd}\",\"yield_time_ms\":{yield_ms}}}")
            }
        }),
        completed("resp-first", tokens),
    ])
}

fn child_sampling_sse() -> String {
    sse(&[
        json!({"type": "response.created", "response": {"id": "resp-child"}}),
        json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "call_id": CHILD_CALL_ID,
                "name": "exec_command",
                "arguments": "{\"cmd\":\"ping -n 8 127.0.0.1\",\"yield_time_ms\":7000}"
            }
        }),
        completed("resp-child", 1_200),
    ])
}

/// The follow-up request carries the exec output item once the session ran the
/// probe script; the item is matched by the call id this fixture issued.
fn probe_output_present(body: &str, call_id: &str) -> bool {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value.get("input").and_then(Value::as_array).cloned())
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(Value::as_str) == Some("custom_tool_call_output")
                    && item.get("call_id").and_then(Value::as_str) == Some(call_id)
            })
        })
}

/// One code-mode `exec` call that reports the session's own runtime tool
/// names between markers: the keys of the documented global `tools` object
/// plus the `ALL_TOOLS` metadata entries, which name the enabled nested tools
/// including deferred ones the static request schemas omit. Each call gets a
/// distinct call and response id, because the runtime catalogue may change
/// between calls.
fn all_tools_probe_sse(call_id: &str) -> String {
    let response_id = format!("resp-{call_id}");
    let input = tool_catalogue::all_tools_probe_script();
    sse(&[
        json!({"type": "response.created", "response": {"id": response_id.clone()}}),
        json!({
            "type": "response.output_item.done",
            "item": {
                "type": "custom_tool_call",
                "call_id": call_id,
                "name": "exec",
                "input": input
            }
        }),
        completed(&response_id, 800),
    ])
}

fn self_skill_read(inner: &Inner) -> Option<String> {
    inner.skill_read.lock().unwrap().clone()
}

fn live_read_or_message(inner: &Inner, id: &str, text: &str, tokens: i64) -> String {
    if let Some(cmd) = inner.skill_exec.lock().unwrap().clone() {
        skill_exec_sse(&cmd)
    } else if let Some(path) = self_skill_read(inner) {
        skill_read_sse(&path)
    } else {
        message_sse(id, text, tokens)
    }
}

fn skill_exec_sse(cmd: &str) -> String {
    let arguments = serde_json::to_string(&json!({
        "cmd": cmd,
        "yield_time_ms": 8000
    }))
    .unwrap();
    sse(&[
        json!({"type": "response.created", "response": {"id": "resp-skill-exec"}}),
        json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "call_id": "skill-catalog-identity-cmd-1",
                "name": "exec_command",
                "arguments": arguments
            }
        }),
        completed("resp-skill-exec", 800),
    ])
}

fn skill_read_sse(path: &str) -> String {
    let arguments = serde_json::to_string(&json!({
        "cmd": format!("Get-Content -LiteralPath '{}'", path.replace('\'', "''")),
        "yield_time_ms": 5000
    }))
    .unwrap();
    sse(&[
        json!({"type": "response.created", "response": {"id": "resp-skill-read"}}),
        json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "call_id": "skill-catalog-identity-read-1",
                "name": "exec_command",
                "arguments": arguments
            }
        }),
        completed("resp-skill-read", 800),
    ])
}

fn compact_sse() -> String {
    sse(&[
        json!({"type": "response.created", "response": {"id": "resp-compact"}}),
        json!({
            "type": "response.output_item.done",
            "item": {
                "type": "message",
                "role": "assistant",
                "id": "msg-compact",
                "content": [{"type": "output_text", "text": SUMMARY_TEXT}]
            }
        }),
        completed("resp-compact", 900),
    ])
}

fn message_sse(id: &str, text: &str, tokens: i64) -> String {
    sse(&[
        json!({"type": "response.created", "response": {"id": id}}),
        json!({
            "type": "response.output_item.done",
            "item": {
                "type": "message",
                "role": "assistant",
                "id": format!("msg-{id}"),
                "content": [{"type": "output_text", "text": text}]
            }
        }),
        completed(id, tokens),
    ])
}

fn completed(id: &str, total_tokens: i64) -> Value {
    json!({
        "type": "response.completed",
        "response": {
            "id": id,
            "usage": {
                "input_tokens": total_tokens,
                "input_tokens_details": null,
                "output_tokens": 0,
                "output_tokens_details": null,
                "total_tokens": total_tokens
            }
        }
    })
}

fn sse(events: &[Value]) -> String {
    let mut out = String::new();
    for event in events {
        let kind = event["type"].as_str().unwrap_or("message");
        out.push_str("event: ");
        out.push_str(kind);
        out.push('\n');
        out.push_str("data: ");
        out.push_str(&event.to_string());
        out.push_str("\n\n");
    }
    out
}

fn persist_request(root: &Path, captured: &CapturedRequest) {
    let name = format!("{:04}-{:?}.json", captured.seq, captured.kind);
    let payload = json!({
        "seq": captured.seq,
        "method": captured.method,
        "path": captured.path,
        "kind": format!("{:?}", captured.kind),
        "unix_ms": captured.unix_ms,
        "body": captured.body,
    });
    let _ = std::fs::write(
        root.join("http").join(name),
        serde_json::to_vec_pretty(&payload).unwrap_or_default(),
    );
}

fn read_http(stream: &mut TcpStream) -> io::Result<HttpRequest> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "empty HTTP request",
            ));
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
        if buf.len() > 64 * 1024 {
            return Err(io::Error::other("HTTP headers exceed bound"));
        }
    };
    let header_text = std::str::from_utf8(&buf[..header_end])
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let mut body = buf[header_end + 4..].to_vec();
    let length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return Err(io::Error::other("HTTP body exceeds bound"));
    }
    while body.len() < length {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(length);
    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    close: bool,
) -> io::Result<()> {
    let connection = if close { "Connection: close\r\n" } else { "" };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n{connection}\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

// ---------------------------------------------------------------------------
// Owned probe-home helpers shared by the catalogue and native lead checks
// ---------------------------------------------------------------------------

/// One probe home's configuration: the canned provider of this run, the bound
/// model, provider and reasoning effort, an owned auth file and the trusted
/// working directory, plus one caller-supplied configuration layer (the
/// registered unrelated MCP server).
pub fn write_probe_config(
    home: &Path,
    workspace: &Path,
    canned: &CannedResponses,
    extra: &str,
    compact_prompt: bool,
) {
    let trusted = serde_json::to_string(&workspace.to_str().unwrap().to_lowercase()).unwrap();
    let compact = if compact_prompt {
        format!("compact_prompt = \"{COMPACT_PROMPT}\"\n")
    } else {
        String::new()
    };
    fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "gpt-6-astra"
model_provider = "{PROVIDER_ID}"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"
model_auto_compact_token_limit = 1000000
model_context_window = 20000
{compact}[model_providers.{PROVIDER_ID}]
name = "Canned skill catalog Responses"
base_url = "{}"
env_key = "{API_KEY_ENV}"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
[windows]
sandbox = "unelevated"
[features]
hooks = false
apps = false
multi_agent = false
memories = false
goals = false
plugins = true
skill_search = true
[projects.{trusted}]
trust_level = "trusted"
{extra}
"#,
            canned.base_url()
        ),
    )
    .unwrap();
    fs::write(
        home.join("auth.json"),
        serde_json::to_vec_pretty(&json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": API_KEY
        }))
        .unwrap(),
    )
    .unwrap();
}

/// Every distinct runtime catalogue reading carried by `requests`, in arrival
/// order: the probe call id the fixture issued and the runtime tool names the
/// session reported through that code-mode `exec` call. Later requests repeat
/// earlier conversation items, so readings are deduplicated by call id; a new
/// entry means the session executed a newer probe call.
pub fn probe_readings(requests: &[CapturedRequest]) -> Vec<(String, Vec<String>)> {
    let mut readings: Vec<(String, Vec<String>)> = Vec::new();
    for request in requests {
        let Ok(body) = serde_json::from_str::<Value>(&request.body) else {
            continue;
        };
        let Some(items) = body.get("input").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            if item.get("type").and_then(Value::as_str) != Some("custom_tool_call_output") {
                continue;
            }
            let Some(call_id) = item.get("call_id").and_then(Value::as_str) else {
                continue;
            };
            if !call_id.starts_with(tool_catalogue::PROBE_CALL_PREFIX)
                || readings.iter().any(|(seen, _)| seen == call_id)
            {
                continue;
            }
            if let Some(names) = item.get("output").and_then(tool_catalogue::probe_names) {
                readings.push((call_id.to_owned(), names));
            }
        }
    }
    readings
}

/// Registers the first-party probe fixture as a real unrelated stdio MCP
/// server inside one probe home: the session must start the server, discover
/// its tools and keep every one of them while the competing native worktree
/// surface is suppressed. Returns the `[mcp_servers]` configuration layer
/// naming the copied fixture, whose sibling receipt records the handshake.
pub fn unrelated_probe_mcp(home: &Path) -> String {
    let directory = home.join("mcp-probe");
    fs::create_dir_all(&directory).unwrap();
    let executable = directory.join("harness-mcp-probe-fixture.exe");
    fs::copy(env!("CARGO_BIN_EXE_harness-mcp-probe-fixture"), &executable).unwrap();
    fs::write(
        executable.with_extension("json"),
        serde_json::to_vec(&json!({"mode": "valid", "kind": "nuphus"})).unwrap(),
    )
    .unwrap();
    format!(
        "[mcp_servers.unrelated_probe]\ncommand = {}\n",
        serde_json::to_string(&executable.to_string_lossy()).unwrap()
    )
}

/// The copied probe fixture's own handshake receipt inside one probe home:
/// the fixture rewrites it beside its copied executable for every method it
/// receives, so one receipt file serves a whole probe home.
pub fn probe_receipt_path(home: &Path) -> PathBuf {
    home.join("mcp-probe")
        .join("harness-mcp-probe-fixture.receipt.json")
}

/// The copied fixture's receipt once it records a complete handshake. One
/// receipt file is shared per probe home, so a caller that needs fresh
/// per-session evidence clears it before the session and waits here after.
pub fn wait_for_probe_receipt(home: &Path) -> Option<Value> {
    let receipt_path = probe_receipt_path(home);
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        let complete = fs::read(&receipt_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        if let Some(value) = complete {
            let handshaken = value["calls"]
                .as_array()
                .is_some_and(|calls| calls.iter().any(|call| call.as_str() == Some("tools/list")));
            if handshaken {
                return Some(value);
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    None
}

/// Clears any receipt an earlier probe session left behind, so the next
/// session's handshake is proven by a fresh file rather than its
/// predecessor's. A failed removal cannot count as fresh evidence.
pub fn clear_probe_receipt(home: &Path) {
    let path = probe_receipt_path(home);
    if let Err(error) = fs::remove_file(&path) {
        assert_eq!(
            error.kind(),
            io::ErrorKind::NotFound,
            "cannot clear the previous MCP handshake receipt at {}: {error}",
            path.display()
        );
    }
}

/// Asserts one real handshake receipt records the initialize/tools-list
/// exchange: the fixture's own evidence that a session reached it as a real
/// stdio MCP server rather than merely echoing its registration.
pub fn assert_probe_receipt(receipt: &Value, label: &str) {
    let calls = receipt["calls"]
        .as_array()
        .expect("the MCP server receipt records its received methods")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    for method in ["initialize", "notifications/initialized", "tools/list"] {
        assert!(
            calls.contains(&method),
            "{label}: the unrelated MCP server must record its real {method} handshake: {calls:?}"
        );
    }
}

/// The recorded managed installation one runtime-catalogue route runs
/// against: this checkout's launcher binary under a recorded build whose
/// source root is this checkout, forwarding to the real installed native CLI.
/// The registration lives only in the probe's own home; no global
/// installation, deployment or configuration is read or changed.
pub struct ManagedInstall {
    pub launcher: PathBuf,
    pub upstream: PathBuf,
    pub build: PathBuf,
    pub cpu_account: PathBuf,
}

/// Records one native build in one exact directory whose `codex.exe` is this
/// checkout's real launcher binary, so a spawned launcher passes its own
/// selected-build check; the remaining recorded binaries are integrity stubs.
/// The record's source root is this checkout, so the launcher reads the live
/// kit manifest, profile and role inputs exactly as a managed session would.
/// The directory is the caller's choice, because `lead start` launches
/// `<home>/harness/bin/codex.exe` and the registration must select that same
/// directory.
pub fn managed_launcher_build_in(build: &Path, source: &Path) -> PathBuf {
    fs::create_dir_all(build).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_codex"), build.join("codex.exe")).unwrap();
    for binary in build_identity::BINARIES {
        let path = build.join(binary);
        if !path.exists() {
            fs::write(&path, binary.as_bytes()).unwrap();
        }
    }
    let record = build_identity::BuildRecord {
        schema: build_identity::SCHEMA,
        source_root: source.to_path_buf(),
        source: build_identity::source_identity(source).unwrap(),
        rustc: "fixture".into(),
        cargo: "fixture".into(),
        target: "x86_64-pc-windows-msvc".into(),
        profile: "release".into(),
        binaries: build_identity::BINARIES
            .iter()
            .map(|name| {
                (
                    (*name).to_string(),
                    build_identity::hash_file(&build.join(name)).unwrap(),
                )
            })
            .collect(),
    };
    fs::write(
        build.join("build.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    build.to_path_buf()
}

/// Records one native build under the evidence root's `native-build`
/// directory.
pub fn managed_launcher_build(evidence: &Path, source: &Path) -> PathBuf {
    managed_launcher_build_in(&evidence.join("native-build"), source)
}

pub fn record_managed_install(evidence: &Path, upstream: &Path, source: &Path) -> ManagedInstall {
    let build = managed_launcher_build(evidence, source);
    ManagedInstall {
        launcher: build.join("codex.exe"),
        upstream: upstream.to_path_buf(),
        build,
        cpu_account: evidence.join("cpu-account"),
    }
}
