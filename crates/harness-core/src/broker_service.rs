//! Shared, authenticated loopback service inside a self-owned Windows Job.
//! The instance lease is held from before publication through backend shutdown.
#![cfg(windows)]
use crate::{
    broker_endpoint::{Endpoint, Instance, random_key},
    broker_http::{self, RequestError},
    broker_requests::{CancelState, Requests},
    broker_state::BrokerRoot,
    process::{Cancellation, Deadline},
    process_service::ServiceGuard,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io,
    net::{Ipv4Addr, TcpListener, TcpStream},
    os::windows::io::AsRawSocket,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const POLL: Duration = Duration::from_millis(20);
const WIRE_TIMEOUT: Duration = Duration::from_secs(2);
const CLEANUP: Duration = Duration::from_secs(5);

struct WorkerControl {
    cleanup: Deadline,
    request: Option<(Deadline, TcpStream)>,
    observed_cancel: bool,
}

impl WorkerControl {
    fn poll(&mut self, cancel: &Cancellation) -> io::Result<bool> {
        if !cancel.is_cancelled()
            && let Some((deadline, stream)) = &self.request
            && (deadline.expired() || peer_finished(stream)?)
        {
            cancel.cancel();
        }
        // Cancellation can arrive through a separate authenticated connection.
        // Observe it too, and never restart the cleanup clock.
        if cancel.is_cancelled() && !self.observed_cancel {
            self.observed_cancel = true;
            self.cleanup = Deadline::after(self.cleanup.remaining().min(CLEANUP))?;
        }
        Ok(self.cleanup.expired())
    }
}

/// Called only after the complete body has been read. This one-request protocol
/// has no further client data: FIN/reset or extra bytes abandon the request.
/// Polling does not change socket mode or race a concurrent response write.
fn peer_finished(stream: &TcpStream) -> io::Result<bool> {
    use windows_sys::Win32::Networking::WinSock::{
        POLLRDNORM, WSAGetLastError, WSAPOLLFD, WSAPoll,
    };
    let mut poll = WSAPOLLFD {
        fd: stream.as_raw_socket() as usize,
        events: POLLRDNORM,
        revents: 0,
    };
    if unsafe { WSAPoll(&mut poll, 1, 0) } == -1 {
        return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
    }
    Ok(poll.revents != 0)
}

/// Providers own their child resources and honor the supplied deadline/cancel.
/// A returned value confirms request-resource reclamation, including normal
/// tool errors and cancellation. Err means infrastructure failure or unconfirmed
/// cleanup: the service stops before accepting more work.
/// `is_idle` and `status` inspect in-memory state only; they must not perform IO
/// or wait for work. Shutdown joins/reclaims all provider-owned resources.
pub trait Backend: Send + Sync + 'static {
    /// Live provider clients can keep observation alive without tool requests.
    fn keep_alive(&self) -> bool {
        false
    }
    fn call(
        &self,
        operation: &str,
        payload: &Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value>;
    fn is_idle(&self) -> bool;
    fn status(&self) -> Value;
    fn shutdown(&self, deadline: Deadline, cancel: &Cancellation) -> io::Result<()>;
}

pub struct Options {
    pub source: String,
    pub idle_timeout: Duration,
    pub request_timeout: Duration,
    pub max_connections: usize,
}

impl Options {
    fn validate(&self) -> io::Result<()> {
        if self.source.len() != 64
            || !self.source.bytes().all(|b| b.is_ascii_hexdigit())
            || self.idle_timeout.is_zero()
            || self.idle_timeout > Duration::from_secs(86400)
            || self.request_timeout.is_zero()
            || self.request_timeout > Duration::from_secs(600)
            || !(1..=64).contains(&self.max_connections)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid broker service limits or source",
            ));
        }
        Ok(())
    }
}

struct State {
    active: usize,
    last_used: Instant,
    retiring: Option<Deadline>,
}

struct Shared {
    state: Mutex<State>,
    requests: Requests,
    endpoint: Endpoint,
    backend: Arc<dyn Backend>,
    options: Options,
}

