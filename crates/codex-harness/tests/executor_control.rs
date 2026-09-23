//! Fixture tests for the control-backed executor conversation driver.
//!
//! The app-server side of every check is a canned, model-free WebSocket
//! endpoint owned by the test: it speaks the contract the change's native
//! probes observed (initialize plus initialized, `thread/start`,
//! `thread/name/set`, `turn/start`, `thread/read includeTurns`, and the
//! `thread/*`, `item/*`, `turn/*` and `error` notifications), so startup,
//! binding verification, lifecycle mapping, final-message extraction and the
//! fail-closed paths are exercised without a model, a subscription or the
//! installed CLI.
#![cfg(windows)]
#[path = "../src/executor_control.rs"]
mod executor_control;

use executor_control::{
    BoundIdentity, ControlPaths, ControlPlan, Conversation, Endpoint, FinalMessage, Lifecycle,
    MAX_EVENTS_PER_PUMP, app_server_spec,
};
use harness_core::{
    orchestration_config::ProfileBinding,
    process::{Job, Limits},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const TOKEN: &str = "9f0c1d2e3a4b5c6d7e8f90123456789abcdef0123456789abcdef0123456789a";
/// The wrong bearer of the refusal check: same shape, different value.
const OTHER_TOKEN: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
const TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f2401";
const FINAL: &str = "CONTROL_FIXTURE_FINAL_MESSAGE";
const MODEL: &str = "deepseek-v4-flash";
const PROVIDER: &str = "deepseek-fixture";
const EFFORT: &str = "max";
const FIXTURE: &str = env!("CARGO_BIN_EXE_harness-executor-fixture");
const WAIT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------- WebSocket

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn header(head: &str, name: &str) -> Option<String> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

/// RFC 6455's own example: it pins both the digest and the base64 encoder.
#[test]
fn accept_key_matches_the_rfc_6455_example() {
    assert_eq!(
        accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
    );
    assert_eq!(
        sha1(b"abc"),
        [
            0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
            0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d
        ]
    );
}

fn accept_key(key: &str) -> String {
    base64(&sha1(
        format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes(),
    ))
}

fn sha1(input: &[u8]) -> [u8; 20] {
    let bit_length = (input.len() as u64) * 8;
    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());
    let mut state = [
        0x67452301u32,
        0xEFCDAB89,
        0x98BADCFE,
        0x10325476,
        0xC3D2E1F0,
    ];
    for block in message.as_chunks::<64>().0 {
        let mut words = [0u32; 80];
        for (index, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
            words[index] = u32::from_be_bytes(*chunk);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) =
            (state[0], state[1], state[2], state[3], state[4]);
        for (index, word) in words.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    let mut digest = [0u8; 20];
    for (index, value) in state.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    digest
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let padded = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let value =
            (u32::from(padded[0]) << 16) | (u32::from(padded[1]) << 8) | u32::from(padded[2]);
        out.push(ALPHABET[(value >> 18) as usize & 63] as char);
        out.push(ALPHABET[(value >> 12) as usize & 63] as char);
        out.push(match chunk.len() {
            1 => '=',
            _ => ALPHABET[(value >> 6) as usize & 63] as char,
        });
        out.push(match chunk.len() {
            3 => ALPHABET[value as usize & 63] as char,
            _ => '=',
        });
    }
    out
}

enum Poll {
    Message(String),
    Closed,
    Idle,
}

enum Frame {
    Text(String),
    Closed,
    Ping,
}

/// The minimal server side of RFC 6455 the canned endpoint needs: one
/// handshake with the capability bearer, masked client frames in, unmasked
/// server frames out. It exists so the driver talks to a real loopback
/// endpoint without a WebSocket dependency the kit does not have.
struct Wire {
    stream: TcpStream,
    buffer: Vec<u8>,
}

/// What the canned endpoint accepts as its bearer.
#[derive(Clone)]
enum Bearer {
    /// The exact value, for the refusal checks.
    Value(String),
    /// The token file the module writes, read at handshake time exactly as the
    /// installed app-server reads it through `--ws-token-file`.
    File(PathBuf),
}

impl Bearer {
    fn resolve(&self) -> Option<String> {
        match self {
            Self::Value(value) => Some(value.clone()),
            Self::File(path) => fs::read_to_string(path)
                .ok()
                .map(|text| text.trim().to_owned()),
        }
    }
}

