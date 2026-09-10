//! Independent local services: a bounded WMI helper starts the same trusted
//! Rust executable outside its client's Job. That executable must dispatch
//! RUN_ARGUMENT to ServiceGuard before loading its service implementation.
//! Neither WMI's PID receipt nor the observation handle grants kill authority.
use crate::{
    cancellable_pipe::{CancellablePipe, PipeIoError, anonymous_pipe},
    dependency_mcp_probe::strict_json,
    native_build::ordinary_ancestors,
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, ProcessIdentity, StopReason},
    registration_native::ReadGuard,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::File,
    io::{self, Write},
    os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
    path::{Path, PathBuf},
    sync::mpsc::{self, Sender},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::*,
    System::{
        Console::{STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle},
        SystemInformation::GetSystemTimeAsFileTime,
        Threading::*,
    },
};

#[path = "process_service_wmi.rs"]
mod wmi;

pub const CREATE_ARGUMENT: &str = "--harness-service-create";
pub const RUN_ARGUMENT: &str = "--harness-service-run";
const MAX_REQUEST: usize = 1024 * 1024;
const MAX_STARTUP_MS: u64 = 120_000;
const CLEANUP: Duration = Duration::from_secs(5);

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn checked(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

/// Windows FILETIME units, also used by ProcessIdentity. Not a monotonic deadline.
pub fn creation_clock() -> u64 {
    let mut value = FILETIME::default();
    unsafe {
        GetSystemTimeAsFileTime(&mut value);
    }
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

fn startup_remaining(until: u64) -> io::Result<Duration> {
    let now = creation_clock() / 10_000;
    let ms = until
        .checked_sub(now)
        .filter(|ms| *ms > 0 && *ms <= MAX_STARTUP_MS)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "service startup deadline expired or invalid",
            )
        })?;
    Ok(Duration::from_millis(ms))
}

fn user(handle: HANDLE) -> io::Result<String> {
    token_sid(handle, TokenUser)
}

