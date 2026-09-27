//! Native warning transport for an attached managed executor frontend.
//!
//! The installed Codex app-server declares warnings only as a server-to-client
//! notification: the `warning` method whose params carry a required `message`
//! and an optional `threadId`. A managed executor host therefore cannot inject
//! one through its own control connection, because the notification belongs to
//! the socket the native frontend opened. This module owns the smallest
//! frontend-facing relay that gives the host that socket.
//!
//! The relay listens on its own loopback port, authenticates the frontend with
//! the conversation's capability token, and forwards app-server bytes unchanged
//! in both directions. Its only edit of the stream is the captured notification
//! shape, appended for the exact thread once the frontend has initialized that
//! thread on its own connection. The recorded control endpoint that
//! `executor message` and `executor stop` address keeps pointing at the real
//! app-server, and the host's own control connection is never relayed.
//!
//! Nothing here is a client-to-server warning request: the installed protocol
//! has none, and no shape outside the captured notification is ever written.
//! A delivery failure is recorded state, never a terminal outcome for healthy
//! model work, and the relay never writes to the terminal it serves.

use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs, io,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tungstenite::{
    Error as WsError, Message, WebSocket, accept_hdr_with_config,
    client::{IntoClientRequest, client_with_config},
    handshake::server::{ErrorResponse, Request, Response},
    http::{HeaderValue, StatusCode},
    protocol::WebSocketConfig,
};

/// The one server-to-client warning notification the installed app-server
/// declares, captured from `codex app-server generate-json-schema --experimental`
/// on codex-cli 0.157.1: the `ServerNotification` variant `WarningNotification`
/// with method `warning`.
pub const WARNING_METHOD: &str = "warning";
/// The captured notification's required parameter.
const WARNING_MESSAGE: &str = "message";
/// The captured notification's optional parameter: the thread the warning
/// applies to, `null` when it applies to no single thread.
const WARNING_THREAD: &str = "threadId";

/// Bound on the relay's upstream connect and on one socket write.
const WRITE: Duration = Duration::from_secs(5);
/// Read timeout of one relay step. It bounds both how long an idle step waits
/// and how long relay shutdown waits for a session.
const POLL: Duration = Duration::from_millis(50);
const ACCEPT_POLL: Duration = Duration::from_millis(5);
/// Bound on waiting for relayed sessions to end after the listener stopped.
const CLOSE_GRACE: Duration = Duration::from_secs(3);
/// Diagnostics waiting for a frontend that initialized the exact thread.
const MAX_QUEUED: usize = 4;
/// Queue history kept for deduplication; dedupe is best effort above it.
const MAX_REMEMBERED: usize = 8;
/// Thread-naming requests one frontend connection may leave awaiting an answer.
const MAX_AWAITING: usize = 64;

// ------------------------------------------------------------------ contract

/// The captured `warning` notification of the installed app-server, in the
/// shape its generated schema declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarningNotification {
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_id: Option<String>,
}

impl WarningNotification {
    /// One notification. The message is required by the captured contract; the
    /// thread target is optional there and is named for this transport, which
    /// only ever addresses the exact thread of the conversation it relays.
    pub fn new(message: &str, thread_id: Option<&str>) -> Self {
        Self {
            message: message.to_owned(),
            thread_id: thread_id.map(str::to_owned),
        }
    }

    /// The complete server-to-client record: a notification with a method and
    /// params and no request id, in the shape the schema declares.
    pub fn record(&self) -> Value {
        json!({"method": WARNING_METHOD, "params": self})
    }
}

/// The warning notification shape one app-server schema export declares. Only
/// the captured shape is supported: a notification with a required `message`
/// string, an optional nullable `threadId` string, and no other parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarningContract {
    method: String,
    /// Parameter name -> the JSON type names the schema allows for it.
    properties: BTreeMap<String, Vec<String>>,
    required: Vec<String>,
}