impl Wire {
    fn accept(mut stream: TcpStream, bearer: &Bearer) -> std::io::Result<Self> {
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut received = Vec::new();
        let mut chunk = [0u8; 512];
        let head_end = loop {
            if let Some(index) = find(&received, b"\r\n\r\n") {
                break index;
            }
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                return Err(std::io::Error::other("handshake closed early"));
            }
            received.extend_from_slice(&chunk[..read]);
            if received.len() > 8192 {
                return Err(std::io::Error::other("handshake exceeds its bound"));
            }
        };
        let head = String::from_utf8_lossy(&received[..head_end]).into_owned();
        let key = header(&head, "sec-websocket-key").unwrap_or_default();
        let expected = bearer.resolve().map(|token| format!("Bearer {token}"));
        if header(&head, "authorization") != expected {
            let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
            let _ = stream.flush();
            return Err(std::io::Error::other("capability token refused"));
        }
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
            accept_key(&key)
        );
        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        let mut wire = Self {
            stream,
            buffer: Vec::new(),
        };
        wire.buffer.extend_from_slice(&received[head_end + 4..]);
        Ok(wire)
    }

    fn poll_text(&mut self, timeout: Duration) -> std::io::Result<Poll> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.take_frame()? {
                Some(Frame::Text(text)) => return Ok(Poll::Message(text)),
                Some(Frame::Closed) => return Ok(Poll::Closed),
                Some(Frame::Ping) => {
                    self.write_frame(0xA, b"")?;
                }
                None => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Ok(Poll::Idle);
                    }
                    self.stream.set_read_timeout(Some(remaining))?;
                    let mut chunk = [0u8; 4096];
                    match self.stream.read(&mut chunk) {
                        Ok(0) => return Ok(Poll::Closed),
                        Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            return Ok(Poll::Idle);
                        }
                        Err(error) => return Err(error),
                    }
                    if self.buffer.len() > 1024 * 1024 + 16 {
                        return Err(std::io::Error::other("canned frame exceeds its bound"));
                    }
                }
            }
        }
    }

    fn take_frame(&mut self) -> std::io::Result<Option<Frame>> {
        if self.buffer.len() < 2 {
            return Ok(None);
        }
        let opcode = self.buffer[0] & 0x0f;
        let masked = self.buffer[1] & 0x80 != 0;
        let (header, length) = match self.buffer[1] & 0x7f {
            126 => {
                if self.buffer.len() < 4 {
                    return Ok(None);
                }
                (
                    4,
                    u16::from_be_bytes([self.buffer[2], self.buffer[3]]) as usize,
                )
            }
            127 => {
                if self.buffer.len() < 10 {
                    return Ok(None);
                }
                let mut size = [0u8; 8];
                size.copy_from_slice(&self.buffer[2..10]);
                let size = u64::from_be_bytes(size);
                if size > 1024 * 1024 {
                    return Err(std::io::Error::other("canned frame exceeds its bound"));
                }
                (10, size as usize)
            }
            small => (2, small as usize),
        };
        let header = if masked { header + 4 } else { header };
        if self.buffer.len() < header + length {
            return Ok(None);
        }
        let mask = masked.then(|| {
            let mut key = [0u8; 4];
            key.copy_from_slice(&self.buffer[header - 4..header]);
            key
        });
        let mut payload = self.buffer[header..header + length].to_vec();
        if let Some(key) = mask {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= key[index % 4];
            }
        }
        self.buffer.drain(..header + length);
        Ok(Some(match opcode {
            0x1 => Frame::Text(String::from_utf8_lossy(&payload).into_owned()),
            0x8 => Frame::Closed,
            0x9 => Frame::Ping,
            other => {
                return Err(std::io::Error::other(format!(
                    "canned frame opcode {other} is unsupported"
                )));
            }
        }))
    }

    fn write_text(&mut self, text: &str) -> std::io::Result<()> {
        self.write_frame(0x1, text.as_bytes())
    }

    fn write_frame(&mut self, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
        let mut frame = vec![0x80 | opcode];
        let length = payload.len();
        if length < 126 {
            frame.push(length as u8);
        } else if length <= usize::from(u16::MAX) {
            frame.push(126);
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        } else {
            frame.push(127);
            frame.extend_from_slice(&(length as u64).to_be_bytes());
        }
        frame.extend_from_slice(payload);
        self.stream.write_all(&frame)?;
        self.stream.flush()
    }
}

// ------------------------------------------------------------- canned server

/// One canned answer to one request method.
#[derive(Clone)]
enum Answer {
    Result(Value),
    Error(Value),
    /// A complete record sent verbatim; `"@request"` as its id becomes the
    /// request's own id, so protocol deviations can be exercised.
    Record(Value),
    /// No answer at all: the client's own bound decides what that means.
    Silence,
}

#[derive(Default)]
struct Canned {
    requests: Vec<Value>,
    answers: HashMap<String, Answer>,
    notifications: VecDeque<Value>,
    connections: usize,
    refusals: usize,
}

/// A canned app-server endpoint on a real loopback port.
struct Server {
    port: u16,
    state: Arc<Mutex<Canned>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Server {
    fn start(bearer: Bearer) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(Canned::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            thread::spawn(move || serve(listener, &bearer, &state, &stop))
        };
        Self {
            port,
            state,
            stop,
            worker: Some(worker),
        }
    }

    fn answer(&self, method: &str, answer: Answer) {
        self.state
            .lock()
            .unwrap()
            .answers
            .insert(method.to_owned(), answer);
    }

    fn push(&self, notification: Value) {
        self.state
            .lock()
            .unwrap()
            .notifications
            .push_back(notification);
    }

    fn requests(&self) -> Vec<Value> {
        self.state.lock().unwrap().requests.clone()
    }

    fn requests_for(&self, method: &str) -> Vec<Value> {
        self.requests()
            .into_iter()
            .filter(|request| request["method"] == method)
            .collect()
    }

