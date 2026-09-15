//! Provider-independent lazy MCP stdio forwarding.
//!
//! One serialized backend worker is owned through a Windows Job with real
//! pipes. It starts only when the first tool request arrives, retires after a
//! bounded idle window, and restarts on the next request after a failure.
//! Admission leases serialize shared-resource use per request; a preflight
//! rejection or a per-request failure keeps the connection and the worker
//! usable. Only an unconfirmed tree reclamation poisons the forwarder and
//! requires closing its connection. Worker stdout is parsed strictly as
//! JSON-RPC and never echoed; worker stderr goes to an owned file.
#![cfg(windows)]

use crate::{
    cancellable_pipe::{CancellablePipe, anonymous_pipe},
    mcp_protocol::{Decoder, Kind, Message, READ_CHUNK},
    process::{
        Cancellation, CommandSpec, Deadline, Job, Limits, Outcome, OwnedProcess, ProcessIdentity,
    },
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CLEANUP: Duration = Duration::from_secs(8);
const WATCHDOG_POLL: Duration = Duration::from_millis(25);

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Launch inputs for one owned backend worker. The provider fully selects the
/// command, environment, working directory, initialize parameters, stderr sink
/// and Job limits; nothing is discovered or acquired here.
pub struct WorkerLaunch {
    pub command: CommandSpec,
    pub stderr: PathBuf,
    pub limits: Limits,
    pub initialize: Value,
    pub expected_server: Option<String>,
    pub startup: Duration,
}

impl WorkerLaunch {
    fn validate(&self) -> io::Result<()> {
        if !self.command.program.is_absolute() {
            return Err(invalid("lazy worker command must be absolute"));
        }
        if self.stderr.as_os_str().is_empty() {
            return Err(invalid("lazy worker stderr sink is required"));
        }
        if !self.initialize.is_object() {
            return Err(invalid(
                "lazy worker initialize parameters must be an object",
            ));
        }
        if self.startup.is_zero() {
            return Err(invalid("lazy worker startup bound must be positive"));
        }
        Ok(())
    }
}

/// A shared-resource admission lease. Acquisition may block until the bounded
/// resource is available; the release after each request must be observable.
pub trait Lease: Send {
    fn acquire(&mut self, deadline: &Deadline, cancellation: &Cancellation) -> io::Result<()>;
    fn release(&mut self) -> io::Result<()>;
}

/// A preflight refusal: no worker operation ran and the owned worker stays
/// usable for later requests.
#[derive(Debug)]
pub struct Rejection {
    pub message: String,
}

impl Rejection {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Inspect and optionally rewrite request arguments before admission.
pub type BeforeRequest =
    Box<dyn Fn(&str, &Value) -> io::Result<Result<Option<Value>, Rejection>> + Send>;
/// Post-process a successful worker result.
pub type AfterRequest = Box<dyn Fn(&str, &Value, Value) -> io::Result<Value> + Send>;
/// Select an admission lease for a request.
pub type LeaseFor = Box<dyn Fn(&str, &Value) -> Option<Box<dyn Lease>> + Send>;
/// Retire provider-owned companions after a confirmed idle worker reclaim.
/// Returned pairs update the next worker launch environment.
pub type OnIdle =
    Box<dyn FnMut() -> io::Result<Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>> + Send>;

struct Worker {
    job: Job,
    child: OwnedProcess,
    input: Option<CancellablePipe>,
    output: CancellablePipe,
    decoder: Decoder,
    stop: Cancellation,
    next_id: u64,
}

impl Worker {
    fn start(launch: &WorkerLaunch) -> io::Result<Self> {
        if let Some(parent) = launch.stderr.parent() {
            fs::create_dir_all(parent)?;
        }
        let stderr = File::create(&launch.stderr)?;
        let (stdin, write) = anonymous_pipe(4096)?;
        let (read, stdout) = anonymous_pipe(4096)?;
        let mut command = clone_spec(&launch.command)?;
        command.stdin = Some(stdin);
        command.stdout = Some(stdout);
        command.stderr = Some(stderr);
        let job = Job::new(launch.limits)?;
        let child = job.spawn(&command)?;
        drop(command);
        let stop = Cancellation::default();
        let mut worker = Self {
            job,
            child,
            input: Some(CancellablePipe::writer(write, stop.clone())?),
            output: CancellablePipe::reader(read, stop.clone())?,
            decoder: Decoder::default(),
            stop,
            next_id: 0,
        };
        worker.initialize(launch)?;
        Ok(worker)
    }

    fn identity(&self) -> ProcessIdentity {
        self.child.identity()
    }

    fn send(
        &mut self,
        value: Value,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.input
            .as_mut()
            .ok_or_else(|| io::Error::other("lazy worker input closed"))?
            .write_all(&bytes, deadline, cancellation)
            .map_err(io::Error::from)
    }

    fn next(&mut self, deadline: Deadline, cancellation: &Cancellation) -> io::Result<Message> {
        loop {
            if let Some(message) = self.decoder.next_message()? {
                return Ok(message);
            }
            let bytes = self
                .output
                .read(READ_CHUNK, deadline, cancellation)
                .map_err(io::Error::from)?;
            if bytes.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "lazy worker closed its pipe",
                ));
            }
            self.decoder.push(&bytes)?;
        }
    }

    fn initialize(&mut self, launch: &WorkerLaunch) -> io::Result<()> {
        let deadline = Deadline::after(launch.startup)?;
        let cancellation = Cancellation::default();
        let result = self.request(
            "initialize",
            launch.initialize.clone(),
            deadline,
            &cancellation,
        )?;
        if result.get("error").is_some() {
            return Err(io::Error::other("lazy worker initialize failed"));
        }
        if let Some(expected) = &launch.expected_server
            && result["result"]["serverInfo"]["name"].as_str() != Some(expected.as_str())
        {
            return Err(io::Error::other("lazy worker server identity mismatch"));
        }
        self.notify(
            "notifications/initialized",
            json!({}),
            deadline,
            &cancellation,
        )
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> io::Result<Value> {
        self.next_id += 1;
        let id = json!(self.next_id);
        self.send(
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            deadline,
            cancellation,
        )?;
        loop {
            let message = self.next(deadline, cancellation)?;
            match message.kind() {
                Kind::Result | Kind::Error if message.id() == Some(&id) => {
                    return Ok(message.into_value());
                }
                // Worker notifications never answer a pending request and are
                // not forwarded through this generic layer.
                Kind::Notification => continue,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unexpected lazy worker response identity",
                    ));
                }
            }
        }
    }

    fn notify(
        &mut self,
        method: &str,
        params: Value,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> io::Result<()> {
        self.send(
            json!({"jsonrpc":"2.0","method":method,"params":params}),
            deadline,
            cancellation,
        )
    }

    fn call_tool(
        &mut self,
        name: &str,
        arguments: Value,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> io::Result<Value> {
        let result = self.request(
            "tools/call",
            json!({"name":name,"arguments":arguments}),
            deadline,
            cancellation,
        )?;
        if result.get("error").is_some() {
            return Err(io::Error::other("lazy worker tool call failed"));
        }
        Ok(result["result"].clone())
    }

    fn reclaim(mut self) -> io::Result<Outcome> {
        if let Some(input) = self.input.take() {
            let _ = input.close(Deadline::after(CLEANUP)?);
        }
        self.job
            .wait(&self.child, Deadline::after(CLEANUP)?, &self.stop, CLEANUP)
    }
}

fn clone_spec(spec: &CommandSpec) -> io::Result<CommandSpec> {
    Ok(CommandSpec {
        program: spec.program.clone(),
        args: spec.args.clone(),
        current_dir: spec.current_dir.clone(),
        env: spec.env.clone(),
        inherit_console: spec.inherit_console,
        new_console: spec.new_console.clone(),
        stdin: spec
            .stdin
            .as_ref()
            .map(|file| file.try_clone())
            .transpose()?,
        stdout: spec
            .stdout
            .as_ref()
            .map(|file| file.try_clone())
            .transpose()?,
        stderr: spec
            .stderr
            .as_ref()
            .map(|file| file.try_clone())
            .transpose()?,
    })
}

struct Inner {
    launch: WorkerLaunch,
    idle: Duration,
    lease_for: Option<LeaseFor>,
    before: Option<BeforeRequest>,
    after: Option<AfterRequest>,
    worker: Option<Worker>,
    lease: Option<Box<dyn Lease>>,
    last_used: Instant,
    generation: u64,
    retired: Option<Outcome>,
    poisoned: Option<String>,
    closed: bool,
    watchdog: Option<JoinHandle<()>>,
    on_idle: Option<OnIdle>,
}

fn tool_error(message: impl AsRef<str>) -> Value {
    json!({
        "content":[{"type":"text","text":message.as_ref()}],
        "isError":true
    })
}

fn release_lease(inner: &mut Inner) -> io::Result<()> {
    if let Some(mut lease) = inner.lease.take() {
        lease.release()?;
    }
    Ok(())
}

/// The provider-independent lazy forwarder. Requests serialize on one lock,
/// matching the seam's bounded single-worker queue.
pub struct LazyWorker {
    inner: Arc<Mutex<Inner>>,
    stop: Arc<AtomicBool>,
}

impl LazyWorker {
    /// Create the forwarder and its idle watchdog. The worker itself starts
    /// only on the first request.
    pub fn new(
        launch: WorkerLaunch,
        idle: Duration,
        lease_for: Option<LeaseFor>,
        before: Option<BeforeRequest>,
        after: Option<AfterRequest>,
    ) -> io::Result<Arc<Self>> {
        launch.validate()?;
        if idle.is_zero() {
            return Err(invalid("lazy worker idle bound must be positive"));
        }
        let inner = Arc::new(Mutex::new(Inner {
            launch,
            idle,
            lease_for,
            before,
            after,
            worker: None,
            lease: None,
            last_used: Instant::now(),
            generation: 0,
            retired: None,
            poisoned: None,
            closed: false,
            watchdog: None,
            on_idle: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let watchdog = {
            let inner = Arc::clone(&inner);
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("lazy-worker-watchdog".into())
                .spawn(move || watch(inner, stop))?
        };
        let forwarder = Arc::new(Self {
            inner: Arc::clone(&inner),
            stop: Arc::clone(&stop),
        });
        inner.lock().unwrap().watchdog = Some(watchdog);
        Ok(forwarder)
    }

    /// Install an idle companion-retirement hook. Safe before the first worker
    /// starts; idle retirement requires a live worker.
    pub fn set_on_idle(self: &Arc<Self>, hook: OnIdle) {
        self.inner.lock().unwrap().on_idle = Some(hook);
    }

    /// The number of workers started so far; zero means nothing started.
    pub fn generation(&self) -> u64 {
        self.inner.lock().unwrap().generation
    }

    /// Whether an owned worker is currently started.
    pub fn has_worker(&self) -> bool {
        self.inner.lock().unwrap().worker.is_some()
    }

    /// The identity of the current worker, when started.
    pub fn worker_identity(&self) -> Option<ProcessIdentity> {
        self.inner
            .lock()
            .unwrap()
            .worker
            .as_ref()
            .map(Worker::identity)
    }

    /// The most recent confirmed tree retirement, idle or failure induced.
    pub fn retirement(&self) -> Option<Outcome> {
        self.inner.lock().unwrap().retired
    }

    /// Execute one serialized tool request against the lazy worker. A normal
    /// value, including an `isError` tool result, keeps the connection; after
    /// a cleanly reclaimed failure the next request restarts the worker. An
    /// `Err` return means cleanup could not be confirmed and the host
    /// connection must close.
    pub fn call(
        &self,
        name: &str,
        arguments: Value,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> io::Result<Value> {
        let mut inner = self.inner.lock().unwrap();
        if inner.closed {
            return Err(io::Error::other("lazy forwarder is closed"));
        }
        if let Some(reason) = inner.poisoned.clone() {
            return Err(io::Error::other(reason));
        }
        inner.last_used = Instant::now();
        if inner.worker.is_none() {
            match Worker::start(&inner.launch) {
                Ok(worker) => {
                    inner.generation += 1;
                    inner.worker = Some(worker);
                }
                Err(error) => {
                    return Ok(tool_error(format!("lazy worker failed to start: {error}")));
                }
            }
        }
        let mut arguments = arguments;
        if let Some(before) = &inner.before {
            match before(name, &arguments) {
                Ok(Ok(updated)) => {
                    if let Some(updated) = updated {
                        arguments = updated;
                    }
                }
                Ok(Err(rejection)) => {
                    return Ok(tool_error(rejection.message));
                }
                Err(error) => {
                    return Ok(tool_error(format!("lazy worker preflight failed: {error}")));
                }
            }
        }
        let mut lease = inner
            .lease_for
            .as_ref()
            .and_then(|lease_for| lease_for(name, &arguments));
        if let Some(lease) = lease.as_mut()
            && let Err(error) = lease.acquire(&deadline, cancellation)
        {
            return Ok(tool_error(format!(
                "lazy worker admission was not granted: {error}"
            )));
        }
        inner.lease = lease;
        let result = inner
            .worker
            .as_mut()
            .expect("worker was started")
            .call_tool(name, arguments.clone(), deadline, cancellation);
        match result {
            Ok(value) => {
                let final_value = match &inner.after {
                    Some(after) => match after(name, &arguments, value) {
                        Ok(processed) => processed,
                        Err(error) => tool_error(format!("lazy worker result rejected: {error}")),
                    },
                    None => value,
                };
                if let Err(error) = release_lease(&mut inner) {
                    return Ok(tool_error(format!(
                        "lazy worker admission release failed: {error}"
                    )));
                }
                Ok(final_value)
            }
            Err(error) => {
                let _ = release_lease(&mut inner);
                // Transport, deadline or cancellation failure: reclaim the
                // whole tree before answering anything.
                let worker = inner.worker.take().expect("worker was started");
                match worker.reclaim() {
                    Ok(outcome) => {
                        inner.retired = Some(outcome);
                        if cancellation.is_cancelled() {
                            // The serving session suppresses the cancelled
                            // response once cleanup is confirmed.
                            return Ok(json!({"content":[],"isError":true}));
                        }
                        Ok(tool_error(format!(
                            "lazy worker request failed ({error}); the owned tree was reclaimed and the next request restarts it"
                        )))
                    }
                    Err(reclaim) => {
                        let reason = format!("lazy worker tree reclamation failed: {reclaim}");
                        inner.poisoned = Some(reason.clone());
                        Err(io::Error::other(reason))
                    }
                }
            }
        }
    }

    /// Start the owned worker if needed and list its tools. Handshake and
    /// tools/list stay off the tool-call admission/preflight path so a catalogue
    /// cache miss can populate without a model-facing call.
    pub fn list_tools(
        &self,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> io::Result<Vec<Value>> {
        let mut inner = self.inner.lock().unwrap();
        if inner.closed {
            return Err(io::Error::other("lazy forwarder is closed"));
        }
        if let Some(reason) = inner.poisoned.clone() {
            return Err(io::Error::other(reason));
        }
        inner.last_used = Instant::now();
        if inner.worker.is_none() {
            match Worker::start(&inner.launch) {
                Ok(worker) => {
                    inner.generation += 1;
                    inner.worker = Some(worker);
                }
                Err(error) => {
                    return Err(io::Error::other(format!(
                        "lazy worker failed to start: {error}"
                    )));
                }
            }
        }
        let result = inner.worker.as_mut().expect("worker was started").request(
            "tools/list",
            json!({}),
            deadline,
            cancellation,
        );
        match result {
            Ok(value) => {
                if value.get("error").is_some() {
                    return Err(io::Error::other("lazy worker tools/list failed"));
                }
                value["result"]["tools"]
                    .as_array()
                    .cloned()
                    .ok_or_else(|| io::Error::other("lazy worker tools/list is not a catalogue"))
            }
            Err(error) => {
                let worker = inner.worker.take().expect("worker was started");
                match worker.reclaim() {
                    Ok(outcome) => {
                        inner.retired = Some(outcome);
                        Err(io::Error::other(format!(
                            "lazy worker tools/list failed ({error}); the owned tree was reclaimed"
                        )))
                    }
                    Err(reclaim) => {
                        let reason = format!("lazy worker tree reclamation failed: {reclaim}");
                        inner.poisoned = Some(reason.clone());
                        Err(io::Error::other(reason))
                    }
                }
            }
        }
    }

    /// Retire the worker now and stop the watchdog. Failing to confirm tree
    /// reclamation is an error; a second close is a no-op.
    pub fn close(&self) -> io::Result<()> {
        let (worker, watchdog) = {
            let mut inner = self.inner.lock().unwrap();
            inner.closed = true;
            (inner.worker.take(), inner.watchdog.take())
        };
        self.stop.store(true, Ordering::SeqCst);
        let mut result = Ok(());
        if let Some(worker) = worker
            && let Err(error) = worker.reclaim()
        {
            result = Err(error);
        }
        if let Some(watchdog) = watchdog {
            let _ = watchdog.join();
        }
        result
    }
}

fn watch(inner: Arc<Mutex<Inner>>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        thread::sleep(WATCHDOG_POLL);
        let mut inner = inner.lock().unwrap();
        if inner.closed || inner.poisoned.is_some() {
            return;
        }
        if inner.worker.is_some()
            && inner.lease.is_none()
            && inner.last_used.elapsed() >= inner.idle
        {
            let worker = inner.worker.take().expect("worker presence was checked");
            match worker.reclaim() {
                Ok(outcome) => inner.retired = Some(outcome),
                Err(error) => {
                    inner.poisoned = Some(format!("idle retirement failed: {error}"));
                    return;
                }
            }
            let mut hook = inner.on_idle.take();
            let updates = if let Some(hook) = hook.as_mut() {
                hook()
            } else {
                Ok(Vec::new())
            };
            inner.on_idle = hook;
            match updates {
                Ok(updates) => {
                    for (name, value) in updates {
                        inner.launch.command.env.insert(name, value);
                    }
                }
                Err(error) => {
                    inner.poisoned = Some(format!("idle retirement failed: {error}"));
                    return;
                }
            }
        }
    }
}