impl Shared {
    fn state(&self) -> io::Result<std::sync::MutexGuard<'_, State>> {
        self.state
            .lock()
            .map_err(|_| io::Error::other("broker state lock poisoned"))
    }
    fn status(&self) -> io::Result<Value> {
        let (active, retiring) = {
            let state = self.state()?;
            (state.active, state.retiring.is_some())
        };
        Ok(
            json!({"pid":self.endpoint.pid, "creation_time":self.endpoint.creation_time,
            "source":self.endpoint.source, "active":active, "retiring":retiring,
            "reserved_running":self.requests.running()?,
            "backend":self.backend.status()}),
        )
    }
}

struct Active<'a>(&'a Shared);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        // A poisoned state is fatal to the supervisor; never resume admission.
        if let Ok(mut state) = self.0.state.lock() {
            state.active -= 1;
            state.last_used = Instant::now();
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    operation: String,
    payload: Value,
    deadline: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Key {
    key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invocation {
    key: String,
    operation: String,
    payload: Value,
}

fn operation_valid(operation: &str) -> bool {
    !operation.is_empty()
        && operation.len() <= 64
        && operation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/' | b'.'))
}

fn provider_operation(operation: &str) -> bool {
    operation_valid(operation)
        && !matches!(operation, "status" | "retire")
        && !operation.starts_with("request/")
}

fn payload<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, DispatchFailure> {
    serde_json::from_value(value.clone()).map_err(|_| {
        DispatchFailure::Rejected(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid broker control payload",
        ))
    })
}

fn request(value: Value, maximum: Duration) -> io::Result<(Request, Deadline)> {
    let request: Request = serde_json::from_value(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid broker request"))?;
    if !operation_valid(&request.operation)
        || !request.payload.is_object()
        || !request.deadline.is_finite()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid broker request",
        ));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::other("broker system clock unavailable"))?
        .as_secs_f64();
    let seconds = request.deadline - now;
    if seconds <= 0.0 {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "broker request deadline elapsed",
        ));
    }
    // Convert once to a capped monotonic clock; wall-clock changes cannot extend work.
    let duration = Duration::from_secs_f64(seconds.min(maximum.as_secs_f64()));
    Ok((request, Deadline::after(duration)?))
}

enum DispatchFailure {
    Rejected(io::Error),
    Fatal(io::Error),
}

impl From<io::Error> for DispatchFailure {
    fn from(error: io::Error) -> Self {
        Self::Fatal(error)
    }
}

fn ledger_failure(error: io::Error) -> DispatchFailure {
    if error.kind() == io::ErrorKind::Other {
        DispatchFailure::Fatal(error)
    } else {
        DispatchFailure::Rejected(error)
    }
}

fn admission(
    state: &State,
    deadline: Deadline,
    cancel: &Cancellation,
) -> Result<(), DispatchFailure> {
    if state.retiring.is_some() {
        return Err(DispatchFailure::Rejected(io::Error::new(
            io::ErrorKind::WouldBlock,
            "broker is retiring",
        )));
    }
    if deadline.expired() || cancel.is_cancelled() {
        return Err(DispatchFailure::Rejected(io::Error::new(
            io::ErrorKind::TimedOut,
            "broker request deadline elapsed",
        )));
    }
    Ok(())
}

fn call_backend(
    shared: &Shared,
    operation: &str,
    payload: &Value,
    deadline: Deadline,
    cancel: &Cancellation,
) -> Result<Value, DispatchFailure> {
    let result = shared.backend.call(operation, payload, deadline, cancel);
    if result.is_err() {
        shared.state()?.retiring = Some(Deadline::after(Duration::ZERO)?);
    }
    result.map_err(DispatchFailure::Fatal)
}