    fn refusals(&self) -> usize {
        self.state.lock().unwrap().refusals
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve(
    listener: TcpListener,
    bearer: &Bearer,
    state: &Arc<Mutex<Canned>>,
    stop: &Arc<AtomicBool>,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                // A real app-server serves several clients, so every accepted
                // connection is served on its own worker.
                let bearer = bearer.clone();
                let state = Arc::clone(state);
                let stop = Arc::clone(stop);
                thread::spawn(move || match Wire::accept(stream, &bearer) {
                    Ok(mut wire) => {
                        state.lock().unwrap().connections += 1;
                        let _ = converse(&mut wire, &state, &stop);
                    }
                    Err(_) => state.lock().unwrap().refusals += 1,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return,
        }
    }
}

/// Serves one connection: queued notifications first, then the answers of the
/// requests that arrive. Returns false when the connection is unusable.
fn converse(wire: &mut Wire, state: &Arc<Mutex<Canned>>, stop: &Arc<AtomicBool>) -> bool {
    while !stop.load(Ordering::SeqCst) {
        let queued: Vec<Value> = state.lock().unwrap().notifications.drain(..).collect();
        for value in queued {
            if wire.write_text(&value.to_string()).is_err() {
                return false;
            }
        }
        match wire.poll_text(Duration::from_millis(5)) {
            Ok(Poll::Message(text)) => {
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    return false;
                };
                let id = value["id"].clone();
                let method = value["method"].as_str().unwrap_or_default().to_owned();
                // The app-server answers requests and stays silent for
                // notifications such as `initialized`.
                let answers = !id.is_null();
                let answer = {
                    let mut canned = state.lock().unwrap();
                    canned.requests.push(value);
                    canned
                        .answers
                        .get(&method)
                        .cloned()
                        .unwrap_or(Answer::Result(json!({})))
                };
                if !answers {
                    continue;
                }
                let record = match answer {
                    Answer::Result(result) => json!({"id": id, "result": result}),
                    Answer::Error(error) => json!({"id": id, "error": error}),
                    Answer::Record(mut record) => {
                        if record["id"] == "@request" {
                            record["id"] = id.clone();
                        }
                        record
                    }
                    Answer::Silence => continue,
                };
                if wire.write_text(&record.to_string()).is_err() {
                    return false;
                }
            }
            Ok(Poll::Idle) => {}
            Ok(Poll::Closed) | Err(_) => return true,
        }
    }
    true
}

// ----------------------------------------------------------------- fixtures

fn identity() -> BoundIdentity {
    BoundIdentity {
        profile: "deepseek".to_owned(),
        model: Some(MODEL.to_owned()),
        model_provider: Some(PROVIDER.to_owned()),
        reasoning_effort: Some(EFFORT.to_owned()),
    }
}

fn thread_start_answer() -> Value {
    json!({
        "thread": {"id": THREAD},
        "model": MODEL,
        "modelProvider": PROVIDER,
        "reasoningEffort": EFFORT
    })
}

fn thread_read_answer() -> Value {
    json!({"thread": {
        "id": THREAD,
        "status": {"type": "idle"},
        "turns": [{
            "id": TURN,
            "status": "completed",
            "items": [
                {"id": "c1", "type": "commandExecution", "command": "pwsh -NoProfile -Command 'echo'", "exitCode": 0},
                {"id": "m1", "type": "agentMessage", "text": FINAL}
            ]
        }]
    }})
}

/// A canned endpoint plus the plan and Job that point the documented command
/// at it. The child is the kit's own executor fixture process: it proves the
/// real spawn, environment and Job ownership, while the protocol answers come
/// from the canned endpoint because the kit has no app-server fixture binary.
struct Fixture {
    _root: tempfile::TempDir,
    slot: PathBuf,
    server: Server,
    plan: ControlPlan,
    marker: PathBuf,
    job: Option<Job>,
}

fn fixture() -> Fixture {
    fixture_with(Answer::Result(thread_start_answer()))
}

fn fixture_with(thread_start: Answer) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let slot = root.path().join("slot");
    fs::create_dir_all(&slot).unwrap();
    let paths = ControlPaths {
        endpoint: root.path().join("control-1.json"),
        token: root.path().join("control-1.token"),
        log: root.path().join("control-1.log"),
    };
    // The canned endpoint reads the bearer from the token file the module
    // writes, exactly as the installed app-server reads it.
    let server = Server::start(Bearer::File(paths.token.clone()));
    server.answer(
        "initialize",
        Answer::Result(json!({"serverInfo": {"name": "canned-executor-control", "version": "1"}})),
    );
    server.answer("thread/start", thread_start);
    server.answer("thread/name/set", Answer::Result(json!({})));
    server.answer(
        "turn/start",
        Answer::Result(json!({"turn": {"id": TURN, "status": "inProgress"}})),
    );
    server.answer("thread/read", Answer::Result(thread_read_answer()));
    let marker = root.path().join("fixture-child.json");
    let mut plan = ControlPlan::new(
        FIXTURE,
        root.path().join("home"),
        &slot,
        "control fixture assignment",
        identity(),
        paths,
    );
    plan.port = Some(server.port);
    plan.approval_policy = Some("never".to_owned());
    plan.sandbox = Some("danger-full-access".to_owned());
    plan.bound = WAIT;
    plan.poll = Duration::from_millis(50);
    plan.env
        .insert("HARNESS_EXECUTOR_FIXTURE_MODE".into(), Some("hang".into()));
    plan.env.insert(
        "HARNESS_EXECUTOR_FIXTURE_STARTED".into(),
        Some(marker.clone().into_os_string()),
    );
    Fixture {
        _root: root,
        slot,
        server,
        plan,
        marker,
        job: Some(Job::new(Limits::default()).unwrap()),
    }
}

impl Fixture {
    fn start(&self) -> std::io::Result<Conversation> {
        Conversation::start(self.job.as_ref().expect("job"), &self.plan)
    }

    fn endpoint_path(&self) -> PathBuf {
        self.plan.paths.endpoint.clone()
    }

    fn child_pid(&self) -> u32 {
        self.child_identity().pid
    }

    /// The identity the fixture child recorded through the plan's environment.
    fn child_identity(&self) -> harness_core::process::ProcessIdentity {
        let until = Instant::now() + WAIT;
        loop {
            if let Ok(bytes) = fs::read(&self.marker) {
                return serde_json::from_slice(&bytes)
                    .expect("the recorded identity is a process identity");
            }
            assert!(
                Instant::now() < until,
                "the fixture child records its identity through plan.env"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    /// The caller's Job stays the sole cleanup authority.
    fn terminate(&mut self) {
        if let Some(job) = self.job.take() {
            let _ = job.terminate(0, Duration::from_secs(5));
        }
    }
}

fn completion_burst(server: &Server) {
    server.push(json!({"method":"thread/started","params":{"thread":{"id":THREAD}}}));
    server.push(
        json!({"method":"thread/status/changed","params":{"threadId":THREAD,"status":{"type":"active"}}}),
    );
    server.push(
        json!({"method":"item/started","params":{"threadId":THREAD,"item":{
        "id":"c1","type":"commandExecution","command":"pwsh -NoProfile -Command 'exit 0'"}}}),
    );
    server.push(json!({"method":"item/completed","params":{"threadId":THREAD,"item":{
        "id":"c1","type":"commandExecution","command":"pwsh -NoProfile -Command 'exit 0'","exitCode":0}}}));
    server.push(
        json!({"method":"item/completed","params":{"threadId":THREAD,"item":{
        "id":"m1","type":"agentMessage","text":FINAL}}}),
    );
    server.push(
        json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{"id":TURN,"status":"completed"}}}),
    );
    server.push(
        json!({"method":"thread/status/changed","params":{"threadId":THREAD,"status":{"type":"idle"}}}),
    );
}

fn drain(conversation: &mut Conversation, expected: usize) -> Vec<executor_control::ControlEvent> {
    let until = Instant::now() + WAIT;
    let mut events = Vec::new();
    while events.len() < expected && Instant::now() < until {
        events.extend(conversation.pump().unwrap());
    }
    assert_eq!(
        events.len(),
        expected,
        "control records: {:?}",
        events
            .iter()
            .map(|event| event.method.clone())
            .collect::<Vec<_>>()
    );
    events
}

