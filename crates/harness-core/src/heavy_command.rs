//! One account-wide heavy-command budget: a bounded slot set and one containment
//! tree per admitted command.
//!
//! The queue lives in a machine-local account directory outside every checkout,
//! worktree and Codex home, so concurrent callers share one allowance instead of
//! multiplying it per consumer. Interactive sessions and model conversations stay
//! outside these slots: an admitted command is a bounded batch operation whose
//! containment Job is terminated, and whose slot is released, before that slot can
//! be taken again. A command that bypasses this entry point is outside the enforced
//! scope.
//!
//! Every native caller that both queues heavy work and mutates build state uses
//! one order: account admission first, then the state lock. A nested native caller
//! inside an admitted tree is accepted only after the kernel confirms that its
//! process belongs to the admitted named Job the inherited marker names; a live
//! peer identity or a copied name is not containment. Nested callers then add only
//! kill-on-close containment. They do not take a second slot or a second aggregate
//! limit, because a nested Windows Job's CPU rate is a proportion of its parent's
//! rate and the outer admission already holds the account envelope. A heavy command
//! must not nest a different account directory.
//!
//! Admission order is slot, then the shared CPU budget, then the account aggregate
//! Job, then the payload. A caller waiting for a slot holds no Job handle and no
//! CPU budget lock. The aggregate Job is `JOB_OBJECT_LIMIT_JOB_MEMORY` only; the
//! shared CPU job is not given a memory limit. Payload creation nests outermost
//! first: the shared CPU job when it exists, then the aggregate job, then the
//! per-tree containment job. Slot count 1 takes the legacy lock exclusively and
//! creates no slot file.
//!
//! The retired per-batch CPU default (50%) is not a second default. With no
//! explicit per-operation limit the shared ceiling is the CPU policy and the
//! batch Job carries no CPU rate; a deliberately configured lower limit keeps
//! its documented host-relative meaning by being translated against the
//! kernel-verified parent rate, rounding down, and is reported. Membership is
//! never taken from a marker: this owner asks the kernel whether this process
//! and the payload it created really belong to the account budget object.
#![cfg(windows)]

use crate::{
    build_identity,
    native_build::{directory, ordinary_ancestors, resolve_tool},
    process::{
        Cancellation, CommandSpec, Deadline, HeavyAggregate, Job, Limits, Outcome, ProcessIdentity,
        SHARED_CPU_PERCENT, SharedCpuBudget, cpu_budget_directory,
    },
    resource_admission::HeavyAdmission,
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
        JobObjects::{AssignProcessToJobObject, IsProcessInJob, OpenJobObjectW},
        Threading::{
            GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        },
    },
};

/// Local account-queue override for owned fixtures and relocated state.
pub const ACCOUNT_ENV: &str = "CODEX_HARNESS_HEAVY_ACCOUNT";
/// Inherited by an admitted command tree so nested native callers share the live
/// lease instead of deadlocking on it. It names the admitted Job and its holder;
/// it is honored only while that holder still runs and the kernel confirms this
/// process is a member of that Job.
pub const LEASE_ENV: &str = "CODEX_HARNESS_HEAVY_LEASE_V1";

const SCHEMA: u32 = 1;
const OWNER: &[u8] = b"codex-harness-heavy-command-v1\n";
const OWNER_FILE: &str = "owner";
const POLICY_FILE: &str = "budget.json";
const HOLDER_FILE: &str = "holder.json";
const HOLDER_EXCLUSIVE_FILE: &str = "holder.exclusive.json";
const MAX_RECORD: u64 = 64 * 1024;
const MAX_LABEL: usize = 400;
const MAX_DIAGNOSTIC: usize = 480;
/// Documented JOB_OBJECT_QUERY right (winnt.h). windows-sys does not export the
/// job access rights; a query handle can neither terminate nor assign, so a
/// verified member never receives cleanup authority over the admitted tree.
const JOB_OBJECT_QUERY: u32 = 0x0004;
/// Documented JOB_OBJECT_ASSIGN_PROCESS right (winnt.h), used only to admit
/// this process into the CPU-only account budget. Assigning carries no
/// termination or configuration authority over the object or its members.
const JOB_OBJECT_ASSIGN_PROCESS: u32 = 0x0001;
/// Session-local Job object name prefix for one admitted tree.
const JOB_NAME_PREFIX: &str = "CodingAgentsHarness.HeavyCommand.";
const MIB: usize = 1024 * 1024;
const DEFAULT_MEMORY_BYTES: usize = 8 * 1024 * MIB;
const DEFAULT_MAX_CONCURRENT_TREES: u32 = 2;
const DEFAULT_DEADLINE_SECONDS: u64 = 1800;
const DEFAULT_QUEUE_WAIT_SECONDS: u64 = 3600;
const MIN_MEMORY_BYTES: usize = 16 * MIB;
const MAX_MEMORY_BYTES: usize = 1024 * 1024 * MIB;
const MAX_SECONDS: u64 = 7 * 24 * 60 * 60;
/// Cleanup budget after the root exits or the command is stopped.
const CLEANUP: Duration = Duration::from_secs(10);
/// Bounded wait for the account CPU budget lock. The budget owner creates or
/// verifies one small object and holds its lock for that work alone.
const CPU_BUDGET_LOCK_WAIT: Duration = Duration::from_secs(10);
/// Rate units for "no rate-controlled ancestor": an inner rate is then a
/// percentage of total host CPU capacity.
const HOST_RATE: u32 = 10_000;
/// The retired per-batch CPU default. A policy that still records exactly this
/// value cannot be distinguished from an intentional override, so the value is
/// preserved, reported and never silently changed into a second default.
const LEGACY_DEFAULT_CPU_PERCENT: f64 = 50.0;

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn foreign(message: &str) -> io::Error {
    io::Error::other(message.to_owned())
}

/// The one machine budget. Memory and CPU are enforced by one Windows Job per
/// admitted command, `deadline_seconds` bounds the command and
/// `queue_wait_seconds` bounds how long a caller waits for the account slot.
/// `max_concurrent_trees` is the positive slot bound (1 restores the exclusive
/// legacy lock). `aggregate_memory_limit_bytes` is the account envelope; a missing
/// policy field uses the effective per-tree limit.
/// An absent `cpu_percent` is the installed default: no per-operation CPU limit
/// exists, so the shared account ceiling is the CPU policy and the command's Job
/// must not add a second rate. A present value is a deliberate per-operation
/// ceiling in percent of total host CPU capacity; it is translated against the
/// kernel-verified parent rate before it reaches a nested Job.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    pub memory_bytes: usize,
    pub cpu_percent: Option<f64>,
    pub deadline_seconds: u64,
    pub queue_wait_seconds: u64,
    pub max_concurrent_trees: u32,
    pub aggregate_memory_limit_bytes: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            memory_bytes: DEFAULT_MEMORY_BYTES,
            cpu_percent: None,
            deadline_seconds: DEFAULT_DEADLINE_SECONDS,
            queue_wait_seconds: DEFAULT_QUEUE_WAIT_SECONDS,
            max_concurrent_trees: DEFAULT_MAX_CONCURRENT_TREES,
            aggregate_memory_limit_bytes: DEFAULT_MEMORY_BYTES,
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
    #[serde(default)]
    max_concurrent_trees: Option<u32>,
    #[serde(default)]
    aggregate_memory_limit_bytes: Option<usize>,
}

/// Whether one reported budget field was present in the policy file.
///
/// A missing file and a file that omits the field are both [`Self::Default`].
/// Inspection never rewrites the file to record that default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyFieldSource {
    /// The field was present in the account policy file.
    PolicyFile,
    /// The field was absent, so the effective value is the installed default.
    Default,
}

impl PolicyFieldSource {
    /// Stable text and JSON token: `policy-file` or `default`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PolicyFile => "policy-file",
            Self::Default => "default",
        }
    }

    const fn from_present(present: bool) -> Self {
        if present {
            Self::PolicyFile
        } else {
            Self::Default
        }
    }
}

/// Effective budget plus the origin of each concurrency field. Query-only:
/// constructing this does not admit a slot, join a Job or rewrite policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BudgetQuery {
    pub budget: Budget,
    pub memory_bytes: PolicyFieldSource,
    pub max_concurrent_trees: PolicyFieldSource,
    pub aggregate_memory_limit_bytes: PolicyFieldSource,
}

