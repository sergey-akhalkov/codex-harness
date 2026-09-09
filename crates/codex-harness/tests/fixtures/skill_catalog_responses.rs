//! Model-free canned Responses HTTP transport for the skill-catalog TUI probe.
//! Binds 127.0.0.1 with an OS-assigned port. This is a local substitute for the
//! reserved openai provider route, not a live proxy or billed model.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
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
pub const EARLY_SKILL: &str = "skill_catalog_early_probe";

const MAX_BODY: usize = 2 * 1024 * 1024;

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
}

pub struct CannedResponses {
    pub addr: SocketAddr,
    inner: Arc<Inner>,
    thread: Option<JoinHandle<()>>,
}

impl CannedResponses {
    pub fn spawn(root: &Path) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        std::fs::create_dir_all(root.join("http"))?;
        let inner = Arc::new(Inner {
            stop: AtomicBool::new(false),
            next_seq: AtomicUsize::new(1),
            requests: Mutex::new(Vec::new()),
            root: root.to_path_buf(),
        });
        let worker = Arc::clone(&inner);
        let thread = thread::spawn(move || accept_loop(listener, worker));
        Ok(Self {
            addr,
            inner,
            thread: Some(thread),
        })
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

fn accept_loop(listener: TcpListener, inner: Arc<Inner>) {
    while !inner.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let inner = Arc::clone(&inner);
                thread::spawn(move || {
                    let _ = handle_connection(stream, inner);
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

fn handle_connection(mut stream: TcpStream, inner: Arc<Inner>) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let request = read_http(&mut stream)?;
    let method = request.method;
    let path = request.path;
    let headers = request.headers;
    let body = request.body;
    if method == "POST"
        && headers
            .get("expect")
            .is_some_and(|value| value.eq_ignore_ascii_case("100-continue"))
    {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    let body_text = String::from_utf8_lossy(&body).into_owned();
    let kind = classify(&method, &path, &body_text);
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
    let (status, content_type, payload) = response_for(&method, &path, kind);
    write_response(&mut stream, status, content_type, payload.as_bytes())
}

fn classify(method: &str, path: &str, body: &str) -> RequestKind {
    let path = path.split('?').next().unwrap_or(path);
    if method == "GET" && path.ends_with("/models") {
        return RequestKind::Models;
    }
    if method != "POST" || !path.contains("responses") {
        return RequestKind::Other;
    }
    if body.contains(COMPACT_PROMPT) {
        RequestKind::Compact
    } else if body.contains(NEXT_TURN_PROMPT) {
        RequestKind::NextTurn
    } else if body.contains(SUMMARY_TEXT) || body.contains(SUMMARY_PREFIX) {
        RequestKind::Continuation
    } else if body.contains(USER_PROMPT) {
        RequestKind::FirstSampling
    } else {
        RequestKind::Other
    }
}

fn response_for(
    method: &str,
    path: &str,
    kind: RequestKind,
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
    let sse = match kind {
        RequestKind::Compact => compact_sse(),
        RequestKind::Continuation => message_sse("cont-1", "SKILL_CATALOG_CONTINUATION_OK", 1200),
        RequestKind::NextTurn => message_sse("next-1", "SKILL_CATALOG_NEXT_TURN_OK", 800),
        RequestKind::FirstSampling | RequestKind::Other => first_sampling_sse(),
        RequestKind::Models => unreachable!(),
    };
    ("200 OK", "text/event-stream", sse)
}

fn first_sampling_sse() -> String {
    sse(&[
        json!({"type": "response.created", "response": {"id": "resp-first"}}),
        json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "call_id": CALL_ID,
                "name": "exec_command",
                "arguments": "{\"cmd\":\"ping -n 40 127.0.0.1\",\"yield_time_ms\":30000}"
            }
        }),
        completed("resp-first", 250_000),
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
) -> io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}
