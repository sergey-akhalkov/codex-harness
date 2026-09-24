//! Independent local services. A bounded helper starts the same trusted Rust
//! executable as a sibling outside the client's job and outside the helper's
//! own kill-on-close job. The payload inherits a same-user anchor's job chain
//! rather than being created by WMI, then joins the account CPU allowance
//! before service work. WMI remains only when that sibling creation is refused.
//! Neither a PID receipt nor the observation handle grants kill authority.
use crate::{
    cancellable_pipe::{CancellablePipe, PipeIoError, anonymous_pipe},
    dependency_mcp_probe::strict_json,
    native_build::ordinary_ancestors,
    process::{
        CPU_BUDGET_ACCOUNT_ENV, Cancellation, CommandSpec, Deadline, Job, Limits, ProcessIdentity,
        SHARED_CPU_PERCENT, SharedCpuBudget, StopReason,
    },
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
    sync::{
        Mutex, OnceLock,
        mpsc::{self, Sender},
    },
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::*,
    System::{
        Console::{STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle},
        JobObjects::{AssignProcessToJobObject, IsProcessInJob, OpenJobObjectW},
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
/// Job-object access rights (winnt.h). The enabled windows-sys features do not
/// export the SystemServices constants, so the documented values are spelled
/// here; only these two rights are ever requested.
const JOB_OBJECT_ASSIGN_PROCESS: u32 = 0x0001;
const JOB_OBJECT_QUERY: u32 = 0x0004;
/// A held account lock must not delay a service past its bounded startup
/// window; `SharedCpuBudget::acquire` uses the same bound.
const SHARED_CPU_JOIN_WAIT: Duration = Duration::from_secs(10);
/// Distinct service identities already reported outside the allowance. Startup
/// and join callers repeat on every request, and repeated identical warnings
/// are noise rather than status.
static REPORTED_OUTSIDE_ALLOWANCE: OnceLock<Mutex<BTreeSet<(u32, u64)>>> = OnceLock::new();

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

fn wide(text: &str) -> io::Result<Vec<u16>> {
    if text.contains('\0') {
        return Err(invalid("service job name contains a NUL"));
    }
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0);
    Ok(units)
}

/// Handle to a named account job object. Callers must hold the verified budget
/// handle for that same name: while that handle lives the object cannot be
/// replaced or renamed, so the name cannot resolve to a foreign object and
/// nothing is adopted here.
fn open_shared_job(name: &str, access: u32) -> io::Result<OwnedHandle> {
    let name = wide(name)?;
    owned(unsafe { OpenJobObjectW(access, 0, name.as_ptr()) })
}

/// Membership check mirroring the process owner's own check; the handles stay
/// non-owning, so this confers no cleanup authority.
fn in_job(process: HANDLE, job: HANDLE) -> io::Result<bool> {
    let mut member = 0;
    checked(unsafe { IsProcessInJob(process, job, &mut member) })?;
    Ok(member != 0)
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

/// Internal marker for the one-shot creation anchor. It is not a service and
/// never receives a request. The payload is created with an explicit environment
/// that does not include this name.
const ANCHOR_ENV: &str = "HARNESS_SERVICE_CREATION_ANCHOR";
const PROCESS_CREATE_PROCESS: u32 = 0x0080;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

/// Hold a same-user process outside the helper's kill-on-close job so one
/// payload can inherit that process's job chain instead of the helper's.
fn hold_creation_anchor() -> ! {
    let bound = Deadline::after(Duration::from_secs(30)).ok();
    while bound.as_ref().is_none_or(|deadline| !deadline.expired()) {
        std::thread::sleep(Duration::from_millis(50));
    }
    std::process::exit(0);
}

fn environment_block(environment: &BTreeMap<String, String>) -> Vec<u16> {
    let mut block = Vec::new();
    for (name, value) in environment {
        block.extend(name.encode_utf16());
        block.push(u16::from(b'='));
        block.extend(value.encode_utf16());
        block.push(0);
    }
    block.push(0);
    block
}

fn anchor_command(program: &Path) -> io::Result<Vec<u16>> {
    let mut command = Vec::new();
    crate::process::quote_argument(program.as_os_str(), &mut command)?;
    command.push(32);
    crate::process::quote_argument(OsStr::new(CREATE_ARGUMENT), &mut command)?;
    Ok(command)
}

struct CreationAnchor(OwnedHandle);
impl Drop for CreationAnchor {
    fn drop(&mut self) {
        unsafe { TerminateProcess(self.0.as_raw_handle(), 0) };
    }
}

struct SuspendedPayload {
    process: OwnedHandle,
    thread: OwnedHandle,
    pid: u32,
    resumed: bool,
}
impl Drop for SuspendedPayload {
    fn drop(&mut self) {
        if !self.resumed {
            unsafe { TerminateProcess(self.process.as_raw_handle(), 0) };
        }
    }
}

fn same_executable(process: HANDLE, program: &Path) -> io::Result<bool> {
    let mut buffer = vec![0u16; 1024];
    let mut length = buffer.len() as u32;
    checked(unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) })?;
    let image = String::from_utf16_lossy(&buffer[..length as usize]);
    let image = image.trim_start_matches(r"\\?\");
    let expected = program.to_string_lossy();
    let expected = expected.trim_start_matches(r"\\?\");
    Ok(image.eq_ignore_ascii_case(expected))
}

/// Start one payload outside the helper's job. Breakaway and a creation-time
/// job list cannot do this: both leave the child in every ancestor that does
/// not release members, including the helper's kill-on-close job. A same-user
/// anchor created by WMI does not inherit that job. Naming it as the creation
/// parent makes the payload inherit the anchor's chain instead, so closing the
/// helper does not kill the service and the payload is not itself a WMI
/// `Win32_Process.Create` child.
fn create_sibling(
    program: &Path,
    command: &[u16],
    directory: &str,
    environment: &BTreeMap<String, String>,
) -> io::Result<u32> {
    let mut anchor_environment = Vec::new();
    if let Ok(root) = std::env::var("SystemRoot") {
        anchor_environment.push(format!("SystemRoot={root}"));
    }
    anchor_environment.push(format!("{ANCHOR_ENV}=1"));
    let anchor_pid = wmi::create(&anchor_command(program)?, directory, &anchor_environment)?;
    let anchor = CreationAnchor(owned(unsafe {
        OpenProcess(
            PROCESS_CREATE_PROCESS | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            anchor_pid,
        )
    })?);
    if !same_executable(anchor.0.as_raw_handle(), program)? {
        return Err(io::Error::other(
            "service creation anchor is not the trusted executable",
        ));
    }
    let application = wide(
        program
            .to_str()
            .ok_or_else(|| invalid("service executable is not Unicode"))?,
    )?;
    let mut line = command.to_vec();
    line.push(0);
    let directory = wide(directory)?;
    let environment = environment_block(environment);
    let mut bytes = 0usize;
    unsafe {
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut bytes);
    }
    if bytes == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut attributes = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        checked(InitializeProcThreadAttributeList(
            attributes.as_mut_ptr().cast(),
            1,
            0,
            &mut bytes,
        ))?;
    }
    let parent = anchor.0.as_raw_handle();
    let updated = unsafe {
        UpdateProcThreadAttribute(
            attributes.as_mut_ptr().cast(),
            0,
            PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
            (&parent as *const HANDLE).cast(),
            std::mem::size_of::<HANDLE>(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if updated == 0 {
        let error = io::Error::last_os_error();
        unsafe { DeleteProcThreadAttributeList(attributes.as_mut_ptr().cast()) };
        return Err(error);
    }
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attributes.as_mut_ptr().cast();
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NO_WINDOW
                | CREATE_UNICODE_ENVIRONMENT
                | CREATE_SUSPENDED
                | EXTENDED_STARTUPINFO_PRESENT,
            environment.as_ptr().cast(),
            directory.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        )
    };
    unsafe { DeleteProcThreadAttributeList(attributes.as_mut_ptr().cast()) };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut payload = SuspendedPayload {
        process: owned(info.hProcess)?,
        thread: owned(info.hThread)?,
        pid: info.dwProcessId,
        resumed: false,
    };
    let resumed = unsafe { ResumeThread(payload.thread.as_raw_handle()) };
    if resumed == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    payload.resumed = true;
    Ok(payload.pid)
}

/// Run only as an early command in a trusted native executable. The outer
/// spawn() call owns this helper's Job and bounds even a blocked provider.
/// Environment values travel through stdin, never argv or handoff files.
/// The payload is created as a sibling outside this helper's job. WMI remains
/// only when that creation is refused; a later join refusal stays visible.
pub fn create_helper() -> io::Result<u32> {
    if std::env::var_os(ANCHOR_ENV).is_some() {
        hold_creation_anchor();
    }
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
    // The helper may have been started through the stable installation link;
    // pin and report the ordinary file it resolves to.
    let program = crate::dependency_discovery::local_path(&std::env::current_exe()?)?;
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
    // Do not start a payload if validation already consumed the deadline.
    startup_remaining(request.startup_until)?;
    match create_sibling(&program, &command, directory, &request.environment) {
        Ok(pid) => Ok(pid),
        Err(sibling) => wmi::create(&command, directory, &environment).map_err(|fallback| {
            io::Error::other(format!(
                "job-free sibling creation failed ({sibling}); WMI fallback failed ({fallback})"
            ))
        }),
    }
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

    /// Kernel membership of this exact retained process in the account
    /// allowance. The caller holds the verified budget handle, so the name
    /// cannot resolve to another object; nothing is adopted, assigned or
    /// restarted by this check.
    pub fn in_shared_cpu_budget(&self, budget: &SharedCpuBudget) -> io::Result<bool> {
        let handle = open_shared_job(budget.name(), JOB_OBJECT_QUERY)?;
        in_job(self.handle.as_raw_handle(), handle.as_raw_handle())
    }

    /// Terminate this exact process after re-validating the recorded identity
    /// on a fresh terminate handle. A stale, exited or mismatched identity is
    /// never killed; callers own the recorded session, not arbitrary PIDs.
    pub fn terminate(&self, exit_code: u32) -> io::Result<bool> {
        if !self.is_running()? {
            return Ok(false);
        }
        let handle = owned(unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                self.identity.pid,
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
        if creation != self.identity.creation_time {
            return Err(io::Error::other(
                "process identity changed before termination; preserving process",
            ));
        }
        checked(unsafe { TerminateProcess(handle.as_raw_handle(), exit_code) })?;
        Ok(true)
    }
}

/// Kernel-verified coverage of one retained service process. `Covered` needs a
/// membership check against the verified account allowance; anything unknown,
/// inaccessible or merely name-similar is never reported as capped.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum SharedCpuCoverage {
    Covered { job: String, rate: u32 },
    Unadmitted { cause: String },
    Unverified { cause: String },
}

impl SharedCpuCoverage {
    pub fn is_covered(&self) -> bool {
        matches!(self, Self::Covered { .. })
    }

    /// One human line for logs and inspection. It never claims a ceiling that
    /// was not verified, and for an unadmitted service it names the recovery
    /// step: a safe restart, never adoption of the running process.
    pub fn describe(&self) -> String {
        match self {
            Self::Covered { job, rate } => format!(
                "inside the shared CPU allowance {job} at {}% of host capacity (hard cap)",
                f64::from(*rate) / 100.0
            ),
            Self::Unadmitted { cause } => format!(
                "outside the shared CPU allowance: {cause}; it runs uncapped by the account budget, so coverage stays incomplete until it is restarted"
            ),
            Self::Unverified { cause } => format!(
                "shared CPU allowance unverified: {cause}; enforcement is unknown for this service"
            ),
        }
    }
}

/// Verify one observed service process against the account CPU allowance. This
/// only reads kernel state: it never assigns, adopts or restarts a process, and
/// a service that is not a member is reported instead of being enrolled.
pub fn shared_cpu_coverage(service: &ServiceProcess, account: Option<&Path>) -> SharedCpuCoverage {
    let directory = match crate::process::cpu_budget_directory(account) {
        Ok(directory) => directory,
        Err(error) => {
            return SharedCpuCoverage::Unverified {
                cause: error.to_string(),
            };
        }
    };
    let percent = match shared_cpu_ceiling_for_account(&directory) {
        Ok(percent) => percent,
        Err(cause) => return SharedCpuCoverage::Unverified { cause },
    };
    let budget = match SharedCpuBudget::acquire(&directory, percent) {
        Ok(budget) => budget,
        Err(error) => {
            return SharedCpuCoverage::Unverified {
                cause: error.to_string(),
            };
        }
    };
    service
        .in_shared_cpu_budget(&budget)
        .and_then(|member| {
            if !member {
                return Ok(SharedCpuCoverage::Unadmitted {
                    cause: format!(
                        "process {} is not a member of the account allowance {}",
                        service.identity().pid,
                        budget.name()
                    ),
                });
            }
            Ok(SharedCpuCoverage::Covered {
                job: budget.name().to_owned(),
                rate: budget.snapshot()?.cpu_rate,
            })
        })
        .unwrap_or_else(|error| SharedCpuCoverage::Unverified {
            cause: error.to_string(),
        })
}

/// Visible fail-open diagnostic for a service outside verified coverage, at most
/// once per distinct process identity because start and join callers repeat.
/// Diagnostics use stderr only, so stdout, MCP STDIO and native protocols stay
/// untouched.
pub fn warn_shared_cpu(scope: &str, identity: ProcessIdentity, coverage: &SharedCpuCoverage) {
    if coverage.is_covered() {
        return;
    }
    let Ok(mut reported) = REPORTED_OUTSIDE_ALLOWANCE
        .get_or_init(|| Mutex::new(BTreeSet::new()))
        .lock()
    else {
        return;
    };
    if !reported.insert((identity.pid, identity.creation_time)) {
        return;
    }
    let _ = writeln!(
        io::stderr(),
        "coding-agents-harness: {scope} is {}",
        coverage.describe()
    );
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
    // A Codex MCP frontend is launched through its stable installation link.
    // The service must be pinned and started as the ordinary file that link
    // resolves to, otherwise the reparse path is refused and re-pointing the
    // link could retarget a service that is already running.
    let program = crate::dependency_discovery::local_path(program)?;
    let _program_guard = ReadGuard::open(&program)?;
    // The service joins the account CPU allowance from its own bootstrap. Pass
    // the account's storage location explicitly when the caller did not, so
    // admission does not depend on ambient variables a minimal service
    // environment omits. The service still validates that allowance itself and
    // never adopts one from a name the caller supplied.
    let mut environment = environment;
    if !environment
        .keys()
        .any(|name| name.eq_ignore_ascii_case(CPU_BUDGET_ACCOUNT_ENV))
        && let Ok(account) = crate::process::cpu_budget_directory(None)
    {
        environment.insert(
            CPU_BUDGET_ACCOUNT_ENV.into(),
            account.to_string_lossy().into_owned(),
        );
    }
    let began = creation_clock();
    let request = Request {
        directory: directory.into(),
        arguments,
        environment,
        startup_until: began / 10_000 + remaining.as_millis() as u64,
        user: current_user()?,
    };
    request.validate()?;
    request.command(&program)?;
    let bytes = serde_json::to_vec(&request)
        .map_err(|_| invalid("service request serialization failed"))?;
    if bytes.len() > MAX_REQUEST {
        return Err(invalid("service request exceeds its bound"));
    }
    let (stdin, write) = anonymous_pipe(4096)?;
    let (read, stdout) = anonymous_pipe(4096)?;
    let mut command = CommandSpec::new(&program);
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
        (0, Some(pid), None) => {
            let service = ServiceProcess::observe(pid, &program, began, &request.user)?;
            // Admission runs in the payload after this receipt. Do not report a
            // miss while that bootstrap is still joining.
            let mut coverage = shared_cpu_coverage(&service, None);
            let settle = std::time::Instant::now() + Duration::from_secs(2);
            while matches!(coverage, SharedCpuCoverage::Unadmitted { .. })
                && std::time::Instant::now() < settle
            {
                std::thread::sleep(Duration::from_millis(20));
                coverage = shared_cpu_coverage(&service, None);
            }
            // Fail-open reporting: a service that could not join the allowance
            // still starts and stays usable, and this client says so instead of
            // implying that the requested ceiling holds.
            warn_shared_cpu(
                &format!("the service started from {} (pid {pid})", program.display()),
                service.identity(),
                &coverage,
            );
            Ok(service)
        }
        (2, None, Some(error)) => Err(io::Error::other(format!("native service helper: {error}"))),
        _ => Err(invalid("inconsistent service helper receipt")),
    }
}