/// Bounded wait for a fixture observation that a worker thread records.
fn wait_for(mut condition: impl FnMut() -> bool, reason: &str) {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("{reason}");
}

// ------------------------------------------------------- startup and binding

#[test]
fn spawned_child_serves_the_documented_command_environment_and_endpoint() {
    let mut fixture = fixture();
    let conversation = fixture.start().expect("the canned endpoint must answer");
    assert_ne!(
        fixture.child_pid(),
        0,
        "the child ran with the plan's environment"
    );
    assert!(
        conversation.process().unwrap().is_running().unwrap(),
        "the app-server child stays inside the caller's Job"
    );
    assert!(conversation.process_identity().is_some());
    assert_eq!(
        conversation.identity().unwrap().model.as_deref(),
        Some(MODEL)
    );
    assert_eq!(conversation.lifecycle(), None);
    assert_eq!(conversation.thread_id(), THREAD);
    assert!(conversation.defect().is_none());
    assert!(conversation.failure().is_none());
    assert!(conversation.turn().is_none());

    // The endpoint is recorded beside the receipt with exactly the bearer the
    // token file carries - the canned endpoint accepted that bearer, so the
    // token file, the client and the record agree - and the bearer stays out
    // of every diagnostic shape.
    let token = fs::read_to_string(&fixture.plan.paths.token).unwrap();
    assert_eq!(token.len(), 64, "32 bytes of OS randomness as hex");
    assert!(token.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    let endpoint = Endpoint::read(&fixture.endpoint_path()).unwrap();
    assert_eq!(endpoint.token(), token);
    assert_eq!(endpoint.thread_id.as_deref(), Some(THREAD));
    assert_eq!(conversation.endpoint().token(), token);
    assert!(!format!("{endpoint:?}").contains(&token));

    // The child ran the exact documented command environment.
    let requests = fixture.server.requests();
    let methods: Vec<&str> = requests
        .iter()
        .filter_map(|request| request["method"].as_str())
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "thread/start",
            "thread/name/set"
        ]
    );
    assert_eq!(
        requests[0]["params"]["capabilities"]["experimentalApi"],
        true
    );
    assert!(
        requests[1].get("id").is_none(),
        "initialized is a notification, not a request"
    );
    let start = &requests[2]["params"];
    assert_eq!(start["cwd"], json!(fixture.slot), "{start}");
    assert_eq!(start["model"], MODEL);
    assert_eq!(start["modelProvider"], PROVIDER);
    assert_eq!(
        start["allowProviderModelFallback"], false,
        "a model fallback would change the routed model"
    );
    assert_eq!(start["approvalPolicy"], "never");
    assert_eq!(start["sandbox"], "danger-full-access");
    assert_eq!(start["config"]["model_reasoning_effort"], EFFORT);
    assert_eq!(requests[3]["params"]["threadId"], THREAD);
    assert_eq!(requests[3]["params"]["name"], "control fixture assignment");
    drop(conversation);
    fixture.terminate();
}

#[test]
fn plan_spawns_the_documented_app_server_command() {
    let fixture = fixture();
    let spec = app_server_spec(&fixture.plan, 51234);
    assert_eq!(spec.program, PathBuf::from(FIXTURE));
    assert_eq!(spec.current_dir.as_deref(), Some(fixture.slot.as_path()));
    let args: Vec<String> = spec
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "app-server".to_owned(),
            "--listen".to_owned(),
            "ws://127.0.0.1:51234".to_owned(),
            "--ws-auth".to_owned(),
            "capability-token".to_owned(),
            "--ws-token-file".to_owned(),
            fixture.plan.paths.token.to_string_lossy().into_owned(),
            "-c".to_owned(),
            "agents.enabled=false".to_owned(),
        ]
    );
    assert_eq!(
        spec.env
            .get(&std::ffi::OsString::from("CODEX_HOME"))
            .cloned()
            .flatten(),
        Some(fixture.plan.home.clone().into_os_string())
    );
    assert_eq!(
        spec.env
            .get(&std::ffi::OsString::from("HARNESS_EXECUTOR_SESSION"))
            .cloned()
            .flatten(),
        Some(std::ffi::OsString::from("1"))
    );
}

#[test]
fn start_refuses_a_thread_that_reports_another_binding() {
    let mut fixture = fixture_with(Answer::Result(json!({
        "thread": {"id": THREAD},
        "model": "other-model",
        "modelProvider": PROVIDER,
        "reasoningEffort": EFFORT
    })));
    let error = fixture.start().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("binding was not preserved: the started thread reports model=other-model"),
        "{error}"
    );
    assert!(
        fixture.server.requests_for("thread/name/set").is_empty(),
        "a refused thread must not be named or driven further"
    );
    assert!(
        !fixture.endpoint_path().exists(),
        "a refused conversation must not be recorded as addressable"
    );
    fixture.terminate();
}

#[test]
fn binding_verification_covers_every_bound_field() {
    let bound = identity();
    assert!(bound.verify(&thread_start_answer()).is_ok());
    assert!(
        bound
            .verify(&json!({"model": MODEL, "modelProvider": PROVIDER}))
            .is_err(),
        "a thread that reports no effort cannot be assumed to run the bound one"
    );
    assert!(
        bound
            .verify(&json!({
                "model": MODEL.to_uppercase(), "modelProvider": PROVIDER, "reasoningEffort": EFFORT
            }))
            .is_ok()
    );
    for (field, value) in [
        ("model", json!("other-model")),
        ("modelProvider", json!("other-provider")),
        ("reasoningEffort", json!("low")),
    ] {
        let mut answer = thread_start_answer();
        if field == "model" {
            answer["modelProvider"] = json!(PROVIDER);
        }
        answer[field] = value;
        let mismatch = bound.verify(&answer).unwrap_err();
        assert!(mismatch.contains(field), "{mismatch}");
    }
    // An unbound profile pins nothing, so nothing can mismatch.
    let open = BoundIdentity {
        profile: "default".to_owned(),
        model: None,
        model_provider: None,
        reasoning_effort: None,
    };
    assert!(open.verify(&json!({})).is_ok());
}