impl WarningContract {
    /// The contract this build implements, captured from the installed
    /// app-server's generated schema on codex-cli 0.157.1.
    pub fn installed() -> Self {
        let mut properties = BTreeMap::new();
        properties.insert(WARNING_MESSAGE.to_owned(), vec!["string".to_owned()]);
        properties.insert(
            WARNING_THREAD.to_owned(),
            vec!["string".to_owned(), "null".to_owned()],
        );
        Self {
            method: WARNING_METHOD.to_owned(),
            properties,
            required: vec![WARNING_MESSAGE.to_owned()],
        }
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    /// Refuses a record that is not this notification: a request id, another
    /// method, a missing required parameter, a mistyped value or an unexpected
    /// parameter. Nothing unsupported is ever written to a frontend.
    pub fn validate(&self, record: &Value) -> io::Result<()> {
        let Some(object) = record.as_object() else {
            return Err(refused("a warning record must be a JSON object"));
        };
        if object.contains_key("id") {
            return Err(refused(
                "the app-server warning contract is a server-to-client notification and has no request id",
            ));
        }
        if object.get("method").and_then(Value::as_str) != Some(self.method()) {
            return Err(refused(
                "the app-server warning contract declares no other method",
            ));
        }
        let Some(params) = object.get("params").and_then(Value::as_object) else {
            return Err(refused(
                "the app-server warning contract requires warning params",
            ));
        };
        for name in &self.required {
            if !params.contains_key(name) {
                return Err(refused(
                    "the app-server warning contract requires every required parameter",
                ));
            }
        }
        for (name, value) in params {
            let Some(allowed) = self.properties.get(name) else {
                return Err(refused(
                    "the app-server warning contract declares no such parameter",
                ));
            };
            if !allowed.iter().any(|kind| kind == json_kind(value)) {
                return Err(refused(
                    "the app-server warning contract declares no such parameter type",
                ));
            }
        }
        Ok(())
    }
}

/// Reads one `codex app-server generate-json-schema --experimental` export and
/// refuses anything but a supported warning notification: the notification
/// variant of the export's server-to-client document, the params it points at,
/// and no warning method in any client-to-server document.
///
/// The host has no generated export at runtime, so this is the capture and
/// verification tool rather than a per-run step; the deterministic check in
/// `tests/executor_warning.rs` runs it against a synthetic export and against
/// the installed one when a checkout provides it.
#[allow(dead_code)] // Exercised by the app-server schema check in tests/executor_warning.rs.
pub fn supported_contract(export: &Path) -> io::Result<WarningContract> {
    let notifications = read_schema(export, "ServerNotification.json")?;
    let variant = warning_variant(&notifications)
        .ok_or_else(|| refused("the app-server schema declares no warning notification"))?;
    let required: Vec<String> = variant
        .get("required")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if required != ["method", "params"] {
        return Err(refused(
            "the app-server warning notification does not take exactly params",
        ));
    }
    let reference = variant
        .get("properties")
        .and_then(|properties| properties.get("params"))
        .and_then(|params| params.get("$ref"))
        .and_then(Value::as_str)
        .and_then(|reference| reference.rsplit('/').next())
        .ok_or_else(|| refused("the app-server warning notification names no params"))?;
    let params = ["v2", ""]
        .iter()
        .map(|version| export.join(version).join(format!("{reference}.json")))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            refused("the app-server schema export declares no warning notification params")
        })?;
    let params: Value =
        serde_json::from_slice(&fs::read(&params)?).map_err(|error| schema_error(&error))?;
    if params.get("type").and_then(Value::as_str) != Some("object") {
        return Err(refused(
            "the app-server warning params are not an object schema",
        ));
    }
    let required = names(params.get("required"), "required")?;
    let mut properties = BTreeMap::new();
    for (name, property) in params
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| refused("the app-server warning params declare no properties"))?
    {
        let kinds = match property.get("type") {
            Some(Value::String(kind)) => vec![kind.clone()],
            Some(Value::Array(kinds)) => kinds
                .iter()
                .map(|kind| {
                    kind.as_str().map(str::to_owned).ok_or_else(|| {
                        refused("the app-server warning params declare no usable type")
                    })
                })
                .collect::<io::Result<Vec<_>>>()?,
            _ => {
                return Err(refused(
                    "the app-server warning params declare no usable type",
                ));
            }
        };
        for kind in &kinds {
            if !matches!(kind.as_str(), "string" | "null") {
                return Err(refused(
                    "this transport only supports string and nullable string warning parameters",
                ));
            }
        }
        properties.insert(name.clone(), kinds);
    }
    for document in [
        "ClientRequest.json",
        "ClientNotification.json",
        "ServerRequest.json",
    ] {
        if warning_variant(&read_schema(export, document)?).is_some() {
            return Err(refused(
                "the app-server schema declares a client-to-server warning request, which this transport does not support",
            ));
        }
    }
    Ok(WarningContract {
        method: WARNING_METHOD.to_owned(),
        properties,
        required,
    })
}