impl BudgetQuery {
    fn installed_defaults() -> Self {
        Self {
            budget: Budget::default(),
            memory_bytes: PolicyFieldSource::Default,
            max_concurrent_trees: PolicyFieldSource::Default,
            aggregate_memory_limit_bytes: PolicyFieldSource::Default,
        }
    }
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
        if let Some(percent) = self.cpu_percent
            && (!percent.is_finite() || !(0.01..=100.0).contains(&percent))
        {
            return Err(invalid(format!(
                "{source}: cpu_percent must be finite and within 0.01..=100 when present"
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
        if self.max_concurrent_trees == 0 {
            return Err(invalid(format!(
                "{source}: max_concurrent_trees must be positive"
            )));
        }
        if self.aggregate_memory_limit_bytes < self.memory_bytes {
            return Err(invalid(format!(
                "{source}: aggregate_memory_limit_bytes {} is below the per-tree memory limit {}; refusing before admission",
                self.aggregate_memory_limit_bytes, self.memory_bytes
            )));
        }
        if !(MIN_MEMORY_BYTES..=MAX_MEMORY_BYTES).contains(&self.aggregate_memory_limit_bytes) {
            return Err(invalid(format!(
                "{source}: aggregate_memory_limit_bytes must be within {MIN_MEMORY_BYTES}..={MAX_MEMORY_BYTES}"
            )));
        }
        Ok(())
    }

    /// Absent local policy keeps the installed defaults. A file that omits the
    /// newer fields still parses: missing slot count is 2 and a missing aggregate
    /// equals the effective per-tree limit. This read never rewrites the file.
    /// Errors name the policy file instead of silently running with a different budget.
    pub fn read(account: &Path) -> io::Result<Self> {
        Ok(Self::query(account)?.budget)
    }

    /// Effective values and the policy-file or default origin of each concurrency
    /// field. A missing aggregate is the default equal to the effective per-tree
    /// limit. This query never rewrites the file, admits a slot or starts work.
    pub fn query(account: &Path) -> io::Result<BudgetQuery> {
        if !account_is_owned(account)? {
            return Ok(BudgetQuery::installed_defaults());
        }
        let path = account.join(POLICY_FILE);
        let Some(bytes) = read_bounded(&path)? else {
            return Ok(BudgetQuery::installed_defaults());
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
        let memory_present = policy.memory_bytes.is_some();
        let slots_present = policy.max_concurrent_trees.is_some();
        let aggregate_present = policy.aggregate_memory_limit_bytes.is_some();
        let memory_bytes = policy.memory_bytes.unwrap_or(DEFAULT_MEMORY_BYTES);
        let budget = Self {
            memory_bytes,
            cpu_percent: policy.cpu_percent,
            deadline_seconds: policy.deadline_seconds.unwrap_or(DEFAULT_DEADLINE_SECONDS),
            queue_wait_seconds: policy
                .queue_wait_seconds
                .unwrap_or(DEFAULT_QUEUE_WAIT_SECONDS),
            max_concurrent_trees: policy
                .max_concurrent_trees
                .unwrap_or(DEFAULT_MAX_CONCURRENT_TREES),
            aggregate_memory_limit_bytes: policy
                .aggregate_memory_limit_bytes
                .unwrap_or(memory_bytes),
        };
        budget.validate(&path.display().to_string())?;
        Ok(BudgetQuery {
            budget,
            memory_bytes: PolicyFieldSource::from_present(memory_present),
            max_concurrent_trees: PolicyFieldSource::from_present(slots_present),
            aggregate_memory_limit_bytes: PolicyFieldSource::from_present(aggregate_present),
        })
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
            cpu_percent: budget.cpu_percent,
            deadline_seconds: Some(budget.deadline_seconds),
            queue_wait_seconds: Some(budget.queue_wait_seconds),
            max_concurrent_trees: Some(budget.max_concurrent_trees),
            aggregate_memory_limit_bytes: Some(budget.aggregate_memory_limit_bytes),
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

/// Translate a host-relative ceiling into the rate units of one inner Job whose
/// nearest rate-controlled ancestor is `parent_rate` (0.01% units of host CPU;
/// `HOST_RATE` when no rate-controlled ancestor was verified). Rounding is
/// downwards, so the translated cap never exceeds the documented host-relative
/// ceiling. `None` means the requested ceiling is not lower than its parent's,
/// so the inner Job must carry no rate at all.
fn translate_ceiling(percent: f64, parent_rate: u32) -> Option<u32> {
    let target = (percent * 100.0).floor() as u64;
    if target >= u64::from(parent_rate) {
        return None;
    }
    let inner = target * 10_000 / u64::from(parent_rate);
    u32::try_from(inner.max(1)).ok()
}

/// The percent that the Job owner's rate control floors to exactly this rate:
/// the intent stays in rate units, so no float drift can shave the cap.
fn percent_of_rate(rate: u32) -> f64 {
    (f64::from(rate) + 0.5) / 100.0
}

/// Effective host-relative ceiling of one translated inner rate, in percent of
/// total host CPU capacity.
fn effective_percent(inner_rate: u32, parent_rate: u32) -> f64 {
    f64::from(inner_rate) * f64::from(parent_rate) / 1_000_000.0
}

/// Percent of host CPU as a compact, stable decimal (`75`, `24.9975`).
fn percent_text(percent: f64) -> String {
    format!("{percent}")
}

fn legacy_default_note() -> String {
    format!(
        "; this value equals the retired {LEGACY_DEFAULT_CPU_PERCENT}% batch default, so a legacy policy file and an intentional override cannot be told apart: the limit is preserved and reported (set --cpu-percent shared to use the shared ceiling)"
    )
}

/// True when the effective policy records a value that cannot be told apart
/// from the retired per-batch default: such a value is preserved and reported
/// rather than silently dropped.
pub fn legacy_default_cpu_percent(budget: &Budget) -> bool {
    budget.cpu_percent == Some(LEGACY_DEFAULT_CPU_PERCENT)
}

/// One line describing the effective per-operation CPU policy in host-relative
/// terms, for inspection. Inspection never joins the budget, so the shared
/// ceiling is named by the installed policy value, not by a readback.
pub fn cpu_policy_summary(budget: &Budget) -> String {
    match budget.cpu_percent {
        None => format!(
            "no per-operation CPU limit; the shared account {SHARED_CPU_PERCENT}% ceiling is the CPU policy"
        ),
        Some(percent) => format!(
            "per-operation CPU limit {percent}% of host CPU, translated against the shared account {SHARED_CPU_PERCENT}% ceiling when this command runs{}",
            if percent == LEGACY_DEFAULT_CPU_PERCENT {
                legacy_default_note()
            } else {
                String::new()
            }
        ),
    }
}

/// Machine-local shared CPU policy record written by the installation lifecycle.
/// Launch paths read it and never write it. The format is the installed schema:
/// `schema` 1 and `ceiling_percent` in percent of total host CPU.
const SHARED_CPU_POLICY_FILE: &str = "shared-cpu-policy.json";
const SHARED_CPU_POLICY_SCHEMA: u64 = 1;
/// Per-launch escape hatch. It wins over the policy record and is not a second record.
pub(crate) const SHARED_CPU_ESCAPE_HATCH: &str = "CODEX_HARNESS_CPU_PERCENT";

/// Ceiling a launch will actually request from the shared account group.
#[derive(Debug)]
pub(crate) struct SharedCpuCeiling {
    pub percent: f64,
    /// Text placed after "requested ceiling" in fail-open diagnostics.
    pub requested: String,
}

/// The policy record or escape hatch cannot be used. Callers warn and must not
/// substitute another ceiling.
#[derive(Debug)]
pub(crate) struct SharedCpuCeilingFault {
    pub requested: String,
    pub stage: &'static str,
    pub cause: String,
    pub recovery: String,
}

/// Compact percent text matching the existing diagnostics (`75`, `40`, `0.5`).
pub(crate) fn shared_cpu_percent_label(percent: f64) -> String {
    percent_text(percent)
}

/// `Some` when the per-launch escape hatch is set. A parse failure is a fault,
/// not a fallthrough to the policy record: the explicit request stays the
/// requested ceiling.
pub(crate) fn shared_cpu_escape_hatch() -> Option<Result<SharedCpuCeiling, SharedCpuCeilingFault>> {
    let value = std::env::var_os(SHARED_CPU_ESCAPE_HATCH).filter(|value| !value.is_empty())?;
    let text = value.to_string_lossy().into_owned();
    let requested = format!("{text}% of host CPU");
    Some(match text.trim().parse::<f64>() {
        Ok(percent) => Ok(SharedCpuCeiling { percent, requested }),
        Err(_) => Err(SharedCpuCeilingFault {
            requested,
            stage: "ceiling configuration",
            cause: format!("{SHARED_CPU_ESCAPE_HATCH} does not hold a number"),
            recovery: format!(
                "set {SHARED_CPU_ESCAPE_HATCH} to a percentage within 0.01..=100 or unset it to use the installed {SHARED_CPU_PERCENT}% default"
            ),
        }),
    })
}

/// Ceiling from the account policy record. An absent record is the installed
/// 75% default. A malformed or unreadable record is a fault and is not rewritten.
pub(crate) fn shared_cpu_policy_ceiling(
    directory: &Path,
) -> Result<SharedCpuCeiling, SharedCpuCeilingFault> {
    let path = directory.join(SHARED_CPU_POLICY_FILE);
    match read_shared_cpu_policy(&path) {
        Ok(None) => Ok(SharedCpuCeiling {
            percent: SHARED_CPU_PERCENT,
            requested: format!("{SHARED_CPU_PERCENT}% of host CPU"),
        }),
        Ok(Some(percent)) => Ok(SharedCpuCeiling {
            percent,
            requested: format!("{}% of host CPU", percent_text(percent)),
        }),
        Err(error) => Err(SharedCpuCeilingFault {
            requested: format!(
                "unreadable shared CPU policy record {} (not a substitute ceiling)",
                path.display()
            ),
            stage: "ceiling configuration",
            cause: error.to_string(),
            recovery: format!(
                "repair {} so it is an ordinary schema {SHARED_CPU_POLICY_SCHEMA} file with ceiling_percent within 0.01..=100, or remove it to use the installed {SHARED_CPU_PERCENT}% default; {SHARED_CPU_ESCAPE_HATCH} remains the per-launch escape hatch",
                path.display()
            ),
        }),
    }
}

fn read_shared_cpu_policy(path: &Path) -> io::Result<Option<f64>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(io::Error::other(format!(
                "shared CPU policy record could not be read ({error}); it was preserved"
            )));
        }
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::other(
                "shared CPU policy record is a link; preserving it",
            ));
        }
        Ok(_) => {}
    }
    build_identity::ordinary(path).map_err(|error| {
        io::Error::other(format!(
            "shared CPU policy record is not an ordinary file ({error}); it was preserved"
        ))
    })?;
    let bytes = fs::read(path).map_err(|error| {
        io::Error::other(format!(
            "shared CPU policy record could not be read ({error}); it was preserved"
        ))
    })?;
    parse_shared_cpu_policy(&bytes).map(Some)
}

fn parse_shared_cpu_policy(bytes: &[u8]) -> io::Result<f64> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
        io::Error::other(format!(
            "shared CPU policy record is not JSON ({error}); it was preserved and was not replaced with the installed default"
        ))
    })?;
    let schema_ok =
        value.get("schema").and_then(serde_json::Value::as_u64) == Some(SHARED_CPU_POLICY_SCHEMA);
    let ceiling = value
        .get("ceiling_percent")
        .and_then(serde_json::Value::as_f64)
        .filter(|percent| percent.is_finite() && (0.01..=100.0).contains(percent));
    match (schema_ok, ceiling) {
        (true, Some(percent)) => Ok(percent),
        (false, _) => Err(io::Error::other(
            "shared CPU policy record schema is not 1; the record was preserved and was not replaced with the installed default",
        )),
        (true, None) => Err(io::Error::other(
            "shared CPU policy record has no ceiling_percent within 0.01..=100; the record was preserved and was not replaced with the installed default",
        )),
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
    /// Query-only handle target: membership in this Job is the admission proof.
    job: String,
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
    job: String,
}

