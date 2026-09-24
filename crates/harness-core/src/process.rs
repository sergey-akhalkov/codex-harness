//! Owned Windows process trees. Jobs are anonymous, non-inheritable and created
//! with kill-on-close. Windows 10+ is required for atomic assignment through
//! JOB_LIST at creation. Neither PIDs nor a caller's containing job confer
//! cleanup authority. Children inherit the environment, accept an explicit cwd
//! and, unless `inherit_console` is set, inherit only allow-listed standard
//! file handles. Interactive console inheritance stays inside the same Job.
//! Besides those exclusive lifecycle jobs, this owner provides one account-wide
//! CPU-rate-only budget Job and one account-wide aggregate memory Job.
//! Participants join those objects, create their payload with their lifecycle
//! Job innermost, and gain no termination authority over peers. The shared CPU
//! job stays rate-only; the aggregate job carries only
//! `JOB_OBJECT_LIMIT_JOB_MEMORY`.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(20);

/// A configured program that Windows cannot load as an image (a broken or
/// non-executable file) otherwise raises a modal loader dialog on the user's
/// desktop for every attempt. The failure must reach the caller as an
/// `io::Error`. Bounded spawning calls this itself; a caller that deliberately
/// starts a possibly invalid program through another API (native launch, an
/// explicit failure fixture) calls it before that attempt.
pub fn suppress_loader_dialogs() {
    use std::sync::Once;
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode(
            SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX,
        );
    });
}

/// Default aggregate CPU ceiling for all local agent work of one Windows
/// account: 75% of total host CPU capacity, expressed like `Limits::cpu_percent`
/// and never per session, project, command or thread.
pub const SHARED_CPU_PERCENT: f64 = 75.0;

/// Optional account directory override for the shared CPU budget, with the same
/// precedence as the heavy-command account: explicit directory, this variable,
/// then the machine account location.
pub const CPU_BUDGET_ACCOUNT_ENV: &str = "CODEX_HARNESS_CPU_ACCOUNT";

/// Account-local storage for the shared CPU budget state. Selection depends on
/// the Windows account alone: no project, checkout, `CODEX_HOME`, terminal tab
/// or build directory participates.
pub fn cpu_budget_directory(explicit: Option<&Path>) -> io::Result<PathBuf> {
    let directory = match explicit {
        Some(path) => path.to_owned(),
        None => match std::env::var_os(CPU_BUDGET_ACCOUNT_ENV).filter(|value| !value.is_empty()) {
            Some(value) => PathBuf::from(value),
            None => {
                let parent = std::env::var_os("LOCALAPPDATA")
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        io::Error::other(
                            "shared CPU budget account storage is unavailable; LOCALAPPDATA is not set",
                        )
                    })?;
                PathBuf::from(parent)
                    .join("coding-agents-harness")
                    .join("cpu-budget")
            }
        },
    };
    if !directory.is_absolute()
        || directory
            .components()
            .any(|part| part == Component::ParentDir)
    {
        return Err(invalid(
            "the shared CPU budget account directory must be absolute and normalized",
        ));
    }
    Ok(directory)
}

#[derive(Clone, Debug, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// A mandatory monotonic deadline; overflowing durations are rejected.
#[derive(Clone, Copy, Debug)]
pub struct Deadline(Instant);

impl Deadline {
    pub fn after(duration: Duration) -> io::Result<Self> {
        Instant::now()
            .checked_add(duration)
            .map(Self)
            .ok_or_else(|| invalid("deadline exceeds the monotonic clock range"))
    }
    pub fn remaining(self) -> Duration {
        self.0.saturating_duration_since(Instant::now())
    }
    pub fn expired(self) -> bool {
        self.remaining().is_zero()
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn check_stop(deadline: Deadline, cancellation: &Cancellation) -> io::Result<()> {
    if cancellation.is_cancelled() {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
    }
    if deadline.expired() {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "deadline expired"));
    }
    Ok(())
}

/// Whole-file exclusive OS lock. Keep the stable lock file; unlinking/replacing
/// it would let two callers lock different file objects under the same path.
/// Acquisition does not truncate existing content; closing the handle unlocks.
#[derive(Debug)]
pub struct ExclusiveFileLock {
    _file: File,
}

impl ExclusiveFileLock {
    pub fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
        Self::acquire_file(path, true)
    }

    /// Locks an existing object without creating or writing a file.
    pub(crate) fn try_acquire_existing(path: &Path) -> io::Result<Option<Self>> {
        Self::acquire_file(path, false)
    }

    fn acquire_file(path: &Path, create: bool) -> io::Result<Option<Self>> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(create)
            .create(create)
            .truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Deny deletion/rename while this file object participates in locking.
            options.share_mode(3); // FILE_SHARE_READ | FILE_SHARE_WRITE
        }
        let file = options.open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }

    pub fn acquire(
        path: &Path,
        deadline: Deadline,
        cancellation: &Cancellation,
    ) -> io::Result<Self> {
        loop {
            check_stop(deadline, cancellation)?;
            if let Some(lock) = Self::try_acquire(path)? {
                return Ok(lock);
            }
            std::thread::sleep(POLL.min(deadline.remaining()));
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    /// Aggregate committed bytes across all members, not working-set size.
    pub memory_bytes: Option<usize>,
    /// Windows hard cap in percent of system CPU, quantized down to 0.01%.
    pub cpu_percent: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub creation_time: u64,
}

#[derive(Debug)]
pub struct CommandSpec {
    /// Explicit absolute executable path; no PATH or shell resolution.
    pub program: std::path::PathBuf,
    pub args: Vec<std::ffi::OsString>,
    pub current_dir: Option<std::path::PathBuf>,
    /// Overrides affect only the new child; None removes the named variable.
    pub env: std::collections::BTreeMap<std::ffi::OsString, Option<std::ffi::OsString>>,
    /// Inherit the parent's console and standard streams for an interactive CLI.
    /// Cannot be combined with redirected handles, a new console or a pseudoconsole.
    pub inherit_console: bool,
    /// Create a separate visible interactive console with this initial title.
    /// Its standard devices cannot be combined with redirected streams or ConPTY.
    pub new_console: Option<std::ffi::OsString>,
    /// None selects NUL. Clones of one File may be used for a combined log.
    pub stdin: Option<File>,
    pub stdout: Option<File>,
    pub stderr: Option<File>,
}

impl CommandSpec {
    pub fn new(program: impl Into<std::path::PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            current_dir: None,
            env: Default::default(),
            inherit_console: false,
            new_console: None,
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }

    /// Snapshot only the three standard handles; absent handles retain NUL.
    #[cfg(windows)]
    pub fn inherit_standard_streams(&mut self) -> io::Result<()> {
        use std::os::windows::io::{AsRawHandle, BorrowedHandle, RawHandle};
        fn file(raw: RawHandle) -> io::Result<Option<File>> {
            if raw.is_null() || raw as isize == -1 {
                return Ok(None);
            }
            // Standard handles remain owned by the process; clone before storing.
            let handle = unsafe { BorrowedHandle::borrow_raw(raw) };
            Ok(Some(handle.try_clone_to_owned()?.into()))
        }
        self.stdin = file(std::io::stdin().as_raw_handle())?;
        self.stdout = file(std::io::stdout().as_raw_handle())?;
        self.stderr = file(std::io::stderr().as_raw_handle())?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum StopReason {
    Exited,
    Timeout,
    Cancelled,
    MemoryLimit,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct JobSnapshot {
    pub active_processes: u32,
    pub memory_limit_bytes: usize,
    pub peak_job_memory_bytes: usize,
    pub cpu_rate: u32,
    pub cpu_hard_cap: bool,
    pub kill_on_close: bool,
    pub handle_inheritable: bool,
    pub cpu_time: Duration,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Outcome {
    pub reason: StopReason,
    /// 124 timeout, 125 observed memory limit, 130 cancellation; otherwise the
    /// actual unsigned Windows exit code. No exit status is silently truncated.
    pub exit_code: u32,
    pub process_exit_code: u32,
    pub job: JobSnapshot,
}

/// Kernel readback of the shared account CPU budget. `cpu_rate` is in 0.01%
/// units of total host CPU capacity, so 7500 is the default 75% ceiling.
/// Readback proves configuration, not measured consumption.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct SharedCpuSnapshot {
    pub cpu_rate: u32,
    pub cpu_hard_cap: bool,
    /// Always false by contract: the accounting job owns no cleanup.
    pub kill_on_close: bool,
    /// Always zero by contract: the accounting job carries no memory limit.
    pub job_memory_limit_bytes: usize,
    pub breakaway_ok: bool,
    pub silent_breakaway_ok: bool,
    pub active_processes: u32,
    pub cpu_time: Duration,
}

/// Kernel readback of one account's aggregate memory job. Readback proves
/// configuration, not measured consumption. A matching object has
/// `JOB_OBJECT_LIMIT_JOB_MEMORY` set, `JobMemoryLimit` equal to the requested
/// envelope, and no per-process memory limit, CPU rate, kill-on-close or
/// breakaway flag.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct HeavyAggregateSnapshot {
    /// `JOBOBJECT_BASIC_LIMIT_INFORMATION::LimitFlags`.
    pub limit_flags: u32,
    pub job_memory_limit_bytes: usize,
    pub process_memory_limit_bytes: usize,
    pub cpu_rate: u32,
    pub cpu_hard_cap: bool,
    pub kill_on_close: bool,
    pub breakaway_ok: bool,
    pub silent_breakaway_ok: bool,
    pub active_processes: u32,
}

#[cfg(not(windows))]
#[derive(Debug)]
pub struct SharedCpuBudget;

#[cfg(not(windows))]
impl SharedCpuBudget {
    pub fn acquire(_: &Path, _: f64) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows 10+ Job Objects are required",
        ))
    }

    pub fn acquire_within(_: &Path, _: f64, _: Deadline, _: &Cancellation) -> io::Result<Self> {
        Self::acquire(Path::new("."), 0.0)
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
pub struct HeavyAggregate;

#[cfg(not(windows))]
impl HeavyAggregate {
    pub fn acquire(_: &Path, _: usize) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows 10+ Job Objects are required",
        ))
    }

    pub fn acquire_within(_: &Path, _: usize, _: Deadline, _: &Cancellation) -> io::Result<Self> {
        Self::acquire(Path::new("."), 0)
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
pub struct Job;

#[cfg(not(windows))]
impl Job {
    pub fn new(_: Limits) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows 10+ Job Objects are required",
        ))
    }

    pub fn new_named(_: Limits, _: &str) -> io::Result<Self> {
        Self::new(Limits::default())
    }
}