/// The method-union variant that declares method `warning`, when the document
/// declares one.
fn warning_variant(document: &Value) -> Option<&Value> {
    document
        .get("oneOf")
        .and_then(Value::as_array)?
        .iter()
        .find(|variant| {
            variant
                .get("properties")
                .and_then(|properties| properties.get("method"))
                .and_then(|method| method.get("enum"))
                .and_then(Value::as_array)
                .is_some_and(|names| names.iter().any(|name| name == WARNING_METHOD))
        })
}

fn read_schema(export: &Path, name: &str) -> io::Result<Value> {
    let path = export.join(name);
    if !path.is_file() {
        return Err(refused(
            "the app-server schema export is missing a required document",
        ));
    }
    serde_json::from_slice(&fs::read(&path)?).map_err(|error| schema_error(&error))
}

fn names(value: Option<&Value>, label: &str) -> io::Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| refused("the app-server warning params declare no usable field list"))?
        .iter()
        .map(|name| {
            name.as_str().map(str::to_owned).ok_or_else(|| {
                refused(format!(
                    "the app-server warning params declare no usable {label} field"
                ))
            })
        })
        .collect()
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// --------------------------------------------------------------------- queue

/// What happened to one diagnostic a host offered to the native warning queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Returned to the cache-guard diagnostic routing (tasks 3.1-3.2).
pub enum QueueOutcome {
    /// The diagnostic is queued for the attached frontend.
    Queued,
    /// The same diagnostic was already queued or delivered: deduplicated.
    Duplicate,
    /// The bounded queue is full; this diagnostic was not added.
    Full,
}

/// The recorded delivery state of one warning surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningState {
    /// Nothing needed to be presented yet.
    Idle,
    /// A diagnostic is queued; the frontend has not taken it yet.
    Queued,
    /// A diagnostic was written to the frontend as the captured notification.
    Sent,
    /// A delivery attempt failed; the diagnostic stays queued and healthy model
    /// work continues untouched.
    Undelivered,
    /// This surface has no native warning consumer at all.
    NoConsumer,
}

impl WarningState {
    /// The receipt and host-log spelling of this state. A successful write is
    /// recorded as a native-format send, never as a warning a human has seen.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Queued => "queued",
            Self::Sent => "native-sent",
            Self::Undelivered => "undelivered",
            Self::NoConsumer => "no-native-consumer",
        }
    }
}

/// The observable state of one warning surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarningStatus {
    /// The outcome of the most recent delivery, or of the surface itself.
    pub state: WarningState,
    /// Diagnostics still waiting for a consumer that initialized the thread.
    pub pending: usize,
}

/// The host-owned queue of degraded diagnostics for one native warning surface.
/// The relayed surface takes them once its frontend initialized the exact
/// thread; a surface without a native consumer records them without presenting
/// them, and says so.
#[derive(Clone)]
pub struct WarningQueue {
    shared: Arc<Mutex<Queue>>,
}

struct Queue {
    /// Whether a native warning consumer is attached at all.
    consumer: bool,
    /// Diagnostics waiting for that consumer.
    pending: VecDeque<String>,
    /// Every diagnostic this surface already queued, so one degraded diagnostic
    /// is never presented twice. Bounded: dedupe is best effort above it.
    seen: VecDeque<String>,
    state: WarningState,
}