fn dispatch(
    shared: &Shared,
    request: &Request,
    deadline: Deadline,
    cancel: &Cancellation,
    cutoff: &Mutex<WorkerControl>,
) -> Result<Value, DispatchFailure> {
    match request.operation.as_str() {
        "status" => shared.status().map_err(DispatchFailure::Fatal),
        "retire" => {
            {
                let mut state = shared.state()?;
                if state.retiring.is_none() {
                    state.retiring =
                        Some(Deadline::after(shared.options.request_timeout + CLEANUP)?);
                }
            }
            shared.status().map_err(DispatchFailure::Fatal)
        }
        "request/reserve" => {
            if request
                .payload
                .as_object()
                .is_none_or(|object| !object.is_empty())
            {
                return Err(DispatchFailure::Rejected(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid broker reservation",
                )));
            }
            let mut state = shared.state()?;
            admission(&state, deadline, cancel)?;
            let key = random_key()?;
            shared
                .requests
                .reserve(key.clone(), deadline)
                .map_err(ledger_failure)?;
            state.last_used = Instant::now();
            Ok(json!({"key":key}))
        }
        "request/invoke" => {
            let invocation: Invocation = payload(&request.payload)?;
            if !provider_operation(&invocation.operation) || !invocation.payload.is_object() {
                return Err(DispatchFailure::Rejected(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid broker invocation",
                )));
            }
            let deadline = {
                let mut state = shared.state()?;
                admission(&state, deadline, cancel)?;
                let reserved = shared
                    .requests
                    .start(&invocation.key, cancel.clone())
                    .map_err(ledger_failure)?;
                let deadline = Deadline::after(deadline.remaining().min(reserved.remaining()))?;
                state.active += 1;
                state.last_used = Instant::now();
                deadline
            };
            let _active = Active(shared);
            {
                let mut control = cutoff
                    .lock()
                    .map_err(|_| io::Error::other("broker worker clock poisoned"))?;
                if let Some((request_deadline, _)) = &mut control.request {
                    *request_deadline = deadline;
                }
                control.cleanup = Deadline::after(
                    control
                        .cleanup
                        .remaining()
                        .min(deadline.remaining() + CLEANUP),
                )?;
            }
            let result = call_backend(
                shared,
                &invocation.operation,
                &invocation.payload,
                deadline,
                cancel,
            )?;
            // Only the provider's confirmed resource reclamation permits this ACK.
            shared.requests.finish(&invocation.key)?;
            Ok(result)
        }
        "request/cancel" => {
            let key: Key = payload(&request.payload)?;
            let state = shared.requests.cancel(&key.key).map_err(ledger_failure)?;
            Ok(json!({"reclaimed":state == CancelState::Reclaimed}))
        }
        "request/release" => {
            let key: Key = payload(&request.payload)?;
            let released = shared.requests.release(&key.key).map_err(ledger_failure)?;
            Ok(json!({"released":released}))
        }
        operation => {
            if !provider_operation(operation) {
                return Err(DispatchFailure::Rejected(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unknown broker control operation",
                )));
            }
            {
                let mut state = shared.state()?;
                admission(&state, deadline, cancel)?;
                state.active += 1;
                state.last_used = Instant::now();
            }
            let _active = Active(shared);
            call_backend(shared, operation, &request.payload, deadline, cancel)
        }
    }
}

fn public_error(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::TimedOut => "broker request deadline elapsed",
        io::ErrorKind::Interrupted => "broker request cancelled",
        io::ErrorKind::WouldBlock => "broker is retiring or busy",
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => "invalid broker request",
        _ => "broker operation failed",
    }
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    body: Value,
    deadline: Deadline,
    cancel: &Cancellation,
) {
    // Broken pipes and request timeouts affect this connection, not the owner.
    let _ = broker_http::write_response(stream, status, &body, deadline, cancel);
}

