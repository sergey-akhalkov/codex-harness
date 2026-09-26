//! Deterministic checks for the native warning transport.
//!
//! The app-server side of every check is either the canned control endpoint or
//! an owned byte-echo endpoint: no model, subscription or installed CLI is
//! involved. The frontend side is an owned WebSocket client, so the handshake,
//! both forwarding directions, the one appended notification and every close
//! path are exercised without a terminal.
//!
//! The warning contract itself is verified against the *shape* of a
//! `codex app-server generate-json-schema --experimental` export, written here
//! as a minimal synthetic document, and against the installed export when
//! `HARNESS_CODEX_APP_SERVER_SCHEMA_DIR` names one.
#![cfg(windows)]
#[path = "fixtures/control_endpoint.rs"]
mod control_endpoint;
#[path = "../src/executor_warning.rs"]
mod executor_warning;

use control_endpoint::{Answer, Bearer, Poll, Server, Wire};
use executor_warning::{
    QueueOutcome, Relay, WarningContract, WarningNotification, WarningQueue, WarningState,
    supported_contract,
};
use harness_core::task_control::ControlConnection;
use harness_core::{
    console::{ConsoleSession, ConsoleSpec},
    process::CommandSpec,
};
use serde_json::{Value, json};
use std::{
    fs, io,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tungstenite::{
    Error as WsError, Message, WebSocket,
    client::{IntoClientRequest, client_with_config},
    http::HeaderValue,
    protocol::WebSocketConfig,
};

const TOKEN: &str = "5c1d2e3a4b5c6d7e8f90123456789abcdef0123456789abcdef0123456789ab";
const OTHER_TOKEN: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
const OTHER_THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f24ff";
const WARNING: &str = "cache guard degraded: exact-session usage is unavailable for this response";
const WAIT: Duration = Duration::from_secs(10);
const SETTLE: Duration = Duration::from_millis(400);

// ------------------------------------------------------------------- exports
// Minimal synthetic stand-ins for the installed export's documents: only the
// fields the transport reads are present, in the shape the installed export
// declares. They are written by the check, never copied from a private export.

const SERVER_NOTIFICATIONS: &str = r##"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "oneOf": [
    {
      "properties": {
        "method": {"enum": ["thread/started"], "type": "string"},
        "params": {"$ref": "#/definitions/ThreadStartedNotification"}
      },
      "required": ["method", "params"],
      "title": "ThreadStartedNotification",
      "type": "object"
    },
    {
      "properties": {
        "method": {"enum": ["warning"], "type": "string"},
        "params": {"$ref": "#/definitions/WarningNotification"}
      },
      "required": ["method", "params"],
      "title": "WarningNotification",
      "type": "object"
    }
  ],
  "title": "ServerNotification",
  "type": "object"
}"##;

const WARNING_PARAMS: &str = r#"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "properties": {
    "message": {"description": "Concise warning message for the user.", "type": "string"},
    "threadId": {
      "description": "Optional thread target when the warning applies to a specific thread.",
      "type": ["string", "null"]
    }
  },
  "required": ["message"],
  "title": "WarningNotification",
  "type": "object"
}"#;

const CLIENT_REQUESTS: &str = r#"{
  "oneOf": [
    {
      "properties": {
        "method": {"enum": ["initialize"], "type": "string"},
        "params": {"type": "object"}
      },
      "required": ["id", "method", "params"],
      "title": "InitializeRequest",
      "type": "object"
    }
  ],
  "title": "ClientRequest",
  "type": "object"
}"#;

const CLIENT_NOTIFICATIONS: &str = r#"{
  "oneOf": [
    {
      "properties": {"method": {"enum": ["initialized"], "type": "string"}},
      "required": ["method"],
      "title": "InitializedNotification",
      "type": "object"
    }
  ],
  "title": "ClientNotification",
  "type": "object"
}"#;

const SERVER_REQUESTS: &str = r#"{
  "oneOf": [
    {
      "properties": {
        "method": {"enum": ["item/commandExecution/requestApproval"], "type": "string"},
        "params": {"type": "object"}
      },
      "required": ["id", "method", "params"],
      "title": "CommandExecutionRequestApproval",
      "type": "object"
    }
  ],
  "title": "ServerRequest",
  "type": "object"
}"#;