impl WarningQueue {
    /// The queue of a surface that has no native warning consumer: diagnostics
    /// stay on the receipt and host-log surface, and this state says so.
    pub fn without_consumer() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Queue {
                consumer: false,
                pending: VecDeque::new(),
                seen: VecDeque::new(),
                state: WarningState::NoConsumer,
            })),
        }
    }

    /// Queues one degraded diagnostic for the native surface, once: a repeat of
    /// the same diagnostic is deduplicated, and the queue is bounded.
    #[allow(dead_code)] // Queued by the cache-guard diagnostic routing (tasks 3.1-3.2).
    pub fn queue(&self, diagnostic: &str) -> QueueOutcome {
        let mut queue = self.lock();
        if queue.seen.iter().any(|seen| seen == diagnostic) {
            return QueueOutcome::Duplicate;
        }
        if queue.pending.len() >= MAX_QUEUED {
            return QueueOutcome::Full;
        }
        if queue.seen.len() >= MAX_REMEMBERED {
            queue.seen.pop_front();
        }
        queue.seen.push_back(diagnostic.to_owned());
        queue.pending.push_back(diagnostic.to_owned());
        if queue.consumer {
            queue.state = WarningState::Queued;
        }
        QueueOutcome::Queued
    }

    /// The recorded state and queue depth of this surface.
    pub fn status(&self) -> WarningStatus {
        let queue = self.lock();
        WarningStatus {
            state: queue.state,
            pending: queue.pending.len(),
        }
    }

    fn attached() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Queue {
                consumer: true,
                pending: VecDeque::new(),
                seen: VecDeque::new(),
                state: WarningState::Idle,
            })),
        }
    }

    /// The front diagnostic awaiting its consumer, without removing it.
    fn front(&self) -> Option<String> {
        self.lock().pending.front().cloned()
    }

    /// The front diagnostic reached the frontend.
    fn confirm_delivered(&self) {
        let mut queue = self.lock();
        queue.pending.pop_front();
        if queue.consumer {
            queue.state = WarningState::Sent;
        }
    }

    /// A delivery attempt failed. The diagnostic stays queued, and the state
    /// records that no frontend has taken it.
    fn record_undelivered(&self) {
        let mut queue = self.lock();
        if queue.consumer {
            queue.state = WarningState::Undelivered;
        }
    }

    fn lock(&self) -> MutexGuard<'_, Queue> {
        // The guarded data is a plain queue: a panicking holder cannot leave it
        // inconsistent, so a poisoned lock is still readable.
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

// --------------------------------------------------------------------- relay

/// The frontend-facing warning relay of one conversation.
///
/// It serves only the native frontend's own connection; the host's control
/// connection and the recorded endpoint keep addressing the app-server.
pub struct Relay {
    port: u16,
    queue: WarningQueue,
    stop: Arc<AtomicBool>,
    sessions: Arc<Mutex<Vec<JoinHandle<()>>>>,
    worker: Option<JoinHandle<()>>,
}

impl Relay {
    /// Starts the relay on its own loopback port in front of the app-server
    /// serving `upstream_port`, authenticating frontends with the same
    /// capability token and addressing warnings to `thread_id`.
    pub fn start(upstream_port: u16, token: &str, thread_id: &str) -> io::Result<Self> {
        if upstream_port == 0 || !capability_token(token) || thread_id.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the native warning relay needs the conversation's capability endpoint and exact thread",
            ));
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let queue = WarningQueue::attached();
        let stop = Arc::new(AtomicBool::new(false));
        let sessions = Arc::new(Mutex::new(Vec::new()));
        let transport = Arc::new(Transport {
            listener,
            upstream_port,
            token: token.to_owned(),
            thread_id: thread_id.to_owned(),
            queue: queue.clone(),
            stop: Arc::clone(&stop),
            sessions: Arc::clone(&sessions),
        });
        let worker = thread::spawn(move || transport.accept_loop());
        Ok(Self {
            port,
            queue,
            stop,
            sessions,
            worker: Some(worker),
        })
    }

    /// The loopback port the native frontend attaches to. It is not the
    /// recorded control endpoint.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The host-owned queue of degraded diagnostics this relay presents.
    pub fn warnings(&self) -> WarningQueue {
        self.queue.clone()
    }

    /// Stops accepting, ends every relayed session and waits, bounded, for the
    /// relay's own threads. A frontend that never reads cannot delay this.
    pub fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let deadline = Instant::now() + CLOSE_GRACE;
        loop {
            let finished = {
                let mut sessions = self.sessions();
                sessions.retain(|handle| !handle.is_finished());
                sessions.is_empty()
            };
            if finished || Instant::now() >= deadline {
                return;
            }
            thread::sleep(ACCEPT_POLL);
        }
    }

    fn sessions(&self) -> MutexGuard<'_, Vec<JoinHandle<()>>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.close();
    }
}