/// The account-wide shared CPU budget as this process verified it: the
/// established object, the parent rate the kernel reports for it, and whether
/// the kernel confirms that this process belongs to it. A missing object or a
/// missing membership is reported as degraded coverage; it is never silently
/// treated as a capped start and never breaks the heavy-command contracts.
struct SharedCpu {
    budget: Option<SharedCpuBudget>,
    parent_rate: u32,
    admitted: bool,
}

impl SharedCpu {
    /// Join the account budget and admit this process before any payload
    /// exists. Called after the account heavy-command queue, so the documented
    /// lock order (queue first, budget second) always holds.
    fn join(cancellation: &Cancellation) -> Self {
        if let Some(hatch) = shared_cpu_escape_hatch() {
            return match hatch {
                Ok(ceiling) => Self::join_at(ceiling, cancellation),
                Err(fault) => Self::policy_fault(&fault),
            };
        }
        let directory = match cpu_budget_directory(None) {
            Ok(directory) => directory,
            Err(error) => return Self::degraded(&error),
        };
        match shared_cpu_policy_ceiling(&directory) {
            Ok(ceiling) => Self::join_at(ceiling, cancellation),
            Err(fault) => Self::policy_fault(&fault),
        }
    }

    fn join_at(ceiling: SharedCpuCeiling, cancellation: &Cancellation) -> Self {
        let directory = match cpu_budget_directory(None) {
            Ok(directory) => directory,
            Err(error) => return Self::degraded_at(ceiling.percent, &error),
        };
        let deadline = match Deadline::after(CPU_BUDGET_LOCK_WAIT) {
            Ok(deadline) => deadline,
            Err(error) => return Self::degraded_at(ceiling.percent, &error),
        };
        let budget = match SharedCpuBudget::acquire_within(
            &directory,
            ceiling.percent,
            deadline,
            cancellation,
        ) {
            Ok(budget) => budget,
            Err(error) => return Self::degraded_at(ceiling.percent, &error),
        };
        let mut state = Self {
            budget: Some(budget),
            parent_rate: HOST_RATE,
            admitted: false,
        };
        state.read_back();
        state.admit();
        state
    }

    /// Degraded state: the heavy-command contracts stay in force, the CPU
    /// policy does not, and the cause is on the diagnostic channel.
    fn degraded(error: &io::Error) -> Self {
        Self::degraded_at(SHARED_CPU_PERCENT, error)
    }

    fn degraded_at(percent: f64, error: &io::Error) -> Self {
        eprintln!(
            "heavy: warning: the shared account CPU budget is unavailable ({error}); this command runs outside the shared {}% ceiling and its coverage is degraded",
            percent_text(percent)
        );
        Self {
            budget: None,
            parent_rate: HOST_RATE,
            admitted: false,
        }
    }

    /// The record could not be used. Do not acquire at the installed default:
    /// that would silently replace the requested ceiling.
    fn policy_fault(fault: &SharedCpuCeilingFault) -> Self {
        eprintln!(
            "heavy: warning: shared CPU policy record is not usable ({}); requested ceiling {}; this command runs once outside a verified shared ceiling and no substitute ceiling was applied; recovery: {}",
            fault.cause, fault.requested, fault.recovery
        );
        Self {
            budget: None,
            parent_rate: HOST_RATE,
            admitted: false,
        }
    }

    /// Kernel readback of the object this process joined: the rate an inner Job
    /// would be relative to, and the containment flags that must stay off.
    fn read_back(&mut self) {
        let Some(budget) = &self.budget else { return };
        match budget.snapshot() {
            Ok(snapshot) if snapshot.cpu_rate > 0 => {
                self.parent_rate = snapshot.cpu_rate;
                eprintln!(
                    "heavy: shared account CPU budget job=\"{}\" cpu_rate={} hard_cap={} members={}",
                    budget.name(),
                    snapshot.cpu_rate,
                    snapshot.cpu_hard_cap,
                    snapshot.active_processes
                );
            }
            Ok(_) => eprintln!(
                "heavy: warning: the shared account CPU budget job=\"{}\" carries no enabled rate; this command has no verified parent ceiling",
                budget.name()
            ),
            Err(error) => {
                eprintln!("heavy: warning: the shared account CPU budget readback failed ({error})")
            }
        }
    }

    /// Kernel membership, never a copied marker: ask the object that the
    /// verified account directory names whether this process belongs to it, and
    /// admit the process only while the kernel says it does not.
    fn admit(&mut self) {
        let Some(budget) = &self.budget else { return };
        if in_job(budget.name()) {
            self.admitted = true;
            eprintln!(
                "heavy: this process is a kernel-verified member of the shared account CPU budget"
            );
            return;
        }
        match admit_current_process(budget.name()) {
            Ok(()) => {
                self.admitted = true;
                eprintln!(
                    "heavy: this process joined the shared account CPU budget before any payload starts"
                );
            }
            Err(error) => eprintln!(
                "heavy: warning: this process could not join the shared account CPU budget ({error}); a payload created by a consumer-owned spawn may run outside it"
            ),
        }
    }

    fn budget(&self) -> Option<&SharedCpuBudget> {
        self.budget.as_ref()
    }

    /// Kernel-verified parent rate in 0.01% units that an inner rate would be
    /// relative to (host rate when no rate-controlled ancestor was verified).
    fn parent_rate(&self) -> u32 {
        self.parent_rate
    }

    /// Inner rate for one owned lifecycle Job: `None` while no per-operation
    /// limit is configured or the limit is not lower than the verified parent
    /// ceiling, so the shared ceiling stays the only cap.
    fn inner_rate(&self, budget: &Budget) -> Option<u32> {
        translate_ceiling(budget.cpu_percent?, self.parent_rate)
    }
}

