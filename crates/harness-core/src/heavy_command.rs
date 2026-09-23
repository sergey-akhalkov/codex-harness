//! One account-wide heavy-command slot: a single local machine budget, one
//! serialized admission queue and one bounded process tree per admitted command.
//!
//! The queue lives in a machine-local account directory outside every checkout,
//! worktree and Codex home, so concurrent callers share one allowance instead of
//! multiplying it per consumer. Interactive sessions and model conversations stay
//! outside this slot: an admitted command is a bounded batch operation whose whole
//! Windows Job is terminated, and whose lease is released, before the next caller
//! is admitted. A command that bypasses this entry point is outside the enforced
//! scope.
//!
//! Every native caller that both queues heavy work and mutates build state uses
//! one order: account admission first, then the state lock. A nested native caller
//! inside an admitted tree detects the live inherited lease instead of re-acquiring
//! it, which is what keeps that order deadlock-free; a heavy command must not nest
//! a different account directory.
#![cfg(windows)]

use crate::{
    build_identity,
    native_build::{directory, ordinary_ancestors, resolve_tool},
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, Outcome, ProcessIdentity},
    resource_admission::{Lease, Resource},
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    fmt, fs,
    io::{self, Read, Write},
    mem::zeroed,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{FILETIME, HANDLE, WAIT_TIMEOUT},
    System::{
        Console::SetConsoleCtrlHandler,
        Threading::{
            GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        },
    },
};

/// Local account-queue override for owned fixtures and relocated state.
pub const ACCOUNT_ENV: &str = "CODEX_HARNESS_HEAVY_ACCOUNT";
/// Inherited by an admitted command tree so nested native callers share the live
/// lease instead of deadlocking on it. It carries the holder identity and is only
/// honored while that exact process still runs.
pub const LEASE_ENV: &str = "CODEX_HARNESS_HEAVY_LEASE_V1";

const SCHEMA: u32 = 1;
const OWNER: &[u8] = b"codex-harness-heavy-command-v1\n";
const OWNER_FILE: &str = "owner";
const POLICY_FILE: &str = "budget.json";
const HOLDER_FILE: &str = "holder.json";
const MAX_RECORD: u64 = 64 * 1024;
const MAX_LABEL: usize = 400;
const MIB: usize = 1024 * 1024;
const DEFAULT_MEMORY_BYTES: usize = 8 * 1024 * MIB;
const DEFAULT_CPU_PERCENT: f64 = 50.0;
const DEFAULT_DEADLINE_SECONDS: u64 = 1800;
const DEFAULT_QUEUE_WAIT_SECONDS: u64 = 3600;
const MIN_MEMORY_BYTES: usize = 16 * MIB;
const MAX_MEMORY_BYTES: usize = 1024 * 1024 * MIB;
const MAX_SECONDS: u64 = 7 * 24 * 60 * 60;
/// Cleanup budget after the root exits or the command is stopped.
const CLEANUP: Duration = Duration::from_secs(10);

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn foreign(message: &str) -> io::Error {
    io::Error::other(message.to_owned())
}

/// The one machine budget. Memory and CPU are enforced by one Windows Job per
/// admitted command, `deadline_seconds` bounds the command and
/// `queue_wait_seconds` bounds how long a caller waits for the account slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    pub memory_bytes: usize,
    pub cpu_percent: f64,
    pub deadline_seconds: u64,
    pub queue_wait_seconds: u64,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            memory_bytes: DEFAULT_MEMORY_BYTES,
            cpu_percent: DEFAULT_CPU_PERCENT,
            deadline_seconds: DEFAULT_DEADLINE_SECONDS,
            queue_wait_seconds: DEFAULT_QUEUE_WAIT_SECONDS,
        }
    }
}

/// Local policy fields are optional so one value can be adjusted in place.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    schema: u32,
    #[serde(default)]
    memory_bytes: Option<usize>,
    #[serde(default)]
    cpu_percent: Option<f64>,
    #[serde(default)]
    deadline_seconds: Option<u64>,
    #[serde(default)]
    queue_wait_seconds: Option<u64>,
}