/// Everything one relay needs: the listener, the conversation it fronts, and
/// the state its sessions share.
struct Transport {
    listener: TcpListener,
    upstream_port: u16,
    token: String,
    thread_id: String,
    queue: WarningQueue,
    stop: Arc<AtomicBool>,
    sessions: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Transport {
    fn accept_loop(self: Arc<Self>) {
        while !self.stop.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let transport = Arc::clone(&self);
                    let session = thread::spawn(move || transport.serve(stream));
                    let mut sessions = self
                        .sessions
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    sessions.retain(|handle| !handle.is_finished());
                    sessions.push(session);
                }
                // A transient accept failure on loopback must not end the
                // relay: the next frontend would see a refused connection.
                Err(_) => thread::sleep(ACCEPT_POLL),
            }
        }
    }

    /// Serves one frontend connection: authenticate it, connect upstream, then
    /// pass both directions through unchanged.
    // The handshake callback's error type is the WebSocket library's own
    // response type; a refused handshake cannot return a smaller one.
    #[allow(clippy::result_large_err)]
    fn serve(&self, stream: TcpStream) {
        // A stream accepted from the nonblocking listener inherits that mode on
        // Windows; the handshake needs a bounded blocking stream. The handshake
        // gets the write bound, and the relayed session tightens to its own
        // read poll so a shutdown never waits on an idle frontend.
        if stream.set_nonblocking(false).is_err()
            || stream.set_read_timeout(Some(WRITE)).is_err()
            || stream.set_write_timeout(Some(WRITE)).is_err()
        {
            return;
        }
        let token = self.token.clone();
        let mut frontend = match accept_hdr_with_config(
            stream,
            move |request: &Request, response: Response| authorize(request, &token, response),
            Some(config()),
        ) {
            Ok(frontend) => frontend,
            // An invalid or missing capability token is refused at the
            // handshake, before the app-server is touched.
            Err(_) => return,
        };
        if frontend.get_mut().set_read_timeout(Some(POLL)).is_err() {
            return;
        }
        let mut upstream = match connect_upstream(self.upstream_port, &self.token) {
            Ok(upstream) => upstream,
            Err(_) => {
                let _ = frontend.close(None);
                return;
            }
        };
        let mut frontend_state = FrontendState::default();
        while !self.stop.load(Ordering::SeqCst) {
            match frontend.read() {
                Ok(message) => {
                    observe_request(&message, &self.thread_id, &mut frontend_state.awaiting);
                    if upstream.send(message).is_err() {
                        break;
                    }
                }
                Err(error) if would_block(&error) => {}
                Err(_) => break,
            }
            match upstream.read() {
                Ok(message) => {
                    observe_response(&message, &mut frontend_state);
                    if frontend.send(message).is_err() {
                        break;
                    }
                }
                Err(error) if would_block(&error) => {}
                Err(_) => break,
            }
            // The exact thread exists on this connection by now, so a queued
            // diagnostic is appended here: after the record that initialized
            // the thread and before anything read later from the app-server.
            if frontend_state.initialized {
                deliver(
                    &mut frontend,
                    &self.queue,
                    &self.thread_id,
                    &mut frontend_state.buffered,
                );
            }
        }
        let _ = frontend.close(None);
        let _ = upstream.close(None);
    }
}

/// What one frontend connection has told this relay so far.
#[derive(Default)]
struct FrontendState {
    /// The frontend initialized or resumed the exact thread on this connection.
    initialized: bool,
    /// Request ids of thread-naming requests awaiting their answer.
    awaiting: BTreeSet<String>,
    /// A diagnostic is already serialized in the socket's own write buffer.
    buffered: bool,
}

impl FrontendState {
    fn initialize(&mut self) {
        self.initialized = true;
    }
}

/// Refuses the handshake unless the frontend presents the conversation's
/// capability token in the same `Authorization` shape the app-server accepts.
// The error type is the WebSocket library's own response type.
#[allow(clippy::result_large_err)]
fn authorize(
    request: &Request,
    token: &str,
    response: Response,
) -> Result<Response, ErrorResponse> {
    let presented = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(presented_bearer);
    if presented.is_some_and(|bearer| constant_time_eq(bearer.as_bytes(), token.as_bytes())) {
        return Ok(response);
    }
    let mut denied = ErrorResponse::new(Some(
        "the native warning relay requires this conversation's capability token".to_owned(),
    ));
    *denied.status_mut() = StatusCode::UNAUTHORIZED;
    Err(denied)
}

fn presented_bearer(header: &str) -> Option<&str> {
    let (scheme, token) = header.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
}

/// Compares a presented bearer with the expected token without leaking its
/// length or the agreeing prefix through timing.
fn constant_time_eq(presented: &[u8], expected: &[u8]) -> bool {
    if presented.len() != expected.len() {
        return false;
    }
    let mut difference = 0u8;
    for (presented, expected) in presented.iter().zip(expected) {
        difference |= presented ^ expected;
    }
    difference == 0
}