#[cfg(windows)]
pub(crate) use windows::quote as quote_argument;
#[cfg(windows)]
pub use windows::{HeavyAggregate, Job, OwnedProcess, SharedCpuBudget, SuspendedProcess};

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::OsStr;
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::IO::*;
    use windows_sys::Win32::System::JobObjects::*;
    use windows_sys::Win32::System::Threading::*;

    // winnt.h; windows-sys exposes this notification in SystemServices, which
    // is otherwise unused here. See JOBOBJECT_ASSOCIATE_COMPLETION_PORT docs.
    const JOB_OBJECT_MSG_JOB_MEMORY_LIMIT: u32 = 10;

    fn checked(value: i32) -> io::Result<()> {
        if value == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    // All handles passed here were newly returned by a successful native create
    // or open call. Pseudo-handles are never wrapped in OwnedHandle.
    fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    fn inherited_copy(file: &File) -> io::Result<OwnedHandle> {
        let mut handle = null_mut();
        unsafe {
            checked(DuplicateHandle(
                GetCurrentProcess(),
                file.as_raw_handle(),
                GetCurrentProcess(),
                &mut handle,
                0,
                1,
                DUPLICATE_SAME_ACCESS,
            ))?;
        }
        owned(handle)
    }

    fn environment(
        overrides: &std::collections::BTreeMap<std::ffi::OsString, Option<std::ffi::OsString>>,
    ) -> io::Result<Option<Vec<u16>>> {
        if overrides.is_empty() {
            return Ok(None);
        }
        let mut variables: Vec<_> = std::env::vars_os().collect();
        for (name, value) in overrides {
            // Harness-owned override names are ASCII. Values retain full UTF-16.
            let key = name
                .to_str()
                .filter(|s| !s.is_empty() && s.is_ascii() && !s.contains(['=', '\0']))
                .ok_or_else(|| invalid("invalid child environment variable name"))?;
            variables.retain(|(existing, _)| !existing.to_string_lossy().eq_ignore_ascii_case(key));
            if let Some(value) = value {
                variables.push((name.clone(), value.clone()));
            }
        }
        variables.sort_by_key(|(name, _)| name.to_string_lossy().to_uppercase());
        let mut block = Vec::new();
        for (name, value) in variables {
            let name = wide(&name)?;
            let value = wide(&value)?;
            block.extend_from_slice(&name[..name.len() - 1]);
            block.push('=' as u16);
            block.extend_from_slice(&value);
        }
        if block.is_empty() {
            block.push(0);
        }
        block.push(0);
        Ok(Some(block))
    }

    fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
        let mut value: Vec<u16> = value.encode_wide().collect();
        if value.contains(&0) {
            return Err(invalid("embedded NUL in process input"));
        }
        value.push(0);
        Ok(value)
    }

    // Microsoft CRT argv quoting, operating on UTF-16 so unpaired surrogates
    // are preserved. Always quote, including empty arguments and trailing '\'.
    pub(crate) fn quote(value: &OsStr, result: &mut Vec<u16>) -> io::Result<()> {
        result.push(34);
        let mut slashes = 0;
        for unit in value.encode_wide() {
            if unit == 0 {
                return Err(invalid("embedded NUL in process argument"));
            }
            if unit == 92 {
                slashes += 1;
                continue;
            }
            result.extend(std::iter::repeat_n(
                92,
                if unit == 34 { 2 * slashes + 1 } else { slashes },
            ));
            slashes = 0;
            result.push(unit);
        }
        result.extend(std::iter::repeat_n(92, 2 * slashes));
        result.push(34);
        Ok(())
    }

    fn ticks(value: FILETIME) -> u64 {
        ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
    }

    fn times(handle: HANDLE) -> io::Result<(u64, Duration)> {
        let (mut creation, mut exit, mut kernel, mut user) =
            unsafe { (zeroed(), zeroed(), zeroed(), zeroed()) };
        unsafe {
            checked(GetProcessTimes(
                handle,
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            ))?;
        }
        Ok((
            ticks(creation),
            Duration::from_nanos((ticks(kernel) + ticks(user)).saturating_mul(100)),
        ))
    }

    fn exited(handle: HANDLE) -> io::Result<bool> {
        match unsafe { WaitForSingleObject(handle, 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }

    fn in_job(process: HANDLE, job: HANDLE) -> io::Result<bool> {
        let mut member = 0;
        unsafe {
            checked(IsProcessInJob(process, job, &mut member))?;
        }
        Ok(member != 0)
    }

    /// Retains the process object even after exit; PID reuse cannot redirect
    /// observation or cleanup. Dropping this handle does not close the job.
    #[derive(Debug)]
    pub struct OwnedProcess {
        handle: OwnedHandle,
        identity: ProcessIdentity,
    }

    impl AsHandle for OwnedProcess {
        fn as_handle(&self) -> BorrowedHandle<'_> {
            self.handle.as_handle()
        }
    }

    impl OwnedProcess {
        pub fn identity(&self) -> ProcessIdentity {
            self.identity
        }
        pub fn is_running(&self) -> io::Result<bool> {
            Ok(!exited(self.handle.as_raw_handle())?)
        }
        pub fn cpu_time(&self) -> io::Result<Duration> {
            Ok(times(self.handle.as_raw_handle())?.1)
        }
        pub fn exit_code(&self) -> io::Result<Option<u32>> {
            if self.is_running()? {
                return Ok(None);
            }
            let mut code = 0;
            unsafe {
                checked(GetExitCodeProcess(self.handle.as_raw_handle(), &mut code))?;
            }
            Ok(Some(code))
        }
        pub fn wait_for_exit(&self, timeout: Duration) -> io::Result<bool> {
            let deadline = Deadline::after(timeout)?;
            loop {
                if !self.is_running()? {
                    return Ok(true);
                }
                if deadline.expired() {
                    return Ok(false);
                }
                std::thread::sleep(POLL.min(deadline.remaining()));
            }
        }
        pub fn wait_unbounded(&self) -> io::Result<()> {
            match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), INFINITE) } {
                WAIT_OBJECT_0 => Ok(()),
                WAIT_FAILED => Err(io::Error::last_os_error()),
                _ => Err(io::Error::other("unexpected process wait status")),
            }
        }
    }

    /// Dropping before resume terminates only the newly created retained handle.
    /// The enclosing job also protects this interval against owner crashes.
    #[derive(Debug)]
    pub struct SuspendedProcess {
        process: Option<OwnedProcess>,
        thread: OwnedHandle,
    }

    impl SuspendedProcess {
        pub fn process(&self) -> &OwnedProcess {
            self.process.as_ref().expect("pending process")
        }
        pub fn resume(mut self) -> io::Result<OwnedProcess> {
            if unsafe { ResumeThread(self.thread.as_raw_handle()) } == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            Ok(self.process.take().expect("pending process"))
        }
    }

    impl Drop for SuspendedProcess {
        fn drop(&mut self) {
            if let Some(process) = &self.process {
                // No unbounded wait in Drop, and no lookup by PID on failure.
                unsafe {
                    TerminateProcess(process.handle.as_raw_handle(), 126);
                }
            }
        }
    }

    // Word storage has the native alignment needed by the opaque attribute list.
    // The supplied attribute values must additionally outlive CreateProcessW.
    struct Attributes {
        storage: Vec<usize>,
    }
    impl Attributes {
        fn new(count: u32) -> io::Result<Self> {
            let mut bytes = 0;
            unsafe {
                InitializeProcThreadAttributeList(null_mut(), count, 0, &mut bytes);
            }
            if bytes == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            unsafe {
                checked(InitializeProcThreadAttributeList(
                    storage.as_mut_ptr().cast(),
                    count,
                    0,
                    &mut bytes,
                ))?;
            }
            Ok(Self { storage })
        }
        fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
            self.storage.as_mut_ptr().cast()
        }
    }
    impl Drop for Attributes {
        fn drop(&mut self) {
            unsafe {
                DeleteProcThreadAttributeList(self.pointer());
            }
        }
    }

    /// A new job is the sole cleanup authority. There is deliberately no API to
    /// adopt a named/containing job or assign an arbitrary process: a named job
    /// is created or refused by this owner, never opened here, and only its
    /// members may open it for a query-only containment check.
    #[derive(Debug)]
    pub struct Job {
        handle: OwnedHandle,
        completion: OwnedHandle,
    }

    impl Job {
        pub fn new(limits: Limits) -> io::Result<Self> {
            Self::create(limits, None)
        }

        /// Same containment and cleanup authority with a session-local object
        /// name, so a nested member can verify its membership with a query-only
        /// handle. A pre-existing name is refused instead of adopted.
        pub fn new_named(limits: Limits, name: &str) -> io::Result<Self> {
            if name.is_empty()
                || name.len() > 128
                || !name.is_ascii()
                || name.contains(['\\', '\0'])
            {
                return Err(invalid(
                    "job object name must be a bounded ASCII session-local name",
                ));
            }
            Self::create(limits, Some(wide(OsStr::new(name))?.as_slice()))
        }

        fn create(limits: Limits, name: Option<&[u16]>) -> io::Result<Self> {
            if limits.memory_bytes == Some(0) {
                return Err(invalid("memory limit must be positive"));
            }
            let rate = limits.cpu_percent.map(rate_control).transpose()?;
            // CreateJobObject opens an existing object of the same name; that
            // would hand this owner a foreign job, so a taken name must fail.
            unsafe { SetLastError(0) };
            let handle = match owned(unsafe {
                CreateJobObjectW(null(), name.map_or(null(), |name| name.as_ptr()))
            }) {
                Ok(_) if name.is_some() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS => {
                    return Err(io::Error::other(
                        "job object name is already in use; preserving the existing job",
                    ));
                }
                other => other?,
            };
            let completion =
                owned(unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) })?;
            let job = Self { handle, completion };
            let mut extended: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
            extended.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if let Some(bytes) = limits.memory_bytes {
                extended.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
                extended.JobMemoryLimit = bytes;
            }
            job.set(JobObjectExtendedLimitInformation, &extended)?;
            if let Some(rate) = rate {
                let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = unsafe { zeroed() };
                cpu.ControlFlags =
                    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
                cpu.Anonymous.CpuRate = rate;
                job.set(JobObjectCpuRateControlInformation, &cpu)?;
            }
            let port = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
                CompletionKey: null_mut(),
                CompletionPort: job.completion.as_raw_handle(),
            };
            job.set(JobObjectAssociateCompletionPortInformation, &port)?;
            Ok(job)
        }

        // Only fixed Win32 structures are passed from this module.
        fn set<T>(&self, class: JOBOBJECTINFOCLASS, value: &T) -> io::Result<()> {
            set_job(self.handle.as_raw_handle(), class, value)
        }

        fn query<T: Copy>(&self, class: JOBOBJECTINFOCLASS) -> io::Result<T> {
            query_job(self.handle.as_raw_handle(), class)
        }

        pub fn snapshot(&self) -> io::Result<JobSnapshot> {
            let extended: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
                self.query(JobObjectExtendedLimitInformation)?;
            let accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION =
                self.query(JobObjectBasicAccountingInformation)?;
            let cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION =
                self.query(JobObjectCpuRateControlInformation)?;
            let mut flags = 0;
            unsafe {
                checked(GetHandleInformation(
                    self.handle.as_raw_handle(),
                    &mut flags,
                ))?;
            }
            Ok(JobSnapshot {
                active_processes: accounting.ActiveProcesses,
                memory_limit_bytes: extended.JobMemoryLimit,
                peak_job_memory_bytes: extended.PeakJobMemoryUsed,
                cpu_rate: if cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_ENABLE != 0 {
                    unsafe { cpu.Anonymous.CpuRate }
                } else {
                    0
                },
                cpu_hard_cap: cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP != 0,
                kill_on_close: extended.BasicLimitInformation.LimitFlags
                    & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                    != 0,
                handle_inheritable: flags & HANDLE_FLAG_INHERIT != 0,
                cpu_time: Duration::from_nanos(
                    (accounting.TotalKernelTime as u64 + accounting.TotalUserTime as u64)
                        .saturating_mul(100),
                ),
            })
        }

        pub fn contains(&self, process: &OwnedProcess) -> io::Result<bool> {
            in_job(process.handle.as_raw_handle(), self.handle.as_raw_handle())
        }

        /// For the trusted service bootstrap only, before starting any worker.
        /// Closing this Job also terminates the current process. Normal service
        /// exit must use process::exit so the OS closes the Job after exit begins.
        pub fn contain_current_process(&self) -> io::Result<()> {
            checked(unsafe {
                AssignProcessToJobObject(self.handle.as_raw_handle(), GetCurrentProcess())
            })?;
            if !in_job(unsafe { GetCurrentProcess() }, self.handle.as_raw_handle())? {
                return Err(io::Error::other("service Job assignment was not observed"));
            }
            Ok(())
        }

        /// Read-only reconciliation: mismatch, exit and foreign membership all
        /// return false. Access-denied/ambiguous evidence remains an error.
        /// This never opens PROCESS_TERMINATE and never grants cleanup authority.
        pub fn owns(&self, identity: ProcessIdentity) -> io::Result<bool> {
            let handle = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    identity.pid,
                )
            };
            if handle.is_null() {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
                    return Ok(false);
                }
                return Err(error);
            }
            let handle = owned(handle)?;
            if times(handle.as_raw_handle())?.0 != identity.creation_time
                || exited(handle.as_raw_handle())?
            {
                return Ok(false);
            }
            in_job(handle.as_raw_handle(), self.handle.as_raw_handle())
        }

        pub fn spawn_suspended(&self, command: &CommandSpec) -> io::Result<SuspendedProcess> {
            spawn_suspended_in(&self.owning_jobs(), command, None)
        }

        /// Outermost job for payload creation. The shared account budget
        /// supplies an ordered pair instead; a lifecycle job alone stays inner.
        fn owning_jobs(&self) -> [HANDLE; 1] {
            [self.handle.as_raw_handle()]
        }

        /// Start a bounded process inside an already created pseudoconsole.
        ///
        /// # Safety
        /// `pseudoconsole` must be a live HPCON from CreatePseudoConsole and must
        /// remain open for the lifetime of this process and its console session.
        pub unsafe fn spawn_console(
            &self,
            command: &CommandSpec,
            pseudoconsole: isize,
        ) -> io::Result<OwnedProcess> {
            spawn_suspended_in(&self.owning_jobs(), command, Some(pseudoconsole))?.resume()
        }
    }

    /// Create one suspended payload in an ordered job list, outermost first, so
    /// the aggregate ceiling covers the lifecycle job and its descendants.
    /// `Job`, `SharedCpuBudget` and `HeavyAggregate` create payloads through this
    /// one owner. The kernel assigns the whole list atomically at creation,
    /// before any payload code runs.
    fn spawn_suspended_in(
        jobs: &[HANDLE],
        command: &CommandSpec,
        pseudoconsole: Option<isize>,
    ) -> io::Result<SuspendedProcess> {
        if jobs.is_empty() {
            return Err(invalid("process creation requires at least one owning job"));
        }
        suppress_loader_dialogs();
        if !command.program.is_absolute() {
            return Err(invalid("executable must be an absolute path"));
        }
        let application = wide(command.program.as_os_str())?;
        let mut console_title = command
            .new_console
            .as_ref()
            .map(|title| wide(title))
            .transpose()?;
        if console_title.is_some()
            && (command.inherit_console
                || pseudoconsole.is_some()
                || command.stdin.is_some()
                || command.stdout.is_some()
                || command.stderr.is_some())
        {
            return Err(invalid(
                "a new visible console requires its own standard devices",
            ));
        }
        if command.inherit_console
            && (pseudoconsole.is_some()
                || command.stdin.is_some()
                || command.stdout.is_some()
                || command.stderr.is_some())
        {
            return Err(invalid(
                "inheriting the parent console cannot be combined with redirected streams",
            ));
        }
        let directory = command
            .current_dir
            .as_ref()
            .map(|p| wide(p.as_os_str()))
            .transpose()?;
        let mut line = Vec::new();
        quote(command.program.as_os_str(), &mut line)?;
        for arg in &command.args {
            line.push(32);
            quote(arg, &mut line)?;
        }
        if line.len() >= 32767 {
            return Err(invalid("Windows command line exceeds 32767 UTF-16 units"));
        }
        line.push(0);
        if pseudoconsole.is_some()
            && (command.stdin.is_some() || command.stdout.is_some() || command.stderr.is_some())
        {
            return Err(invalid(
                "pseudoconsole streams cannot be redirected separately",
            ));
        }
        let mut attributes = Attributes::new(2)?;
        if let Some(console) = pseudoconsole {
            unsafe {
                checked(UpdateProcThreadAttribute(
                    attributes.pointer(),
                    0,
                    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                    console as *const std::ffi::c_void,
                    size_of::<isize>(),
                    null_mut(),
                    null(),
                ))?;
            }
        }
        // A pseudoconsole supplies its own console standard handles. Even
        // without STARTF_USESTDHANDLES, inheriting our NUL handles can replace
        // them. Only the ordinary redirected process path has HANDLE_LIST.
        let streams =
            if !command.inherit_console && pseudoconsole.is_none() && console_title.is_none() {
                let nul = OpenOptions::new().read(true).write(true).open("NUL")?;
                Some([
                    inherited_copy(command.stdin.as_ref().unwrap_or(&nul))?,
                    inherited_copy(command.stdout.as_ref().unwrap_or(&nul))?,
                    inherited_copy(command.stderr.as_ref().unwrap_or(&nul))?,
                ])
            } else {
                None
            };
        let inherited = streams
            .as_ref()
            .map(|s| s.each_ref().map(AsRawHandle::as_raw_handle));
        if let Some(inherited) = &inherited {
            unsafe {
                checked(UpdateProcThreadAttribute(
                    attributes.pointer(),
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    inherited.as_ptr().cast(),
                    size_of_val(inherited),
                    null_mut(),
                    null(),
                ))?;
            }
        }
        // JOB_LIST order defines nesting: the first handle is outermost, so
        // a shared budget always stays outside the lifecycle job.
        unsafe {
            checked(UpdateProcThreadAttribute(
                attributes.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                jobs.as_ptr().cast(),
                size_of_val(jobs),
                null_mut(),
                null(),
            ))?;
        }
        let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        // Explicit null handles on ConPTY prevent the parent's redirected
        // streams from replacing the pseudoconsole's standard devices.
        // Same contract as the retained real C# console acceptance oracle.
        startup.StartupInfo.dwFlags = if console_title.is_some() || command.inherit_console {
            0
        } else {
            STARTF_USESTDHANDLES
        };
        if let Some(title) = &mut console_title {
            startup.StartupInfo.lpTitle = title.as_mut_ptr();
        }
        if let Some(inherited) = inherited {
            startup.StartupInfo.hStdInput = inherited[0];
            startup.StartupInfo.hStdOutput = inherited[1];
            startup.StartupInfo.hStdError = inherited[2];
        }
        startup.lpAttributeList = attributes.pointer();
        let mut info: PROCESS_INFORMATION = unsafe { zeroed() };
        let environment = environment(&command.env)?;
        unsafe {
            checked(CreateProcessW(
                application.as_ptr(),
                line.as_mut_ptr(),
                null(),
                null(),
                i32::from(streams.is_some() || command.inherit_console),
                CREATE_SUSPENDED
                    | EXTENDED_STARTUPINFO_PRESENT
                    | if console_title.is_some() {
                        CREATE_NEW_CONSOLE
                    } else if command.inherit_console || pseudoconsole.is_some() {
                        0
                    } else {
                        CREATE_NO_WINDOW
                    }
                    | CREATE_UNICODE_ENVIRONMENT,
                environment
                    .as_ref()
                    .map_or(null(), |block| block.as_ptr().cast()),
                directory.as_ref().map_or(null(), |d| d.as_ptr()),
                &startup.StartupInfo,
                &mut info,
            ))?;
        }
        // Creation/assignment failure above returns no running child. After
        // success, wrap both handles before any further fallible operation.
        let process_handle = owned(info.hProcess)?;
        let thread = owned(info.hThread)?;
        let mut suspended = SuspendedProcess {
            process: Some(OwnedProcess {
                handle: process_handle,
                identity: ProcessIdentity {
                    pid: info.dwProcessId,
                    creation_time: 0,
                },
            }),
            thread,
        };
        let process = suspended.process.as_mut().expect("pending process");
        process.identity.creation_time = times(process.handle.as_raw_handle())?.0;
        // Verify the complete ordered list: creation-time assignment is
        // atomic, so either every owning job holds this process or none does.
        for job in jobs {
            if !in_job(process.handle.as_raw_handle(), *job)? {
                return Err(io::Error::other(
                    "created process is outside its required job list",
                ));
            }
        }
        Ok(suspended)
    }

    impl Job {
        pub fn spawn(&self, command: &CommandSpec) -> io::Result<OwnedProcess> {
            self.spawn_suspended(command)?.resume()
        }

        fn stop(&self, code: u32) -> io::Result<()> {
            // Defense in depth: no public operation can attach this process.
            if in_job(unsafe { GetCurrentProcess() }, self.handle.as_raw_handle())? {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "refusing to terminate a containing job",
                ));
            }
            unsafe { checked(TerminateJobObject(self.handle.as_raw_handle(), code)) }
        }

        fn wait_empty(
            &self,
            timeout: Duration,
            root: Option<&OwnedProcess>,
        ) -> io::Result<JobSnapshot> {
            let deadline = Deadline::after(timeout)?;
            loop {
                let snapshot = self.snapshot()?;
                // Job accounting can reach zero before the root's process
                // handle becomes signalled. Confirm both within one budget.
                if snapshot.active_processes == 0
                    && root.map(|p| p.is_running()).transpose()? != Some(true)
                {
                    return Ok(snapshot);
                }
                if deadline.expired() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "owned job cleanup deadline expired",
                    ));
                }
                std::thread::sleep(POLL.min(deadline.remaining()));
            }
        }

        /// Stops all members and confirms zero active processes within one total
        /// budget. Even on error/timeout, consuming self closes the kill-on-close
        /// handle; Drop itself never waits or enumerates descendants.
        pub fn terminate(self, code: u32, cleanup_timeout: Duration) -> io::Result<JobSnapshot> {
            self.stop(code)?;
            self.wait_empty(cleanup_timeout, None)
        }

        fn memory_notification(&self) -> io::Result<bool> {
            // Bounded drain: a stream of short-lived descendants cannot starve
            // cancellation/deadline checks. PIDs in packets are never acted on.
            for _ in 0..256 {
                let (mut message, mut key, mut overlapped) = (0, 0, null_mut());
                if unsafe {
                    GetQueuedCompletionStatus(
                        self.completion.as_raw_handle(),
                        &mut message,
                        &mut key,
                        &mut overlapped,
                        0,
                    )
                } == 0
                {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() == Some(WAIT_TIMEOUT as i32) {
                        return Ok(false);
                    }
                    return Err(error);
                }
                if key == 0 && message == JOB_OBJECT_MSG_JOB_MEMORY_LIMIT {
                    return Ok(true);
                }
            }
            Ok(false)
        }

        /// Wait for a root; on every outcome clean its entire job, including
        /// descendants surviving immediate root exit. Cancellation wins ties
        /// over memory, exit and deadline; observed memory wins over root exit.
        /// Use a separate job for independent commands.
        pub fn wait(
            self,
            process: &OwnedProcess,
            deadline: Deadline,
            cancellation: &Cancellation,
            cleanup_timeout: Duration,
        ) -> io::Result<Outcome> {
            if !self.contains(process)? {
                return Err(invalid("process does not belong to this job"));
            }
            let reason = loop {
                if cancellation.is_cancelled() {
                    break StopReason::Cancelled;
                }
                if self.memory_notification()? {
                    break StopReason::MemoryLimit;
                }
                if !process.is_running()? {
                    break StopReason::Exited;
                }
                if deadline.expired() {
                    break StopReason::Timeout;
                }
                std::thread::sleep(POLL.min(deadline.remaining()));
            };
            let code = match reason {
                StopReason::Timeout => 124,
                StopReason::MemoryLimit => 125,
                StopReason::Cancelled => 130,
                StopReason::Exited => process.exit_code()?.expect("exited process"),
            };
            self.stop(code)?;
            let job = self.wait_empty(cleanup_timeout, Some(process))?;
            let process_exit_code = process
                .exit_code()?
                .ok_or_else(|| io::Error::other("root still running after job cleanup"))?;
            Ok(Outcome {
                reason,
                exit_code: code,
                process_exit_code,
                job,
            })
        }

        /// Wait indefinitely for the session root, then reap leftover members.
        /// Helper jobs with an execution budget keep using `wait`.
        pub fn wait_foreground(
            self,
            process: &OwnedProcess,
            cleanup_timeout: Duration,
        ) -> io::Result<u32> {
            if !self.contains(process)? {
                return Err(invalid("process does not belong to this job"));
            }
            process.wait_unbounded()?;
            let code = process
                .exit_code()?
                .ok_or_else(|| io::Error::other("root still running after wait"))?;
            self.stop(code)?;
            self.wait_empty(cleanup_timeout, Some(process))?;
            Ok(code)
        }

        /// Wait for an ordinary interactive session root, then preserve every
        /// remaining member: upstream-managed background processes keep their
        /// own lifetime after a normal wrapper exit. Kill-on-close stays armed
        /// while this launcher is alive, so an abnormal launcher death still
        /// reaps the session tree; only the normal-exit path disarms it.
        pub fn wait_session_root(self, process: &OwnedProcess) -> io::Result<u32> {
            if !self.contains(process)? {
                return Err(invalid("process does not belong to this job"));
            }
            process.wait_unbounded()?;
            let code = process
                .exit_code()?
                .ok_or_else(|| io::Error::other("root still running after wait"))?;
            let mut extended: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
                self.query(JobObjectExtendedLimitInformation)?;
            extended.BasicLimitInformation.LimitFlags &= !JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            self.set(JobObjectExtendedLimitInformation, &extended)?;
            Ok(code)
        }
    }

    /// Session-local Job object name prefix for one account's shared budget; the
    /// suffix is the account-directory hash, so the identity survives any number
    /// of concurrent launchers of that account.
    const SHARED_CPU_JOB_PREFIX: &str = "CodingAgentsHarness.SharedCpu.";
    /// Account-local ownership record and stable lock for that object.
    const BUDGET_RECORD: &str = "cpu-budget.json";
    const BUDGET_LOCK: &str = "cpu-budget.lock";
    const BUDGET_SCHEMA: u32 = 1;
    const MAX_BUDGET_RECORD: u64 = 64 * 1024;
    /// One create plus one small record write is the whole critical section, so
    /// this bound is only reached by a pathological holder; a caller that cannot
    /// enter it reports the failure instead of silently creating a second
    /// allowance.
    const BUDGET_LOCK_WAIT: Duration = Duration::from_secs(10);

    /// Resolve one CPU percentage to the kernel's 0.01% rate units. The exclusive
    /// `Limits` path and the shared account budget accept exactly the same range,
    /// so neither can request an unenforceable rate.
    fn rate_control(percent: f64) -> io::Result<u32> {
        match percent {
            p if p.is_finite() && (0.01..=100.0).contains(&p) => Ok((p * 100.0).floor() as u32),
            _ => Err(invalid("CPU percent must be finite and within 0.01..=100")),
        }
    }

    /// FNV-1a over the canonical account directory. The object name therefore
    /// depends on the Windows account location alone, never on a project,
    /// `CODEX_HOME`, terminal tab or build directory. Case is folded because
    /// Windows paths are case-insensitive.
    fn budget_key(text: &str) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for byte in text.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    fn shared_cpu_job_name(directory: &Path) -> String {
        let account = directory.to_string_lossy().to_lowercase();
        format!("{SHARED_CPU_JOB_PREFIX}{:016x}", budget_key(&account))
    }

    /// Create the account directory when absent and return its canonical form.
    /// Junction/symlink indirection is refused so two callers cannot reach two
    /// lock files for one intended account.
    fn owned_budget_directory(directory: &Path) -> io::Result<PathBuf> {
        match std::fs::symlink_metadata(directory) {
            Ok(_) => crate::build_identity::ordinary(directory)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                std::fs::create_dir_all(directory)?
            }
            Err(error) => return Err(error),
        }
        if !directory.is_dir() {
            return Err(io::Error::other(
                "the shared CPU budget account path is not a directory",
            ));
        }
        directory.canonicalize()
    }

    /// Account ownership record for one shared CPU budget: which canonical
    /// account directory established the object, under which name, for which
    /// rate, and by which creator. A record copied into another directory names
    /// a different account directory and therefore authorizes nothing.
    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct BudgetRecord {
        schema: u32,
        account: PathBuf,
        job: String,
        cpu_rate: u32,
        pid: u32,
        creation_time: u64,
    }

    fn read_budget_record(
        path: &Path,
        directory: &Path,
        name: &str,
        rate: u32,
    ) -> io::Result<Option<BudgetRecord>> {
        use std::io::Read;
        match std::fs::symlink_metadata(path) {
            Ok(_) => crate::build_identity::ordinary(path)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_BUDGET_RECORD + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BUDGET_RECORD {
            return Err(io::Error::other(
                "the shared CPU budget ownership record exceeds its bound",
            ));
        }
        let record: BudgetRecord = serde_json::from_slice(&bytes).map_err(|error| {
            io::Error::other(format!(
                "the shared CPU budget ownership record at {} is unreadable ({error}); preserving the existing budget",
                path.display()
            ))
        })?;
        if record.schema != BUDGET_SCHEMA
            || record.account.as_path() != directory
            || record.job != name
        {
            return Err(io::Error::other(format!(
                "the shared CPU budget ownership record at {} does not describe this account directory; preserving the existing budget",
                path.display()
            )));
        }
        if record.cpu_rate != rate {
            return Err(io::Error::other(format!(
                "the account shared CPU budget is already established at {} percent; preserving it",
                f64::from(record.cpu_rate) / 100.0
            )));
        }
        Ok(Some(record))
    }

    fn write_budget_record(path: &Path, directory: &Path, name: &str, rate: u32) -> io::Result<()> {
        let identity = current_identity()?;
        let record = BudgetRecord {
            schema: BUDGET_SCHEMA,
            account: directory.to_owned(),
            job: name.to_owned(),
            cpu_rate: rate,
            pid: identity.pid,
            creation_time: identity.creation_time,
        };
        let staging = path.with_file_name(format!("{BUDGET_RECORD}.staging"));
        std::fs::write(&staging, serde_json::to_vec_pretty(&record)?)?;
        std::fs::rename(&staging, path)
    }

    fn current_identity() -> io::Result<ProcessIdentity> {
        let (creation_time, _) = times(unsafe { GetCurrentProcess() })?;
        Ok(ProcessIdentity {
            pid: std::process::id(),
            creation_time,
        })
    }

    fn set_job<T>(handle: HANDLE, class: JOBOBJECTINFOCLASS, value: &T) -> io::Result<()> {
        unsafe {
            checked(SetInformationJobObject(
                handle,
                class,
                (value as *const T).cast(),
                size_of::<T>() as u32,
            ))
        }
    }

    fn query_job<T: Copy>(handle: HANDLE, class: JOBOBJECTINFOCLASS) -> io::Result<T> {
        let mut value = std::mem::MaybeUninit::<T>::zeroed();
        unsafe {
            checked(QueryInformationJobObject(
                handle,
                class,
                value.as_mut_ptr().cast(),
                size_of::<T>() as u32,
                null_mut(),
            ))?;
            Ok(value.assume_init())
        }
    }

    fn shared_cpu_snapshot(handle: HANDLE) -> io::Result<SharedCpuSnapshot> {
        let extended: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
            query_job(handle, JobObjectExtendedLimitInformation)?;
        let accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION =
            query_job(handle, JobObjectBasicAccountingInformation)?;
        let cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION =
            query_job(handle, JobObjectCpuRateControlInformation)?;
        Ok(SharedCpuSnapshot {
            cpu_rate: if cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_ENABLE != 0 {
                unsafe { cpu.Anonymous.CpuRate }
            } else {
                0
            },
            cpu_hard_cap: cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP != 0,
            kill_on_close: extended.BasicLimitInformation.LimitFlags
                & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                != 0,
            job_memory_limit_bytes: extended.JobMemoryLimit,
            breakaway_ok: extended.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_BREAKAWAY_OK
                != 0,
            silent_breakaway_ok: extended.BasicLimitInformation.LimitFlags
                & JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK
                != 0,
            active_processes: accounting.ActiveProcesses,
            cpu_time: Duration::from_nanos(
                (accounting.TotalKernelTime as u64 + accounting.TotalUserTime as u64)
                    .saturating_mul(100),
            ),
        })
    }

    /// One named object must be this owner's CPU-only accounting job; a
    /// lifecycle job, a foreign limit or a different rate is preserved, never
    /// adopted and never modified.
    fn verify_shared_cpu(handle: HANDLE, name: &str, rate: u32) -> io::Result<()> {
        let snapshot = shared_cpu_snapshot(handle)?;
        if snapshot.cpu_rate == rate
            && snapshot.cpu_hard_cap
            && !snapshot.kill_on_close
            && snapshot.job_memory_limit_bytes == 0
            && !snapshot.breakaway_ok
            && !snapshot.silent_breakaway_ok
        {
            return Ok(());
        }
        Err(io::Error::other(format!(
            "the job object named {name} does not carry the required CPU-only settings; preserving it"
        )))
    }

    /// The single account-wide CPU-rate-only budget Job. Participants keep their
    /// exclusive lifecycle Job as an inner job, so the shared ceiling covers
    /// every admitted payload while each session retains its own cleanup
    /// authority. There is deliberately no terminate/wait API: this owner can
    /// never kill a peer, and closing one participant's handle leaves the object
    /// and its enforcement in place for every member that remains.
    #[derive(Debug)]
    pub struct SharedCpuBudget {
        handle: OwnedHandle,
        name: String,
        directory: PathBuf,
    }

    impl SharedCpuBudget {
        /// Join or establish this account's shared CPU budget. `percent` is a
        /// ceiling in percent of total host CPU capacity; the desktop policy uses
        /// `SHARED_CPU_PERCENT`. Concurrent first callers converge on one object
        /// through the account-local `ExclusiveFileLock`, and every caller
        /// validates the ownership record and the effective kernel settings
        /// before admitting work.
        ///
        /// A job object outlives its handles while members remain, so closing one
        /// participant's handle never kills peers and never lifts their ceiling.
        /// The object *name*, however, is released with the last handle: callers
        /// that must keep one group for the account hold this handle for as long
        /// as their members run, and a later caller then rejoins the same object.
        pub fn acquire(directory: &Path, percent: f64) -> io::Result<Self> {
            Self::acquire_within(
                directory,
                percent,
                Deadline::after(BUDGET_LOCK_WAIT)?,
                &Cancellation::default(),
            )
        }

        /// Bounded form of `acquire`: a caller that cannot enter the account
        /// critical section within its own deadline reports the failure instead
        /// of creating a second allowance.
        pub fn acquire_within(
            directory: &Path,
            percent: f64,
            deadline: Deadline,
            cancellation: &Cancellation,
        ) -> io::Result<Self> {
            let rate = rate_control(percent)?;
            let directory = owned_budget_directory(directory)?;
            let name = shared_cpu_job_name(&directory);
            // The lock file is stable and never deleted: unlinking it would let
            // two callers lock different objects under one path.
            let _lock =
                ExclusiveFileLock::acquire(&directory.join(BUDGET_LOCK), deadline, cancellation)?;
            Self::establish(&directory, &name, rate)
        }

        fn establish(directory: &Path, name: &str, rate: u32) -> io::Result<Self> {
            let record = directory.join(BUDGET_RECORD);
            let recorded = read_budget_record(&record, directory, name, rate)?;
            let object = wide(OsStr::new(name))?;
            unsafe { SetLastError(0) };
            let handle = owned(unsafe { CreateJobObjectW(null(), object.as_ptr()) })?;
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                // CreateJobObjectW opens a same-named object instead of failing,
                // so an existing object is adopted only when this account's own
                // record and its effective settings both describe it.
                if recorded.is_none() {
                    return Err(io::Error::other(format!(
                        "a job object named {name} already exists without this account's ownership record; preserving it"
                    )));
                }
                verify_shared_cpu(handle.as_raw_handle(), name, rate)?;
            } else {
                // A fresh object takes the CPU rate and nothing else: no
                // kill-on-close, no memory limit and no breakaway authority.
                let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = unsafe { zeroed() };
                cpu.ControlFlags =
                    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
                cpu.Anonymous.CpuRate = rate;
                set_job(
                    handle.as_raw_handle(),
                    JobObjectCpuRateControlInformation,
                    &cpu,
                )?;
                verify_shared_cpu(handle.as_raw_handle(), name, rate)?;
                // A failed record write drops this handle and therefore destroys
                // the memberless object instead of leaving an unowned name.
                write_budget_record(&record, directory, name, rate)?;
            }
            Ok(Self {
                handle,
                name: name.to_owned(),
                directory: directory.to_owned(),
            })
        }

        /// Session-local kernel name of this budget object. A nested caller can
        /// verify its own membership against this name without cleanup authority.
        pub fn name(&self) -> &str {
            &self.name
        }

        /// Canonical account directory that owns this budget.
        pub fn directory(&self) -> &Path {
            &self.directory
        }

        /// Kernel readback of rate, containment flags and current membership.
        /// Readback proves configuration, not measured consumption.
        pub fn snapshot(&self) -> io::Result<SharedCpuSnapshot> {
            shared_cpu_snapshot(self.handle.as_raw_handle())
        }

        /// Kernel membership check against this exact object.
        pub fn contains(&self, process: &OwnedProcess) -> io::Result<bool> {
            in_job(process.handle.as_raw_handle(), self.handle.as_raw_handle())
        }

        /// Create a payload as a member of this shared budget (outer) and the
        /// supplied lifecycle Job (inner), atomically before any payload code
        /// runs. The complete ordered list is verified after creation.
        pub fn spawn_suspended(
            &self,
            lifecycle: &Job,
            command: &CommandSpec,
        ) -> io::Result<SuspendedProcess> {
            spawn_suspended_in(&self.owning_jobs(lifecycle), command, None)
        }

        pub fn spawn(&self, lifecycle: &Job, command: &CommandSpec) -> io::Result<OwnedProcess> {
            self.spawn_suspended(lifecycle, command)?.resume()
        }

        /// Start a bounded process inside an already created pseudoconsole.
        ///
        /// # Safety
        /// `pseudoconsole` must be a live HPCON from CreatePseudoConsole and must
        /// remain open for the lifetime of this process and its console session.
        pub unsafe fn spawn_console(
            &self,
            lifecycle: &Job,
            command: &CommandSpec,
            pseudoconsole: isize,
        ) -> io::Result<OwnedProcess> {
            spawn_suspended_in(&self.owning_jobs(lifecycle), command, Some(pseudoconsole))?.resume()
        }

        /// Outermost first: the shared ceiling must stay outside every lifecycle
        /// job, so terminating one participant's job cannot reach its peers.
        fn owning_jobs(&self, lifecycle: &Job) -> [HANDLE; 2] {
            [
                self.handle.as_raw_handle(),
                lifecycle.handle.as_raw_handle(),
            ]
        }
    }

    const HEAVY_AGGREGATE_PREFIX: &str = "CodingAgentsHarness.HeavyAggregate.";
    const AGGREGATE_RECORD: &str = "heavy-aggregate.json";
    const AGGREGATE_LOCK: &str = "heavy-aggregate.lock";
    const AGGREGATE_SCHEMA: u32 = 1;

    fn heavy_aggregate_job_name(directory: &Path) -> String {
        let account = directory.to_string_lossy().to_lowercase();
        format!("{HEAVY_AGGREGATE_PREFIX}{:016x}", budget_key(&account))
    }

    /// Account ownership record for the aggregate memory job. The kernel object
    /// remains the live conflict: a released name is not a frozen limit, but an
    /// existing object is never adopted unless this record names this account.
    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct AggregateRecord {
        schema: u32,
        account: PathBuf,
        job: String,
        memory_bytes: usize,
        pid: u32,
        creation_time: u64,
    }

    fn read_aggregate_record(
        path: &Path,
        directory: &Path,
        name: &str,
    ) -> io::Result<Option<AggregateRecord>> {
        use std::io::Read;
        match std::fs::symlink_metadata(path) {
            Ok(_) => crate::build_identity::ordinary(path)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_BUDGET_RECORD + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BUDGET_RECORD {
            return Err(io::Error::other(
                "the heavy aggregate ownership record exceeds its bound",
            ));
        }
        let record: AggregateRecord = serde_json::from_slice(&bytes).map_err(|error| {
            io::Error::other(format!(
                "heavy aggregate name mismatch: ownership record at {} is unreadable ({error}); preserving the existing object",
                path.display()
            ))
        })?;
        if record.schema != AGGREGATE_SCHEMA
            || record.account.as_path() != directory
            || record.job != name
        {
            return Err(io::Error::other(format!(
                "heavy aggregate name mismatch: ownership record at {} does not describe this account directory; preserving the existing object",
                path.display()
            )));
        }
        Ok(Some(record))
    }

    fn write_aggregate_record(
        path: &Path,
        directory: &Path,
        name: &str,
        memory_bytes: usize,
    ) -> io::Result<()> {
        let identity = current_identity()?;
        let record = AggregateRecord {
            schema: AGGREGATE_SCHEMA,
            account: directory.to_owned(),
            job: name.to_owned(),
            memory_bytes,
            pid: identity.pid,
            creation_time: identity.creation_time,
        };
        let staging = path.with_file_name(format!("{AGGREGATE_RECORD}.staging"));
        std::fs::write(&staging, serde_json::to_vec_pretty(&record)?)?;
        std::fs::rename(&staging, path)
    }

    fn heavy_aggregate_snapshot(handle: HANDLE) -> io::Result<HeavyAggregateSnapshot> {
        let extended: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
            query_job(handle, JobObjectExtendedLimitInformation)?;
        let accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION =
            query_job(handle, JobObjectBasicAccountingInformation)?;
        let cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION =
            query_job(handle, JobObjectCpuRateControlInformation)?;
        let flags = extended.BasicLimitInformation.LimitFlags;
        Ok(HeavyAggregateSnapshot {
            limit_flags: flags,
            job_memory_limit_bytes: extended.JobMemoryLimit,
            process_memory_limit_bytes: extended.ProcessMemoryLimit,
            cpu_rate: if cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_ENABLE != 0 {
                unsafe { cpu.Anonymous.CpuRate }
            } else {
                0
            },
            cpu_hard_cap: cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP != 0,
            kill_on_close: flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE != 0,
            breakaway_ok: flags & JOB_OBJECT_LIMIT_BREAKAWAY_OK != 0,
            silent_breakaway_ok: flags & JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK != 0,
            active_processes: accounting.ActiveProcesses,
        })
    }

    /// An existing named object is this account's memory-only aggregate, or it
    /// is preserved. This never calls `SetInformationJobObject`.
    fn verify_heavy_aggregate(handle: HANDLE, name: &str, memory_bytes: usize) -> io::Result<()> {
        let snapshot = heavy_aggregate_snapshot(handle)?;
        let process_memory = snapshot.limit_flags & JOB_OBJECT_LIMIT_PROCESS_MEMORY != 0;
        if snapshot.limit_flags == JOB_OBJECT_LIMIT_JOB_MEMORY
            && !process_memory
            && snapshot.job_memory_limit_bytes == memory_bytes
            && snapshot.process_memory_limit_bytes == 0
            && snapshot.cpu_rate == 0
            && !snapshot.cpu_hard_cap
            && !snapshot.kill_on_close
            && !snapshot.breakaway_ok
            && !snapshot.silent_breakaway_ok
        {
            return Ok(());
        }
        Err(io::Error::other(format!(
            "heavy aggregate limit mismatch: job object named {name} has flags {:#x}, job memory {} bytes, process memory {} bytes and cpu rate {}; preserving it",
            snapshot.limit_flags,
            snapshot.job_memory_limit_bytes,
            snapshot.process_memory_limit_bytes,
            snapshot.cpu_rate
        )))
    }

    /// The account-wide aggregate commit-memory job. Callers create or open one
    /// named object, hold its handle for the tree lifetime, and nest it between
    /// the optional shared CPU job and the per-tree lifecycle job. This owner
    /// has no terminate API: closing one handle cannot kill a peer.
    #[derive(Debug)]
    pub struct HeavyAggregate {
        handle: OwnedHandle,
        name: String,
        directory: PathBuf,
        memory_bytes: usize,
    }

    impl HeavyAggregate {
        /// Join or establish this account's aggregate memory job.
        /// `memory_bytes` is the job-wide commit limit (`JobMemoryLimit`).
        /// Concurrent callers converge through `heavy-aggregate.lock`; the
        /// returned owner holds the job handle, not that lock.
        pub fn acquire(directory: &Path, memory_bytes: usize) -> io::Result<Self> {
            Self::acquire_within(
                directory,
                memory_bytes,
                Deadline::after(BUDGET_LOCK_WAIT)?,
                &Cancellation::default(),
            )
        }

        /// Bounded form of `acquire`. A caller that cannot enter the establish
        /// lock within its deadline reports the failure instead of creating a
        /// second envelope.
        pub fn acquire_within(
            directory: &Path,
            memory_bytes: usize,
            deadline: Deadline,
            cancellation: &Cancellation,
        ) -> io::Result<Self> {
            if memory_bytes == 0 {
                return Err(invalid("aggregate memory limit must be positive"));
            }
            let directory = owned_budget_directory(directory)?;
            let name = heavy_aggregate_job_name(&directory);
            // The lock file is stable and never deleted: unlinking it would let
            // two callers lock different objects under one path. It covers
            // establish only and is dropped before this function returns.
            let _lock = ExclusiveFileLock::acquire(
                &directory.join(AGGREGATE_LOCK),
                deadline,
                cancellation,
            )?;
            Self::establish(&directory, &name, memory_bytes)
        }

        fn establish(directory: &Path, name: &str, memory_bytes: usize) -> io::Result<Self> {
            let record_path = directory.join(AGGREGATE_RECORD);
            let recorded = read_aggregate_record(&record_path, directory, name)?;
            let object = wide(OsStr::new(name))?;
            unsafe { SetLastError(0) };
            let handle = owned(unsafe { CreateJobObjectW(null(), object.as_ptr()) })?;
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                // CreateJobObjectW opens a same-named object instead of failing.
                // Adopt it only when this account's record and the kernel
                // readback both describe the requested envelope. Never set
                // limits on this path.
                let Some(record) = recorded else {
                    return Err(io::Error::other(format!(
                        "heavy aggregate name mismatch: a job object named {name} already exists without this account's ownership record; preserving it"
                    )));
                };
                if record.memory_bytes != memory_bytes {
                    return Err(io::Error::other(format!(
                        "heavy aggregate limit mismatch: already established at {} bytes; preserving it",
                        record.memory_bytes
                    )));
                }
                verify_heavy_aggregate(handle.as_raw_handle(), name, memory_bytes)?;
            } else {
                // A fresh object takes the job-wide memory limit and nothing
                // else: no per-process memory limit, no CPU rate, no
                // kill-on-close and no breakaway authority.
                let mut extended: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
                extended.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_JOB_MEMORY;
                extended.JobMemoryLimit = memory_bytes;
                set_job(
                    handle.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    &extended,
                )?;
                verify_heavy_aggregate(handle.as_raw_handle(), name, memory_bytes)?;
                // A failed record write drops this handle and therefore destroys
                // the memberless object instead of leaving an unowned name.
                write_aggregate_record(&record_path, directory, name, memory_bytes)?;
            }
            Ok(Self {
                handle,
                name: name.to_owned(),
                directory: directory.to_owned(),
                memory_bytes,
            })
        }

        /// Session-local kernel name of this aggregate object.
        pub fn name(&self) -> &str {
            &self.name
        }

        /// Canonical account directory that owns this aggregate.
        pub fn directory(&self) -> &Path {
            &self.directory
        }

        /// Kernel readback of the memory limit, containment flags and membership.
        pub fn snapshot(&self) -> io::Result<HeavyAggregateSnapshot> {
            heavy_aggregate_snapshot(self.handle.as_raw_handle())
        }

        /// Kernel membership check against this exact object.
        pub fn contains(&self, process: &OwnedProcess) -> io::Result<bool> {
            in_job(process.handle.as_raw_handle(), self.handle.as_raw_handle())
        }

        /// Create a suspended payload in the optional shared CPU job (outermost),
        /// this aggregate job, and the per-tree lifecycle job (innermost).
        /// Readback is verified again before creation. This does not change the
        /// shared CPU job's limits or its own job list.
        pub fn spawn_suspended(
            &self,
            cpu: Option<&SharedCpuBudget>,
            lifecycle: &Job,
            command: &CommandSpec,
        ) -> io::Result<SuspendedProcess> {
            verify_heavy_aggregate(self.handle.as_raw_handle(), &self.name, self.memory_bytes)?;
            let jobs = [
                cpu.map(|budget| budget.handle.as_raw_handle())
                    .unwrap_or(null_mut()),
                self.handle.as_raw_handle(),
                lifecycle.handle.as_raw_handle(),
            ];
            let list = if cpu.is_some() { &jobs[..] } else { &jobs[1..] };
            spawn_suspended_in(list, command, None)
        }
    }
}