/// Join the account-wide CPU allowance before any service payload work, and
/// return the handle the service keeps for its whole lifetime, the verified
/// host rate, and whether this process already belonged to another job.
///
/// The sibling creation path leaves this process outside the helper and client
/// jobs. It joins the allowance itself and only then creates its lifecycle job
/// inside that allowance. A process already in another job can join only an
/// empty allowance or one already in its own hierarchy. A kernel refusal is a
/// failed allowance, not a retry or an assumed cap. WMI fallback can still land
/// in such a job, and that refusal stays visible.
fn join_shared_cpu(deadline: Deadline) -> io::Result<(SharedCpuBudget, u32, bool)> {
    let percent = if let Some(hatch) = crate::heavy_command::shared_cpu_escape_hatch() {
        hatch.map_err(policy_join_error)?.percent
    } else {
        let directory = crate::process::cpu_budget_directory(None)?;
        crate::heavy_command::shared_cpu_policy_ceiling(&directory)
            .map_err(policy_join_error)?
            .percent
    };
    let directory = crate::process::cpu_budget_directory(None)
        .map_err(|error| ceiling_error(percent, error))?;
    let budget =
        SharedCpuBudget::acquire_within(&directory, percent, deadline, &Cancellation::default())
            .map_err(|error| ceiling_error(percent, error))?;
    let snapshot = budget.snapshot()?;
    if !snapshot.cpu_hard_cap
        || snapshot.kill_on_close
        || snapshot.job_memory_limit_bytes != 0
        || snapshot.breakaway_ok
        || snapshot.silent_breakaway_ok
    {
        return Err(ceiling_error(
            percent,
            io::Error::other(
                "the account CPU allowance does not carry the required CPU-only settings; preserving it",
            ),
        ));
    }
    // Recorded for reporting: a service that starts inside another job nests the
    // allowance under that container instead of standing at the root.
    let contained = in_job(unsafe { GetCurrentProcess() }, std::ptr::null_mut())?;
    let handle = open_shared_job(budget.name(), JOB_OBJECT_ASSIGN_PROCESS | JOB_OBJECT_QUERY)?;
    let assigned = unsafe { AssignProcessToJobObject(handle.as_raw_handle(), GetCurrentProcess()) };
    if assigned == 0 {
        let error = io::Error::last_os_error();
        return Err(ceiling_error(
            percent,
            io::Error::other(format!(
                "the kernel refused joining the account allowance ({error}); the service already belongs to another job ({contained}), and a contained process can only join an empty allowance or one inside its own job hierarchy"
            )),
        ));
    }
    if !in_job(unsafe { GetCurrentProcess() }, handle.as_raw_handle())? {
        return Err(ceiling_error(
            percent,
            io::Error::other("shared CPU allowance assignment was not observed"),
        ));
    }
    Ok((budget, snapshot.cpu_rate, contained))
}