#[test]
fn startup_fails_closed_when_the_child_exits_before_serving() {
    let root = tempfile::tempdir().unwrap();
    let slot = root.path().join("slot");
    fs::create_dir_all(&slot).unwrap();
    let paths = ControlPaths {
        endpoint: root.path().join("control-1.json"),
        token: root.path().join("control-1.token"),
        log: root.path().join("control-1.log"),
    };
    let mut plan = ControlPlan::new(
        FIXTURE,
        root.path().join("home"),
        &slot,
        "exiting child",
        identity(),
        paths,
    );
    plan.bound = Duration::from_secs(5);
    plan.poll = Duration::from_millis(50);
    plan.env.insert(
        "HARNESS_EXECUTOR_FIXTURE_MODE".into(),
        Some("complete".into()),
    );
    let job = Job::new(Limits::default()).unwrap();
    let error = Conversation::start(&job, &plan).unwrap_err();
    assert!(
        error.to_string().contains("exited with exit code 0"),
        "{error}"
    );
    assert!(
        error.to_string().contains("control-1.log"),
        "the failure names the log to read: {error}"
    );
    let token = fs::read_to_string(&plan.paths.token).unwrap();
    assert_eq!(token.len(), 64, "the capability token is 32 bytes of hex");
    assert!(token.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    assert!(plan.paths.log.exists(), "the child log exists");
    let _ = job.terminate(0, Duration::from_secs(5));
}

#[test]
fn control_state_sits_beside_the_receipt_and_carries_the_resolved_binding() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("checkout");
    fs::create_dir_all(&source).unwrap();
    let paths = ControlPaths::for_slot(&root.path().join("home"), &source, 3).unwrap();
    assert_eq!(paths.endpoint.file_name().unwrap(), "control-3.json");
    assert_eq!(paths.token.file_name().unwrap(), "control-3.token");
    assert_eq!(paths.log.file_name().unwrap(), "control-3.log");
    let directory = paths.endpoint.parent().unwrap();
    assert_eq!(directory, paths.token.parent().unwrap());
    assert_eq!(directory, paths.log.parent().unwrap());
    assert_eq!(
        paths.endpoint.with_file_name("spawn-3.json").parent(),
        Some(directory),
        "the endpoint record is written where the dispatch receipt of the same slot lives"
    );

    let binding = BoundIdentity::resolve(&ProfileBinding {
        profile: "deepseek".to_owned(),
        model: Some(MODEL.to_owned()),
        model_provider: Some(PROVIDER.to_owned()),
        reasoning_effort: Some(EFFORT.to_owned()),
    });
    assert_eq!(binding, identity());
}

#[test]
fn startup_fails_closed_when_the_child_never_serves_the_endpoint() {
    let root = tempfile::tempdir().unwrap();
    let slot = root.path().join("slot");
    fs::create_dir_all(&slot).unwrap();
    let marker = root.path().join("child.json");
    let paths = ControlPaths {
        endpoint: root.path().join("control-1.json"),
        token: root.path().join("control-1.token"),
        log: root.path().join("control-1.log"),
    };
    let mut plan = ControlPlan::new(
        FIXTURE,
        root.path().join("home"),
        &slot,
        "silent child",
        identity(),
        paths,
    );
    plan.bound = Duration::from_secs(1);
    plan.poll = Duration::from_millis(50);
    plan.env
        .insert("HARNESS_EXECUTOR_FIXTURE_MODE".into(), Some("hang".into()));
    plan.env.insert(
        "HARNESS_EXECUTOR_FIXTURE_STARTED".into(),
        Some(marker.clone().into_os_string()),
    );
    let job = Job::new(Limits::default()).unwrap();
    let error = Conversation::start(&job, &plan).unwrap_err();
    assert!(error.to_string().contains("did not serve"), "{error}");
    assert!(error.to_string().contains("control-1.log"), "{error}");
    // The child is still running: the module never reaps what the caller's Job
    // owns, and the failure names it instead of hiding it.
    let user = harness_core::process_service::current_user().unwrap();
    let recorded: harness_core::process::ProcessIdentity =
        serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    let child = harness_core::process_service::ServiceProcess::observe(
        recorded.pid,
        Path::new(FIXTURE),
        recorded.creation_time,
        &user,
    )
    .unwrap();
    assert!(
        child.is_running().unwrap(),
        "the caller's Job still owns the child"
    );
    let _ = job.terminate(0, Duration::from_secs(5));
}

// ---------------------------------------------------- attached conversations

/// A recorded endpoint plus the canned server behind it: exactly the recorded
/// state `executor message` and `executor stop` read, so adopting it is tested
/// where those commands will use it.
struct Recorded {
    _root: tempfile::TempDir,
    server: Server,
    endpoint: Endpoint,
    record: PathBuf,
}

fn recorded() -> Recorded {
    recorded_with(thread_read_answer())
}

fn recorded_with(read: Value) -> Recorded {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    server.answer("initialize", Answer::Result(json!({})));
    server.answer("thread/read", Answer::Result(read));
    server.answer(
        "turn/start",
        Answer::Result(json!({"turn": {"id": TURN, "status": "inProgress"}})),
    );
    let record = root.path().join("control-1.json");
    fs::write(
        &record,
        serde_json::to_vec_pretty(&json!({
            "schema": 1, "port": server.port, "token": TOKEN, "threadId": THREAD
        }))
        .unwrap(),
    )
    .unwrap();
    let endpoint = Endpoint::read(&record).unwrap();
    Recorded {
        _root: root,
        server,
        endpoint,
        record,
    }
}

impl Recorded {
    fn attach(&self) -> Conversation {
        Conversation::attach(&self.endpoint, WAIT).unwrap()
    }
}

