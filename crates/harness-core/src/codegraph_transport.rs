//! Bounded direct MCP transport. The caller owns package and index validation.
#![cfg(windows)]
use crate::{
    cancellable_pipe::{CancellablePipe, PipeIoError, anonymous_pipe},
    mcp_protocol::{Kind, Message},
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, Outcome},
};
use serde_json::{Value, json};
use std::{
    io::{self, Read},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

const FRAME_LIMIT: usize = 256 * 1024;
const CLEANUP: Duration = Duration::from_secs(5);

fn join_bounded<T>(worker: JoinHandle<T>, name: &str) -> io::Result<T> {
    let deadline = Deadline::after(CLEANUP + Duration::from_secs(1))?;
    while !worker.is_finished() {
        if deadline.expired() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("CodeGraph {name} cleanup was not confirmed"),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    worker
        .join()
        .map_err(|_| io::Error::other(format!("CodeGraph {name} panicked")))
}

/// A Windows semaphore is account-wide and independent of CODEX_HOME or cwd.
/// Its permit can be released by the supervising thread after Job reclamation.
struct Slot(std::os::windows::io::OwnedHandle);
impl Slot {
    fn acquire() -> io::Result<Self> {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0,
            System::Threading::{CreateSemaphoreW, WaitForSingleObject},
        };
        let name: Vec<u16> = format!(
            "Global\\CodingAgentsHarness.CodeGraph.{}",
            crate::process_service::current_user()?
        )
        .encode_utf16()
        .chain([0])
        .collect();
        let raw = unsafe { CreateSemaphoreW(std::ptr::null(), 1, 1, name.as_ptr()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let handle = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(raw) };
        if unsafe { WaitForSingleObject(raw, 0) } != WAIT_OBJECT_0 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "CodeGraph account worker slot is busy; no additional worker started",
            ));
        }
        Ok(Self(handle))
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            windows_sys::Win32::System::Threading::ReleaseSemaphore(
                self.0.as_raw_handle(),
                1,
                std::ptr::null_mut(),
            );
        }
    }
}

/// Keep the account permit across quiescent generation recovery/commit as well
/// as the child process. Only the native runtime owner may reuse a permit.
#[derive(Clone)]
pub(crate) struct Admission(Arc<Slot>);
impl Admission {
    pub(crate) fn acquire() -> io::Result<Self> {
        Ok(Self(Arc::new(Slot::acquire()?)))
    }
}

#[derive(Default)]
struct Diagnostics {
    bytes: Vec<u8>,
    omitted: usize,
    line: Vec<u8>,
    watcher_active: bool,
    completed_refreshes: u64,
    failure: Option<String>,
}

impl Diagnostics {
    fn append(&mut self, bytes: &[u8]) {
        let keep = bytes.len().min(32768usize.saturating_sub(self.bytes.len()));
        self.bytes.extend_from_slice(&bytes[..keep]);
        self.omitted = self.omitted.saturating_add(bytes.len() - keep);
        for &byte in bytes {
            if byte == b'\n' {
                let line = String::from_utf8_lossy(&self.line);
                if line.contains("[CodeGraph MCP] File watcher active") {
                    self.watcher_active = true;
                }
                if line.contains("[CodeGraph MCP] Auto-synced ")
                    || line.contains("[CodeGraph MCP] Caught up ")
                {
                    self.completed_refreshes = self.completed_refreshes.saturating_add(1);
                }
                if [
                    "[CodeGraph MCP] Auto-sync error:",
                    "[CodeGraph MCP] Catch-up sync failed:",
                    "[CodeGraph MCP] File watcher degraded",
                    "[CodeGraph MCP] File watcher unavailable",
                ]
                .iter()
                .any(|marker| line.contains(marker))
                {
                    self.failure.get_or_insert_with(|| line.into_owned());
                }
                self.line.clear();
            } else if self.line.len() < 4096 {
                self.line.push(byte);
            }
        }
        if self.omitted > FRAME_LIMIT {
            self.failure
                .get_or_insert_with(|| "CodeGraph diagnostic output exceeded its bound".into());
        }
    }
}

/// Read-only storage/ownership check, evaluated before launch and while the
/// owned process is alive. A failed check terminates that process tree.
pub type Monitor = Arc<dyn Fn() -> io::Result<()> + Send + Sync>;