fn shared_cpu_ceiling_for_account(directory: &Path) -> Result<f64, String> {
    if let Some(hatch) = crate::heavy_command::shared_cpu_escape_hatch() {
        return match hatch {
            Ok(ceiling) => Ok(ceiling.percent),
            Err(fault) => Err(policy_join_error(fault).to_string()),
        };
    }
    match crate::heavy_command::shared_cpu_policy_ceiling(directory) {
        Ok(ceiling) => Ok(ceiling.percent),
        Err(fault) => Err(policy_join_error(fault).to_string()),
    }
}

fn policy_join_error(fault: crate::heavy_command::SharedCpuCeilingFault) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "shared CPU policy record is not usable ({}); requested ceiling {}; this service runs once outside a verified shared ceiling and no substitute ceiling was applied; recovery: {}",
            fault.cause, fault.requested, fault.recovery
        ),
    )
}

fn ceiling_error(percent: f64, error: io::Error) -> io::Error {
    io::Error::other(format!(
        "shared {}% ceiling: {error}",
        crate::heavy_command::shared_cpu_percent_label(percent)
    ))
}

/// Ceiling for the service's own lifecycle Job. Windows expresses a nested
/// job's rate against its rate-controlled parent, so an inner percentage would
/// otherwise silently shrink to a fraction of the shared allowance. A requested
/// ceiling that is not lower than the allowance adds no separate limit at all,
/// because the allowance already binds the whole tree.
fn nested_cpu_percent(requested: Option<f64>, host_rate: u32) -> Option<f64> {
    let requested = requested?;
    let host_percent = f64::from(host_rate) / 100.0;
    if !(0.01..host_percent).contains(&requested) {
        return None;
    }
    let nested = requested / (host_percent / 100.0);
    nested.is_finite().then_some(nested)
}

