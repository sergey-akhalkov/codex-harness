//! Owned Windows process trees. Jobs are anonymous, non-inheritable and created
//! here; neither PIDs nor a caller's containing job confer cleanup authority.
//! Windows 10+ is required for atomic assignment through JOB_LIST at creation.
//! These children inherit the environment, accept an explicit cwd and inherit
//! only allow-listed standard file handles. Interactive console support remains
//! a separate boundary.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(20);

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Inherit the parent's console when used for an interactive foreground CLI.
    pub inherit_console: bool,
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
}

#[cfg(windows)]
pub(crate) use windows::quote as quote_argument;
#[cfg(windows)]
pub use windows::{Job, OwnedProcess, SuspendedProcess};

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

    /// A new anonymous job is the sole cleanup authority. There is deliberately
    /// no API to adopt a named/containing job or assign an arbitrary process.
    #[derive(Debug)]
    pub struct Job {
        handle: OwnedHandle,
        completion: OwnedHandle,
    }

    impl Job {
        pub fn new(limits: Limits) -> io::Result<Self> {
            if limits.memory_bytes == Some(0) {
                return Err(invalid("memory limit must be positive"));
            }
            let rate = match limits.cpu_percent {
                None => None,
                Some(p) if p.is_finite() && (0.01..=100.0).contains(&p) => {
                    Some((p * 100.0).floor() as u32)
                }
                Some(_) => return Err(invalid("CPU percent must be finite and within 0.01..=100")),
            };
            let handle = owned(unsafe { CreateJobObjectW(null(), null()) })?;
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
            unsafe {
                checked(SetInformationJobObject(
                    self.handle.as_raw_handle(),
                    class,
                    (value as *const T).cast(),
                    size_of::<T>() as u32,
                ))
            }
        }

        fn query<T: Copy>(&self, class: JOBOBJECTINFOCLASS) -> io::Result<T> {
            let mut value = std::mem::MaybeUninit::<T>::zeroed();
            unsafe {
                checked(QueryInformationJobObject(
                    self.handle.as_raw_handle(),
                    class,
                    value.as_mut_ptr().cast(),
                    size_of::<T>() as u32,
                    null_mut(),
                ))?;
                Ok(value.assume_init())
            }
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
            self.spawn_suspended_impl(command, None)
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
            self.spawn_suspended_impl(command, Some(pseudoconsole))?
                .resume()
        }

        fn spawn_suspended_impl(
            &self,
            command: &CommandSpec,
            pseudoconsole: Option<isize>,
        ) -> io::Result<SuspendedProcess> {
            if !command.program.is_absolute() {
                return Err(invalid("executable must be an absolute path"));
            }
            let application = wide(command.program.as_os_str())?;
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
            let streams = if pseudoconsole.is_none() {
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
            let mut job_handle = self.handle.as_raw_handle();
            unsafe {
                checked(UpdateProcThreadAttribute(
                    attributes.pointer(),
                    0,
                    PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                    (&mut job_handle as *mut HANDLE).cast(),
                    size_of::<HANDLE>(),
                    null_mut(),
                    null(),
                ))?;
            }
            let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            // Explicit null handles on ConPTY prevent the parent's redirected
            // streams from replacing the pseudoconsole's standard devices.
            // Same contract as the retained real C# console acceptance oracle.
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
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
                    i32::from(pseudoconsole.is_none()),
                    CREATE_SUSPENDED
                        | EXTENDED_STARTUPINFO_PRESENT
                        | if command.inherit_console || pseudoconsole.is_some() {
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
            if !self.contains(process)? {
                return Err(io::Error::other(
                    "created process is outside its required job",
                ));
            }
            Ok(suspended)
        }

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
    }
}