#[test]
fn lifecycle_mapping_follows_the_thread_records() {
    let mut fixture = fixture();
    let mut conversation = fixture.start().unwrap();
    completion_burst(&fixture.server);
    let events = drain(&mut conversation, 7);
    let states: Vec<Option<Lifecycle>> = events.iter().map(|event| event.lifecycle).collect();
    assert_eq!(
        states,
        [
            Some(Lifecycle::NativeStart),
            Some(Lifecycle::Running),
            Some(Lifecycle::Running),
            Some(Lifecycle::Running),
            Some(Lifecycle::Running),
            Some(Lifecycle::Completed),
            None
        ],
        "idle after a completed turn changes nothing"
    );
    assert!(events.iter().all(|event| event.deviation.is_none()));
    assert_eq!(conversation.lifecycle(), Some(Lifecycle::Completed));
    assert!(Lifecycle::Completed.is_terminal());
    assert_eq!(
        conversation.lifecycle().unwrap().receipt_state(),
        "completed"
    );
    assert_eq!(conversation.failure(), None);
    assert_eq!(conversation.defect(), None);
    assert_eq!(Lifecycle::NativeStart.receipt_state(), "native-start");
    assert_eq!(Lifecycle::Defect.receipt_state(), "defect");
    assert!(!Lifecycle::Running.is_terminal());

    let mut rendered = Vec::new();
    for event in &events {
        event.render(&mut rendered).unwrap();
    }
    let text = String::from_utf8(rendered).unwrap();
    assert!(
        text.contains(&format!("state: native start (session {THREAD})")),
        "{text}"
    );
    assert!(text.contains("state: thread active"), "{text}");
    assert!(
        text.contains("command: pwsh -NoProfile -Command 'exit 0' (exit 0)"),
        "{text}"
    );
    assert!(text.contains(&format!("assistant: {FINAL}")), "{text}");
    assert!(text.contains("state: turn completed (completed)"), "{text}");

    // The final message comes from the thread items, and the read is the
    // verified shape.
    assert_eq!(
        conversation.final_message().unwrap(),
        FinalMessage::Present(FINAL.to_owned())
    );
    let read = fixture.server.requests_for("thread/read");
    assert_eq!(read[0]["params"]["includeTurns"], true);
    assert_eq!(read[0]["params"]["threadId"], THREAD);
    drop(conversation);
    fixture.terminate();
}

#[test]
fn failed_and_interrupted_turns_are_distinguished() {
    let recorded = recorded();
    let mut conversation = recorded.attach();
    recorded.server.push(json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{
        "id":TURN,"status":"failed",
        "error":{"message":"provider refused the request","codexErrorInfo":"usageLimitExceeded"}}}}));
    let events = drain(&mut conversation, 1);
    assert_eq!(events[0].lifecycle, Some(Lifecycle::Failed));
    assert_eq!(conversation.failure(), Some("provider refused the request"));
    assert_eq!(conversation.lifecycle().unwrap().receipt_state(), "failed");
    assert!(conversation.lifecycle().unwrap().is_terminal());
    recorded.server.push(
        json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{
        "id":TURN,"status":"interrupted"}}}),
    );
    let events = drain(&mut conversation, 1);
    assert_eq!(events[0].lifecycle, Some(Lifecycle::Interrupted));
    assert_eq!(
        conversation.lifecycle().unwrap().receipt_state(),
        "interrupted"
    );
    // Item activity after an interrupted turn never reopens the state.
    recorded.server.push(
        json!({"method":"item/completed","params":{"threadId":THREAD,"item":{
        "id":"c1","type":"commandExecution","command":"pwsh","exitCode":0}}}),
    );
    let events = drain(&mut conversation, 1);
    assert_eq!(events[0].lifecycle, None);
    assert_eq!(conversation.lifecycle(), Some(Lifecycle::Interrupted));
}

#[test]
fn protocol_deviations_are_recorded_and_never_invent_state() {
    let recorded = recorded();
    let mut conversation = recorded.attach();
    recorded.server.push(
        json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{
        "id":TURN,"status":"cancelled-by-a-future-build"}}}),
    );
    recorded
        .server
        .push(json!({"method":"item/completed","params":{"threadId":THREAD,"item":{"id":"x"}}}));
    recorded.server.push(json!({"method":"thread/status/changed","params":{"threadId":THREAD,"status":{"type":"hibernating"}}}));
    recorded
        .server
        .push(json!({"method":"codex/future-notification","params":{"threadId":THREAD}}));
    recorded.server.push(json!({"method":"item/completed","params":{"threadId":"another-thread","item":{"id":"y","type":"agentMessage","text":"not ours"}}}));
    recorded.server.push(json!({"method":"error","params":{"threadId":THREAD,"willRetry":true,"error":{"message":"transient"}}}));
    let events = drain(&mut conversation, 6);
    assert_eq!(events[0].lifecycle, Some(Lifecycle::Defect));
    assert!(
        events[0]
            .deviation
            .as_deref()
            .unwrap()
            .contains("unknown turn status"),
        "{:?}",
        events[0].deviation
    );
    assert!(
        events[1]
            .deviation
            .as_deref()
            .unwrap()
            .contains("no item type")
    );
    assert!(
        events[2]
            .deviation
            .as_deref()
            .unwrap()
            .contains("unknown status type"),
        "{:?}",
        events[2].deviation
    );
    assert_eq!(
        events[3].lifecycle, None,
        "an unknown method is not a deviation"
    );
    assert_eq!(events[3].deviation, None);
    assert_eq!(
        events[4].lifecycle, None,
        "another thread is not this conversation"
    );
    assert_eq!(events[4].deviation, None);
    assert_eq!(events[5].lifecycle, None, "a retrying error is not a state");
    assert_eq!(conversation.lifecycle(), Some(Lifecycle::Defect));
    assert!(
        conversation
            .defect()
            .unwrap()
            .contains("unknown turn status"),
        "the first deviation is retained"
    );
    assert_eq!(
        conversation.failure(),
        None,
        "a retrying error is not a recorded cause"
    );
    let mut rendered = Vec::new();
    events[5].render(&mut rendered).unwrap();
    assert!(
        String::from_utf8(rendered)
            .unwrap()
            .contains("error: transient (retrying)")
    );
}