/// Admit this process into the named CPU-only budget when the kernel allows it,
/// then confirm membership with the kernel. Assigning needs only the documented
/// JOB_OBJECT_ASSIGN_PROCESS right and grants no termination or configuration
/// authority over the object or its members.
fn admit_current_process(name: &str) -> io::Result<()> {
    use std::os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    };
    let object: Vec<u16> = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let raw = unsafe { OpenJobObjectW(JOB_OBJECT_ASSIGN_PROCESS, 0, object.as_ptr()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    if unsafe { AssignProcessToJobObject(handle.as_raw_handle(), GetCurrentProcess()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if !in_job(name) {
        return Err(io::Error::other(
            "the kernel did not observe the account budget assignment",
        ));
    }
    Ok(())
}

/// A held account slot. Drop removes only this admission's holder record, then
/// releases the slot and the aggregate handle. It does not clear another
/// holder's record.
pub struct Holder {
    _admission: HeavyAdmission,
    slot_index: Option<u32>,
    aggregate: HeavyAggregate,
    record: PathBuf,
    identity: ProcessIdentity,
    job: String,
    cpu: SharedCpu,
}

impl Holder {
    fn acquire(
        account: &Path,
        budget: &Budget,
        label: &str,
        cancellation: &Cancellation,
    ) -> io::Result<Self> {
        let identity = current_identity()?;
        // The same OS-random key source the broker endpoint uses; the name is
        // unguessable so it cannot be squatted by an unrelated caller.
        let job = format!("{JOB_NAME_PREFIX}{}", crate::broker_endpoint::random_key()?);
        let slot_count = budget.max_concurrent_trees;
        // Slot first. The waiting callback runs before this returns, so a waiter
        // holds no CPU budget lock and no aggregate Job handle.
        let admission = HeavyAdmission::acquire(
            account,
            slot_count,
            budget.queue_deadline()?,
            cancellation,
            || {
                eprintln!("{}", queue_diagnostic(account, slot_count));
            },
        )?;
        let cpu = SharedCpu::join(cancellation);
        let aggregate = HeavyAggregate::acquire_within(
            account,
            budget.aggregate_memory_limit_bytes,
            Deadline::after(CPU_BUDGET_LOCK_WAIT)?,
            cancellation,
        )?;
        let record = holder_record_path(account, admission.slot_index());
        if let Err(error) = write_holder(&record, identity, label, &job) {
            eprintln!("heavy: holder record not written: {error}");
        }
        Ok(Self {
            slot_index: admission.slot_index(),
            _admission: admission,
            aggregate,
            record,
            identity,
            job,
            cpu,
        })
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.record);
    }
}

/// A verified nested admission: the kernel confirmed this process belongs to
/// the admitted Job the inherited marker names, so that tree already applies the
/// aggregate heavy-command budget.
pub struct Inherited {
    job: String,
    holder: ProcessIdentity,
    cpu: SharedCpu,
}

/// Account admission for one native operation.
pub enum Admission {
    /// This process holds the account slot and releases it when dropped.
    Held(Box<Holder>),
    Inherited(Inherited),
}

impl Admission {
    /// Queue for the account slot, or join the live lease this process tree
    /// already inherited from an admitted Job. The account heavy-command queue
    /// is taken first and the shared CPU budget second; a caller that waits for
    /// the slot holds no budget lock and no Job handle. The aggregate Job is
    /// joined only after both of those, and a nested call does not acquire a slot.
    pub fn acquire(
        account: &Path,
        budget: &Budget,
        label: &str,
        cancellation: &Cancellation,
    ) -> io::Result<Self> {
        budget.validate("heavy-command budget")?;
        prepare(account)?;
        if let Some(marker) = inherited(std::env::var_os(LEASE_ENV).as_deref(), account) {
            eprintln!(
                "heavy: inheriting the aggregate heavy-command budget through admitted Job \"{}\" (holder pid={}); this process is a verified member",
                marker.job, marker.holder.pid
            );
            return Ok(Self::Inherited(Inherited {
                job: marker.job,
                holder: marker.holder,
                cpu: SharedCpu::join(cancellation),
            }));
        }
        Ok(Self::Held(
            Holder::acquire(account, budget, label, cancellation)?.into(),
        ))
    }

    fn cpu(&self) -> &SharedCpu {
        match self {
            Self::Held(holder) => &holder.cpu,
            Self::Inherited(inherited) => &inherited.cpu,
        }
    }

    /// The verified shared account CPU budget, when this run has one.
    pub fn budget(&self) -> Option<&SharedCpuBudget> {
        self.cpu().budget()
    }

    /// One line describing the CPU policy this admitted tree will apply, in
    /// host-relative terms. The parent rate is the kernel readback, never the
    /// requested value.
    pub fn cpu_report(&self, budget: &Budget) -> String {
        if matches!(self, Self::Inherited(_)) {
            return "heavy: cpu policy: the admitted tree already applies the aggregate CPU policy; this nested call adds no second CPU cap".into();
        }
        let parent = self.cpu().parent_rate();
        match self.cpu().budget() {
            None => match budget.cpu_percent {
                None => "heavy: cpu policy: the shared account ceiling was not established (degraded); no per-operation limit is configured".into(),
                Some(percent) => format!(
                    "heavy: cpu policy: the shared account ceiling was not established (degraded); per-operation limit {percent}% of host CPU applies against host capacity{}",
                    if percent == LEGACY_DEFAULT_CPU_PERCENT {
                        legacy_default_note()
                    } else {
                        String::new()
                    }
                ),
            },
            Some(_) => match (budget.cpu_percent, self.cpu().inner_rate(budget)) {
                (None, _) => "heavy: cpu policy: no per-operation limit; the shared account CPU ceiling is the CPU policy for this batch".into(),
                (Some(percent), None) => format!(
                    "heavy: cpu policy: per-operation limit {percent}% of host CPU is not lower than the shared account ceiling {}%; the shared ceiling governs and no inner rate is applied{}",
                    percent_text(f64::from(parent) / 100.0),
                    if percent == LEGACY_DEFAULT_CPU_PERCENT {
                        legacy_default_note()
                    } else {
                        String::new()
                    }
                ),
                (Some(percent), Some(inner)) => format!(
                    "heavy: cpu policy: per-operation limit {percent}% of host CPU; inner cpu_rate={inner} against the verified parent rate {} (effective {}% of host CPU){}",
                    percent_text(f64::from(parent) / 100.0),
                    percent_text(effective_percent(inner, parent)),
                    if percent == LEGACY_DEFAULT_CPU_PERCENT {
                        legacy_default_note()
                    } else {
                        String::new()
                    }
                ),
            },
        }
    }

    pub fn holder(&self) -> ProcessIdentity {
        match self {
            Self::Held(holder) => holder.identity,
            Self::Inherited(inherited) => inherited.holder,
        }
    }

    /// The admitted Job every marker of this tree names.
    pub fn job_name(&self) -> &str {
        match self {
            Self::Held(holder) => holder.job.as_str(),
            Self::Inherited(inherited) => inherited.job.as_str(),
        }
    }

    /// `aggregate` when this process owns the lease, `containment` when an
    /// admitted Job already applies the budget for this tree.
    pub fn scope(&self) -> &'static str {
        match self {
            Self::Held(_) => "aggregate",
            Self::Inherited(_) => "containment",
        }
    }

    /// Limits for the per-tree containment Job. The account aggregate envelope
    /// is a separate Job the slot owner already holds. This Job keeps the
    /// per-tree memory limit and any translated per-operation CPU rate. A
    /// verified nested caller adds containment only, because Windows applies a
    /// nested Job's CPU rate as a proportion of its parent's rate
    /// (JOBOBJECT_CPU_RATE_CONTROL_INFORMATION Remarks). With no explicit
    /// per-operation limit the shared outer ceiling is the CPU policy, so the
    /// Job carries no rate at all.
    pub fn limits(&self, budget: &Budget) -> Limits {
        match self {
            Self::Held(_) => Limits {
                memory_bytes: Some(budget.memory_bytes),
                cpu_percent: self.inner_percent(budget),
            },
            Self::Inherited(_) => Limits {
                memory_bytes: None,
                cpu_percent: None,
            },
        }
    }

    /// The translated inner rate for this tree, if any.
    fn inner_rate(&self, budget: &Budget) -> Option<u32> {
        match self {
            Self::Held(_) => self.cpu().inner_rate(budget),
            Self::Inherited(_) => None,
        }
    }

    fn inner_percent(&self, budget: &Budget) -> Option<f64> {
        self.inner_rate(budget).map(percent_of_rate)
    }

    /// The one owned Job every consumer of this owner runs its children in,
    /// together with the marker those children inherit. The lease holder names
    /// the Job and applies the aggregate limits; a verified nested caller adds
    /// an anonymous kill-on-close Job with no second cap. The kernel readback of
    /// the created Job must carry exactly the intended rate: a translated limit
    /// is never assumed, and an accidental second cap is refused.
    pub fn owned_job(&self, budget: &Budget, account: &Path) -> io::Result<(Job, OsString)> {
        let limits = self.limits(budget);
        let job = match self {
            Self::Held(holder) => Job::new_named(limits, &holder.job)?,
            Self::Inherited(_) => Job::new(limits)?,
        };
        let rate = job.snapshot()?.cpu_rate;
        match self.inner_rate(budget) {
            Some(intended) if rate != intended => {
                return Err(io::Error::other(format!(
                    "the per-operation CPU limit did not reach the admitted Job (kernel readback {rate}, intended {intended}); refusing to run with an unverified CPU policy"
                )));
            }
            None if rate != 0 => {
                return Err(io::Error::other(format!(
                    "the admitted Job carries an unexpected CPU rate {rate} while no per-operation limit applies"
                )));
            }
            _ => {}
        }
        Ok((job, self.marker(account)?))
    }

    /// One diagnostics line describing the Job that actually enforces this
    /// tree, plus the account aggregate readback when this process holds it.
    pub fn job_line(&self, job: &Job) -> io::Result<String> {
        let snapshot = job.snapshot()?;
        let mut line = format!(
            "heavy: job scope={} name={} memory_limit_bytes={} cpu_rate={} kill_on_close={}",
            self.scope(),
            self.job_name(),
            snapshot.memory_limit_bytes,
            snapshot.cpu_rate,
            snapshot.kill_on_close
        );
        if let Self::Held(holder) = self {
            let aggregate = holder.aggregate.snapshot()?;
            line.push_str(&format!(
                " slot={} aggregate_job=\"{}\" aggregate_limit_flags={} aggregate_memory_limit_bytes={} aggregate_process_memory_limit_bytes={} aggregate_cpu_rate={} aggregate_kill_on_close={}",
                match holder.slot_index {
                    Some(index) => index.to_string(),
                    None => "exclusive".to_owned(),
                },
                holder.aggregate.name(),
                aggregate.limit_flags,
                aggregate.job_memory_limit_bytes,
                aggregate.process_memory_limit_bytes,
                aggregate.cpu_rate,
                aggregate.kill_on_close
            ));
        }
        Ok(line)
    }

    /// The marker every admitted process passes to its children so nested native
    /// callers can share this lease.
    pub fn marker(&self, account: &Path) -> io::Result<OsString> {
        let marker = LeaseMarker {
            schema: SCHEMA,
            account: account.to_owned(),
            job: self.job_name().to_owned(),
            holder: self.holder(),
        };
        Ok(serde_json::to_string(&marker)?.into())
    }

    /// Whether this held admission's aggregate job contains `process`. A nested
    /// admission has no second aggregate handle and cannot answer.
    pub(crate) fn payload_is_in_aggregate(
        &self,
        process: &crate::process::OwnedProcess,
    ) -> io::Result<bool> {
        match self {
            Self::Held(holder) => holder.aggregate.contains(process),
            Self::Inherited(_) => Err(io::Error::other(
                "a nested admission does not hold a second aggregate handle",
            )),
        }
    }

    /// Create a suspended payload in the admitted envelope. A held admission
    /// places it in the shared CPU job when one exists, then the account
    /// aggregate, then `lifecycle`. Membership that is false or unreadable is
    /// refused here, before the caller can resume the payload. A nested
    /// admission does not acquire a second aggregate limit.
    pub fn spawn_in_envelope(
        &self,
        lifecycle: &Job,
        command: &CommandSpec,
    ) -> io::Result<crate::process::SuspendedProcess> {
        let suspended = match self {
            Self::Held(holder) => {
                holder
                    .aggregate
                    .spawn_suspended(holder.cpu.budget(), lifecycle, command)?
            }
            Self::Inherited(_) => match self.budget() {
                Some(shared) => shared.spawn_suspended(lifecycle, command)?,
                None => lifecycle.spawn_suspended(command)?,
            },
        };
        if let Self::Held(holder) = self {
            aggregate_membership_gate(
                self.payload_is_in_aggregate(suspended.process()),
                suspended.process().identity().pid,
                holder.aggregate.name(),
            )?;
            eprintln!(
                "heavy: payload pid={} is a kernel-verified member of the account aggregate job=\"{}\"",
                suspended.process().identity().pid,
                holder.aggregate.name()
            );
        }
        Ok(suspended)
    }
}

/// False or unreadable aggregate membership refuses the start. A warning is not
/// acceptance: the caller must not resume the payload after this returns an error.
fn aggregate_membership_gate(
    observed: io::Result<bool>,
    pid: u32,
    job_name: &str,
) -> io::Result<()> {
    match observed {
        Ok(true) => Ok(()),
        Ok(false) => Err(io::Error::other(format!(
            "payload pid={pid} is outside the account aggregate job=\"{job_name}\"; refusing to start"
        ))),
        Err(error) => Err(io::Error::other(format!(
            "account aggregate membership could not be read ({error}); refusing to start"
        ))),
    }
}

/// Verified containment, not identity: the marker is honored only while its
/// recorded holder still runs and the kernel confirms that this process belongs
/// to the Job the marker names, and only while that name is one of this owner's
/// admitted-tree names. A live peer PID, a stale marker, a copied Job name or a
/// marker that names an unrelated Job this process happens to be in therefore
/// cannot claim the aggregate allowance.
fn inherited(value: Option<&OsStr>, account: &Path) -> Option<LeaseMarker> {
    let marker: LeaseMarker = serde_json::from_str(&value?.to_string_lossy()).ok()?;
    if marker.schema != SCHEMA
        || !same_directory(&marker.account, account)
        || !marker.job.starts_with(JOB_NAME_PREFIX)
    {
        return None;
    }
    if !live_process(marker.holder) || !in_job(&marker.job) {
        return None;
    }
    Some(marker)
}

/// Open the admitted Job with a query-only handle and ask the kernel whether
/// this process is a member. The handle is transient on purpose: holding it
/// would keep the kill-on-close object alive after its owner exits.
fn in_job(name: &str) -> bool {
    use std::os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    };
    let object: Vec<u16> = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let raw = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, object.as_ptr()) };
    if raw.is_null() {
        return false;
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut member = 0;
    let queried =
        unsafe { IsProcessInJob(GetCurrentProcess(), handle.as_raw_handle(), &mut member) };
    queried != 0 && member != 0
}