/// Concise fail-open diagnostic for a service outside the allowance: failed
/// ceiling, cause, affected scope and recovery step. It never claims that the
/// ceiling is enforced and never implies that a peer lost its own allowance.
fn shared_cpu_service_warning(error: &io::Error) -> String {
    if error.kind() == io::ErrorKind::InvalidData {
        return format!("coding-agents-harness: {error}");
    }
    let ceiling = error
        .to_string()
        .strip_prefix("shared ")
        .and_then(|rest| rest.split(" ceiling:").next())
        .filter(|label| label.ends_with('%'))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{SHARED_CPU_PERCENT}%"));
    format!(
        "coding-agents-harness: this service and its children run outside the shared {ceiling} CPU allowance for this account: {error}. Other sessions keep their own allowance, and coverage stays incomplete until the account CPU budget is usable and this service is restarted into it."
    )
}

/// The service owns this Job, not any client. Bootstrap callers must enter
/// before opening backend code or starting children, publish readiness only
/// after their instance lock + endpoint, and implement a bounded idle shutdown.
pub struct ServiceGuard {
    job: Job,
    shared: Option<SharedCpuBudget>,
    shared_cpu_container: bool,
    shared_cpu_warning: Option<String>,
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
        // Account identity and the bounded startup window are checked before any
        // shared or service state is created.
        match current_user() {
            Ok(actual) if actual == expected_user => (),
            _ => std::process::exit(2),
        }
        let remaining =
            startup_remaining(startup_until).unwrap_or_else(|_| std::process::exit(124));
        // The account allowance is joined first so it stays outside the
        // service's own lifecycle Job. A service that cannot join it still
        // starts, reported as degraded, rather than losing availability.
        let mut limits = limits;
        let mut shared_cpu_warning = None;
        let mut shared_cpu_container = false;
        let shared = match join_shared_cpu(
            Deadline::after(remaining.min(SHARED_CPU_JOIN_WAIT))
                .unwrap_or_else(|_| std::process::exit(2)),
        ) {
            Ok((budget, host_rate, contained)) => {
                limits.cpu_percent = nested_cpu_percent(limits.cpu_percent, host_rate);
                shared_cpu_container = contained;
                Some(budget)
            }
            Err(error) => {
                // Human diagnostics never use stdout, and the write is
                // harmless when this service has no stream yet: the log
                // attached by `redirect_standard_streams` repeats it.
                let warning = shared_cpu_service_warning(&error);
                let _ = writeln!(io::stderr(), "{warning}");
                shared_cpu_warning = Some(warning);
                None
            }
        };
        let job = Job::new(limits)?;
        if job.contain_current_process().is_err() {
            std::process::exit(2);
        }
        // Once self-contained, exit explicitly on bootstrap errors. Dropping
        // the live Job would otherwise obscure the selected failure exit code.
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
            shared,
            shared_cpu_container,
            shared_cpu_warning,
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
        // The log is the established diagnostic channel for services, so a
        // start-time allowance warning is repeated here once. Diagnostics never
        // reach stdout or any machine protocol stream.
        if let Some(warning) = &self.shared_cpu_warning {
            let _ = writeln!(io::stderr(), "{warning}");
        }
        Ok(())
    }
    pub fn job(&self) -> &Job {
        &self.job
    }
    /// The verified account allowance this service joined, retained for the
    /// service's lifetime. `None` means the service runs degraded.
    pub fn shared_cpu(&self) -> Option<&SharedCpuBudget> {
        self.shared.as_ref()
    }
    /// True when this service already belonged to another job at bootstrap (a
    /// session or provider job), so the account allowance is nested inside that
    /// container instead of standing at the root of the hierarchy. Membership,
    /// settings and lifecycle ownership are unaffected; the nesting position
    /// decides which later participants can still join the same allowance, so
    /// inspection reports it instead of assuming a root position.
    pub fn shared_cpu_container(&self) -> bool {
        self.shared_cpu_container
    }
    /// Degraded start diagnostic: what failed and which action admits the
    /// service next time. `None` while the allowance is verified.
    pub fn shared_cpu_warning(&self) -> Option<&str> {
        self.shared_cpu_warning.as_deref()
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

#[cfg(test)]
mod tests {
    use super::nested_cpu_percent;

    #[test]
    fn nested_cpu_percent_preserves_host_relative_meaning() {
        // 25% of host under the 75% allowance is 33.33% of the parent, which the
        // kernel quantizes down to 3333 hundredths of a percent.
        let nested = nested_cpu_percent(Some(25.0), 7500).unwrap();
        assert_eq!((nested * 100.0).floor() as u32, 3333);
        assert_eq!(nested_cpu_percent(Some(0.5), 7500), Some(0.5 / 0.75));
        assert_eq!(nested_cpu_percent(Some(0.01), 7500), Some(0.01 / 0.75));
        // A ceiling that is not lower than the allowance adds no second limit.
        assert_eq!(nested_cpu_percent(Some(75.0), 7500), None);
        assert_eq!(nested_cpu_percent(Some(100.0), 7500), None);
        assert_eq!(nested_cpu_percent(None, 7500), None);
        assert_eq!(nested_cpu_percent(Some(f64::NAN), 7500), None);
        assert_eq!(nested_cpu_percent(Some(0.0), 7500), None);
    }
}