#[test]
fn assignment_turn_start_records_the_turn_and_rejects_a_missing_turn() {
    let recorded = recorded();
    let mut conversation = recorded.attach();
    let turn = conversation
        .assign("apply the correction to the same conversation")
        .unwrap();
    assert_eq!(turn.turn_id, TURN);
    assert_eq!(turn.status, "inProgress");
    assert_eq!(conversation.turn().unwrap().turn_id, TURN);
    let requests = recorded.server.requests_for("turn/start");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["params"]["threadId"], THREAD);
    assert_eq!(requests[0]["params"]["input"][0]["type"], "text");
    assert_eq!(
        requests[0]["params"]["input"][0]["text"],
        "apply the correction to the same conversation"
    );
    assert!(
        conversation.assign("").is_err(),
        "an empty input is refused"
    );
    recorded.server.answer(
        "turn/start",
        Answer::Result(json!({"turn": {"status": "inProgress"}})),
    );
    let error = conversation.assign("second submission").unwrap_err();
    assert!(
        error.to_string().contains("without a turn identity"),
        "{error}"
    );
}

#[test]
fn call_returns_one_bounded_native_answer() {
    let recorded = recorded();
    recorded.server.answer(
        "turn/interrupt",
        Answer::Result(json!({"interrupted": true})),
    );
    let mut conversation = recorded.attach();
    let answer = conversation
        .call(
            "turn/interrupt",
            json!({"threadId": THREAD, "turnId": TURN}),
        )
        .unwrap();
    assert_eq!(answer["interrupted"], true);
    let seen = recorded.server.requests_for("turn/interrupt");
    assert_eq!(seen[0]["params"]["turnId"], TURN);
}

#[test]
fn final_message_distinguishes_present_empty_and_missing() {
    let recorded = recorded_with(thread_read_answer());
    let mut conversation = recorded.attach();
    assert_eq!(
        conversation.final_message().unwrap(),
        FinalMessage::Present(FINAL.to_owned())
    );
    recorded.server.answer(
        "thread/read",
        Answer::Result(json!({"thread": {"id": THREAD, "turns": [
            {"id": TURN, "status": "completed", "items": [
                {"id": "m1", "type": "agentMessage", "text": "   "}]}]}})),
    );
    assert_eq!(conversation.final_message().unwrap(), FinalMessage::Empty);
    recorded.server.answer(
        "thread/read",
        Answer::Result(json!({"thread": {"id": THREAD, "turns": [
            {"id": TURN, "status": "completed", "items": [
                {"id": "c1", "type": "commandExecution"}]}]}})),
    );
    assert_eq!(conversation.final_message().unwrap(), FinalMessage::Missing);
    recorded.server.answer(
        "thread/read",
        Answer::Result(json!({"thread": {"id": "another-thread", "turns": []}})),
    );
    let error = conversation.final_message().unwrap_err();
    assert!(error.to_string().contains("instead of"), "{error}");
}

#[test]
fn protocol_errors_fail_the_call_and_a_deadline_is_not_progress() {
    let recorded = recorded();
    let mut conversation = recorded.attach();
    recorded.server.answer(
        "thread/read",
        Answer::Record(json!({"id": "@request", "result": {"thread": {"id": THREAD}}})),
    );
    assert!(conversation.thread_state().is_ok());
    recorded.server.answer(
        "thread/read",
        Answer::Record(json!({"id": 4242, "result": {}})),
    );
    let error = conversation.thread_state().unwrap_err();
    assert!(
        error.to_string().contains("carried request identity 4242"),
        "{error}"
    );
    recorded.server.answer(
        "thread/read",
        Answer::Error(json!({"code": -32601, "message": "no such method"})),
    );
    let error = conversation.thread_state().unwrap_err();
    assert!(error.to_string().contains("was rejected"), "{error}");
    recorded
        .server
        .answer("thread/read", Answer::Record(json!({"id": "@request"})));
    let error = conversation.thread_state().unwrap_err();
    assert!(error.to_string().contains("carried no result"), "{error}");
    recorded
        .server
        .answer("thread/read", Answer::Record(json!("not a control object")));
    let error = conversation.thread_state().unwrap_err();
    assert!(error.to_string().contains("not a JSON object"), "{error}");
    recorded.server.answer("thread/read", Answer::Silence);
    let mut silent = Conversation::attach(&recorded.endpoint, Duration::from_millis(300)).unwrap();
    let error = silent.thread_state().unwrap_err();
    assert!(
        error.to_string().contains("was not answered within"),
        "{error}"
    );
    assert_eq!(silent.lifecycle(), None, "a deadline establishes no state");
}

#[test]
fn one_pump_is_bounded_and_leaves_the_rest_queued() {
    let recorded = recorded();
    let mut conversation = recorded.attach();
    for index in 0..MAX_EVENTS_PER_PUMP + 8 {
        recorded.server.push(
            json!({"method":"item/completed","params":{"threadId":THREAD,"item":{
            "id":format!("m{index}"),"type":"agentMessage","text":format!("line {index}")}}}),
        );
    }
    let mut first = conversation.pump().unwrap();
    while first.len() < MAX_EVENTS_PER_PUMP {
        let batch = conversation.pump().unwrap();
        assert!(batch.len() <= MAX_EVENTS_PER_PUMP);
        first.extend(batch);
    }
    assert_eq!(first.len(), MAX_EVENTS_PER_PUMP);
    assert!(
        first
            .iter()
            .all(|event| event.lifecycle == Some(Lifecycle::Running))
    );
    let rest = drain(&mut conversation, 8);
    assert_eq!(rest.len(), 8);
    assert_eq!(conversation.lifecycle(), Some(Lifecycle::Running));
}

#[test]
fn endpoint_records_are_validated_and_a_thread_is_required() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("control-2.json");
    assert!(
        Endpoint::read(&path).is_err(),
        "a missing record is not an endpoint"
    );
    for record in [
        json!({"schema": 2, "port": 1234, "token": TOKEN, "threadId": null}),
        json!({"schema": 1, "port": 0, "token": TOKEN, "threadId": null}),
        json!({"schema": 1, "port": 1234, "token": "short", "threadId": null}),
        json!({"schema": 1, "port": 1234, "token": TOKEN, "threadId": ""}),
        json!({"schema": 1, "port": 1234, "token": TOKEN, "threadId": null, "extra": true}),
    ] {
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(Endpoint::read(&path).is_err(), "{record}");
    }
    fs::write(
        &path,
        serde_json::to_vec(&json!({"schema": 1, "port": 1234, "token": TOKEN, "threadId": null}))
            .unwrap(),
    )
    .unwrap();
    let endpoint = Endpoint::read(&path).unwrap();
    let error = Conversation::attach(&endpoint, Duration::from_millis(300)).unwrap_err();
    assert!(error.to_string().contains("names no thread"), "{error}");
    let recorded = recorded();
    let round_trip = Endpoint::read(&recorded.record).unwrap();
    assert_eq!(round_trip.token(), TOKEN);
    assert_eq!(round_trip.thread_id.as_deref(), Some(THREAD));
}