fn same_directory(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn write_holder(path: &Path, identity: ProcessIdentity, label: &str, job: &str) -> io::Result<()> {
    let record = HolderRecord {
        schema: SCHEMA,
        pid: identity.pid,
        creation_time: identity.creation_time,
        started_unix_ms: unix_millis(),
        command: label.to_owned(),
        job: job.to_owned(),
    };
    let file_name = path
        .file_name()
        .ok_or_else(|| invalid("holder record path has no file name".to_owned()))?;
    let staging = path.with_file_name(format!("{}.tmp", file_name.to_string_lossy()));
    ordinary_ancestors(&staging)?;
    fs::write(&staging, serde_json::to_vec(&record)?)?;
    fs::rename(&staging, path)
}

fn holder_record_path(account: &Path, slot: Option<u32>) -> PathBuf {
    match slot {
        Some(index) => account.join(format!("holder.slot-{index}.json")),
        None => account.join(HOLDER_EXCLUSIVE_FILE),
    }
}

/// Live holder descriptions only. A dead or unreadable record is not a current
/// holder. `holder.json` is still read so a legacy exclusive holder can be named.
fn live_holder_descriptions(account: &Path, slot_count: u32) -> Vec<String> {
    let mut paths = vec![
        account.join(HOLDER_FILE),
        account.join(HOLDER_EXCLUSIVE_FILE),
    ];
    for index in 0..slot_count {
        paths.push(holder_record_path(account, Some(index)));
    }
    let mut descriptions = Vec::new();
    for path in paths {
        let Some(description) = describe_live_holder(&path) else {
            continue;
        };
        if !descriptions.contains(&description) {
            descriptions.push(description);
        }
    }
    descriptions
}

fn describe_live_holder(path: &Path) -> Option<String> {
    let Ok(Some(bytes)) = read_bounded(path) else {
        return None;
    };
    let Ok(record) = serde_json::from_slice::<HolderRecord>(&bytes) else {
        return None;
    };
    let identity = ProcessIdentity {
        pid: record.pid,
        creation_time: record.creation_time,
    };
    if record.schema != SCHEMA || !live_process(identity) {
        return None;
    }
    Some(format!("pid={} command={}", record.pid, record.command))
}

/// One bounded queue line. Busy and total slots are always present. When no
/// current holder record exists, the line says the legacy lock is held instead
/// of inventing a holder.
fn queue_diagnostic(account: &Path, slot_count: u32) -> String {
    let holders = live_holder_descriptions(account, slot_count);
    let detail = if holders.is_empty() {
        "the legacy lock is held".to_owned()
    } else {
        format!("holders {}", holders.join("; "))
    };
    bound_line(format!(
        "heavy: waiting for a free heavy-command slot; {}/{slot_count} busy; {detail}",
        holders.len()
    ))
}

fn bound_line(line: String) -> String {
    let line = line.replace(['\r', '\n'], " ");
    if line.chars().count() > MAX_DIAGNOSTIC {
        line.chars().take(MAX_DIAGNOSTIC).collect::<String>() + "..."
    } else {
        line
    }
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
///
/// The payload is created while it is still suspended, in the shared account
/// CPU budget (outermost, when established), the account aggregate Job, and this
/// operation's lifecycle Job (innermost). A held payload whose aggregate
/// membership is false or unreadable fails as [`RunError::Start`] before any
/// payload code runs. A nested call does not acquire another aggregate limit.
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
    let (job, marker) = admission
        .owned_job(budget, account)
        .map_err(RunError::Start)?;
    command.env.insert(OsString::from(LEASE_ENV), Some(marker));
    eprintln!("{}", admission.job_line(&job).map_err(RunError::Start)?);
    eprintln!("{}", admission.cpu_report(budget));
    let suspended = admission
        .spawn_in_envelope(&job, &command)
        .map_err(RunError::Start)?;
    match admission.budget() {
        Some(shared)
            if shared
                .contains(suspended.process())
                .map_err(RunError::Start)? =>
        {
            eprintln!(
                "heavy: payload pid={} is a kernel-verified member of the shared account CPU budget job=\"{}\"",
                suspended.process().identity().pid,
                shared.name()
            );
        }
        Some(shared) => eprintln!(
            "heavy: warning: payload pid={} is outside the shared account CPU budget job=\"{}\"; this command runs without the shared CPU ceiling",
            suspended.process().identity().pid,
            shared.name()
        ),
        None => {}
    }
    let child = suspended.resume().map_err(RunError::Start)?;
    eprintln!(
        "heavy: started pid={} memory_limit_bytes={} cpu_percent={} deadline_seconds={}",
        child.identity().pid,
        budget.memory_bytes,
        match budget.cpu_percent {
            Some(percent) => percent_text(percent),
            None => format!(
                "shared({})",
                percent_text(f64::from(admission.cpu().parent_rate()) / 100.0)
            ),
        },
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
        assert_eq!(
            budget.cpu_percent, None,
            "the installed default is no per-operation CPU limit"
        );
        assert_eq!(budget.deadline_seconds, 1800);
        assert_eq!(budget.queue_wait_seconds, 3600);
        assert_eq!(budget.max_concurrent_trees, 2);
        assert_eq!(budget.aggregate_memory_limit_bytes, budget.memory_bytes);
        budget.validate("default").unwrap();
        for invalid in [
            Budget {
                memory_bytes: 0,
                ..budget
            },
            Budget {
                max_concurrent_trees: 0,
                ..budget
            },
            Budget {
                memory_bytes: 32 * MIB,
                aggregate_memory_limit_bytes: 16 * MIB,
                ..budget
            },
            Budget {
                cpu_percent: Some(0.0),
                ..budget
            },
            Budget {
                cpu_percent: Some(f64::NAN),
                ..budget
            },
            Budget {
                cpu_percent: Some(100.5),
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
        assert!(
            Budget {
                cpu_percent: Some(100.0),
                max_concurrent_trees: 1,
                aggregate_memory_limit_bytes: budget.memory_bytes + MIB,
                ..budget
            }
            .validate("fixture")
            .is_ok()
        );
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
            cpu_percent: Some(25.0),
            ..Budget::default()
        };
        assert_eq!(
            Budget::write(&account, &adjusted).unwrap(),
            policy_path(&account)
        );
        assert_eq!(Budget::read(&account).unwrap(), adjusted);
        // The installed default is representable and round-trips: an absent
        // field and an explicit reset both mean "the shared ceiling governs".
        fs::write(policy_path(&account), b"{\"schema\":1}").unwrap();
        assert_eq!(Budget::read(&account).unwrap(), Budget::default());
        fs::write(
            policy_path(&account),
            b"{\"schema\":1,\"cpu_percent\":null}",
        )
        .unwrap();
        assert_eq!(Budget::read(&account).unwrap().cpu_percent, None);
        // A legacy file recording the retired default stays an effective limit.
        fs::write(
            policy_path(&account),
            b"{\"schema\":1,\"cpu_percent\":50.0}",
        )
        .unwrap();
        let legacy = Budget::read(&account).unwrap();
        assert_eq!(legacy.cpu_percent, Some(50.0));
        assert!(legacy_default_cpu_percent(&legacy));
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
    fn lease_marker_needs_the_same_account_a_live_holder_and_job_membership() {
        let temp = tempfile::tempdir().unwrap();
        let account = temp.path().join("account");
        prepare(&account).unwrap();
        let live = current_identity().unwrap();
        let marker = LeaseMarker {
            schema: SCHEMA,
            account: account.clone(),
            job: format!("{JOB_NAME_PREFIX}not-an-admitted-job"),
            holder: live,
        };
        let value = serde_json::to_string(&marker).unwrap();
        // A live holder identity and an existing account are not containment:
        // this process is in no such Job, so the marker is refused.
        assert!(inherited(Some(OsStr::new(&value)), &account).is_none());
        // A marker that names an unrelated Job this process may well be inside
        // is refused as well: only this owner's admitted-tree names are honored.
        let unrelated = LeaseMarker {
            schema: SCHEMA,
            account: account.clone(),
            job: "SomeUnrelatedJob".into(),
            holder: live,
        };
        let unrelated = serde_json::to_string(&unrelated).unwrap();
        assert!(inherited(Some(OsStr::new(&unrelated)), &account).is_none());
        let elsewhere = temp.path().join("elsewhere");
        prepare(&elsewhere).unwrap();
        assert!(inherited(Some(OsStr::new(&value)), &elsewhere).is_none());
        let dead = LeaseMarker {
            holder: ProcessIdentity {
                pid: live.pid,
                creation_time: live.creation_time.wrapping_add(1),
            },
            ..marker
        };
        let value = serde_json::to_string(&dead).unwrap();
        assert!(inherited(Some(OsStr::new(&value)), &account).is_none());
        assert!(inherited(None, &account).is_none());
        assert!(inherited(Some(OsStr::new("not json")), &account).is_none());
    }

    #[test]
    fn translated_limits_keep_host_relative_meaning_and_round_down() {
        // 25% of host CPU below a 75% parent: the inner Job may use a third of
        // its parent, and the effective host ceiling never exceeds the request.
        assert_eq!(translate_ceiling(25.0, 7500), Some(3333));
        assert!(
            effective_percent(3333, 7500) <= 25.0,
            "rounding must stay conservative"
        );
        // The retired default becomes a host-relative 50%, not a second 50% of
        // the shared ceiling (which would leave 37.5%).
        assert_eq!(translate_ceiling(50.0, 7500), Some(6666));
        assert!(effective_percent(6666, 7500) <= 50.0);
        // Without a rate-controlled ancestor an inner rate is a host rate.
        assert_eq!(translate_ceiling(25.0, HOST_RATE), Some(2500));
        // A ceiling that is not lower than the parent's adds no inner rate at
        // all, so nothing can multiply.
        assert_eq!(translate_ceiling(75.0, 7500), None);
        assert_eq!(translate_ceiling(90.0, 7500), None);
        // The percent handed to the Job owner floors back to the exact rate.
        for rate in [1, 100, 2500, 3333, 6666, 9999] {
            assert_eq!((percent_of_rate(rate) * 100.0).floor() as u32, rate);
        }
    }

    #[test]
    fn policy_summary_names_the_shared_ceiling_and_the_legacy_value() {
        let summary = cpu_policy_summary(&Budget::default());
        assert!(summary.contains("no per-operation CPU limit"), "{summary}");
        assert!(summary.contains("shared account 75% ceiling"), "{summary}");
        let limited = cpu_policy_summary(&Budget {
            cpu_percent: Some(25.0),
            ..Budget::default()
        });
        assert!(limited.contains("25% of host CPU"), "{limited}");
        assert!(!limited.contains("retired"), "{limited}");
        let legacy = Budget {
            cpu_percent: Some(LEGACY_DEFAULT_CPU_PERCENT),
            ..Budget::default()
        };
        assert!(legacy_default_cpu_percent(&legacy));
        let summary = cpu_policy_summary(&legacy);
        assert!(
            summary.contains("retired 50% batch default"),
            "the ambiguous legacy value must be reported, not silently changed: {summary}"
        );
        assert!(!legacy_default_cpu_percent(&Budget::default()));
    }

    #[test]
    fn shared_cpu_policy_record_selects_the_ceiling_without_a_silent_substitute() {
        let temp = tempfile::tempdir().unwrap();
        let account = temp.path();
        let absent = shared_cpu_policy_ceiling(account).unwrap();
        assert_eq!(absent.percent, SHARED_CPU_PERCENT);
        assert_eq!(absent.requested, "75% of host CPU");

        let path = account.join(SHARED_CPU_POLICY_FILE);
        fs::write(&path, br#"{"schema":1,"ceiling_percent":40.0}"#).unwrap();
        let edited = shared_cpu_policy_ceiling(account).unwrap();
        assert_eq!(edited.percent, 40.0);
        assert_eq!(edited.requested, "40% of host CPU");
        assert_eq!(
            fs::read(&path).unwrap(),
            br#"{"schema":1,"ceiling_percent":40.0}"#
        );

        fs::write(&path, b"{not json").unwrap();
        let fault = shared_cpu_policy_ceiling(account).unwrap_err();
        assert!(
            fault.requested.contains("not a substitute ceiling"),
            "{fault:?}"
        );
        assert!(fault.cause.contains("not JSON"), "{fault:?}");
        assert_eq!(fault.stage, "ceiling configuration");
        assert!(fault.recovery.contains("repair"), "{fault:?}");
        assert_eq!(fs::read(&path).unwrap(), b"{not json");

        fs::write(&path, br#"{"schema":2,"ceiling_percent":40.0}"#).unwrap();
        let fault = shared_cpu_policy_ceiling(account).unwrap_err();
        assert!(fault.cause.contains("schema is not 1"), "{fault:?}");
        assert_eq!(
            fs::read(&path).unwrap(),
            br#"{"schema":2,"ceiling_percent":40.0}"#
        );
    }

    #[test]
    fn named_job_names_are_created_once_or_refused() {
        let name = format!("{JOB_NAME_PREFIX}fixture-{}", std::process::id());
        let job = Job::new_named(Limits::default(), &name).unwrap();
        let refused = Job::new_named(Limits::default(), &name).unwrap_err();
        assert!(refused.to_string().contains("already in use"), "{refused}");
        let long = "x".repeat(129);
        for invalid in ["", "local\\name", long.as_str()] {
            assert!(
                Job::new_named(Limits::default(), invalid).is_err(),
                "{invalid:?}"
            );
        }
        drop(job);
        // The name is released with the last handle, so the owner can recreate it.
        let reused = Job::new_named(Limits::default(), &name).unwrap();
        drop(reused);
    }

    #[test]
    fn labels_stay_bounded_single_line_descriptions() {
        let args = vec![OsString::from("--release"), OsString::from("x".repeat(900))];
        let text = label(OsStr::new("cargo"), &args);
        assert!(text.starts_with("cargo --release "), "{text}");
        assert!(text.len() <= MAX_LABEL + 8, "{}", text.len());
        assert!(!text.contains('\n'));
    }

    fn owned_account() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let account = temp.path().join("account");
        prepare(&account).unwrap();
        (temp, account)
    }

    fn test_budget(slots: u32, queue_wait_seconds: u64) -> Budget {
        Budget {
            memory_bytes: 64 * MIB,
            aggregate_memory_limit_bytes: 64 * MIB,
            max_concurrent_trees: slots,
            queue_wait_seconds,
            deadline_seconds: 30,
            cpu_percent: None,
        }
    }

    struct RestoreEnv {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl RestoreEnv {
        fn set(key: &'static str, value: &std::ffi::OsStr) -> Self {
            let previous = std::env::var_os(key);
            // Tests run with one thread. The installed Windows contract permits
            // this process-global mutation; Drop restores the previous value.
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            // Same single-thread contract as `set`.
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn expect_admission_err(result: io::Result<Admission>) -> io::Error {
        match result {
            Err(error) => error,
            Ok(_) => panic!("admission succeeded; expected an error"),
        }
    }

    #[test]
    fn legacy_policy_keeps_new_defaults_without_rewriting_bytes() {
        let (_temp, account) = owned_account();
        let path = policy_path(&account);
        let bytes = br#"{"schema":1,"memory_bytes":16777216}"#;
        fs::write(&path, bytes).unwrap();
        let budget = Budget::read(&account).unwrap();
        assert_eq!(budget.memory_bytes, 16 * MIB);
        assert_eq!(budget.max_concurrent_trees, 2);
        assert_eq!(budget.aggregate_memory_limit_bytes, budget.memory_bytes);
        let query = Budget::query(&account).unwrap();
        assert_eq!(query.budget, budget);
        assert_eq!(query.memory_bytes, PolicyFieldSource::PolicyFile);
        assert_eq!(query.max_concurrent_trees, PolicyFieldSource::Default);
        assert_eq!(
            query.aggregate_memory_limit_bytes,
            PolicyFieldSource::Default
        );
        assert_eq!(query.aggregate_memory_limit_bytes.as_str(), "default");
        assert_eq!(fs::read(&path).unwrap(), bytes);

        let explicit = Budget {
            memory_bytes: 16 * MIB,
            aggregate_memory_limit_bytes: 32 * MIB,
            max_concurrent_trees: 1,
            ..Budget::default()
        };
        Budget::write(&account, &explicit).unwrap();
        assert_eq!(Budget::read(&account).unwrap(), explicit);
        let written = Budget::query(&account).unwrap();
        assert_eq!(written.memory_bytes, PolicyFieldSource::PolicyFile);
        assert_eq!(written.max_concurrent_trees, PolicyFieldSource::PolicyFile);
        assert_eq!(
            written.aggregate_memory_limit_bytes,
            PolicyFieldSource::PolicyFile
        );

        let mixed_bytes = br#"{"schema":1,"memory_bytes":16777216,"max_concurrent_trees":1}"#;
        fs::write(&path, mixed_bytes).unwrap();
        let mixed = Budget::query(&account).unwrap();
        assert_eq!(mixed.budget.memory_bytes, 16 * MIB);
        assert_eq!(mixed.budget.max_concurrent_trees, 1);
        assert_eq!(mixed.budget.aggregate_memory_limit_bytes, 16 * MIB);
        assert_eq!(mixed.memory_bytes, PolicyFieldSource::PolicyFile);
        assert_eq!(mixed.max_concurrent_trees, PolicyFieldSource::PolicyFile);
        assert_eq!(
            mixed.aggregate_memory_limit_bytes,
            PolicyFieldSource::Default
        );
        assert_eq!(fs::read(&path).unwrap(), mixed_bytes);

        let rejected =
            br#"{"schema":1,"memory_bytes":33554432,"aggregate_memory_limit_bytes":16777216}"#;
        fs::write(&path, rejected).unwrap();
        let error = Budget::read(&account).unwrap_err().to_string();
        assert!(error.contains("refusing before admission"), "{error}");
        let query_error = Budget::query(&account).unwrap_err().to_string();
        assert!(
            query_error.contains("refusing before admission"),
            "{query_error}"
        );
        assert_eq!(fs::read(&path).unwrap(), rejected);

        let missing = tempfile::tempdir().unwrap();
        let absent = missing.path().join("absent");
        let defaults = Budget::query(&absent).unwrap();
        assert_eq!(defaults, BudgetQuery::installed_defaults());
        assert!(!absent.exists(), "a query must not create an account");
    }

    #[test]
    fn invalid_policy_is_refused_before_any_admission_file() {
        let temp = tempfile::tempdir().unwrap();
        let account = temp.path().join("missing");
        for budget in [
            Budget {
                max_concurrent_trees: 0,
                ..Budget::default()
            },
            Budget {
                memory_bytes: 32 * MIB,
                aggregate_memory_limit_bytes: 16 * MIB,
                ..Budget::default()
            },
        ] {
            let error = Admission::acquire(&account, &budget, "invalid", &Cancellation::default())
                .map_or_else(|error| error, |_| panic!("invalid budget was admitted"));
            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{error}");
            assert!(!account.exists(), "{error}");
        }
    }

    #[test]
    fn slot_admission_preserves_timeout_and_cancellation_kinds() {
        let (_temp, account) = owned_account();
        let budget = test_budget(1, 1);
        let held =
            Admission::acquire(&account, &budget, "holder", &Cancellation::default()).unwrap();
        assert!(matches!(&held, Admission::Held(holder) if holder.slot_index.is_none()));
        assert!(account.join("heavy-command.lock").is_file());
        assert!(!account.join("heavy-command.slot-0.lock").exists());

        let started = Instant::now();
        let timed_out = expect_admission_err(Admission::acquire(
            &account,
            &budget,
            "waiter",
            &Cancellation::default(),
        ));
        assert_eq!(timed_out.kind(), io::ErrorKind::TimedOut, "{timed_out}");
        assert!(
            timed_out.to_string().contains("resource busy"),
            "{timed_out}"
        );
        assert!(started.elapsed() < Duration::from_secs(4), "{timed_out}");
        assert!(account.join(HOLDER_EXCLUSIVE_FILE).is_file());

        let cancel = Cancellation::default();
        cancel.cancel();
        let interrupted =
            expect_admission_err(Admission::acquire(&account, &budget, "cancelled", &cancel));
        assert_eq!(
            interrupted.kind(),
            io::ErrorKind::Interrupted,
            "{interrupted}"
        );
        drop(held);
        assert!(!account.join(HOLDER_EXCLUSIVE_FILE).exists());
    }

    #[test]
    fn per_slot_records_release_only_their_own_holder() {
        let (_temp, account) = owned_account();
        let budget = test_budget(2, 1);
        let first =
            Admission::acquire(&account, &budget, "first", &Cancellation::default()).unwrap();
        let second =
            Admission::acquire(&account, &budget, "second", &Cancellation::default()).unwrap();
        let (released, kept) = {
            let (Admission::Held(first_holder), Admission::Held(second_holder)) = (&first, &second)
            else {
                panic!("both direct admissions must hold a slot");
            };
            assert_ne!(first_holder.slot_index, second_holder.slot_index);
            assert_eq!(
                first_holder.aggregate.name(),
                second_holder.aggregate.name()
            );
            let aggregate = first_holder.aggregate.snapshot().unwrap();
            assert_eq!(
                aggregate.job_memory_limit_bytes,
                budget.aggregate_memory_limit_bytes
            );
            assert!(first_holder.record.is_file());
            assert!(second_holder.record.is_file());
            assert_ne!(first_holder.record, second_holder.record);
            assert!(!account.join(HOLDER_FILE).exists());
            (first_holder.record.clone(), second_holder.record.clone())
        };
        drop(first);
        assert!(
            !released.exists(),
            "dropped admission left its holder record"
        );
        assert!(
            kept.is_file(),
            "dropping one admission removed the other record"
        );
        let third =
            Admission::acquire(&account, &budget, "third", &Cancellation::default()).unwrap();
        assert!(matches!(third, Admission::Held(_)));
    }

    #[test]
    fn queue_diagnostic_names_holders_or_the_legacy_lock() {
        let (_temp, account) = owned_account();
        let empty = queue_diagnostic(&account, 2);
        assert_eq!(
            empty,
            "heavy: waiting for a free heavy-command slot; 0/2 busy; the legacy lock is held"
        );
        assert!(!empty.contains('\n'));

        let identity = current_identity().unwrap();
        let record = serde_json::json!({
            "schema": SCHEMA,
            "pid": identity.pid,
            "creation_time": identity.creation_time,
            "started_unix_ms": 1,
            "command": "cargo test",
            "job": format!("{JOB_NAME_PREFIX}fixture"),
        });
        fs::write(
            account.join("holder.slot-0.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        let busy = queue_diagnostic(&account, 2);
        assert!(busy.contains("1/2 busy"), "{busy}");
        assert!(busy.contains("holders pid="), "{busy}");
        assert!(busy.contains("command=cargo test"), "{busy}");
        assert!(!busy.contains("legacy lock"), "{busy}");
        assert!(!busy.contains('\n'));

        let long = "x".repeat(600);
        let record = serde_json::json!({
            "schema": SCHEMA,
            "pid": identity.pid,
            "creation_time": identity.creation_time,
            "started_unix_ms": 1,
            "command": long,
            "job": format!("{JOB_NAME_PREFIX}fixture"),
        });
        fs::write(
            account.join("holder.slot-1.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        let bounded = queue_diagnostic(&account, 2);
        assert!(
            bounded.chars().count() <= MAX_DIAGNOSTIC + 3,
            "{}",
            bounded.chars().count()
        );
        assert!(!bounded.contains('\n'));
    }

    #[test]
    fn slot_waiter_does_not_hold_the_cpu_or_aggregate_lock() {
        let (temp, account) = owned_account();
        let cpu_account = temp.path().join("cpu");
        fs::create_dir_all(&cpu_account).unwrap();
        let _cpu_env = RestoreEnv::set(
            crate::process::CPU_BUDGET_ACCOUNT_ENV,
            cpu_account.as_os_str(),
        );
        let budget = test_budget(1, 1);
        let held =
            Admission::acquire(&account, &budget, "holder", &Cancellation::default()).unwrap();
        let cpu_lock = crate::process::ExclusiveFileLock::acquire(
            &cpu_account.join("cpu-budget.lock"),
            Deadline::after(Duration::from_secs(2)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
        let aggregate_lock = crate::process::ExclusiveFileLock::acquire(
            &account.join("heavy-aggregate.lock"),
            Deadline::after(Duration::from_secs(2)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
        let started = Instant::now();
        let waiter = std::thread::spawn(move || {
            Admission::acquire(&account, &budget, "waiter", &Cancellation::default())
        });
        let error = expect_admission_err(waiter.join().unwrap());
        assert_eq!(error.kind(), io::ErrorKind::TimedOut, "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "a slot waiter blocked on a CPU or aggregate lock: {error} after {:?}",
            started.elapsed()
        );
        drop(cpu_lock);
        drop(aggregate_lock);
        drop(held);
    }

    #[test]
    fn run_diagnostics_include_aggregate_and_per_tree_readback() {
        use windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_JOB_MEMORY;

        let (_temp, account) = owned_account();
        let budget = test_budget(1, 30);
        let admission =
            Admission::acquire(&account, &budget, "readback", &Cancellation::default()).unwrap();
        let (job, _) = admission.owned_job(&budget, &account).unwrap();
        let line = admission.job_line(&job).unwrap();
        assert!(line.starts_with("heavy: job scope=aggregate "), "{line}");
        assert!(
            line.contains(&format!("memory_limit_bytes={}", budget.memory_bytes)),
            "{line}"
        );
        assert!(line.contains("kill_on_close=true"), "{line}");
        assert!(
            line.contains(&format!(
                "aggregate_memory_limit_bytes={}",
                budget.aggregate_memory_limit_bytes
            )),
            "{line}"
        );
        assert!(
            line.contains("aggregate_process_memory_limit_bytes=0"),
            "{line}"
        );
        assert!(line.contains("aggregate_cpu_rate=0"), "{line}");
        assert!(line.contains("aggregate_kill_on_close=false"), "{line}");
        assert!(
            line.contains(&format!(
                "aggregate_limit_flags={JOB_OBJECT_LIMIT_JOB_MEMORY}"
            )),
            "{line}"
        );
        assert!(!line.contains('\n'), "{line}");
        let Admission::Held(holder) = &admission else {
            panic!("readback admission must hold the slot");
        };
        let aggregate = holder.aggregate.snapshot().unwrap();
        assert_eq!(aggregate.limit_flags, JOB_OBJECT_LIMIT_JOB_MEMORY);
        assert_eq!(aggregate.process_memory_limit_bytes, 0);
        assert_eq!(aggregate.cpu_rate, 0);
        assert!(!aggregate.kill_on_close);
    }

    #[test]
    fn direct_run_exits_with_the_payload_code_inside_both_jobs() {
        use crate::process::StopReason;
        use windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_JOB_MEMORY;

        let (_temp, account) = owned_account();
        let budget = Budget {
            memory_bytes: 4 * 1024 * MIB,
            aggregate_memory_limit_bytes: 4 * 1024 * MIB,
            deadline_seconds: 60,
            ..test_budget(1, 30)
        };
        let admission =
            Admission::acquire(&account, &budget, "cmd", &Cancellation::default()).unwrap();
        let program = std::env::current_exe().unwrap();
        let run = execute(
            &budget,
            &account,
            &program,
            &[
                OsString::from("--exact"),
                OsString::from("heavy_command::tests::direct_exit_probe"),
                OsString::from("--ignored"),
                OsString::from("--nocapture"),
            ],
            &admission,
            &Cancellation::default(),
        )
        .unwrap_or_else(|error| panic!("direct run failed: {error}"));
        assert_eq!(run.outcome.reason, StopReason::Exited);
        assert_eq!(run.outcome.exit_code, 3, "exit codes must pass through");
        assert_eq!(run.outcome.job.memory_limit_bytes, budget.memory_bytes);
        assert!(run.outcome.job.kill_on_close);
        let Admission::Held(holder) = &admission else {
            panic!("direct run must hold a slot");
        };
        let aggregate = holder.aggregate.snapshot().unwrap();
        assert_eq!(
            aggregate.job_memory_limit_bytes,
            budget.aggregate_memory_limit_bytes
        );
        assert_eq!(aggregate.limit_flags, JOB_OBJECT_LIMIT_JOB_MEMORY);
        assert_eq!(aggregate.process_memory_limit_bytes, 0);
        assert_eq!(aggregate.cpu_rate, 0);
        assert!(!aggregate.kill_on_close);
    }

    #[test]
    fn nested_run_inherits_the_outer_admission() {
        let (temp, account) = owned_account();
        let result = temp.path().join("nested-result");
        let budget = Budget {
            memory_bytes: 4 * 1024 * MIB,
            aggregate_memory_limit_bytes: 4 * 1024 * MIB,
            max_concurrent_trees: 1,
            queue_wait_seconds: 5,
            deadline_seconds: 60,
            cpu_percent: None,
        };
        Budget::write(&account, &budget).unwrap();
        let _account_env = RestoreEnv::set("HARNESS_HEAVY_PROBE_ACCOUNT", account.as_os_str());
        let _result_env = RestoreEnv::set("HARNESS_HEAVY_PROBE_RESULT", result.as_os_str());
        let admission =
            Admission::acquire(&account, &budget, "outer", &Cancellation::default()).unwrap();
        let program = std::env::current_exe().unwrap();
        let run = execute(
            &budget,
            &account,
            &program,
            &[
                OsString::from("--exact"),
                OsString::from("heavy_command::tests::nested_heavy_admission_probe"),
                OsString::from("--ignored"),
                OsString::from("--nocapture"),
            ],
            &admission,
            &Cancellation::default(),
        )
        .unwrap_or_else(|error| panic!("nested run failed to start: {error}"));
        let recorded = fs::read_to_string(&result).unwrap_or_else(|_| "<missing>".to_owned());
        assert_eq!(run.outcome.exit_code, 0, "{recorded}");
        assert_eq!(recorded, "inherited");
        // The outer slot is still held, so a nested call that took a second slot
        // would still be waiting. A new direct caller must now be the one that waits.
        let peer_budget = Budget {
            queue_wait_seconds: 1,
            ..budget
        };
        let peer = expect_admission_err(Admission::acquire(
            &account,
            &peer_budget,
            "peer",
            &Cancellation::default(),
        ));
        assert_eq!(peer.kind(), io::ErrorKind::TimedOut, "{peer}");
    }

    #[test]
    #[ignore = "child of nested_run_inherits_the_outer_admission"]
    fn nested_heavy_admission_probe() {
        let account = PathBuf::from(std::env::var_os("HARNESS_HEAVY_PROBE_ACCOUNT").unwrap());
        let result = PathBuf::from(std::env::var_os("HARNESS_HEAVY_PROBE_RESULT").unwrap());
        let budget = Budget::read(&account).unwrap();
        let admission =
            Admission::acquire(&account, &budget, "nested-probe", &Cancellation::default())
                .unwrap();
        let kind = match &admission {
            Admission::Inherited(_) => "inherited",
            Admission::Held(_) => "held",
        };
        fs::write(&result, kind).unwrap();
        assert_eq!(kind, "inherited");
        assert_eq!(admission.scope(), "containment");
        assert!(admission.limits(&budget).memory_bytes.is_none());
        assert!(admission.limits(&budget).cpu_percent.is_none());
    }

    #[test]
    #[ignore = "child of direct_run_exits_with_the_payload_code_inside_both_jobs"]
    fn direct_exit_probe() {
        std::process::exit(3);
    }

    #[test]
    fn held_spawn_outside_the_aggregate_is_refused_before_resume() {
        let (temp, account) = owned_account();
        let other = temp.path().join("other-account");
        prepare(&other).unwrap();
        let budget = Budget {
            memory_bytes: 64 * MIB,
            aggregate_memory_limit_bytes: 64 * MIB,
            deadline_seconds: 30,
            queue_wait_seconds: 30,
            ..test_budget(1, 30)
        };
        let admission =
            Admission::acquire(&account, &budget, "held", &Cancellation::default()).unwrap();
        let Admission::Held(holder) = &admission else {
            panic!("the refusal fixture must hold a slot");
        };
        let outside =
            crate::process::HeavyAggregate::acquire(&other, budget.aggregate_memory_limit_bytes)
                .unwrap();
        let lifecycle = Job::new(Limits {
            memory_bytes: Some(64 * MIB),
            cpu_percent: None,
        })
        .unwrap();
        let program = PathBuf::from(
            std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows")),
        )
        .join("System32")
        .join("cmd.exe");
        let suspended = outside
            .spawn_suspended(None, &lifecycle, &CommandSpec::new(program))
            .unwrap_or_else(|error| {
                panic!("could not create a payload outside the admission aggregate: {error}")
            });
        let error = aggregate_membership_gate(
            holder.aggregate.contains(suspended.process()),
            suspended.process().identity().pid,
            holder.aggregate.name(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("outside the account aggregate"),
            "{error}"
        );
        assert!(error.to_string().contains("refusing to start"), "{error}");
        let start = RunError::Start(io::Error::other(error.to_string()));
        assert!(
            start.to_string().contains("could not be started"),
            "{start}"
        );
        assert!(suspended.process().is_running().unwrap());
        drop(suspended);

        let unread = aggregate_membership_gate(
            Err(io::Error::other("snapshot failed")),
            7,
            holder.aggregate.name(),
        )
        .unwrap_err();
        assert!(unread.to_string().contains("could not be read"), "{unread}");
        assert!(unread.to_string().contains("refusing to start"), "{unread}");
        let start = RunError::Start(unread);
        assert!(
            start.to_string().contains("could not be started"),
            "{start}"
        );
    }
}