fn write_export(root: &Path) {
    fs::create_dir_all(root.join("v2")).unwrap();
    fs::write(root.join("ServerNotification.json"), SERVER_NOTIFICATIONS).unwrap();
    fs::write(root.join("v2/WarningNotification.json"), WARNING_PARAMS).unwrap();
    fs::write(root.join("ClientRequest.json"), CLIENT_REQUESTS).unwrap();
    fs::write(root.join("ClientNotification.json"), CLIENT_NOTIFICATIONS).unwrap();
    fs::write(root.join("ServerRequest.json"), SERVER_REQUESTS).unwrap();
}

// -------------------------------------------------------------------- clients

/// What one frontend read observed.
enum Wait {
    Text(String),
    Closed,
    Idle,
}

impl Wait {
    fn text(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Closed => panic!("the frontend socket closed"),
            Self::Idle => panic!("the frontend received nothing"),
        }
    }
}

fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(4096)
        .write_buffer_size(0)
        .max_write_buffer_size(1024 * 1024 + 4096)
        .max_message_size(Some(1024 * 1024))
        .max_frame_size(Some(1024 * 1024))
}

/// The frontend double: one authenticated WebSocket client of the relay.
struct Frontend {
    socket: WebSocket<TcpStream>,
}

impl Frontend {
    fn attach(port: u16, token: &str) -> Self {
        let address = format!("127.0.0.1:{port}");
        let stream = TcpStream::connect(&address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        let mut request = format!("ws://{address}").into_client_request().unwrap();
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        let (socket, _) = client_with_config(request, stream, Some(config())).unwrap();
        Self { socket }
    }

    fn send_text(&mut self, text: &str) {
        self.socket.send(Message::Text(text.into())).unwrap();
    }

    fn request(&mut self, id: u64, method: &str, params: Value) {
        self.send_text(&json!({"id": id, "method": method, "params": params}).to_string());
    }

    /// Waits for the next text record. An idle read keeps waiting; any read
    /// failure other than the local poll timeout is a closed socket.
    fn receive(&mut self, timeout: Duration) -> Wait {
        let deadline = Instant::now() + timeout;
        loop {
            match self.socket.read() {
                Ok(Message::Text(text)) => return Wait::Text(text.to_string()),
                Ok(Message::Close(_)) => return Wait::Closed,
                Ok(_) => {}
                Err(WsError::Io(error))
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return Wait::Closed,
            }
            if Instant::now() >= deadline {
                return Wait::Idle;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn close(&mut self) {
        let _ = self.socket.close(None);
    }
}

/// One raw handshake, so a refusal is observable as the app-server contract
/// shapes it: an HTTP status, not a WebSocket close.
fn handshake(port: u16, bearer: Option<&str>) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    let mut request = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n"
    );
    if let Some(bearer) = bearer {
        request.push_str(&format!("Authorization: {bearer}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut buffer = [0u8; 512];
    let read = stream.read(&mut buffer).unwrap();
    String::from_utf8_lossy(&buffer[..read]).into_owned()
}

/// Owned byte-echo app-server double. It speaks the capability handshake and
/// returns every text frame verbatim, so byte preservation is observable
/// without an answer layer that would re-serialize JSON.
struct Echo {
    port: u16,
    sessions: Arc<AtomicUsize>,
    closed: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Echo {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let sessions = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicUsize::new(0));
        let worker = {
            let stop = Arc::clone(&stop);
            let sessions = Arc::clone(&sessions);
            let closed = Arc::clone(&closed);
            thread::spawn(move || {
                let mut workers: Vec<JoinHandle<()>> = Vec::new();
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            sessions.fetch_add(1, Ordering::SeqCst);
                            let stop = Arc::clone(&stop);
                            let closed = Arc::clone(&closed);
                            workers.push(thread::spawn(move || {
                                if let Ok(mut wire) =
                                    Wire::accept(stream, &Bearer::Value(TOKEN.to_owned()))
                                {
                                    while !stop.load(Ordering::SeqCst) {
                                        match wire.poll_text(Duration::from_millis(5)) {
                                            Ok(Poll::Message(text)) => {
                                                if wire.write_text(&text).is_err() {
                                                    break;
                                                }
                                            }
                                            Ok(Poll::Idle) => {}
                                            Ok(Poll::Closed) | Err(_) => break,
                                        }
                                    }
                                }
                                closed.fetch_add(1, Ordering::SeqCst);
                            }));
                            workers.retain(|handle| !handle.is_finished());
                        }
                        Err(_) => thread::sleep(Duration::from_millis(5)),
                    }
                }
            })
        };
        Self {
            port,
            sessions,
            closed,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for Echo {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn wait_for(mut condition: impl FnMut() -> bool, reason: &str) {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("{reason}");
}

fn record_of(text: String) -> Value {
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("not a JSON record: {error}: {text}"))
}

// ------------------------------------------------------------------ contract

#[test]
fn the_implemented_warning_matches_the_declared_app_server_contract() {
    let root = tempfile::tempdir().unwrap();
    write_export(root.path());
    let declared = supported_contract(root.path()).unwrap();
    assert_eq!(
        declared,
        WarningContract::installed(),
        "the implemented warning model and the declared export disagree"
    );
    assert_eq!(declared.method(), "warning");
    let record = WarningNotification::new(WARNING, Some(THREAD)).record();
    declared
        .validate(&record)
        .expect("the declared contract accepts the implemented record");
    assert_eq!(
        record,
        json!({"method": "warning", "params": {"message": WARNING, "threadId": THREAD}})
    );
    assert!(
        record.get("id").is_none(),
        "a warning is a server-to-client notification, never a request: {record}"
    );
    // The captured `threadId` is optional and nullable, so the same method can
    // address no single thread.
    let untargeted = WarningNotification::new(WARNING, None).record();
    declared.validate(&untargeted).unwrap();
    assert_eq!(
        untargeted,
        json!({"method": "warning", "params": {"message": WARNING}})
    );
}

#[test]
fn unsupported_shapes_and_client_side_warning_requests_are_refused() {
    let contract = WarningContract::installed();
    let records = [
        // A request shape: the installed protocol has no client warning request.
        json!({"id": 1, "method": "warning", "params": {"message": WARNING}}),
        // Unknown parameter.
        json!({"method": "warning", "params": {"message": WARNING, "path": "C:/private"}}),
        // Missing required parameter.
        json!({"method": "warning", "params": {"threadId": THREAD}}),
        // Mistyped required parameter.
        json!({"method": "warning", "params": {"message": 1}}),
        json!({"method": "warning", "params": {"message": WARNING, "threadId": 3}}),
        // Another method.
        json!({"method": "guardianWarning", "params": {"message": WARNING}}),
        // Params that are not an object.
        json!({"method": "warning", "params": "degraded"}),
    ];
    for record in records {
        assert!(
            contract.validate(&record).is_err(),
            "an unsupported record was accepted: {record}"
        );
    }

    // The export itself is the gate: a declaration this transport cannot read
    // is refused rather than guessed at.
    let root = tempfile::tempdir().unwrap();
    write_export(root.path());
    supported_contract(root.path()).unwrap();

    fs::write(
        root.path().join("v2/WarningNotification.json"),
        r#"{"properties":{"message":{"type":"object"}},"required":["message"],"title":"WarningNotification","type":"object"}"#,
    )
    .unwrap();
    assert!(supported_contract(root.path()).is_err());

    write_export(root.path());
    fs::write(
        root.path().join("ServerNotification.json"),
        r#"{"oneOf":[{"properties":{"method":{"enum":["thread/started"],"type":"string"}},"required":["method","params"],"title":"ThreadStartedNotification","type":"object"}],"title":"ServerNotification","type":"object"}"#,
    )
    .unwrap();
    assert!(supported_contract(root.path()).is_err());

    write_export(root.path());
    fs::write(
        root.path().join("ClientRequest.json"),
        r#"{"oneOf":[{"properties":{"method":{"enum":["warning"],"type":"string"},"params":{"type":"object"}},"required":["id","method","params"],"title":"WarningRequest","type":"object"}],"title":"ClientRequest","type":"object"}"#,
    )
    .unwrap();
    assert!(
        supported_contract(root.path()).is_err(),
        "a client-to-server warning request is not part of the installed surface"
    );
}

#[test]
#[ignore = "requires HARNESS_CODEX_APP_SERVER_SCHEMA_DIR naming a codex app-server generate-json-schema --experimental export"]
fn the_installed_app_server_schema_declares_the_implemented_contract() {
    let export = std::env::var_os("HARNESS_CODEX_APP_SERVER_SCHEMA_DIR")
        .expect("HARNESS_CODEX_APP_SERVER_SCHEMA_DIR naming a generated export");
    let declared = supported_contract(Path::new(&export)).unwrap();
    assert_eq!(
        declared,
        WarningContract::installed(),
        "the installed app-server warning contract changed; the transport must not guess a shape"
    );
}

/// The initialized exact thread is what releases a queued diagnostic, and the
/// request shape that proves it belongs to the installed frontend. The canned
/// endpoint cannot show that contract, so this check attaches the installed TUI
/// through the relay with the command line the host itself uses.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE (an absolute native Codex CLI); owned console and canned endpoint, no model"]
fn the_installed_frontend_releases_a_queued_diagnostic() {
    let exe = std::env::var_os("HARNESS_CONTROL_CODEX_EXE")
        .map(PathBuf::from)
        .expect("explicit native Codex executable");
    assert!(exe.is_absolute() && exe.is_file(), "{}", exe.display());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    server.answer(
        "initialize",
        Answer::Result(json!({
            "codexHome": std::env::temp_dir().to_string_lossy(),
            "platformFamily": "windows",
            "platformOs": "windows",
            "userAgent": "native-warning-transport-check",
        })),
    );
    server.answer(
        "thread/resume",
        Answer::Result(
            json!({"thread": {"id": THREAD, "cwd": std::env::temp_dir().to_string_lossy()}}),
        ),
    );
    // The TUI asks the app-server about its account during bootstrap, before it
    // resumes the thread; answer the documented shape so the installed frontend
    // reaches the request this check is about.
    server.answer(
        "account/read",
        Answer::Result(json!({"requiresOpenaiAuth": false, "account": {"type": "apiKey"}})),
    );
    let mut relay = Relay::start(server.port, TOKEN, THREAD).unwrap();
    let warnings = relay.warnings();
    assert_eq!(warnings.queue(WARNING), QueueOutcome::Queued);

    let home = tempfile::tempdir().unwrap();
    let mut spec = CommandSpec::new(&exe);
    spec.args = vec![
        "--remote".into(),
        format!("ws://127.0.0.1:{}", relay.port()).into(),
        "--remote-auth-token-env".into(),
        "HARNESS_WARNING_CHECK_TOKEN".into(),
        "-c".into(),
        "agents.enabled=false".into(),
        "--no-alt-screen".into(),
        "resume".into(),
        THREAD.into(),
    ];
    spec.env
        .insert("HARNESS_WARNING_CHECK_TOKEN".into(), Some(TOKEN.into()));
    spec.env.insert(
        "CODEX_HOME".into(),
        Some(home.path().as_os_str().to_owned()),
    );
    let session = ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while Instant::now() < until && warnings.status().state != WarningState::Sent {
        thread::sleep(Duration::from_millis(200));
    }
    let status = warnings.status();
    let transcript = session.transcript();
    let requests = server.requests();
    let methods: Vec<&str> = requests
        .iter()
        .filter_map(|request| request["method"].as_str())
        .collect();
    // The installed frontend initialized the exact thread with its own
    // thread-naming request. These recorded methods are the evidence that its
    // traffic reached the app-server through the relay.
    eprintln!("installed frontend methods: {methods:?}");
    drop(session);
    relay.close();
    assert_eq!(
        status.state,
        WarningState::Sent,
        "the installed frontend did not initialize the exact thread: {status:?}; methods: {methods:?}; transcript: {transcript}"
    );
    assert!(
        requests
            .iter()
            .any(|request| request["params"].to_string().contains(THREAD)),
        "the frontend's own thread-naming request reached the app-server: {requests:?}"
    );
}

// --------------------------------------------------------------------- relay

#[test]
fn the_relay_refuses_an_endpoint_it_cannot_authenticate() {
    assert!(Relay::start(0, TOKEN, THREAD).is_err());
    assert!(Relay::start(1, "short", THREAD).is_err());
    assert!(Relay::start(1, OTHER_TOKEN, "").is_err());
}

#[test]
fn the_warning_surface_records_honest_delivery_states() {
    // The receipt and host-log spellings this transport records.
    assert_eq!(WarningState::Idle.as_str(), "idle");
    assert_eq!(WarningState::Queued.as_str(), "queued");
    assert_eq!(WarningState::Sent.as_str(), "native-sent");
    assert_eq!(WarningState::Undelivered.as_str(), "undelivered");
    assert_eq!(WarningState::NoConsumer.as_str(), "no-native-consumer");

    // A surface with no native warning consumer records diagnostics without
    // ever claiming that one was presented.
    let warnings = WarningQueue::without_consumer();
    assert_eq!(warnings.status().state, WarningState::NoConsumer);
    assert_eq!(warnings.status().pending, 0);
    assert_eq!(warnings.queue(WARNING), QueueOutcome::Queued);
    assert_eq!(
        warnings.status().state,
        WarningState::NoConsumer,
        "a surface with no consumer must not report a native send"
    );
    assert_eq!(warnings.status().pending, 1);
    assert_eq!(warnings.queue(WARNING), QueueOutcome::Duplicate);
    for index in 0..8 {
        let more = format!("{WARNING} ({index})");
        let outcome = warnings.queue(&more);
        assert_ne!(outcome, QueueOutcome::Duplicate, "{more}");
        if outcome == QueueOutcome::Full {
            break;
        }
    }
    assert_eq!(
        warnings.queue(&format!("{WARNING} (late)")),
        QueueOutcome::Full
    );
    assert_eq!(
        warnings.status().pending,
        4,
        "the queue is bounded and refuses rather than dropping silently"
    );
}

#[test]
fn a_missing_or_wrong_capability_token_is_refused_at_the_handshake() {
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let mut relay = Relay::start(server.port, TOKEN, THREAD).unwrap();
    assert_ne!(relay.port(), server.port);

    for bearer in [None, Some("Bearer wrong-token")] {
        let response = handshake(relay.port(), bearer);
        assert!(
            response.starts_with("HTTP/1.1 401"),
            "an unauthenticated frontend must be refused: {response}"
        );
    }
    assert_eq!(
        server.connections(),
        0,
        "a refused handshake must not reach the app-server"
    );

    let response = handshake(relay.port(), Some(&format!("Bearer {TOKEN}")));
    assert!(
        response.starts_with("HTTP/1.1 101"),
        "the conversation's own token must attach: {response}"
    );
    wait_for(
        || server.connections() == 1,
        "the relay did not authenticate itself to the app-server",
    );
    relay.close();
}

#[test]
fn forwarding_preserves_bytes_in_both_directions() {
    let echo = Echo::start();
    let mut relay = Relay::start(echo.port, TOKEN, THREAD).unwrap();
    let mut frontend = Frontend::attach(relay.port(), TOKEN);
    wait_for(
        || echo.sessions.load(Ordering::SeqCst) == 1,
        "the relay did not connect upstream",
    );

    // Deliberately non-canonical JSON, an unknown method, unicode and a payload
    // that crosses the short-frame length boundary: the relay must not
    // re-serialize, normalize or drop any of it.
    for text in [
        r#"{ "method" : "fixture/unknown" , "id" : 1 , "params" : { "b" : 2, "a" : [1,  2] } }"#,
        r#"{"method":"thread/read","id":2,"params":{"threadId":"01a0c719-f4d4-7880-a9d2-1a96ee0f23f4","note":"кавычки \" и юникод ✓"}}"#,
        &format!(
            r#"{{"method":"fixture/large","id":3,"params":{{"blob":"{}"}}}}"#,
            "x".repeat(70_000)
        ),
    ] {
        frontend.send_text(text);
        match frontend.receive(WAIT) {
            Wait::Text(received) => {
                assert_eq!(received, text, "the relay altered a forwarded record")
            }
            other => panic!("no forwarded record: {}", matches!(other, Wait::Closed)),
        }
    }

    // A close from the frontend ends its session and its upstream connection.
    frontend.close();
    wait_for(
        || echo.closed.load(Ordering::SeqCst) >= 1,
        "the relay kept the upstream connection of a closed frontend",
    );
    relay.close();
}

#[test]
fn a_queued_warning_waits_for_the_exact_thread_and_is_presented_once() {
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    server.answer(
        "thread/resume",
        Answer::Result(json!({"thread": {"id": THREAD}})),
    );
    let mut relay = Relay::start(server.port, TOKEN, THREAD).unwrap();
    let warnings = relay.warnings();
    assert_eq!(warnings.status().state, WarningState::Idle);
    assert_eq!(warnings.queue(WARNING), QueueOutcome::Queued);
    assert_eq!(warnings.status().state, WarningState::Queued);
    assert_eq!(warnings.status().pending, 1);

    let mut frontend = Frontend::attach(relay.port(), TOKEN);
    // The frontend has not initialized anything: a queued diagnostic stays
    // queued instead of being appended to an unrelated connection.
    assert!(matches!(frontend.receive(SETTLE), Wait::Idle));

    // Ongoing traffic that names no thread is forwarded untouched and arrives
    // before any warning.
    frontend.request(
        1,
        "initialize",
        json!({"clientInfo": {"name": "warning-check"}}),
    );
    server.push(json!({"method": "thread/started", "params": {"thread": {"id": OTHER_THREAD}}}));
    // The answer and the pushed notification are both forwarded untouched, in
    // whichever order the app-server produces them.
    let (mut answered, mut started) = (false, false);
    while !(answered && started) {
        let record = record_of(frontend.receive(WAIT).text());
        assert_ne!(
            record["method"], "warning",
            "a warning preceded initialization: {record}"
        );
        answered |= record["id"] == 1;
        started |= record["method"] == "thread/started";
    }
    // A resume of another thread is another conversation's initialization.
    frontend.request(2, "thread/resume", json!({"threadId": OTHER_THREAD}));
    assert_eq!(record_of(frontend.receive(WAIT).text())["id"], 2);
    assert!(matches!(frontend.receive(SETTLE), Wait::Idle));
    assert_eq!(warnings.status().pending, 1);

    // The exact thread initializes here, so the diagnostic follows that answer.
    frontend.request(3, "thread/resume", json!({"threadId": THREAD}));
    let answer = record_of(frontend.receive(WAIT).text());
    assert_eq!(
        answer["id"], 3,
        "the answer must precede the warning: {answer}"
    );
    let warning = record_of(frontend.receive(WAIT).text());
    assert_eq!(warning["method"], "warning", "{warning}");
    assert_eq!(warning["params"]["message"], WARNING, "{warning}");
    assert_eq!(
        warning["params"]["threadId"], THREAD,
        "a warning is addressed to the exact thread: {warning}"
    );
    assert_eq!(
        warning["params"].as_object().unwrap().len(),
        2,
        "only the captured parameters are written: {warning}"
    );
    assert!(warning.get("id").is_none(), "{warning}");
    assert_eq!(warnings.status().state, WarningState::Sent);
    assert_eq!(warnings.status().pending, 0);

    // One degraded diagnostic is presented once, however often it is offered.
    assert_eq!(warnings.queue(WARNING), QueueOutcome::Duplicate);
    let mut second = WARNING.to_owned();
    second.push_str("(again)");
    assert_eq!(warnings.queue(&second), QueueOutcome::Queued);
    // Ongoing traffic pushed after the warning is still forwarded, and the
    // repeated diagnostic is never presented again.
    server.push(json!({"method": "turn/completed", "params": {"turn": {"id": "t1", "status": "completed"}}}));
    let after = [
        record_of(frontend.receive(WAIT).text()),
        record_of(frontend.receive(WAIT).text()),
    ];
    assert!(
        after
            .iter()
            .any(|record| record["method"] == "turn/completed"),
        "ongoing traffic did not survive the warning: {after:?}"
    );
    let mut presented = after.iter().filter(|record| record["method"] == "warning");
    let delivered = presented
        .next()
        .expect("the second degraded diagnostic was never presented");
    assert_eq!(delivered["params"]["message"], second, "{delivered}");
    assert!(
        presented.next().is_none(),
        "the same diagnostic was presented more than once: {after:?}"
    );
    for record in &after {
        assert_ne!(
            record["params"]["message"], WARNING,
            "a deduplicated diagnostic was presented twice: {record}"
        );
    }
    match frontend.receive(SETTLE) {
        Wait::Idle => {}
        Wait::Text(text) => panic!("the same diagnostic was presented twice: {text}"),
        Wait::Closed => panic!("the relay closed a healthy session"),
    }

    frontend.close();
    relay.close();
}

/// A warning the transport cannot write fails on its own, is recorded as
/// undelivered while it stays queued, and never ends the healthy session: the
/// exact thread keeps receiving forwarded traffic and the recorded control
/// endpoint keeps answering.
#[test]
fn a_failed_warning_send_records_undelivered_without_ending_the_session() {
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    server.answer(
        "thread/resume",
        Answer::Result(json!({"thread": {"id": THREAD}})),
    );
    server.answer(
        "thread/read",
        Answer::Result(json!({"thread": {"id": THREAD, "turns": []}})),
    );
    let mut relay = Relay::start(server.port, TOKEN, THREAD).unwrap();
    let warnings = relay.warnings();
    let mut frontend = Frontend::attach(relay.port(), TOKEN);
    // The exact thread initializes on this connection first.
    frontend.request(1, "thread/resume", json!({"threadId": THREAD}));
    assert_eq!(record_of(frontend.receive(WAIT).text())["id"], 1);
    // A diagnostic the transport refuses to write fails the delivery attempt
    // deterministically; nothing outside the captured shape is ever written.
    let oversized = format!("cache guard degraded: {}", "x".repeat(2 * 1024 * 1024));
    assert_eq!(warnings.queue(&oversized), QueueOutcome::Queued);
    wait_for(
        || warnings.status().state == WarningState::Undelivered,
        "the failed send was not recorded as undelivered",
    );
    assert_eq!(
        warnings.status().pending,
        1,
        "a failed diagnostic stays queued instead of being dropped"
    );
    assert_eq!(warnings.queue(&oversized), QueueOutcome::Duplicate);
    // The session stays healthy: forwarded traffic arrives and the recorded
    // endpoint still answers a concurrent control request.
    server.push(json!({
        "method": "turn/completed",
        "params": {"turn": {"id": "t1", "status": "completed"}}
    }));
    let record = record_of(frontend.receive(WAIT).text());
    assert_eq!(record["method"], "turn/completed", "{record}");
    let mut control = ControlConnection::connect(server.port, TOKEN, WAIT).unwrap();
    control
        .send(
            &json!({"id": 7, "method": "thread/read", "params": {"threadId": THREAD}}),
            WAIT,
        )
        .unwrap();
    let mut answer = None;
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline && answer.is_none() {
        answer = control.receive(Duration::from_millis(200)).unwrap();
    }
    let answer = answer.expect("the recorded endpoint answered no control request");
    assert_eq!(answer["id"], 7, "{answer}");
    assert_eq!(answer["result"]["thread"]["id"], THREAD, "{answer}");

    frontend.close();
    relay.close();
}

#[test]
fn control_traffic_against_the_recorded_endpoint_survives_the_relay() {
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    server.answer(
        "thread/read",
        Answer::Result(json!({"thread": {"id": THREAD, "turns": []}})),
    );
    let mut relay = Relay::start(server.port, TOKEN, THREAD).unwrap();
    let mut frontend = Frontend::attach(relay.port(), TOKEN);
    frontend.request(1, "thread/resume", json!({"threadId": THREAD}));
    let _ = frontend.receive(WAIT);

    // `executor message` and `executor stop` resolve a run through the recorded
    // endpoint, which still addresses the app-server: a concurrent control
    // request must work while the frontend is relayed.
    let mut control = ControlConnection::connect(server.port, TOKEN, WAIT).unwrap();
    control
        .send(
            &json!({"id": 7, "method": "thread/read", "params": {"threadId": THREAD}}),
            WAIT,
        )
        .unwrap();
    let mut answer = None;
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline && answer.is_none() {
        answer = control.receive(Duration::from_millis(200)).unwrap();
    }
    let answer = answer.expect("the recorded endpoint answered no control request");
    assert_eq!(answer["id"], 7, "{answer}");
    assert_eq!(answer["result"]["thread"]["id"], THREAD, "{answer}");
    assert_eq!(
        server.requests_for("thread/read").len(),
        1,
        "the control request reached the app-server itself"
    );
    assert_eq!(
        server.connections(),
        2,
        "the frontend and the host both attach"
    );

    frontend.close();
    relay.close();
}

#[test]
fn closing_the_relay_or_losing_the_upstream_ends_the_frontend_session() {
    // Losing the upstream ends the frontend's session; the frontend sees a
    // closed socket rather than a hang.
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let mut relay = Relay::start(server.port, TOKEN, THREAD).unwrap();
    let mut frontend = Frontend::attach(relay.port(), TOKEN);
    drop(server);
    wait_for(
        || matches!(frontend.receive(Duration::from_millis(100)), Wait::Closed),
        "the frontend socket outlived its app-server",
    );
    relay.close();

    // Closing the relay ends the frontend's session and returns promptly.
    let echo = Echo::start();
    let mut relay = Relay::start(echo.port, TOKEN, THREAD).unwrap();
    let mut frontend = Frontend::attach(relay.port(), TOKEN);
    wait_for(
        || echo.sessions.load(Ordering::SeqCst) == 1,
        "the relay did not connect upstream",
    );
    let started = Instant::now();
    relay.close();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "relay shutdown waited on an idle frontend: {:?}",
        started.elapsed()
    );
    wait_for(
        || matches!(frontend.receive(Duration::from_millis(100)), Wait::Closed),
        "the frontend socket outlived the relay",
    );
}

#[test]
fn no_credential_or_host_log_content_reaches_the_relayed_payloads() {
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    server.answer(
        "thread/resume",
        Answer::Result(json!({"thread": {"id": THREAD}})),
    );
    let mut relay = Relay::start(server.port, TOKEN, THREAD).unwrap();
    let warnings = relay.warnings();
    // The diagnostic is the only host content that may cross the relay.
    assert_eq!(warnings.queue(WARNING), QueueOutcome::Queued);
    let mut frontend = Frontend::attach(relay.port(), TOKEN);
    frontend.request(1, "thread/resume", json!({"threadId": THREAD}));

    let mut received = Vec::new();
    match frontend.receive(WAIT) {
        Wait::Text(text) => received.push(text),
        other => panic!("unexpected read: {}", matches!(other, Wait::Closed)),
    }
    match frontend.receive(WAIT) {
        Wait::Text(text) => received.push(text),
        other => panic!("unexpected read: {}", matches!(other, Wait::Closed)),
    }
    for text in &received {
        assert!(
            !text.contains(TOKEN) && !text.contains("Bearer") && !text.contains(OTHER_TOKEN),
            "the capability token leaked into a relayed payload: {text}"
        );
    }
    let warning: Value = serde_json::from_str(&received[1]).unwrap();
    assert_eq!(warning["method"], "warning", "{warning}");
    // The appended record is the diagnostic and its thread, nothing else: no
    // host log line, no local path and no receipt field rides along.
    assert_eq!(
        warning["params"],
        json!({"message": WARNING, "threadId": THREAD}),
        "{warning}"
    );

    frontend.close();
    relay.close();
}