fn connection(
    mut stream: TcpStream,
    shared: Arc<Shared>,
    cutoff: Arc<Mutex<WorkerControl>>,
    cancel: Cancellation,
) -> io::Result<()> {
    // Winsock accept inherits listener properties. Timed blocking IO is needed
    // here so a quiet peer does not spin through WSAEWOULDBLOCK retries.
    stream.set_nonblocking(false)?;
    let wire_deadline = Deadline::after(WIRE_TIMEOUT)?;
    let value = match broker_http::read_request(
        &mut stream,
        shared.endpoint.token(),
        wire_deadline,
        &cancel,
    ) {
        Ok(value) => value,
        Err(RequestError::Unauthorized) => {
            respond(
                &mut stream,
                403,
                json!({"error":"forbidden"}),
                wire_deadline,
                &cancel,
            );
            return Ok(());
        }
        Err(RequestError::Invalid) => {
            respond(
                &mut stream,
                400,
                json!({"error":"invalid broker request"}),
                wire_deadline,
                &cancel,
            );
            return Ok(());
        }
        Err(RequestError::Io(_)) => return Ok(()),
    };
    let (request, deadline) = match request(value, shared.options.request_timeout) {
        Ok(request) => request,
        Err(error) => {
            respond(
                &mut stream,
                400,
                json!({"error":public_error(&error)}),
                wire_deadline,
                &cancel,
            );
            return Ok(());
        }
    };
    *cutoff
        .lock()
        .map_err(|_| io::Error::other("broker worker clock poisoned"))? = WorkerControl {
        cleanup: Deadline::after(deadline.remaining() + CLEANUP)?,
        request: Some((deadline, stream.try_clone()?)),
        observed_cancel: false,
    };
    let result = dispatch(&shared, &request, deadline, &cancel, &cutoff);
    let body = match result {
        Ok(result) => json!({"result":result}),
        Err(DispatchFailure::Rejected(error)) => json!({"error":public_error(&error)}),
        Err(DispatchFailure::Fatal(error)) => return Err(error),
    };
    respond(&mut stream, 200, body, deadline, &cancel);
    Ok(())
}

struct Worker {
    handle: JoinHandle<io::Result<()>>,
    cutoff: Arc<Mutex<WorkerControl>>,
    cancel: Cancellation,
}

fn reap(workers: &mut Vec<Worker>) -> io::Result<()> {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].handle.is_finished() {
            let worker = workers.swap_remove(index);
            worker
                .handle
                .join()
                .map_err(|_| io::Error::other("broker request worker panicked"))??;
        } else {
            let worker = &workers[index];
            let mut control = worker
                .cutoff
                .lock()
                .map_err(|_| io::Error::other("broker worker clock poisoned"))?;
            if control.poll(&worker.cancel)? {
                worker.cancel.cancel();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "broker worker exceeded cleanup deadline",
                ));
            }
            index += 1;
        }
    }
    Ok(())
}

fn serve(
    root: &BrokerRoot,
    guard: &mut ServiceGuard,
    options: Options,
    backend: Arc<dyn Backend>,
) -> io::Result<()> {
    options.validate()?;
    let mut instance = Instance::claim(root)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let endpoint = instance.publish(listener.local_addr()?.port(), &options.source)?;
    let shared = Arc::new(Shared {
        requests: Requests::new(128)?,
        state: Mutex::new(State {
            active: 0,
            last_used: Instant::now(),
            retiring: None,
        }),
        endpoint,
        backend,
        options,
    });
    guard.mark_ready()?;
    let mut workers = Vec::new();
    loop {
        reap(&mut workers)?;
        {
            let mut state = shared.state()?;
            if let Some(until) = state.retiring {
                if state.active == 0 && shared.backend.is_idle() {
                    break;
                }
                if until.expired() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "broker retirement deadline elapsed",
                    ));
                }
            } else if state.active == 0
                && state.last_used.elapsed() >= shared.options.idle_timeout
                && !shared.backend.keep_alive()
                && shared.backend.is_idle()
            {
                // Gate admission under the same lock used by request workers.
                state.retiring = Some(Deadline::after(CLEANUP)?);
                break;
            }
        }
        if workers.len() < shared.options.max_connections {
            match listener.accept() {
                Ok((stream, address)) if address.ip().is_loopback() => {
                    let shared = Arc::clone(&shared);
                    let cutoff = Arc::new(Mutex::new(WorkerControl {
                        cleanup: Deadline::after(WIRE_TIMEOUT + CLEANUP)?,
                        request: None,
                        observed_cancel: false,
                    }));
                    let thread_cutoff = Arc::clone(&cutoff);
                    let cancel = Cancellation::default();
                    let thread_cancel = cancel.clone();
                    let handle = thread::Builder::new()
                        .name("broker-request".into())
                        .spawn(move || connection(stream, shared, thread_cutoff, thread_cancel))?;
                    workers.push(Worker {
                        handle,
                        cutoff,
                        cancel,
                    });
                    continue;
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        thread::sleep(POLL);
    }
    // Stop accepting, then let the retire reply and other already accepted IO
    // finish. New ordinary operations are gated even if their headers arrived earlier.
    drop(listener);
    let cleanup = Deadline::after(CLEANUP)?;
    while !workers.is_empty() {
        reap(&mut workers)?;
        if cleanup.expired() {
            for worker in &workers {
                worker.cancel.cancel();
            }
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "broker connections did not finish shutdown",
            ));
        }
        thread::sleep(POLL);
    }
    let shutdown_backend = Arc::clone(&shared.backend);
    let cancel = Cancellation::default();
    let thread_cancel = cancel.clone();
    let shutdown = thread::Builder::new()
        .name("broker-shutdown".into())
        .spawn(move || shutdown_backend.shutdown(cleanup, &thread_cancel))?;
    while !shutdown.is_finished() {
        if cleanup.expired() {
            cancel.cancel();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "broker backend did not finish shutdown",
            ));
        }
        thread::sleep(POLL);
    }
    shutdown
        .join()
        .map_err(|_| io::Error::other("broker backend shutdown panicked"))??;
    instance.close()
}