impl Budget {
    /// Machine values stay local; an invalid or out-of-range policy is an error
    /// before any command starts.
    pub fn validate(&self, source: &str) -> io::Result<()> {
        if !(MIN_MEMORY_BYTES..=MAX_MEMORY_BYTES).contains(&self.memory_bytes) {
            return Err(invalid(format!(
                "{source}: memory_bytes must be within {MIN_MEMORY_BYTES}..={MAX_MEMORY_BYTES}"
            )));
        }
        if !self.cpu_percent.is_finite() || !(0.01..=100.0).contains(&self.cpu_percent) {
            return Err(invalid(format!(
                "{source}: cpu_percent must be finite and within 0.01..=100"
            )));
        }
        for (name, value) in [
            ("deadline_seconds", self.deadline_seconds),
            ("queue_wait_seconds", self.queue_wait_seconds),
        ] {
            if !(1..=MAX_SECONDS).contains(&value) {
                return Err(invalid(format!(
                    "{source}: {name} must be within 1..={MAX_SECONDS}"
                )));
            }
        }
        Ok(())
    }

    /// Absent local policy keeps the installed defaults. Errors name the policy
    /// file instead of silently running with a different budget.
    pub fn read(account: &Path) -> io::Result<Self> {
        if !account_is_owned(account)? {
            return Ok(Self::default());
        }
        let path = account.join(POLICY_FILE);
        let Some(bytes) = read_bounded(&path)? else {
            return Ok(Self::default());
        };
        let policy: Policy = serde_json::from_slice(&bytes).map_err(|error| {
            invalid(format!(
                "{} is not a valid heavy-command policy: {error}",
                path.display()
            ))
        })?;
        if policy.schema != SCHEMA {
            return Err(invalid(format!(
                "{}: unsupported policy schema {}",
                path.display(),
                policy.schema
            )));
        }
        let budget = Self {
            memory_bytes: policy.memory_bytes.unwrap_or(DEFAULT_MEMORY_BYTES),
            cpu_percent: policy.cpu_percent.unwrap_or(DEFAULT_CPU_PERCENT),
            deadline_seconds: policy.deadline_seconds.unwrap_or(DEFAULT_DEADLINE_SECONDS),
            queue_wait_seconds: policy
                .queue_wait_seconds
                .unwrap_or(DEFAULT_QUEUE_WAIT_SECONDS),
        };
        budget.validate(&path.display().to_string())?;
        Ok(budget)
    }

    /// Persist one complete local policy; the account directory stays outside
    /// every tracked configuration.
    pub fn write(account: &Path, budget: &Budget) -> io::Result<PathBuf> {
        budget.validate("heavy-command budget")?;
        prepare(account)?;
        let path = account.join(POLICY_FILE);
        ordinary_ancestors(&path)?;
        if path.exists() {
            build_identity::ordinary(&path)?;
        }
        let policy = Policy {
            schema: SCHEMA,
            memory_bytes: Some(budget.memory_bytes),
            cpu_percent: Some(budget.cpu_percent),
            deadline_seconds: Some(budget.deadline_seconds),
            queue_wait_seconds: Some(budget.queue_wait_seconds),
        };
        let staging = policy_staging(account);
        if staging.exists() {
            build_identity::ordinary(&staging)?;
            fs::remove_file(&staging)?;
        }
        fs::write(&staging, serde_json::to_vec_pretty(&policy)?)?;
        fs::rename(&staging, &path)?;
        Ok(path)
    }

    pub fn deadline(&self) -> io::Result<Deadline> {
        Deadline::after(Duration::from_secs(self.deadline_seconds))
    }

    pub fn queue_deadline(&self) -> io::Result<Deadline> {
        Deadline::after(Duration::from_secs(self.queue_wait_seconds))
    }
}

/// The local policy file for one account directory.
pub fn policy_path(account: &Path) -> PathBuf {
    account.join(POLICY_FILE)
}

fn policy_staging(account: &Path) -> PathBuf {
    account.join("budget.json.tmp")
}