/// Result of a deliberate published CLI operation. Output is captured once,
/// bounded in memory and never treated as proof of successful indexing alone.
#[derive(serde::Serialize)]
pub struct CommandResult {
    pub outcome: Outcome,
    pub stdout: String,
    pub stderr: String,
    pub omitted_stdout_bytes: usize,
    pub omitted_stderr_bytes: usize,
    pub monitor_failure: Option<String>,
}

fn capture(mut input: impl Read, stop: Cancellation) -> io::Result<(Vec<u8>, usize)> {
    let mut retained = Vec::new();
    let mut omitted = 0usize;
    let mut bytes = [0; 4096];
    loop {
        let count = input.read(&mut bytes)?;
        if count == 0 {
            return Ok((retained, omitted));
        }
        let keep = count.min(FRAME_LIMIT.saturating_sub(retained.len()));
        retained.extend_from_slice(&bytes[..keep]);
        omitted = omitted.saturating_add(count - keep);
        if omitted > FRAME_LIMIT {
            stop.cancel();
        }
    }
}

pub fn run_command(
    command: CommandSpec,
    deadline: Deadline,
    cancel: &Cancellation,
    monitor: Option<Monitor>,
) -> io::Result<CommandResult> {
    run_command_admitted(command, deadline, cancel, monitor, Admission::acquire()?)
}