#[test]
fn attach_refuses_another_bearer() {
    let recorded = recorded();
    fs::write(
        &recorded.record,
        serde_json::to_vec(&json!({
            "schema": 1, "port": recorded.server.port, "token": OTHER_TOKEN, "threadId": THREAD
        }))
        .unwrap(),
    )
    .unwrap();
    let foreign = Endpoint::read(&recorded.record).unwrap();
    assert_ne!(foreign.token(), recorded.endpoint.token());
    assert!(Conversation::attach(&foreign, Duration::from_millis(500)).is_err());
    wait_for(
        || recorded.server.refusals() == 1,
        "the endpoint refused the foreign bearer",
    );
    assert!(
        recorded.server.requests().is_empty(),
        "no request crosses a refused handshake"
    );
}

// --------------------------------------------------------- opt-in native CLI

/// Opt-in check against the installed Codex CLI: the real `codex app-server`
/// accepts this driver's launch command, initialize handshake, `thread/start`
/// binding (cwd, model, provider, effort) and recording, and the recorded
/// endpoint is addressable again for the later `message`/`stop` slices.
///
/// It makes no model request: no turn is submitted, and the provider the
/// configuration names points at a closed loopback port.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned model-free native app-server"]
fn native_app_server_binds_the_resolved_profile_and_records_an_addressable_endpoint() {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    assert!(exe.is_absolute() && exe.is_file(), "{exe:?}");
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let slot = root.path().join("slot");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&slot).unwrap();
    let closed = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let closed_port = closed.local_addr().unwrap().port();
    drop(closed);
    let trusted = serde_json::to_string(&slot.to_string_lossy()).unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            r#"
model = "gpt-6-astra"
model_provider = "control_fixture"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
cli_auth_credentials_store = "file"
[model_providers.control_fixture]
name = "Owned control fixture"
base_url = "http://127.0.0.1:{closed_port}/v1"
wire_api = "responses"
env_key = "HARNESS_CONTROL_FIXTURE_KEY"
requires_openai_auth = false
request_max_retries = 0
stream_max_retries = 0
supports_websockets = false
[analytics]
enabled = false
[projects.{trusted}]
trust_level = "trusted"
"#
        ),
    )
    .unwrap();
    let paths = ControlPaths {
        endpoint: root.path().join("control-1.json"),
        token: root.path().join("control-1.token"),
        log: root.path().join("control-1.log"),
    };
    let mut plan = ControlPlan::new(
        &exe,
        &home,
        &slot,
        "native control fixture assignment",
        BoundIdentity {
            profile: "default".to_owned(),
            model: Some("gpt-6-astra".to_owned()),
            model_provider: Some("control_fixture".to_owned()),
            reasoning_effort: Some("low".to_owned()),
        },
        paths,
    );
    plan.approval_policy = Some("never".to_owned());
    plan.sandbox = Some("danger-full-access".to_owned());
    plan.bound = Duration::from_secs(60);
    plan.poll = Duration::from_millis(200);
    for key in [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "CODEX_ACCESS_TOKEN",
        "HARNESS_CONTROL_CODEX_EXE",
    ] {
        plan.env.insert(key.into(), None);
    }
    plan.env.insert(
        "HARNESS_CONTROL_FIXTURE_KEY".into(),
        Some("synthetic-owned-fixture".into()),
    );
    let path = std::env::var_os("PATH").map(|path| {
        std::env::join_paths(std::env::split_paths(&path).filter(|entry| {
            !entry
                .to_string_lossy()
                .to_lowercase()
                .contains("windowsapps")
        }))
        .unwrap()
    });
    plan.env.insert("PATH".into(), path);
    let job = Job::new(Limits::default()).unwrap();
    let mut conversation = Conversation::start(&job, &plan).expect("native app-server startup");
    let thread = conversation.thread_id().to_owned();
    assert!(!thread.is_empty());
    assert!(
        conversation.endpoint().thread_id.as_deref() == Some(thread.as_str()),
        "the recorded endpoint names the started thread"
    );
    let recorded = Endpoint::read(&plan.paths.endpoint).unwrap();
    assert_eq!(recorded.thread_id.as_deref(), Some(thread.as_str()));
    assert_eq!(recorded.token(), conversation.endpoint().token());
    let token = fs::read_to_string(&plan.paths.token).unwrap();
    assert_eq!(token.len(), 64);
    assert!(conversation.process().unwrap().is_running().unwrap());
    // The real CLI announces the thread it just started.
    let observed: Vec<String> = Vec::new();
    let until = Instant::now() + WAIT;
    let native_start = loop {
        if let Some(event) = conversation
            .pump()
            .unwrap()
            .into_iter()
            .find(|event| event.lifecycle == Some(Lifecycle::NativeStart))
        {
            break event;
        }
        assert!(
            Instant::now() < until,
            "the native thread start was never observed; records: {observed:?}"
        );
    };
    assert_eq!(native_start.method.as_deref(), Some("thread/started"));
    // The recorded endpoint answers a second client: the addressing the later
    // `executor message` and `executor stop` slices depend on.
    let mut attached = Conversation::attach(
        &Endpoint::read(&plan.paths.endpoint).unwrap(),
        Duration::from_secs(30),
    )
    .expect("the recorded endpoint is addressable");
    assert_eq!(attached.thread_id(), thread);
    let state = attached
        .call(
            "thread/read",
            json!({"threadId": thread, "includeTurns": true}),
        )
        .unwrap();
    assert_eq!(state["thread"]["id"], thread);
    assert!(
        state["thread"]["status"]["type"].is_string(),
        "the thread reports a native status: {state}"
    );
    drop(attached);
    drop(conversation);
    let _ = job.terminate(0, Duration::from_secs(10));
}