/// The capability-token shape the control transport and the app-server agree
/// on; a relay started with anything else would authenticate nothing.
fn capability_token(token: &str) -> bool {
    token.len() >= 32 && token.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn connect_upstream(port: u16, token: &str) -> io::Result<WebSocket<TcpStream>> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let stream = TcpStream::connect_timeout(&address, WRITE)?;
    stream.set_read_timeout(Some(POLL))?;
    stream.set_write_timeout(Some(WRITE))?;
    let mut request = format!("ws://{address}")
        .into_client_request()
        .map_err(protocol_error)?;
    request.headers_mut().insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(protocol_error)?,
    );
    let (socket, _) =
        client_with_config(request, stream, Some(config())).map_err(protocol_error)?;
    Ok(socket)
}

/// A thread-naming request: the frontend asks the app-server for the exact
/// thread. Any other request, or a request that names another thread, cannot
/// make this connection the one a warning may be appended to.
fn observe_request(message: &Message, thread_id: &str, awaiting: &mut BTreeSet<String>) {
    let Message::Text(text) = message else {
        return;
    };
    let Ok(record) = serde_json::from_str::<Value>(text.as_str()) else {
        return;
    };
    if record.get("method").and_then(Value::as_str).is_none() {
        return;
    }
    let Some(id) = record.get("id").filter(|id| !id.is_null()) else {
        return;
    };
    if record
        .get("params")
        .is_some_and(|params| names_thread(params, thread_id))
        && awaiting.len() < MAX_AWAITING
    {
        awaiting.insert(id.to_string());
    }
}

/// The app-server answered a thread-naming request. A served request proves the
/// frontend now holds the exact thread; a refusal proves nothing and is
/// forgotten.
fn observe_response(message: &Message, state: &mut FrontendState) {
    let Message::Text(text) = message else {
        return;
    };
    let Ok(record) = serde_json::from_str::<Value>(text.as_str()) else {
        return;
    };
    let Some(id) = record.get("id") else {
        return;
    };
    if !state.awaiting.remove(&id.to_string()) {
        return;
    }
    if record.get("error").is_none() {
        state.initialize();
    }
}

fn names_thread(value: &Value, thread_id: &str) -> bool {
    match value {
        Value::String(text) => text == thread_id,
        Value::Array(items) => items.iter().any(|item| names_thread(item, thread_id)),
        Value::Object(fields) => fields.values().any(|value| names_thread(value, thread_id)),
        _ => false,
    }
}

/// Writes every queued diagnostic, oldest first, as the captured notification.
/// A failed write is recorded state: the diagnostic stays queued, healthy model
/// work continues, and a partial write is completed by a later flush instead of
/// being duplicated.
fn deliver(
    socket: &mut WebSocket<TcpStream>,
    queue: &WarningQueue,
    thread_id: &str,
    buffered: &mut bool,
) {
    loop {
        if *buffered {
            if socket.flush().is_err() {
                queue.record_undelivered();
                return;
            }
            *buffered = false;
            queue.confirm_delivered();
            continue;
        }
        let Some(diagnostic) = queue.front() else {
            return;
        };
        let record = WarningNotification::new(&diagnostic, Some(thread_id)).record();
        if WarningContract::installed().validate(&record).is_err() {
            // This build and the captured contract disagree. Nothing outside
            // the captured shape is written, and the attempt is recorded.
            queue.record_undelivered();
            return;
        }
        match socket.send(Message::Text(record.to_string().into())) {
            Ok(()) => queue.confirm_delivered(),
            Err(error) => {
                queue.record_undelivered();
                // An interrupted write is kept in the socket's own write
                // buffer: the next flush completes it rather than repeating it.
                *buffered = would_block(&error);
                return;
            }
        }
    }
}

fn config() -> WebSocketConfig {
    // No harness-side message, frame or write-buffer size cap: the frontend's
    // own exact-thread resume state legitimately exceeds any small bound, and
    // both peers - the native frontend and the owned app-server - are local
    // processes of this host.
    WebSocketConfig::default()
        .read_buffer_size(4096)
        .write_buffer_size(0)
        .max_write_buffer_size(usize::MAX)
        .max_message_size(None)
        .max_frame_size(None)
}

fn would_block(error: &WsError) -> bool {
    matches!(
        error,
        WsError::Io(inner)
            if matches!(
                inner.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            )
    )
}

fn refused(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn schema_error(error: &serde_json::Error) -> io::Error {
    refused(format!(
        "the app-server schema export is not readable: {error}"
    ))
}

fn protocol_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