pub(crate) fn run_command_admitted(
    mut command: CommandSpec,
    deadline: Deadline,
    cancel: &Cancellation,
    monitor: Option<Monitor>,
    admission: Admission,
) -> io::Result<CommandResult> {
    if cancel.is_cancelled() || deadline.expired() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "CodeGraph operation cancelled or expired",
        ));
    }
    let operation_deadline = Deadline::after(deadline.remaining().min(Duration::from_secs(600)))?;
    if let Some(check) = &monitor {
        check()?;
    }
    let slot = admission.0;
    let (stdin, write) = anonymous_pipe(4096)?;
    let (read, stdout) = anonymous_pipe(4096)?;
    let (error_read, stderr) = anonymous_pipe(4096)?;
    command.stdin = Some(stdin);
    command.stdout = Some(stdout);
    command.stderr = Some(stderr);
    let job = Job::new(Limits {
        memory_bytes: Some(2 * 1024 * 1024 * 1024),
        cpu_percent: Some(25.0),
    })?;
    let process = job.spawn(&command)?;
    drop(command);
    drop(write); // Deliberate operations cannot solicit interactive input.
    let stop = Cancellation::default();
    let out_stop = stop.clone();
    let err_stop = stop.clone();
    let output_reader = thread::spawn(move || capture(read, out_stop));
    let error_reader = thread::spawn(move || capture(error_read, err_stop));
    let external = cancel.clone();
    let watch_stop = stop.clone();
    let failure = Arc::new(Mutex::new(None));
    let watch_failure = failure.clone();
    let watcher = thread::spawn(move || {
        while !watch_stop.is_cancelled() {
            if external.is_cancelled() {
                watch_stop.cancel();
                break;
            }
            if let Some(check) = &monitor
                && let Err(error) = check()
            {
                if let Ok(mut failure) = watch_failure.lock() {
                    *failure = Some(error.to_string());
                }
                watch_stop.cancel();
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
    let outcome = job.wait(&process, operation_deadline, &stop, CLEANUP);
    stop.cancel();
    if outcome
        .as_ref()
        .is_ok_and(|result| result.job.active_processes == 0)
    {
        drop(slot);
    } else {
        std::mem::forget(slot);
    }
    let output = join_bounded(output_reader, "command stdout").and_then(|v| v);
    let errors = join_bounded(error_reader, "command stderr").and_then(|v| v);
    join_bounded(watcher, "command monitor")?;
    let outcome = outcome?;
    let (stdout, omitted_stdout_bytes) = output?;
    let (stderr, omitted_stderr_bytes) = errors?;
    let monitor_failure = failure
        .lock()
        .map_err(|_| io::Error::other("CodeGraph command monitor poisoned"))?
        .clone();
    Ok(CommandResult {
        outcome,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        omitted_stdout_bytes,
        omitted_stderr_bytes,
        monitor_failure,
    })
}

pub struct Worker {
    identity: crate::process::ProcessIdentity,
    input: Option<CancellablePipe>,
    output: Option<CancellablePipe>,
    pending: Vec<u8>,
    rejected_frame: Option<Vec<u8>>,
    next_id: u64,
    stop: Cancellation,
    supervisor: Option<JoinHandle<io::Result<Outcome>>>,
    stderr: Option<JoinHandle<io::Result<()>>>,
    monitor: Option<JoinHandle<()>>,
    diagnostics: Arc<Mutex<Diagnostics>>,
    lease: Deadline,
}

impl Worker {
    /// This worker's immutable command/root and nonrenewable lease govern all
    /// watcher/catch-up work. No upstream daemon or owned scripts are launched.
    pub fn start(command: CommandSpec, lease: Duration, cancel: &Cancellation) -> io::Result<Self> {
        Self::start_monitored(command, lease, cancel, None)
    }

    pub fn start_monitored(
        command: CommandSpec,
        lease: Duration,
        cancel: &Cancellation,
        monitor: Option<Monitor>,
    ) -> io::Result<Self> {
        Self::start_admitted(command, lease, cancel, monitor, Admission::acquire()?)
    }

    pub(crate) fn start_admitted(
        mut command: CommandSpec,
        lease: Duration,
        cancel: &Cancellation,
        monitor: Option<Monitor>,
        admission: Admission,
    ) -> io::Result<Self> {
        if lease.is_zero() || lease > Duration::from_secs(600) || cancel.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid/cancelled CodeGraph worker lease",
            ));
        }
        if let Some(check) = &monitor {
            check()?;
        }
        let slot = admission.0;
        let (stdin, write) = anonymous_pipe(4096)?;
        let (read, stdout) = anonymous_pipe(4096)?;
        let (mut error_read, stderr) = anonymous_pipe(4096)?;
        command.stdin = Some(stdin);
        command.stdout = Some(stdout);
        command.stderr = Some(stderr);
        let job = Job::new(Limits {
            memory_bytes: Some(2 * 1024 * 1024 * 1024),
            cpu_percent: Some(25.0),
        })?;
        let process = job.spawn(&command)?;
        let identity = process.identity();
        drop(command);
        let stop = Cancellation::default();
        let deadline = Deadline::after(lease)?;
        let job_stop = stop.clone();
        let supervisor = thread::spawn(move || {
            let outcome = job.wait(&process, deadline, &job_stop, CLEANUP);
            job_stop.cancel();
            if outcome
                .as_ref()
                .is_ok_and(|result| result.job.active_processes == 0)
            {
                drop(slot);
            } else {
                // Keep admission closed in this host if cleanup is uncertain.
                // The handle is reclaimed when the host itself exits.
                std::mem::forget(slot);
            }
            outcome
        });
        let diagnostics = Arc::new(Mutex::new(Diagnostics::default()));
        let retained = diagnostics.clone();
        let error_stop = stop.clone();
        let stderr = thread::spawn(move || {
            let mut bytes = [0; 4096];
            loop {
                let count = error_read.read(&mut bytes)?;
                if count == 0 {
                    return Ok(());
                }
                let mut log = retained
                    .lock()
                    .map_err(|_| io::Error::other("CodeGraph diagnostics poisoned"))?;
                log.append(&bytes[..count]);
                if log.failure.is_some() {
                    error_stop.cancel();
                }
            }
        });
        let monitor = monitor.map(|check| {
            let check_stop = stop.clone();
            let retained = diagnostics.clone();
            thread::spawn(move || {
                while !check_stop.is_cancelled() {
                    if let Err(error) = check() {
                        if let Ok(mut log) = retained.lock() {
                            log.failure = Some(format!("CodeGraph storage monitor: {error}"));
                        }
                        check_stop.cancel();
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            })
        });
        // Construct the owner before fallible pipe initialization, so failure
        // still joins the supervisor and closes every inherited handle.
        let mut worker = Self {
            identity,
            input: None,
            output: None,
            pending: Vec::new(),
            rejected_frame: None,
            next_id: 0,
            stop,
            supervisor: Some(supervisor),
            stderr: Some(stderr),
            monitor,
            diagnostics,
            lease: deadline,
        };
        worker.input = Some(CancellablePipe::writer(write, worker.stop.clone())?);
        worker.output = Some(CancellablePipe::reader(read, worker.stop.clone())?);
        Ok(worker)
    }

    pub fn diagnostics(&self) -> Value {
        match self.diagnostics.lock() {
            Ok(log) => {
                json!({"stderr":String::from_utf8_lossy(&log.bytes),"omitted_stderr_bytes":log.omitted,
                    "watcher_active":log.watcher_active,"completed_refreshes":log.completed_refreshes,"failure":log.failure})
            }
            Err(_) => json!({"error":"CodeGraph diagnostic storage failed"}),
        }
    }

    pub fn identity(&self) -> Value {
        json!({"pid":self.identity.pid,"creation_time":self.identity.creation_time,"lease_expired":self.lease.expired()})
    }

    pub fn failure_diagnostics(&self) -> Value {
        let mut value = self.diagnostics();
        let frame = self.rejected_frame.as_deref().unwrap_or(&self.pending);
        let kept = frame.len().min(32768);
        if kept > 0 {
            value["upstream_frame_prefix"] = json!(String::from_utf8_lossy(&frame[..kept]));
            value["captured_frame_bytes"] = json!(frame.len());
            value["frame_detail_partial"] = json!(true);
        }
        value
    }

    pub fn initialize(&mut self, deadline: Deadline, cancel: &Cancellation) -> io::Result<Value> {
        let result = self.request("initialize",json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"coding-agents-harness-pack","version":env!("CARGO_PKG_VERSION")}}),deadline,cancel)?;
        if result.get("error").is_some() || !result["result"]["serverInfo"].is_object() {
            return Err(io::Error::other(
                "CodeGraph initialization failed or omitted server identity",
            ));
        }
        self.send(
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            deadline,
            cancel,
        )?;
        Ok(result["result"].clone())
    }

    fn send(&mut self, value: Value, deadline: Deadline, cancel: &Cancellation) -> io::Result<()> {
        if self.lease.expired() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "CodeGraph worker lease expired; explicit new request required",
            ));
        }
        let mut data = serde_json::to_vec(&value)?;
        if data.len() > 16384 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "CodeGraph request exceeds frame allowance",
            ));
        }
        data.push(b'\n');
        self.input
            .as_mut()
            .ok_or_else(|| io::Error::other("CodeGraph input closed"))?
            .write_all(&data, deadline, cancel)?;
        Ok(())
    }

    fn next(&mut self, deadline: Deadline, cancel: &Cancellation) -> io::Result<Message> {
        loop {
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=end).collect();
                let bytes = &line[..line.len() - 1];
                let parsed = Message::parse(bytes.strip_suffix(b"\r").unwrap_or(bytes));
                if parsed.is_err() {
                    self.rejected_frame = Some(line);
                }
                return parsed;
            }
            if self.pending.len() > FRAME_LIMIT {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CodeGraph upstream frame exceeds 256 KiB capture limit",
                ));
            }
            let chunk = self
                .output
                .as_mut()
                .ok_or_else(|| io::Error::other("CodeGraph output closed"))?
                .read(4096, deadline, cancel)
                .map_err(|error| match error {
                    PipeIoError::EndOfFile => io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "CodeGraph upstream closed before its response",
                    ),
                    error => error.into(),
                })?;
            if chunk.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "CodeGraph upstream closed before its response",
                ));
            }
            self.pending.extend_from_slice(&chunk);
            if self
                .pending
                .iter()
                .position(|b| *b == b'\n')
                .unwrap_or(self.pending.len())
                > FRAME_LIMIT
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CodeGraph upstream frame exceeds 256 KiB capture limit",
                ));
            }
        }
    }

    /// Return the full bounded upstream envelope; provider errors remain errors.
    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        let deadline = Deadline::after(deadline.remaining().min(self.lease.remaining()))?;
        self.next_id += 1;
        let id = json!(self.next_id);
        self.send(
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            deadline,
            cancel,
        )?;
        let mut notifications = 0;
        loop {
            let message = self.next(deadline, cancel)?;
            match message.kind() {
                Kind::Result | Kind::Error if message.id() == Some(&id) => {
                    return Ok(message.into_value());
                }
                Kind::Notification => {
                    notifications += 1;
                    if notifications > 64 {
                        return Err(io::Error::other("CodeGraph notification flood"));
                    }
                    if message.method() == Some("notifications/message") {
                        let text = serde_json::to_vec(message.value())?;
                        let mut log = self
                            .diagnostics
                            .lock()
                            .map_err(|_| io::Error::other("CodeGraph diagnostics poisoned"))?;
                        let keep = text.len().min(32768usize.saturating_sub(log.bytes.len()));
                        log.bytes.extend_from_slice(&text[..keep]);
                        log.omitted += text.len() - keep;
                    }
                }
                Kind::Request => {
                    self.send(json!({"jsonrpc":"2.0","id":message.id(),"error":{"code":-32601,"message":"Managed CodeGraph has a fixed root and no client-side methods"}}),deadline,cancel)?;
                    notifications += 1;
                    if notifications > 64 {
                        return Err(io::Error::other("CodeGraph reverse-request flood"));
                    }
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unexpected CodeGraph response identity",
                    ));
                }
            }
        }
    }

    pub fn tool(
        &mut self,
        name: &str,
        arguments: Value,
        deadline: Deadline,
        cancel: &Cancellation,
    ) -> io::Result<Value> {
        let limits = json!({"limit":arguments.get("limit"),"depth":arguments.get("depth"),"exhaustive":false});
        let reply = self.request(
            "tools/call",
            json!({"name":name,"arguments":arguments}),
            deadline,
            cancel,
        )?;
        let mut diagnostics = self.diagnostics();
        if diagnostics["failure"].is_string() {
            return Err(io::Error::other(
                diagnostics["failure"].as_str().unwrap().to_owned(),
            ));
        }
        if let Some(error) = reply.get("error") {
            return Ok(json!({"isError":true,"error":error,"diagnostics":diagnostics}));
        }
        let mut result = reply
            .get("result")
            .cloned()
            .ok_or_else(|| io::Error::other("CodeGraph result is missing"))?;
        if !result.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CodeGraph tool result must be an object",
            ));
        }
        let content = result.get("content").and_then(Value::as_array);
        if content.is_none_or(|blocks| {
            blocks
                .iter()
                .any(|block| block["type"] != "text" || !block["text"].is_string())
        }) || (content.is_some_and(Vec::is_empty)
            && result.get("structuredContent").is_none()
            && result["isError"] != true)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CodeGraph tool content is missing, unsupported or unexpectedly empty",
            ));
        }
        if result
            .to_string()
            .contains("showing all definitions instead")
        {
            result["isError"] = json!(true);
            result["qualifier_mismatch"] = json!(true);
        }
        // These exact startup notices add no warning beyond the watcher and
        // refresh state. Preserve every other diagnostic, including failures.
        if name != "codegraph_status"
            && let Some(stderr) = diagnostics["stderr"].as_str()
        {
            diagnostics["stderr"] = json!(stderr.lines().filter(|line| !matches!(*line,
                    "[CodeGraph MCP] File watcher debounce: 500ms (CODEGRAPH_WATCH_DEBOUNCE_MS)"
                    | "[CodeGraph MCP] File watcher active — graph will auto-sync on changes"))
                    .collect::<Vec<_>>().join("\n"));
        }
        if diagnostics["stderr"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
            || diagnostics.get("error").is_some()
        {
            result["diagnostics"] = diagnostics;
        }
        result["worker"] = self.identity();
        result["request_limits"] = limits;
        if matches!(
            name,
            "codegraph_callers" | "codegraph_callees" | "codegraph_impact"
        ) {
            result["managed_ambiguity"] =
                json!(
                    result
                        .get("content")
                        .and_then(Value::as_array)
                        .is_some_and(|blocks| blocks
                            .iter()
                            .filter_map(|block| block["text"].as_str())
                            .any(|text| text.lines().any(|line| line.starts_with("**")
                                && line.contains(" distinct definitions"))))
                );
        }
        Ok(result)
    }

    pub fn close(&mut self) -> io::Result<Option<Outcome>> {
        self.stop.cancel();
        let mut failure = None;
        for pipe in [self.input.take(), self.output.take()]
            .into_iter()
            .flatten()
        {
            if let Err(error) = pipe.close(Deadline::after(CLEANUP)?) {
                failure = Some(io::Error::from(error));
            }
        }
        let outcome = self
            .supervisor
            .take()
            .map(|worker| join_bounded(worker, "supervisor").and_then(|v| v))
            .transpose()?;
        if let Some(worker) = self.stderr.take() {
            join_bounded(worker, "stderr reader")??;
        }
        if let Some(worker) = self.monitor.take() {
            join_bounded(worker, "storage monitor")?;
        }
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(outcome)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