/// Explicit directory, then the local override, then the machine account
/// location. Nothing here depends on the checkout, worktree or CODEX_HOME.
pub fn account_dir(explicit: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(path) = explicit {
        return absolute_directory(path, "--account");
    }
    if let Some(value) = std::env::var_os(ACCOUNT_ENV).filter(|value| !value.is_empty()) {
        return absolute_directory(Path::new(&value), ACCOUNT_ENV);
    }
    let parent = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            io::Error::other(
                "heavy-command account storage is unavailable; LOCALAPPDATA is not set",
            )
        })?;
    Ok(Path::new(&parent)
        .join("coding-agents-harness")
        .join("heavy-command"))
}

fn absolute_directory(path: &Path, source: &str) -> io::Result<PathBuf> {
    if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
        return Err(invalid(format!(
            "{source} must be an absolute normalized directory"
        )));
    }
    Ok(path.to_owned())
}

/// Create the owned account directory when absent and never adopt a foreign one.
pub fn prepare(account: &Path) -> io::Result<()> {
    if account_is_owned(account)? {
        return Ok(());
    }
    directory(account)?;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(account.join(OWNER_FILE))
    {
        Ok(mut file) => {
            file.write_all(OWNER)?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    if account_is_owned(account)? {
        Ok(())
    } else {
        Err(foreign(
            "Heavy-command account ownership is missing; preserving it.",
        ))
    }
}

fn account_is_owned(account: &Path) -> io::Result<bool> {
    ordinary_ancestors(account)?;
    if !account.exists() {
        return Ok(false);
    }
    build_identity::ordinary(account)?;
    if !account.is_dir() {
        return Err(foreign(
            "Heavy-command account is not a directory; preserving it.",
        ));
    }
    let marker = account.join(OWNER_FILE);
    if !marker.exists() {
        return Err(foreign(
            "Heavy-command account has no ownership record; preserving it.",
        ));
    }
    build_identity::ordinary(&marker)?;
    let mut bytes = Vec::new();
    fs::File::open(&marker)?
        .take((OWNER.len() + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes != OWNER {
        return Err(foreign(
            "Heavy-command account has foreign ownership; preserving it.",
        ));
    }
    Ok(true)
}

fn read_bounded(path: &Path) -> io::Result<Option<Vec<u8>>> {
    ordinary_ancestors(path)?;
    match fs::symlink_metadata(path) {
        Ok(_) => build_identity::ordinary(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_RECORD + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err(invalid(format!(
            "{} exceeds its record bound",
            path.display()
        )));
    }
    Ok(Some(bytes))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseMarker {
    schema: u32,
    account: PathBuf,
    holder: ProcessIdentity,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HolderRecord {
    schema: u32,
    pid: u32,
    creation_time: u64,
    started_unix_ms: u128,
    command: String,
}

/// A held account slot. Drop clears the diagnostic holder record while the slot
/// is still owned and then releases the lease.
pub struct Holder {
    _lease: Lease,
    account: PathBuf,
    identity: ProcessIdentity,
}

impl Holder {
    fn acquire(
        account: &Path,
        budget: &Budget,
        label: &str,
        cancellation: &Cancellation,
    ) -> io::Result<Self> {
        let identity = current_identity()?;
        let lease = Lease::acquire_reporting(
            account,
            Resource::HeavyCommand,
            budget.queue_deadline()?,
            cancellation,
            || {
                eprintln!(
                    "heavy: waiting for the account heavy-command slot; holder {}",
                    holder_description(account)
                );
            },
        )?;
        let holder = Self {
            _lease: lease,
            account: account.to_owned(),
            identity,
        };
        if let Err(error) = write_holder(account, identity, label) {
            eprintln!("heavy: holder record not written: {error}");
        }
        Ok(holder)
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.account.join(HOLDER_FILE));
    }
}

/// Account admission for one native operation.
pub enum Admission {
    /// This process holds the account slot and releases it when dropped.
    Held(Holder),
    /// A live ancestor already holds the account slot for this command tree.
    Inherited(ProcessIdentity),
}

impl Admission {
    /// Queue for the account slot, or join the live lease this process tree
    /// already inherited from an admitted ancestor.
    pub fn acquire(
        account: &Path,
        budget: &Budget,
        label: &str,
        cancellation: &Cancellation,
    ) -> io::Result<Self> {
        prepare(account)?;
        if let Some(holder) = inherited_holder(std::env::var_os(LEASE_ENV).as_deref(), account) {
            eprintln!(
                "heavy: sharing the account heavy-command slot already held by pid={} for this command tree",
                holder.pid
            );
            return Ok(Self::Inherited(holder));
        }
        Ok(Self::Held(Holder::acquire(
            account,
            budget,
            label,
            cancellation,
        )?))
    }

    pub fn holder(&self) -> ProcessIdentity {
        match self {
            Self::Held(holder) => holder.identity,
            Self::Inherited(identity) => *identity,
        }
    }

    /// The marker every admitted process passes to its children so nested native
    /// callers can share this lease.
    pub fn marker(&self, account: &Path) -> io::Result<OsString> {
        let marker = LeaseMarker {
            schema: SCHEMA,
            account: account.to_owned(),
            holder: self.holder(),
        };
        Ok(serde_json::to_string(&marker)?.into())
    }
}

fn inherited_holder(value: Option<&OsStr>, account: &Path) -> Option<ProcessIdentity> {
    let marker: LeaseMarker = serde_json::from_str(&value?.to_string_lossy()).ok()?;
    if marker.schema != SCHEMA || !same_directory(&marker.account, account) {
        return None;
    }
    live_process(marker.holder).then_some(marker.holder)
}

fn same_directory(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn write_holder(account: &Path, identity: ProcessIdentity, label: &str) -> io::Result<()> {
    let record = HolderRecord {
        schema: SCHEMA,
        pid: identity.pid,
        creation_time: identity.creation_time,
        started_unix_ms: unix_millis(),
        command: label.to_owned(),
    };
    let staging = account.join("holder.json.tmp");
    ordinary_ancestors(&staging)?;
    fs::write(&staging, serde_json::to_vec(&record)?)?;
    fs::rename(&staging, account.join(HOLDER_FILE))
}

/// Best-effort description of the recorded slot holder for queue diagnostics.
fn holder_description(account: &Path) -> String {
    let path = account.join(HOLDER_FILE);
    let Ok(Some(bytes)) = read_bounded(&path) else {
        return "is not recorded".into();
    };
    let Ok(record) = serde_json::from_slice::<HolderRecord>(&bytes) else {
        return "record is unreadable".into();
    };
    let identity = ProcessIdentity {
        pid: record.pid,
        creation_time: record.creation_time,
    };
    if record.schema != SCHEMA || !live_process(identity) {
        return "is no longer running".into();
    }
    format!("pid={} command={}", record.pid, record.command)
}

/// One admitted command's outcome and wall-clock duration.
pub struct Run {
    pub outcome: Outcome,
    pub elapsed: Duration,
}

/// A failure before the command ran (`Start`) or while its owned tree was being
/// cleaned after it ran (`Cleanup`). Both remain distinct from a stop reason.
#[derive(Debug)]
pub enum RunError {
    Start(io::Error),
    Cleanup(io::Error),
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(error) => write!(formatter, "the command could not be started: {error}"),
            Self::Cleanup(error) => write!(
                formatter,
                "the command tree could not be cleaned within its budget: {error}"
            ),
        }
    }
}

impl std::error::Error for RunError {}

/// Resolve a program directly, without a shell, keeping the rustup argv[0]
/// contract of the native tool resolution.
pub fn resolve_program(program: &OsStr) -> io::Result<PathBuf> {
    resolve_tool(program).map_err(|error| {
        io::Error::other(format!(
            "heavy command '{}' is unavailable ({error}); a heavy command is executed directly and never through a shell",
            program.to_string_lossy()
        ))
    })
}

/// Bounded single-line description of one heavy command for diagnostics.
pub fn label(program: &OsStr, args: &[OsString]) -> String {
    let mut text = String::new();
    for part in std::iter::once(program.to_owned()).chain(args.iter().cloned()) {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&part.to_string_lossy());
        if text.chars().count() > MAX_LABEL {
            break;
        }
    }
    if text.chars().count() > MAX_LABEL {
        text = text.chars().take(MAX_LABEL).collect::<String>() + "...";
    }
    text
}

/// Run one command inside a fresh bounded Job under the account budget. The
/// command tree is cleaned before this returns; only a cleanup deadline failure
/// is reported as `Cleanup`.
pub fn execute(
    budget: &Budget,
    account: &Path,
    program: &Path,
    args: &[OsString],
    admission: &Admission,
    cancellation: &Cancellation,
) -> Result<Run, RunError> {
    let deadline = budget.deadline().map_err(RunError::Start)?;
    let mut command = CommandSpec::new(program.to_owned());
    command.args = args.to_vec();
    command
        .inherit_standard_streams()
        .map_err(RunError::Start)?;
    command.env.insert(
        OsString::from(LEASE_ENV),
        Some(admission.marker(account).map_err(RunError::Start)?),
    );
    let job = Job::new(Limits {
        memory_bytes: Some(budget.memory_bytes),
        cpu_percent: Some(budget.cpu_percent),
    })
    .map_err(RunError::Start)?;
    let child = job.spawn(&command).map_err(RunError::Start)?;
    eprintln!(
        "heavy: started pid={} memory_limit_bytes={} cpu_percent={} deadline_seconds={}",
        child.identity().pid,
        budget.memory_bytes,
        budget.cpu_percent,
        budget.deadline_seconds
    );
    let started = Instant::now();
    let outcome = job
        .wait(&child, deadline, cancellation, CLEANUP)
        .map_err(RunError::Cleanup)?;
    Ok(Run {
        outcome,
        elapsed: started.elapsed(),
    })
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default()
}

fn ticks(value: FILETIME) -> u64 {
    ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
}

fn creation_time(handle: HANDLE) -> Option<u64> {
    let (mut creation, mut exit, mut kernel, mut user) =
        unsafe { (zeroed(), zeroed(), zeroed(), zeroed()) };
    if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return None;
    }
    Some(ticks(creation))
}

fn current_identity() -> io::Result<ProcessIdentity> {
    let creation_time =
        creation_time(unsafe { GetCurrentProcess() }).ok_or_else(io::Error::last_os_error)?;
    Ok(ProcessIdentity {
        pid: unsafe { GetCurrentProcessId() },
        creation_time,
    })
}

/// Read-only liveness: never grants cleanup authority over the process.
fn live_process(identity: ProcessIdentity) -> bool {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    let raw = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            identity.pid,
        )
    };
    if raw.is_null() {
        return false;
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    // A signalled process object means the recorded holder has exited.
    if unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } != WAIT_TIMEOUT {
        return false;
    }
    creation_time(handle.as_raw_handle()) == Some(identity.creation_time)
}

static ACTIVE: OnceLock<Cancellation> = OnceLock::new();

/// Survives Ctrl+C/Ctrl+Break long enough to stop waiting or clean the admitted
/// tree and report a distinct interruption instead of dying in the console
/// handler. Both processes share the console and receive the event.
pub struct Interrupt;

impl Interrupt {
    pub fn install(cancellation: &Cancellation) -> io::Result<Self> {
        let _ = ACTIVE.set(cancellation.clone());
        if unsafe { SetConsoleCtrlHandler(Some(console_control), 1) } == 0 {
            let error = io::Error::last_os_error();
            // A pipe-only invocation can have no attached console.
            if error.raw_os_error() != Some(6) {
                return Err(error);
            }
        }
        Ok(Self)
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        unsafe {
            SetConsoleCtrlHandler(Some(console_control), 0);
        }
    }
}

unsafe extern "system" fn console_control(event: u32) -> i32 {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    if matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        if let Some(cancellation) = ACTIVE.get() {
            cancellation.cancel();
        }
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_installed_values_and_validate() {
        let budget = Budget::default();
        assert_eq!(budget.memory_bytes, 8 * 1024 * MIB);
        assert_eq!(budget.cpu_percent, 50.0);
        assert_eq!(budget.deadline_seconds, 1800);
        assert_eq!(budget.queue_wait_seconds, 3600);
        budget.validate("default").unwrap();
        for invalid in [
            Budget {
                memory_bytes: 0,
                ..budget
            },
            Budget {
                cpu_percent: 0.0,
                ..budget
            },
            Budget {
                cpu_percent: f64::NAN,
                ..budget
            },
            Budget {
                deadline_seconds: 0,
                ..budget
            },
            Budget {
                queue_wait_seconds: MAX_SECONDS + 1,
                ..budget
            },
        ] {
            assert!(invalid.validate("fixture").is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn local_policy_round_trips_and_rejects_foreign_or_broken_files() {
        let temp = tempfile::tempdir().unwrap();
        let account = temp.path().join("account");
        assert_eq!(Budget::read(&account).unwrap(), Budget::default());
        prepare(&account).unwrap();
        prepare(&account).unwrap();
        assert_eq!(Budget::read(&account).unwrap(), Budget::default());
        let adjusted = Budget {
            memory_bytes: 1024 * MIB,
            cpu_percent: 25.0,
            ..Budget::default()
        };
        assert_eq!(
            Budget::write(&account, &adjusted).unwrap(),
            policy_path(&account)
        );
        assert_eq!(Budget::read(&account).unwrap(), adjusted);
        fs::write(policy_path(&account), b"{").unwrap();
        assert!(
            Budget::read(&account)
                .unwrap_err()
                .to_string()
                .contains("policy")
        );
        fs::write(policy_path(&account), b"{\"schema\":1,\"cpu_percent\":0.0}").unwrap();
        let error = Budget::read(&account).unwrap_err().to_string();
        assert!(error.contains("cpu_percent"), "{error}");
        fs::write(policy_path(&account), b"{\"schema\":1,\"memories\":1}").unwrap();
        assert!(Budget::read(&account).is_err());
        let foreign = temp.path().join("foreign");
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("keep"), "keep").unwrap();
        assert!(prepare(&foreign).is_err());
        assert!(Budget::read(&foreign).is_err());
        assert_eq!(fs::read_to_string(foreign.join("keep")).unwrap(), "keep");
    }

    #[test]
    fn lease_marker_is_only_honored_for_the_same_account_and_a_live_holder() {
        let temp = tempfile::tempdir().unwrap();
        let account = temp.path().join("account");
        prepare(&account).unwrap();
        let live = current_identity().unwrap();
        let marker = LeaseMarker {
            schema: SCHEMA,
            account: account.clone(),
            holder: live,
        };
        let value = serde_json::to_string(&marker).unwrap();
        assert_eq!(
            inherited_holder(Some(OsStr::new(&value)), &account),
            Some(live)
        );
        let elsewhere = temp.path().join("elsewhere");
        prepare(&elsewhere).unwrap();
        assert_eq!(inherited_holder(Some(OsStr::new(&value)), &elsewhere), None);
        let dead = LeaseMarker {
            holder: ProcessIdentity {
                pid: live.pid,
                creation_time: live.creation_time.wrapping_add(1),
            },
            ..marker
        };
        let value = serde_json::to_string(&dead).unwrap();
        assert_eq!(inherited_holder(Some(OsStr::new(&value)), &account), None);
        assert_eq!(inherited_holder(None, &account), None);
        assert_eq!(
            inherited_holder(Some(OsStr::new("not json")), &account),
            None
        );
    }

    #[test]
    fn labels_stay_bounded_single_line_descriptions() {
        let args = vec![OsString::from("--release"), OsString::from("x".repeat(900))];
        let text = label(OsStr::new("cargo"), &args);
        assert!(text.starts_with("cargo --release "), "{text}");
        assert!(text.len() <= MAX_LABEL + 8, "{}", text.len());
        assert!(!text.contains('\n'));
    }
}