/// Enter only after `ServiceGuard::enter`, with the provider already in that
/// Job. Every return/fatal worker path exits the process and reclaims its Job;
/// an uncooperative provider cannot leave a detached request thread alive.
pub fn run(
    root: &BrokerRoot,
    mut guard: ServiceGuard,
    options: Options,
    backend: Arc<dyn Backend>,
) -> ! {
    match serve(root, &mut guard, options, backend) {
        Ok(()) => guard.exit(0),
        Err(error) => crate::process_service::exit_with_diagnostic(2, public_error(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_from_another_connection_shortens_cleanup_once() {
        let cancel = Cancellation::default();
        let mut control = WorkerControl {
            cleanup: Deadline::after(Duration::from_secs(60)).unwrap(),
            request: None,
            observed_cancel: false,
        };
        cancel.cancel();
        assert!(!control.poll(&cancel).unwrap());
        let first = control.cleanup;
        assert!(first.remaining() <= CLEANUP);
        thread::sleep(Duration::from_millis(10));
        assert!(!control.poll(&cancel).unwrap());
        assert!(control.cleanup.remaining() <= first.remaining() + Duration::from_millis(1));
    }
    #[test]
    fn peer_monitor_detects_disconnect_without_waiting_or_consuming_a_request() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let client =
            TcpStream::connect_timeout(&listener.local_addr().unwrap(), Duration::from_secs(1))
                .unwrap();
        let (server, _) = listener.accept().unwrap();
        assert!(!peer_finished(&server).unwrap());
        client.shutdown(std::net::Shutdown::Both).unwrap();
        let until = Deadline::after(Duration::from_secs(1)).unwrap();
        while !peer_finished(&server).unwrap() && !until.expired() {
            thread::sleep(POLL);
        }
        assert!(peer_finished(&server).unwrap());
    }
    #[test]
    fn unix_deadline_is_capped_and_invalid_envelopes_cannot_admit_work() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let (_, deadline) = request(
            json!({"operation":"echo","payload":{},"deadline":now+3600.0}),
            Duration::from_millis(200),
        )
        .unwrap();
        assert!(!deadline.expired());
        assert!(deadline.remaining() <= Duration::from_millis(200));
        for value in [
            json!({"operation":"echo","payload":{},"deadline":now-1.0}),
            json!({"operation":"echo","payload":[],"deadline":now+1.0}),
            json!({"operation":"echo\nsecret","payload":{},"deadline":now+1.0}),
            json!({"operation":"echo","payload":{},"deadline":now+1.0,"extra":true}),
            json!({"operation":"echo","payload":{}}),
        ] {
            assert!(request(value, Duration::from_secs(1)).is_err());
        }
    }
}