fn token_sid(handle: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<String> {
    let mut token = std::ptr::null_mut();
    checked(unsafe { OpenProcessToken(handle, TOKEN_QUERY, &mut token) })?;
    let token = owned(token)?;
    let mut needed = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            std::ptr::null_mut(),
            0,
            &mut needed,
        );
    }
    if needed == 0 || needed > 65536 {
        return Err(invalid("service account identity size limit"));
    }
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    checked(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    })?;
    let sid = unsafe {
        match class {
            value if value == TokenUser => (*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid,
            value if value == TokenOwner => (*(buffer.as_ptr().cast::<TOKEN_OWNER>())).Owner,
            _ => return Err(invalid("unsupported service account identity class")),
        }
    };
    checked(unsafe { IsValidSid(sid) })?;
    let size = unsafe { GetLengthSid(sid) } as usize;
    if !(8..=68).contains(&size) {
        return Err(invalid("service account SID size limit"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), size) };
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn current_user() -> io::Result<String> {
    user(unsafe { GetCurrentProcess() })
}

/// Windows may assign a new object's owner to the token's default owner group
/// (Administrators on this elevated host) while its DACL names the user alone.
pub(crate) fn current_default_owner() -> io::Result<String> {
    token_sid(unsafe { GetCurrentProcess() }, TokenOwner)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    directory: PathBuf,
    arguments: Vec<String>,
    environment: BTreeMap<String, String>,
    startup_until: u64,
    user: String,
}

impl Request {
    fn validate(&self) -> io::Result<()> {
        startup_remaining(self.startup_until)?;
        if self.user != current_user()? {
            return Err(invalid("service account identity mismatch"));
        }
        if !self.directory.is_absolute() || !self.directory.is_dir() {
            return Err(invalid("service requires an existing absolute directory"));
        }
        ordinary_ancestors(&self.directory)?;
        let mut names = BTreeSet::new();
        let mut units = 1usize;
        for (name, value) in &self.environment {
            if name.is_empty()
                || name.contains(['=', '\0'])
                || value.contains('\0')
                || !names.insert(name.to_uppercase())
            {
                return Err(invalid(
                    "service environment contains invalid or duplicate names",
                ));
            }
            units = units
                .saturating_add(name.encode_utf16().count() + value.encode_utf16().count() + 2);
        }
        if self.environment.is_empty() || units > 32767 {
            return Err(invalid(
                "service explicit environment is empty or exceeds its bound",
            ));
        }
        if self.arguments.len() > 128 || self.arguments.iter().any(|a| a.contains('\0')) {
            return Err(invalid(
                "service arguments are invalid or exceed their bound",
            ));
        }
        Ok(())
    }

    fn command(&self, program: &Path) -> io::Result<Vec<u16>> {
        let mut result = Vec::new();
        for arg in std::iter::once(program.as_os_str())
            .chain([
                OsStr::new(RUN_ARGUMENT),
                OsStr::new(&self.startup_until.to_string()),
                OsStr::new(&self.user),
            ])
            .chain(self.arguments.iter().map(OsStr::new))
        {
            if !result.is_empty() {
                result.push(32);
            }
            crate::process::quote_argument(arg, &mut result)?;
        }
        if result.len() >= 32767 {
            return Err(invalid("service command exceeds Windows bound"));
        }
        Ok(result)
    }
}

fn read_bounded(
    reader: &mut CancellablePipe,
    limit: usize,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<Vec<u8>> {
    let mut result = Vec::new();
    loop {
        let part = match reader.read(4096, deadline, cancel) {
            Ok(part) => part,
            Err(PipeIoError::EndOfFile) => return Ok(result),
            Err(error) => return Err(error.into()),
        };
        if part.is_empty() {
            return Ok(result);
        }
        if result.len() + part.len() > limit {
            return Err(invalid("service pipe input exceeds its bound"));
        }
        result.extend(part);
    }
}

/// Run only as an early command in a trusted native executable. The outer
/// spawn() call owns this helper's Job and bounds even a blocked COM provider.
/// Environment values travel through stdin and COM, never argv or handoff files.
pub fn create_helper() -> io::Result<u32> {
    let cancel = Cancellation::default();
    let mut input = CancellablePipe::reader(
        File::from(io::stdin().as_handle().try_clone_to_owned()?),
        cancel.clone(),
    )?;
    let bytes = read_bounded(
        &mut input,
        MAX_REQUEST,
        Deadline::after(Duration::from_secs(120))?,
        &cancel,
    )?;
    let request: Request = serde_json::from_value(
        strict_json(&bytes).map_err(|_| invalid("invalid service request JSON"))?,
    )
    .map_err(|_| invalid("invalid service request fields"))?;
    request.validate()?;
    let program = std::env::current_exe()?.canonicalize()?;
    let _program_guard = ReadGuard::open(&program)?;
    let command = request.command(&program)?;
    let directory = request
        .directory
        .to_str()
        .ok_or_else(|| invalid("service directory is not Unicode"))?;
    let environment: Vec<_> = request
        .environment
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    // Do not invoke WMI if validation/setup already consumed the deadline.
    startup_remaining(request.startup_until)?;
    wmi::create(&command, directory, &environment)
}

/// Fixed bootstrap protocol. Validation errors never include the request body.
pub fn create_helper_entry() -> ! {
    let (reply, code) = match create_helper() {
        Ok(pid) => (serde_json::json!({"pid": pid}), 0),
        Err(error) => (serde_json::json!({"error": error.to_string()}), 2),
    };
    let output = serde_json::to_vec(&reply).unwrap_or_default();
    let _ = io::stdout().write_all(&output);
    let _ = io::stdout().flush();
    std::process::exit(code)
}

/// Read-only retained process observation. Drop closes this handle, never the
/// service. Identity errors preserve the process and do not confer ownership.
pub struct ServiceProcess {
    handle: OwnedHandle,
    identity: ProcessIdentity,
}

impl ServiceProcess {
    pub fn observe(
        pid: u32,
        program: &Path,
        created_after: u64,
        expected_user: &str,
    ) -> io::Result<Self> {
        let process = Self::open(pid)?;
        if process.identity.creation_time < created_after
            || process.identity.creation_time > creation_clock()
        {
            return Err(io::Error::other(
                "service process identity mismatch; preserving process",
            ));
        }
        process.validate(program, expected_user)?;
        if !process.is_running()? {
            return Err(io::Error::other(
                "service exited before identity validation",
            ));
        }
        Ok(process)
    }

    /// Read an exact recorded identity. A missing/exited/reused PID is stale;
    /// access failures and a live same-identity image/account mismatch remain
    /// errors. Neither outcome grants authority to terminate that process.
    pub fn inspect(
        identity: ProcessIdentity,
        program: &Path,
        expected_user: &str,
    ) -> io::Result<Option<Self>> {
        let process = match Self::open(identity.pid) {
            Ok(process) => process,
            Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if process.identity != identity || !process.is_running()? {
            return Ok(None);
        }
        process.validate(program, expected_user)?;
        Ok(Some(process))
    }

    fn open(pid: u32) -> io::Result<Self> {
        let handle = owned(unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        })?;
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut cpu = FILETIME::default();
        checked(unsafe {
            GetProcessTimes(
                handle.as_raw_handle(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut cpu,
            )
        })?;
        let creation =
            (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        Ok(Self {
            handle,
            identity: ProcessIdentity {
                pid,
                creation_time: creation,
            },
        })
    }

    fn validate(&self, program: &Path, expected_user: &str) -> io::Result<()> {
        let mut image = vec![0u16; 32768];
        let mut length = image.len() as u32;
        checked(unsafe {
            QueryFullProcessImageNameW(
                self.handle.as_raw_handle(),
                0,
                image.as_mut_ptr(),
                &mut length,
            )
        })?;
        use std::os::windows::ffi::OsStringExt;
        let actual = PathBuf::from(std::ffi::OsString::from_wide(&image[..length as usize]))
            .canonicalize()?;
        if actual != program.canonicalize()? || user(self.handle.as_raw_handle())? != expected_user
        {
            return Err(io::Error::other(
                "service process identity mismatch; preserving process",
            ));
        }
        Ok(())
    }
    pub fn identity(&self) -> ProcessIdentity {
        self.identity
    }
    pub fn is_running(&self) -> io::Result<bool> {
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub fn wait_for_exit(&self, deadline: Deadline) -> io::Result<bool> {
        while self.is_running()? {
            if deadline.expired() {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(20).min(deadline.remaining()));
        }
        Ok(true)
    }
    pub fn exit_code(&self) -> io::Result<Option<u32>> {
        if self.is_running()? {
            return Ok(None);
        }
        let mut code = 0;
        checked(unsafe { GetExitCodeProcess(self.handle.as_raw_handle(), &mut code) })?;
        Ok(Some(code))
    }
}

/// Caller supplies a trusted executable implementing both bootstrap commands,
/// an owned directory and the COMPLETE service environment. No acquisition,
/// shell resolution, service registration or persistent launcher is performed.
pub fn spawn(
    program: &Path,
    directory: &Path,
    arguments: Vec<String>,
    environment: BTreeMap<String, String>,
    deadline: Deadline,
    cancel: &Cancellation,
) -> io::Result<ServiceProcess> {
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "service startup cancelled",
        ));
    }
    let remaining = deadline.remaining();
    if remaining.is_zero() || remaining > Duration::from_millis(MAX_STARTUP_MS) {
        return Err(invalid(
            "service startup requires a deadline within 120 seconds",
        ));
    }
    if !program.is_absolute() {
        return Err(invalid("service executable must be absolute"));
    }
    let _program_guard = ReadGuard::open(program)?;
    let began = creation_clock();
    let request = Request {
        directory: directory.into(),
        arguments,
        environment,
        startup_until: began / 10_000 + remaining.as_millis() as u64,
        user: current_user()?,
    };
    request.validate()?;
    request.command(program)?;
    let bytes = serde_json::to_vec(&request)
        .map_err(|_| invalid("service request serialization failed"))?;
    if bytes.len() > MAX_REQUEST {
        return Err(invalid("service request exceeds its bound"));
    }
    let (stdin, write) = anonymous_pipe(4096)?;
    let (read, stdout) = anonymous_pipe(4096)?;
    let mut command = CommandSpec::new(program);
    command.args.push(CREATE_ARGUMENT.into());
    command.current_dir = Some(directory.into());
    command.stdin = Some(stdin);
    command.stdout = Some(stdout);
    // Helper failure stdout is a fixed numeric error only; stderr is NUL.
    let job = Job::new(Limits {
        memory_bytes: Some(256 * 1024 * 1024),
        cpu_percent: Some(25.0),
    })?;
    let child = job.spawn(&command)?;
    drop(command);
    let io_result = (|| -> io::Result<Vec<u8>> {
        let mut writer = CancellablePipe::writer(write, cancel.clone())?;
        writer.write_all(&bytes, deadline, cancel)?;
        writer.close(Deadline::after(CLEANUP)?)?;
        let mut reader = CancellablePipe::reader(read, cancel.clone())?;
        let result = read_bounded(&mut reader, 1024, deadline, cancel);
        reader.close(Deadline::after(CLEANUP)?)?;
        result
    })();
    let bytes = match io_result {
        Ok(bytes) => bytes,
        Err(error) => {
            job.terminate(130, CLEANUP)?;
            return Err(error);
        }
    };
    let outcome = job.wait(&child, deadline, cancel, CLEANUP)?;
    if outcome.reason != StopReason::Exited || outcome.job.active_processes != 0 {
        return Err(io::Error::other(
            "native service helper failed or exceeded its bound",
        ));
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Receipt {
        pid: Option<u32>,
        error: Option<String>,
    }
    let receipt: Receipt = serde_json::from_value(
        strict_json(&bytes).map_err(|_| invalid("invalid service helper receipt"))?,
    )
    .map_err(|_| invalid("invalid service helper receipt"))?;
    match (outcome.exit_code, receipt.pid, receipt.error) {
        (0, Some(pid), None) => ServiceProcess::observe(pid, program, began, &request.user),
        (2, None, Some(error)) => Err(io::Error::other(format!("native service helper: {error}"))),
        _ => Err(invalid("inconsistent service helper receipt")),
    }
}

/// The service owns this Job, not any client. Bootstrap callers must enter
/// before opening backend code or starting children, publish readiness only
/// after their instance lock + endpoint, and implement a bounded idle shutdown.
pub struct ServiceGuard {
    job: Job,
    ready: Option<Sender<()>>,
    watchdog: Option<std::thread::JoinHandle<()>>,
    startup_deadline: Deadline,
    standard_streams: Option<[File; 3]>,
}
impl Drop for ServiceGuard {
    fn drop(&mut self) {
        // Closing the self-owned Job during unwinding otherwise terminates this
        // process with zero on Windows, hiding the panic. Explicit exit begins
        // process teardown first; the OS still closes the Job and reaps children.
        std::process::exit(if std::thread::panicking() { 101 } else { 2 });
    }
}
/// Logging must never postpone mandatory Job reclamation. A provider can hold
/// Rust's stderr lock, or the destination itself can block. Give this best-effort
/// fixed diagnostic a separate bounded opportunity, then exit regardless.
pub(crate) fn exit_with_diagnostic(code: i32, message: &'static str) -> ! {
    if let Ok(logger) = std::thread::Builder::new()
        .name("service-fatal-log".into())
        .spawn(move || eprintln!("{message}"))
    {
        if let Ok(until) = Deadline::after(Duration::from_millis(100)) {
            while !logger.is_finished() && !until.expired() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        if logger.is_finished() {
            let _ = logger.join();
        }
    }
    std::process::exit(code)
}

impl ServiceGuard {
    pub fn enter(startup_until: u64, expected_user: &str, limits: Limits) -> io::Result<Self> {
        let job = Job::new(limits)?;
        if job.contain_current_process().is_err() {
            std::process::exit(2);
        }
        // Once self-contained, exit explicitly on bootstrap errors. Dropping
        // the live Job would otherwise obscure the selected failure exit code.
        match current_user() {
            Ok(actual) if actual == expected_user => (),
            _ => std::process::exit(2),
        }
        let remaining =
            startup_remaining(startup_until).unwrap_or_else(|_| std::process::exit(124));
        let startup_deadline = Deadline::after(remaining).unwrap_or_else(|_| std::process::exit(2));
        let (ready, receive) = mpsc::channel();
        let watchdog = std::thread::Builder::new()
            .name("service-readiness".into())
            .spawn(move || {
                if startup_deadline.expired()
                    || receive.recv_timeout(startup_deadline.remaining()).is_err()
                {
                    exit_with_diagnostic(
                        124,
                        "Shared service startup deadline elapsed before endpoint publication",
                    );
                }
            })
            .unwrap_or_else(|_| std::process::exit(2));
        Ok(Self {
            job,
            ready: Some(ready),
            watchdog: Some(watchdog),
            startup_deadline,
            standard_streams: None,
        })
    }
    /// Set native handles before loading service code. The caller owns and
    /// has validated the writable local log. Its clones live with the service.
    pub fn redirect_standard_streams(&mut self, log: &File) -> io::Result<()> {
        if self.standard_streams.is_some() || !log.metadata()?.is_file() {
            return Err(invalid("service logging requires one ordinary file setup"));
        }
        self.standard_streams = Some([File::open("NUL")?, log.try_clone()?, log.try_clone()?]);
        let handles = self.standard_streams.as_ref().unwrap();
        for (kind, file) in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
            .into_iter()
            .zip(handles)
        {
            checked(unsafe { SetStdHandle(kind, file.as_raw_handle()) })?;
        }
        Ok(())
    }
    pub fn job(&self) -> &Job {
        &self.job
    }
    pub fn mark_ready(&mut self) -> io::Result<()> {
        if let Some(ready) = self.ready.take() {
            if self.startup_deadline.expired() {
                std::process::exit(124);
            }
            ready
                .send(())
                .map_err(|_| io::Error::other("service readiness deadline expired"))?;
        }
        if let Some(watchdog) = self.watchdog.take() {
            watchdog
                .join()
                .map_err(|_| io::Error::other("service readiness monitor failed"))?;
        }
        Ok(())
    }
    /// OS process teardown closes the non-inheritable Job and reclaims children.
    pub fn exit(self, code: i32) -> ! {
        std::process::exit(code)
    }
}
