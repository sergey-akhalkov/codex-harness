//! Continuous supervision for one improvement run.
//!
//! A new `start` persists continuous supervision unless `--supervision once`
//! is explicit. A run with no supervision file keeps the single `advance`
//! used by legacy recovery; that frozen input is not rewritten. Continuous
//! mode stays available across idle, blocked and successor work until stop
//! or ownership loss. It waits natively, prepares a missing runtime through
//! `native_build::prepare`, consumes an exact supported decision, and starts
//! one independently specified successor only after verifying retained
//! lineage. It does not publish a live installation, reset
//! `decision-recorded` / `activation-confirmed`, or start model work to stay
//! busy.

use super::{Run, advance_run, attempt_evidence, invalid, reconcile, terminal_outcome};
use harness_core::build_identity;
use harness_core::improvement_activation::{
    self, ActivationOutcome, ActivationRequest, CheckSpec, IntegrationOutcome, IntegrationReceipt,
    IntegrationRequest,
};
use harness_core::improvement_experiment::ExperimentBindings;
use harness_core::improvement_loop::{
    AttemptState, ComparisonArm, EffectKind, MAX_RUN_SPEC_BYTES, OwnerRecord, Phase, RunStore,
    current_process_identity, owner_is_live, read_json, write_json_atomic,
};
use harness_core::improvement_policy::{PolicyDecision, PolicyEvaluation};
use harness_core::improvement_runtime::ArmRuntime;
use harness_core::process::ProcessIdentity;
use harness_core::process_service::{self, ServiceProcess};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const POLL: Duration = Duration::from_millis(200);
const SUPERVISION_FILE: &str = "supervision.json";
const PREPARED_BUILDS_FILE: &str = "prepared-builds.json";
const BUILD_JOB_FILE: &str = "build-job.json";
const BUILD_OUTPUT_FILE: &str = "build-output.json";
const BUILD_LOG_FILE: &str = "build-child.log";
const STOP_REQUEST_FILE: &str = "stop-request.json";
const STOP_ACK_FILE: &str = "stop-acknowledgement.json";
const INTEGRATION_CHECK_FILE: &str = "integration-check.json";
const INTEGRATION_FILE: &str = "integration.json";
const ACTIVATION_FILE: &str = "activation.json";
const SUCCESSOR_FILE: &str = "successor.json";
const LINEAGE_FILE: &str = "lineage.json";
const CONTINUATION_FILE: &str = "continuation.json";
const SUCCESSOR_WAIT: Duration = Duration::from_secs(30);
static TOKEN_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Supervision {
    Once,
    Continuous,
}

impl Supervision {
    fn parse(value: &str) -> io::Result<Self> {
        match value {
            "once" => Ok(Self::Once),
            "continuous" => Ok(Self::Continuous),
            _ => Err(invalid(
                "--supervision is once or continuous; continuous keeps driving settled work, once performs a single advance",
            )),
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Continuous => "continuous",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupervisionRecord {
    schema: u32,
    mode: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedBuilds {
    schema: u32,
    #[serde(default)]
    baseline: Option<PathBuf>,
    #[serde(default)]
    candidate: Option<PathBuf>,
    #[serde(default)]
    baseline_identity: Option<String>,
    #[serde(default)]
    candidate_identity: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildJob {
    schema: u32,
    arm: String,
    source: PathBuf,
    state: PathBuf,
    output: PathBuf,
    /// `intent` is written before spawn. Identity is added only after the
    /// child is observed. A resume must not invent either fact.
    #[serde(default)]
    phase: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    program: Option<PathBuf>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrepareOutput {
    schema: u32,
    build: PathBuf,
    reused: bool,
    source_identity: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IntegrationCheckRecord {
    schema: u32,
    program: PathBuf,
    #[serde(default)]
    args: Vec<String>,
    timeout_seconds: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SuccessorRecord {
    schema: u32,
    spec: PathBuf,
    run: PathBuf,
    #[serde(default)]
    integration_check: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContinuationRecord {
    schema: u32,
    /// Legacy flag. Presence of this file is not suppression: `phase`
    /// separates intent, observed launch and completion.
    #[serde(default)]
    successor_started: bool,
    #[serde(default)]
    note: String,
    /// One of `idle`, `intent`, `observed`, `completed`, `stopped`. Absent on
    /// legacy records, which are interpreted from `successor_started`.
    #[serde(default)]
    phase: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    spec: Option<PathBuf>,
    #[serde(default)]
    spec_sha256: Option<String>,
    #[serde(default)]
    run: Option<PathBuf>,
    /// The retained predecessor lineage this successor descends from. The
    /// successor is never started while it is missing.
    #[serde(default)]
    lineage: Option<PathBuf>,
    #[serde(default)]
    lineage_sha256: Option<String>,
    #[serde(default)]
    decision: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    created: Option<u64>,
    #[serde(default)]
    program: Option<PathBuf>,
    #[serde(default)]
    exit_code: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StopRequest {
    schema: u32,
    #[serde(default)]
    token: String,
    reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StopAcknowledgement {
    schema: u32,
    token: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn fresh_token() -> String {
    format!("{}-{}", now_ms(), TOKEN_SEQ.fetch_add(1, Ordering::Relaxed))
}

/// Legacy stop requests written before tokens carry no token; they are still
/// consumed exactly once, under a stable spelling.
fn normalize_stop_token(token: &str) -> String {
    if token.is_empty() {
        "legacy-untokened".to_owned()
    } else {
        token.to_owned()
    }
}

/// The retained build job's lifecycle stage. `intent` is written before the
/// spawn; identity is added only after the child is observed.
fn job_phase(job: &BuildJob) -> &str {
    if !job.phase.is_empty() {
        return &job.phase;
    }
    if job.pid.is_some() {
        "observed-legacy"
    } else {
        "legacy"
    }
}

/// The continuation marker's lifecycle stage, including legacy records.
fn continuation_phase(record: &ContinuationRecord) -> &str {
    if !record.phase.is_empty() {
        return &record.phase;
    }
    if record.successor_started {
        "observed-legacy"
    } else {
        "idle-legacy"
    }
}

fn hash_file(path: &Path) -> io::Result<String> {
    build_identity::hash_file(path)
}

pub(super) fn supervision_of(run: &Run) -> io::Result<Supervision> {
    let path = run.store.root().join(SUPERVISION_FILE);
    if !path.is_file() {
        return Ok(Supervision::Once);
    }
    let record: SupervisionRecord = read_json(&path, 4096)?;
    if record.schema != 1 {
        return Err(invalid(
            "the retained supervision record has an unsupported schema",
        ));
    }
    Supervision::parse(&record.mode)
}

pub(super) fn persist_supervision(run: &Run, mode: Supervision) -> io::Result<()> {
    write_json_atomic(
        &run.store.root().join(SUPERVISION_FILE),
        &SupervisionRecord {
            schema: 1,
            mode: mode.as_str().to_owned(),
        },
    )
}

pub(super) fn parse_supervision(value: Option<&str>) -> io::Result<Supervision> {
    match value {
        // New starts are continuous. Legacy recovery reads a missing file as
        // `once` in `supervision_of` and does not rewrite that frozen input.
        None => Ok(Supervision::Continuous),
        Some(value) => Supervision::parse(value),
    }
}

pub(super) fn persist_integration_check(run: &Run, source: &Path) -> io::Result<()> {
    if !source.is_absolute() || !source.is_file() {
        return Err(invalid(
            "--integration-check must be an absolute JSON file declaring program, args and timeout_seconds",
        ));
    }
    let record: IntegrationCheckRecord = read_json(source, MAX_RUN_SPEC_BYTES)?;
    validate_check(&record)?;
    write_json_atomic(&run.store.root().join(INTEGRATION_CHECK_FILE), &record)
}

pub(super) fn persist_successor(
    run: &Run,
    spec: Option<&str>,
    successor_run: Option<&str>,
    integration_check: Option<&str>,
) -> io::Result<()> {
    match (spec, successor_run) {
        (None, None) => Ok(()),
        (Some(spec), Some(successor_run)) => {
            let spec = PathBuf::from(spec);
            let successor_run = PathBuf::from(successor_run);
            if !spec.is_absolute() || !spec.is_file() || !successor_run.is_absolute() {
                return Err(invalid(
                    "--successor-spec and --successor-run must be absolute, and the spec file must exist",
                ));
            }
            if successor_run == run.store.root() {
                return Err(invalid(
                    "the successor run directory must be distinct from this run",
                ));
            }
            let check = match integration_check {
                Some(path) => {
                    let path = PathBuf::from(path);
                    if !path.is_absolute() || !path.is_file() {
                        return Err(invalid(
                            "the successor integration check must be an absolute file",
                        ));
                    }
                    Some(path)
                }
                None => None,
            };
            write_json_atomic(
                &run.store.root().join(SUCCESSOR_FILE),
                &SuccessorRecord {
                    schema: 1,
                    spec,
                    run: successor_run,
                    integration_check: check,
                },
            )
        }
        _ => Err(invalid(
            "--successor-spec and --successor-run must be supplied together",
        )),
    }
}

/// A second start/resume is refused while the recorded owner still runs.
/// Stop may take over; this guard is only for another controller.
pub(super) fn refuse_live_controller(run: &Run) -> io::Result<()> {
    let Some(owner) = run.store.owner()? else {
        return Ok(());
    };
    let (pid, created, _) = current_process_identity()?;
    if owner.pid == pid && owner.created == created {
        return Ok(());
    }
    if owner_is_live(&owner)? == Some(true) {
        return Err(invalid(format!(
            "run {} is already controlled by live process {} ({}); a second controller is refused - use `improve status` or `improve stop`",
            run.spec.run,
            owner.pid,
            owner.program.display()
        )));
    }
    Ok(())
}

/// Consumes one pending stop request if one is on disk and returns its token.
/// A resume uses this so a stop that was never observed by a live controller
/// suspends the run once instead of lingering as a permanent refusal.
pub(super) fn consume_pending_stop(run: &Run) -> io::Result<Option<String>> {
    acknowledge_exact_stop(run.store.root())
}

pub(super) fn write_stop_request(root: &Path, reason: &str) -> io::Result<()> {
    write_json_atomic(
        &root.join(STOP_REQUEST_FILE),
        &StopRequest {
            schema: 1,
            token: fresh_token(),
            reason: reason.to_owned(),
        },
    )
}

/// Consumes the stop request currently on disk. A newer request written after
/// this token was read is left in place, so a concurrent stop is not lost.
pub(super) fn acknowledge_exact_stop(root: &Path) -> io::Result<Option<String>> {
    let path = root.join(STOP_REQUEST_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let request: StopRequest = read_json(&path, 4096)?;
    let token = normalize_stop_token(&request.token);
    write_json_atomic(
        &root.join(STOP_ACK_FILE),
        &StopAcknowledgement {
            schema: 1,
            token: token.clone(),
        },
    )?;
    if path.is_file() {
        let current: StopRequest = read_json(&path, 4096)?;
        if normalize_stop_token(&current.token) == token {
            fs::remove_file(&path)?;
        }
    }
    Ok(Some(token))
}

pub(super) fn stop_build_job(root: &Path) -> io::Result<Option<String>> {
    let path = root.join(BUILD_JOB_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let job: BuildJob = read_json(&path, MAX_RUN_SPEC_BYTES)?;
    let (Some(pid), Some(created), Some(program)) = (job.pid, job.created, job.program.clone())
    else {
        return Ok(Some(format!(
            "the {} build has no observed process identity ({}); it was not signalled and is not started again",
            job.arm,
            job_phase(&job)
        )));
    };
    let user = process_service::current_user()?;
    let inspected = ServiceProcess::inspect(
        ProcessIdentity {
            pid,
            creation_time: created,
        },
        &program,
        &user,
    )?;
    let Some(process) = inspected else {
        return Ok(Some(format!(
            "build child {} is not the recorded live process; it was not signalled",
            pid
        )));
    };
    // The recorded program, pid and creation time are the child this run
    // spawned. A mismatch above already refused the signal.
    let stopped = process.terminate(130)?;
    Ok(Some(format!(
        "recorded {} build child {} {}",
        job.arm,
        pid,
        if stopped {
            "was stopped"
        } else {
            "had already exited"
        }
    )))
}

pub(super) fn prepare_runtime(args: &[std::ffi::OsString]) -> io::Result<i32> {
    let options = super::Options::parse(args, &["--source", "--state", "--output"], &[])?;
    let source = PathBuf::from(options.required("--source")?);
    let state = PathBuf::from(options.required("--state")?);
    let output = PathBuf::from(options.required("--output")?);
    if !source.is_absolute() || !state.is_absolute() || !output.is_absolute() {
        return Err(invalid(
            "prepare-runtime --source, --state and --output must be absolute paths",
        ));
    }
    let prepared = harness_core::native_build::prepare(&source, &state, "cargo".as_ref())?;
    write_json_atomic(
        &output,
        &PrepareOutput {
            schema: 1,
            build: prepared.build,
            reused: prepared.reused,
            source_identity: prepared.source_identity,
        },
    )?;
    Ok(0)
}

pub(super) fn drive(run: &mut Run) -> io::Result<Vec<String>> {
    let mut notes = Vec::new();
    if supervision_of(run)? != Supervision::Continuous {
        return Ok(notes);
    }
    notes.push(
        "controller: continuous supervision is driving settled work; status and stop stay available"
            .to_owned(),
    );
    let mut idle_spins = 0u32;
    loop {
        if should_leave(run, &mut notes)? {
            notes.push("controller: stop or ownership change; no further dispatch".to_owned());
            return Ok(notes);
        }
        apply_prepared(run)?;
        if let Some(id) = in_flight_id(&run.cursor) {
            notes.push(format!(
                "controller: waiting for attempt {id}; no new model work is dispatched"
            ));
            wait_for_attempt(run, &id, &mut notes)?;
            if should_leave(run, &mut notes)? {
                notes.push("controller: left the wait without dispatching again".to_owned());
                return Ok(notes);
            }
            continue;
        }
        match run.cursor.phase {
            Phase::Stopped => return Ok(notes),
            Phase::Idle => {
                if continuation_expected(run) {
                    finish_continuation(run, &mut notes)?;
                } else {
                    notes.push(
                        "controller: idle; no model work is started only to stay busy".to_owned(),
                    );
                }
                // A stop requested during the continuation is consumed here,
                // so it does not linger after the controller leaves.
                should_leave(run, &mut notes)?;
                return Ok(notes);
            }
            Phase::ActivationConfirmed => {
                finish_continuation(run, &mut notes)?;
                should_leave(run, &mut notes)?;
                return Ok(notes);
            }
            Phase::Blocked => {
                notes.push(
                    "controller: blocked; the recorded condition must change before another dispatch"
                        .to_owned(),
                );
                return Ok(notes);
            }
            _ => {}
        }
        let before = fingerprint(&run.cursor);
        if run
            .cursor
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate.is_ready())
        {
            if !ensure_missing_builds(run, &mut notes)? {
                should_leave(run, &mut notes)?;
                return Ok(notes);
            }
            apply_prepared(run)?;
        }
        if should_leave(run, &mut notes)? {
            return Ok(notes);
        }
        notes.extend(advance_run(run)?);
        if run.cursor.phase == Phase::DecisionRecorded {
            consume_decision(run, &mut notes)?;
        }
        if matches!(run.cursor.phase, Phase::Idle | Phase::ActivationConfirmed) {
            if continuation_expected(run) {
                finish_continuation(run, &mut notes)?;
            }
            should_leave(run, &mut notes)?;
            return Ok(notes);
        }
        if matches!(run.cursor.phase, Phase::Blocked | Phase::Stopped) {
            return Ok(notes);
        }
        if in_flight_id(&run.cursor).is_some() {
            idle_spins = 0;
            continue;
        }
        if run.cursor.phase == Phase::DecisionRecorded
            && run
                .cursor
                .condition
                .as_deref()
                .is_some_and(|condition| condition.starts_with("waiting for removal authority"))
        {
            idle_spins = 0;
            continue;
        }
        if fingerprint(&run.cursor) == before {
            idle_spins += 1;
            if idle_spins > 1 {
                notes.push(
                    "controller: no further eligible transition from the retained evidence"
                        .to_owned(),
                );
                return Ok(notes);
            }
        } else {
            idle_spins = 0;
        }
    }
}

/// A completed decision or confirmed activation leaves retained lineage; a
/// declared successor is then consumed even after a rejection, while a plain
/// intake idle starts no continuation work.
fn continuation_expected(run: &Run) -> bool {
    run.cursor.phase == Phase::ActivationConfirmed
        || run.store.root().join(LINEAGE_FILE).is_file()
        || run
            .cursor
            .comparison
            .as_ref()
            .and_then(|state| state.decision.as_deref())
            .is_some()
}

/// Consumes the exact stop request when one is present, so a stop is a
/// one-shot command: the controller leaves and a later resume does not treat
/// the stale request as a new stop.
fn should_leave(run: &Run, notes: &mut Vec<String>) -> io::Result<bool> {
    if stop_requested(run.store.root()) {
        if let Some(token) = acknowledge_exact_stop(run.store.root())? {
            notes.push(format!(
                "controller: consumed the exact stop request ({token}); retained effects stay as recorded"
            ));
        }
        return Ok(true);
    }
    if run.cursor.phase == Phase::Stopped {
        return Ok(true);
    }
    if run.store.owner()?.is_none() {
        return Ok(true);
    }
    Ok(!still_owner(run)?)
}

fn still_owner(run: &Run) -> io::Result<bool> {
    let Some(owner) = run.store.owner()? else {
        return Ok(false);
    };
    let (pid, created, _) = current_process_identity()?;
    Ok(owner.pid == pid && owner.created == created)
}

fn stop_requested(root: &Path) -> bool {
    root.join(STOP_REQUEST_FILE).is_file()
}

fn in_flight_id(cursor: &harness_core::improvement_loop::Cursor) -> Option<String> {
    cursor
        .attempts
        .iter()
        .find(|attempt| attempt.state.is_in_flight())
        .map(|attempt| attempt.id.clone())
}

fn fingerprint(cursor: &harness_core::improvement_loop::Cursor) -> String {
    let attempts = cursor
        .attempts
        .iter()
        .map(|attempt| format!("{}:{}", attempt.id, attempt.state.as_str()))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{}|{}|{}|{}",
        cursor.phase.as_str(),
        attempts,
        cursor
            .comparison
            .as_ref()
            .and_then(|state| state.decision.as_deref())
            .unwrap_or(""),
        cursor.condition.as_deref().unwrap_or("")
    )
}

fn wait_for_attempt(run: &mut Run, id: &str, notes: &mut Vec<String>) -> io::Result<()> {
    let attempt = run
        .cursor
        .attempts
        .iter()
        .find(|attempt| attempt.id == id)
        .cloned()
        .ok_or_else(|| invalid(format!("attempt {id} disappeared before its wait")))?;
    run.cursor.condition = Some(format!(
        "waiting for attempt {id}; status and stop remain available and no new model work is dispatched"
    ));
    run.store.save_cursor(&run.cursor)?;
    release(run);
    loop {
        if stop_requested(run.store.root()) {
            reacquire(run)?;
            notes.push(format!(
                "controller: stop requested while attempt {id} was in flight; it is not resubmitted"
            ));
            return Ok(());
        }
        match attempt_evidence(&attempt) {
            super::AttemptEvidence::Verified { state, .. }
                if terminal_outcome(&state).is_some() =>
            {
                break;
            }
            super::AttemptEvidence::Refused(_) => break,
            _ => thread::sleep(POLL),
        }
    }
    reacquire(run)?;
    let (cursor, report, reconcile_notes) = reconcile(run)?;
    run.cursor = cursor;
    run.store.save_cursor(&run.cursor)?;
    notes.extend(reconcile_notes);
    if !report.settled.is_empty() {
        notes.push(format!(
            "controller: settled attempt(s) {} from their receipts; none were replayed",
            report.settled.join(", ")
        ));
    }
    Ok(())
}

fn release(run: &mut Run) {
    run.guard.take();
}

fn reacquire(run: &mut Run) -> io::Result<()> {
    if run.guard.is_some() {
        apply_prepared(run)?;
        return Ok(());
    }
    let root = run.store.root().to_path_buf();
    let (store, guard) = RunStore::open_locked(&root)?;
    run.spec = store.spec()?;
    run.cursor = store.cursor()?;
    run.store = store;
    run.guard = Some(guard);
    apply_prepared(run)
}

fn apply_prepared(run: &mut Run) -> io::Result<()> {
    let path = run.store.root().join(PREPARED_BUILDS_FILE);
    if !path.is_file() {
        return Ok(());
    }
    let prepared: PreparedBuilds = read_json(&path, MAX_RUN_SPEC_BYTES)?;
    let Some(comparison) = run.spec.comparison.as_mut() else {
        return Ok(());
    };
    // The frozen spec file is left unchanged. A missing declared path is
    // filled only in this process from the build the native owner published.
    if let Some(build) = prepared.baseline
        && !comparison.runtimes.baseline_build.is_dir()
    {
        comparison.runtimes.baseline_build = build;
    }
    if let Some(build) = prepared.candidate
        && !comparison.runtimes.candidate_build.is_dir()
    {
        comparison.runtimes.candidate_build = build;
    }
    Ok(())
}

/// Returns false when preparation failed or stop interrupted it. A present
/// declared build is left to the comparison owner, including a stale one.
fn ensure_missing_builds(run: &mut Run, notes: &mut Vec<String>) -> io::Result<bool> {
    let Some(comparison) = run.spec.comparison.clone() else {
        return Ok(true);
    };
    let Some(candidate) = run.cursor.candidate.clone() else {
        return Ok(true);
    };
    if !candidate.is_ready() {
        return Ok(true);
    }
    let candidate_source = candidate
        .worktree
        .as_ref()
        .map(|checkout| checkout.path.clone())
        .ok_or_else(|| invalid("the ready candidate records no checkout to build"))?;
    if !comparison.runtimes.baseline_build.is_dir() {
        let source = materialize_baseline_source(run)?;
        if !publish_missing_build(run, "baseline", &source, &comparison.runtimes.state, notes)? {
            return Ok(false);
        }
    }
    if !comparison.runtimes.candidate_build.is_dir()
        && !publish_missing_build(
            run,
            "candidate",
            &candidate_source,
            &comparison.runtimes.state,
            notes,
        )?
    {
        return Ok(false);
    }
    Ok(true)
}

fn materialize_baseline_source(run: &Run) -> io::Result<PathBuf> {
    let path = run.store.root().join("baseline-source");
    if !path.exists() {
        let status = Command::new("git")
            .current_dir(&run.spec.project)
            .args(["worktree", "add", "--detach"])
            .arg(&path)
            .arg(&run.spec.base_revision)
            .status()?;
        if !status.success() {
            return Err(invalid(format!(
                "the frozen baseline source could not be materialized at {}",
                path.display()
            )));
        }
    }
    let head = Command::new("git")
        .current_dir(&path)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !head.status.success() {
        return Err(invalid(format!(
            "the baseline source at {} is not a Git checkout",
            path.display()
        )));
    }
    let text = String::from_utf8_lossy(&head.stdout);
    if text.trim() != run.spec.base_revision {
        return Err(invalid(format!(
            "the baseline source at {} is at {} instead of {}",
            path.display(),
            text.trim(),
            run.spec.base_revision
        )));
    }
    Ok(path)
}

/// The outcome of reconciling a retained build job before another build.
enum BuildReconciliation {
    /// The recorded child left a readable receipt; the build is completed.
    Prepared {
        arm: String,
        prepared: PrepareOutput,
    },
    /// The recorded child cannot be reconciled to a receipt. The build is not
    /// started again automatically and the reason is retained on the cursor.
    Unknown(String),
    /// A stop was requested; nothing is dispatched again by this controller.
    Stopped,
}

fn publish_missing_build(
    run: &mut Run,
    arm: &str,
    source: &Path,
    state: &Path,
    notes: &mut Vec<String>,
) -> io::Result<bool> {
    if stop_requested(run.store.root()) {
        return Ok(false);
    }
    // A retained build job is reconciled before any other build is started.
    match reconcile_build_job(run, notes)? {
        Some(BuildReconciliation::Prepared {
            arm: recorded_arm,
            prepared,
        }) => {
            if recorded_arm != arm {
                let reason = format!(
                    "the retained build receipt belongs to the {recorded_arm} runtime while the missing {arm} runtime is required; the mismatch is not resolved automatically"
                );
                run.cursor.block(reason.clone());
                run.store.save_cursor(&run.cursor)?;
                notes.push(reason);
                return Ok(false);
            }
            record_prepared_build(run, arm, prepared, notes)?;
            return Ok(true);
        }
        Some(BuildReconciliation::Unknown(reason)) => {
            run.cursor.block(reason.clone());
            run.cursor.effect(
                EffectKind::BuildPrepared,
                format!("arm={arm} reconciled-unknown"),
            );
            run.store.save_cursor(&run.cursor)?;
            notes.push(reason);
            return Ok(false);
        }
        Some(BuildReconciliation::Stopped) => return Ok(false),
        None => {}
    }
    let output = run.store.root().join(BUILD_OUTPUT_FILE);
    let _ = fs::remove_file(&output);
    let log_path = run.store.root().join(BUILD_LOG_FILE);
    let job_path = run.store.root().join(BUILD_JOB_FILE);
    // Intent is written before the spawn; identity is added only after the
    // child is observed, so a resume never invents either fact.
    write_json_atomic(
        &job_path,
        &BuildJob {
            schema: 1,
            arm: arm.to_owned(),
            source: source.to_path_buf(),
            state: state.to_path_buf(),
            output: output.clone(),
            phase: "intent".to_owned(),
            token: fresh_token(),
            pid: None,
            created: Some(now_ms()),
            program: None,
            exit_code: None,
            error: None,
        },
    )?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let exe = std::env::current_exe()?;
    let mut command = Command::new(&exe);
    command
        .arg("improve")
        .arg("prepare-runtime")
        .arg("--source")
        .arg(source)
        .arg("--state")
        .arg(state)
        .arg("--output")
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    for key in rejected_build_overrides() {
        eprintln!("controller: removing ambient build override {key} from the native build child");
        command.env_remove(key);
    }
    let mut child = command.spawn().map_err(|error| {
        let _ = fs::remove_file(&job_path);
        invalid(format!(
            "the native build child for the {arm} runtime could not be started: {error}"
        ))
    })?;
    let user = process_service::current_user()?;
    let observed = match ServiceProcess::observe(child.id(), &exe, 0, &user) {
        Ok(observed) => observed,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&job_path);
            return Err(invalid(format!(
                "the native build child could not be identified and was stopped: {error}"
            )));
        }
    };
    write_json_atomic(
        &job_path,
        &BuildJob {
            schema: 1,
            arm: arm.to_owned(),
            source: source.to_path_buf(),
            state: state.to_path_buf(),
            output: output.clone(),
            phase: "observed".to_owned(),
            token: fresh_token(),
            pid: Some(observed.identity().pid),
            created: Some(observed.identity().creation_time),
            program: Some(exe),
            exit_code: None,
            error: None,
        },
    )?;
    run.cursor.condition = Some(format!(
        "preparing the missing {arm} runtime through native_build::prepare (pid {}); status and stop remain available",
        observed.identity().pid
    ));
    run.store.save_cursor(&run.cursor)?;
    release(run);
    let status = loop {
        if stop_requested(run.store.root()) {
            let _ = child.kill();
            let _ = child.wait();
            reacquire(run)?;
            let _ = fs::remove_file(&job_path);
            notes.push(format!(
                "controller: stopped while the {arm} build child was running; it is not dispatched again"
            ));
            return Ok(false);
        }
        match child.try_wait()? {
            Some(status) => break status,
            None => thread::sleep(POLL),
        }
    };
    reacquire(run)?;
    let _ = fs::remove_file(&job_path);
    if !status.success() {
        let reason = format!(
            "the native {arm} build failed ({status}); see {}",
            log_path.display()
        );
        run.cursor.block(reason.clone());
        run.cursor
            .effect(EffectKind::BuildPrepared, format!("arm={arm} failed"));
        run.store.save_cursor(&run.cursor)?;
        notes.push(reason);
        return Ok(false);
    }
    let prepared: PrepareOutput = read_json(&output, MAX_RUN_SPEC_BYTES).map_err(|error| {
        invalid(format!(
            "the native {arm} build exited 0 but its receipt is unreadable: {error}"
        ))
    })?;
    record_prepared_build(run, arm, prepared, notes)?;
    Ok(true)
}

/// Reconciles the build job currently on disk without ever starting a second
/// build while one is recorded. Intent-only records carry no observed
/// identity; they are abandoned to the native owner's own state lock. An
/// observed record is followed to its process exit and then to its receipt.
fn reconcile_build_job(
    run: &mut Run,
    notes: &mut Vec<String>,
) -> io::Result<Option<BuildReconciliation>> {
    let path = run.store.root().join(BUILD_JOB_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let job: BuildJob = read_json(&path, MAX_RUN_SPEC_BYTES)?;
    let Some((pid, created, program)) = job
        .pid
        .zip(job.created)
        .zip(job.program.clone())
        .map(|((pid, created), program)| (pid, created, program))
    else {
        notes.push(format!(
            "controller: the retained {} build job records intent without an observed process identity ({}); the native owner still serializes builds for this state, so a fresh observed attempt is prepared",
            job.arm,
            job_phase(&job)
        ));
        let _ = fs::remove_file(&path);
        return Ok(None);
    };
    let user = process_service::current_user()?;
    let identity = ProcessIdentity {
        pid,
        creation_time: created,
    };
    if ServiceProcess::inspect(identity, &program, &user)?.is_some() {
        run.cursor.condition = Some(format!(
            "waiting for the recorded {} build child {pid}; it is not started again and stop stays available",
            job.arm
        ));
        run.store.save_cursor(&run.cursor)?;
        release(run);
        loop {
            if stop_requested(run.store.root()) {
                let stopped = stop_build_job(run.store.root())?;
                reacquire(run)?;
                notes.push(format!(
                    "controller: stop was requested while reconciling the recorded {} build child; {}",
                    job.arm,
                    stopped.unwrap_or_else(|| "its identity was not resolved".to_owned())
                ));
                return Ok(Some(BuildReconciliation::Stopped));
            }
            if ServiceProcess::inspect(identity, &program, &user)?.is_none() {
                break;
            }
            thread::sleep(POLL);
        }
        reacquire(run)?;
    }
    if job.output.is_file()
        && let Ok(prepared) = read_json::<PrepareOutput>(&job.output, MAX_RUN_SPEC_BYTES)
    {
        let _ = fs::remove_file(&path);
        notes.push(format!(
            "controller: reconciled the recorded {} build child {pid} with its retained receipt (reused={})",
            job.arm, prepared.reused
        ));
        return Ok(Some(BuildReconciliation::Prepared {
            arm: job.arm,
            prepared,
        }));
    }
    Ok(Some(BuildReconciliation::Unknown(format!(
        "the recorded {} build child {pid} is not running and left no readable receipt at {}; its outcome is unknown and the build is not started again automatically",
        job.arm,
        job.output.display()
    ))))
}

/// Records one completed native build receipt for this arm and clears the
/// transient preparing condition.
fn record_prepared_build(
    run: &mut Run,
    arm: &str,
    prepared: PrepareOutput,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let mut record = if run.store.root().join(PREPARED_BUILDS_FILE).is_file() {
        read_json(
            &run.store.root().join(PREPARED_BUILDS_FILE),
            MAX_RUN_SPEC_BYTES,
        )?
    } else {
        PreparedBuilds {
            schema: 1,
            baseline: None,
            candidate: None,
            baseline_identity: None,
            candidate_identity: None,
        }
    };
    match arm {
        "baseline" => {
            record.baseline = Some(prepared.build.clone());
            record.baseline_identity = Some(prepared.source_identity.clone());
        }
        _ => {
            record.candidate = Some(prepared.build.clone());
            record.candidate_identity = Some(prepared.source_identity.clone());
        }
    }
    write_json_atomic(&run.store.root().join(PREPARED_BUILDS_FILE), &record)?;
    run.cursor.effect(
        EffectKind::BuildPrepared,
        format!(
            "arm={arm} build={} reused={} identity={}",
            prepared.build.display(),
            prepared.reused,
            prepared.source_identity
        ),
    );
    run.cursor.condition = None;
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!(
        "controller: prepared the missing {arm} runtime at {} (reused={})",
        prepared.build.display(),
        prepared.reused
    ));
    Ok(())
}

fn consume_decision(run: &mut Run, notes: &mut Vec<String>) -> io::Result<()> {
    if run.cursor.phase != Phase::DecisionRecorded {
        return Ok(());
    }
    if run.spec.comparison.is_none() {
        return Ok(());
    }
    let evaluation_path = run
        .cursor
        .comparison
        .as_ref()
        .and_then(|state| state.evaluation.clone())
        .unwrap_or_else(|| run.store.root().join("comparison").join("evaluation.json"));
    if !evaluation_path.is_file() {
        run.cursor.block(format!(
            "decision-recorded has no evaluation receipt at {}; integration is not attempted",
            evaluation_path.display()
        ));
        run.store.save_cursor(&run.cursor)?;
        return Ok(());
    }
    let evaluation: PolicyEvaluation = read_json(&evaluation_path, MAX_RUN_SPEC_BYTES)?;
    if evaluation.decision != PolicyDecision::Adopt {
        record_lineage(run, &evaluation, notes)?;
        run.cursor.phase = Phase::Idle;
        run.cursor.condition = Some(format!(
            "decision {} leaves the baseline unchanged; workload artifacts stay retained and no model work is started to repeat this experiment",
            evaluation.decision.as_str()
        ));
        run.cursor.effect(
            EffectKind::ContinuationRecorded,
            format!("decision={} idle", evaluation.decision.as_str()),
        );
        run.store.save_cursor(&run.cursor)?;
        notes.push(format!(
            "controller: decision {} did not authorize integration or activation",
            evaluation.decision.as_str()
        ));
        return Ok(());
    }
    if !run
        .spec
        .permits(harness_core::improvement_loop::PublicationStage::Integration)
    {
        record_lineage(run, &evaluation, notes)?;
        // The experiment is complete for this run: leaving it at
        // `decision-recorded` would only replay the same exact consumption.
        run.cursor.phase = Phase::Idle;
        run.cursor.condition = Some(
            "the adopted decision is published, but this run's publication scope does not permit integration; the baseline is unchanged and live publication was not performed"
                .to_owned(),
        );
        run.store.save_cursor(&run.cursor)?;
        notes.push(
            "controller: adoption is outside this run's integration authority; nothing was activated"
                .to_owned(),
        );
        return Ok(());
    }
    let check_path = run.store.root().join(INTEGRATION_CHECK_FILE);
    if !check_path.is_file() {
        run.cursor.block(
            "integration is in scope but no integration-check.json was declared; the baseline is unchanged",
        );
        run.store.save_cursor(&run.cursor)?;
        notes.push(
            "controller: refused integration without a declared combined-tree check".to_owned(),
        );
        return Ok(());
    }
    let check: IntegrationCheckRecord = read_json(&check_path, MAX_RUN_SPEC_BYTES)?;
    validate_check(&check)?;
    let bindings = load_bindings(run)?;
    let prior = load_optional::<IntegrationReceipt>(&run.store.root().join(INTEGRATION_FILE))?;
    let evidence = run.store.root().join("integration-evidence");
    let request = IntegrationRequest {
        spec: &run.spec,
        bindings: &bindings,
        evaluation: &evaluation,
        experiment: run.cursor.experiment.clone(),
        frozen_removal: run.cursor.removal_frozen.clone(),
        mainline: run.spec.project.clone(),
        check: CheckSpec {
            program: check.program,
            args: check.args.iter().map(Into::into).collect(),
            timeout: Duration::from_secs(check.timeout_seconds),
        },
        evidence,
        prior,
    };
    match improvement_activation::integrate(&request)? {
        IntegrationOutcome::Blocked(blocked) => {
            if blocked.pending {
                run.cursor.condition = Some(format!(
                    "waiting for removal authority before integration: {}",
                    blocked.reason
                ));
                run.store.save_cursor(&run.cursor)?;
                release(run);
                thread::sleep(POLL);
                reacquire(run)?;
                notes.push(
                    "controller: integration is waiting for removal authority; the baseline is unchanged and no model work was started"
                        .to_owned(),
                );
                return Ok(());
            }
            run.cursor.block(format!(
                "integration refused without changing the baseline: {}",
                blocked.reason
            ));
            run.cursor.effect(
                EffectKind::IntegrationConsumed,
                format!("blocked: {}", blocked.reason),
            );
            run.store.save_cursor(&run.cursor)?;
            notes.push(format!(
                "controller: integration blocked ({})",
                blocked.reason
            ));
            Ok(())
        }
        IntegrationOutcome::Integrated(receipt) | IntegrationOutcome::Confirmed(receipt) => {
            write_json_atomic(&run.store.root().join(INTEGRATION_FILE), &receipt)?;
            run.cursor.effect(
                EffectKind::IntegrationConsumed,
                format!(
                    "revision={} applied={}",
                    receipt.integrated_revision, receipt.applied
                ),
            );
            run.store.save_cursor(&run.cursor)?;
            notes.push(format!(
                "controller: integration owner {} revision {}",
                if receipt.applied {
                    "applied"
                } else {
                    "confirmed"
                },
                receipt.integrated_revision
            ));
            activate_integrated(run, &bindings, &evaluation, &receipt, notes)
        }
    }
}

fn activate_integrated(
    run: &mut Run,
    bindings: &ExperimentBindings,
    evaluation: &PolicyEvaluation,
    integration: &IntegrationReceipt,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let runtime_path = run
        .cursor
        .comparison
        .as_ref()
        .and_then(|state| state.arm(ComparisonArm::Candidate).runtime.clone())
        .ok_or_else(|| invalid("the candidate arm has no retained runtime receipt"))?;
    let runtime: ArmRuntime = read_json(&runtime_path, MAX_RUN_SPEC_BYTES)?;
    let state = run
        .spec
        .comparison
        .as_ref()
        .map(|comparison| comparison.runtimes.state.clone())
        .ok_or_else(|| invalid("activation requires the declared runtime state"))?;
    let attempt_active = run.cursor.attempts.iter().any(|attempt| {
        attempt.state == AttemptState::Started || attempt.state == AttemptState::Requested
    });
    let request = ActivationRequest {
        spec: &run.spec,
        bindings,
        evaluation,
        experiment: run.cursor.experiment.clone(),
        frozen_removal: run.cursor.removal_frozen.clone(),
        mainline: run.spec.project.clone(),
        integration: integration.clone(),
        state,
        runtime: &runtime,
        attempt_active,
    };
    // The candidate arm's measured dispatch legitimately appended the trusted
    // workspace entries its own dispatcher authorized; the accounting owner
    // verifies consumption that way, and activation must accept exactly the
    // same authorized slot set.
    let trusted = super::improvement_comparison::dispatch_workspaces(run, ComparisonArm::Candidate);
    match improvement_activation::activate_with_trust(&request, &trusted)? {
        ActivationOutcome::Blocked(blocked) => {
            run.cursor.block(format!(
                "activation refused; the integrated tree is unchanged by this owner: {}",
                blocked.reason
            ));
            run.cursor.effect(
                EffectKind::ActivationConsumed,
                format!("blocked: {}", blocked.reason),
            );
            run.store.save_cursor(&run.cursor)?;
            notes.push(format!(
                "controller: activation blocked ({})",
                blocked.reason
            ));
            Ok(())
        }
        ActivationOutcome::Activated(receipt) | ActivationOutcome::Confirmed(receipt) => {
            write_json_atomic(&run.store.root().join(ACTIVATION_FILE), &receipt)?;
            run.cursor.phase = Phase::ActivationConfirmed;
            run.cursor.condition = Some(
                "experimental baseline activation is confirmed; live publication was not performed"
                    .to_owned(),
            );
            run.cursor.effect(
                EffectKind::ActivationConsumed,
                format!(
                    "revision={} applied={} runtime={}",
                    receipt.integrated_revision,
                    receipt.applied,
                    receipt.runtime_source.display()
                ),
            );
            run.store.save_cursor(&run.cursor)?;
            notes.push(format!(
                "controller: activation owner {} revision {}",
                if receipt.applied {
                    "selected"
                } else {
                    "confirmed"
                },
                receipt.integrated_revision
            ));
            record_lineage(run, evaluation, notes)
        }
    }
}

fn load_bindings(run: &Run) -> io::Result<ExperimentBindings> {
    let path = run.store.comparison_bindings_path();
    read_json(&path, MAX_RUN_SPEC_BYTES).map_err(|error| {
        invalid(format!(
            "the comparison bindings at {} are unreadable: {error}",
            path.display()
        ))
    })
}

fn load_optional<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<Option<T>> {
    if !path.is_file() {
        return Ok(None);
    }
    read_json(path, MAX_RUN_SPEC_BYTES).map(Some)
}

fn validate_check(record: &IntegrationCheckRecord) -> io::Result<()> {
    if record.schema != 1 || !record.program.is_absolute() || record.timeout_seconds == 0 {
        return Err(invalid(
            "integration-check.json requires schema 1, an absolute program and a positive timeout_seconds",
        ));
    }
    Ok(())
}

fn record_lineage(
    run: &mut Run,
    evaluation: &PolicyEvaluation,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let path = run.store.root().join(LINEAGE_FILE);
    if path.is_file() {
        return Ok(());
    }
    let comparison = run.cursor.comparison.clone();
    let workload = run
        .spec
        .comparison
        .as_ref()
        .map(|comparison| comparison.workload_card.clone());
    write_json_atomic(
        &path,
        &json!({
            "schema": 1,
            "hypothesis": run.spec.hypothesis_item,
            "decision": evaluation.decision.as_str(),
            "workload_card": workload,
            "baseline_revision": comparison.as_ref().and_then(|state| state.baseline.revision.clone()),
            "candidate_revision": comparison.as_ref().and_then(|state| state.candidate.revision.clone()),
            "comparison_dir": run.store.root().join("comparison").display().to_string(),
        }),
    )?;
    notes.push(format!(
        "controller: retained lineage at {} without rewriting frozen inputs",
        path.display()
    ));
    Ok(())
}

/// Consumes the declared continuation of a completed experiment. The marker
/// records truthful lifecycle phases (`idle`, `intent`, `observed`,
/// `completed`, `stopped`) so a later declared successor is never suppressed
/// by an earlier no-successor record, and a launch is never recorded as
/// started before its process was observed.
fn finish_continuation(run: &mut Run, notes: &mut Vec<String>) -> io::Result<()> {
    let marker = run.store.root().join(CONTINUATION_FILE);
    let successor_path = run.store.root().join(SUCCESSOR_FILE);
    let declared: Option<SuccessorRecord> = if successor_path.is_file() {
        Some(read_json(&successor_path, MAX_RUN_SPEC_BYTES)?)
    } else {
        None
    };
    let existing: Option<ContinuationRecord> = if marker.is_file() {
        Some(read_json(&marker, MAX_RUN_SPEC_BYTES)?)
    } else {
        None
    };
    if let Some(record) = &existing {
        match continuation_phase(record) {
            "completed" if declared_matches(record, declared.as_ref()) => {
                notes.push(
                    "controller: the declared successor already completed; it is not started again"
                        .to_owned(),
                );
                return Ok(());
            }
            "stopped" if declared_matches(record, declared.as_ref()) => {
                notes.push(
                    "controller: the declared successor was already stopped through its own owner; it is not started again"
                        .to_owned(),
                );
                return Ok(());
            }
            "observed" | "observed-legacy" => {
                return reconcile_observed_continuation(run, record.clone(), notes);
            }
            "idle" | "idle-legacy" | "intent" => {}
            _ => {}
        }
    }
    let Some(successor) = declared else {
        return record_idle_continuation(run, existing.is_some(), notes);
    };
    if successor.spec == run.store.spec_path() {
        run.cursor.block(
            "the successor spec is this run's spec; an unchanged inconclusive or completed experiment is not repeated",
        );
        run.store.save_cursor(&run.cursor)?;
        return Ok(());
    }
    // B-artifact lineage: a successor is never started without the retained
    // lineage record of the completed experiment it continues.
    let lineage_path = run.store.root().join(LINEAGE_FILE);
    if !lineage_path.is_file() {
        run.cursor.condition = Some(
            "a successor is declared, but the retained lineage of the completed experiment is missing; a successor is not started without provable lineage"
                .to_owned(),
        );
        run.store.save_cursor(&run.cursor)?;
        notes
            .push("controller: refused to start the successor without retained lineage".to_owned());
        return Ok(());
    }
    let spec_sha256 = hash_file(&successor.spec)?;
    let lineage_sha256 = hash_file(&lineage_path)?;
    // The retained lineage is the authoritative record of the completed
    // decision this successor continues.
    let decision = read_json::<serde_json::Value>(&lineage_path, 4096)
        .ok()
        .and_then(|lineage| lineage["decision"].as_str().map(str::to_owned))
        .or_else(|| {
            run.cursor
                .comparison
                .as_ref()
                .and_then(|state| state.decision.clone())
        })
        .unwrap_or_else(|| {
            if run.cursor.phase == Phase::ActivationConfirmed {
                "activated".to_owned()
            } else {
                "none".to_owned()
            }
        });
    let base = ContinuationRecord {
        schema: 1,
        successor_started: true,
        note: format!(
            "successor spec {} run {}",
            successor.spec.display(),
            successor.run.display()
        ),
        phase: "intent".to_owned(),
        token: fresh_token(),
        spec: Some(successor.spec.clone()),
        spec_sha256: Some(spec_sha256),
        run: Some(successor.run.clone()),
        lineage: Some(lineage_path.clone()),
        lineage_sha256: Some(lineage_sha256),
        decision: Some(decision.clone()),
        pid: None,
        created: None,
        program: None,
        exit_code: None,
    };
    // A successor controller that already runs is adopted instead of being
    // started a second time.
    if let Some(owner) = live_successor_owner(&successor.run)? {
        let observed = ContinuationRecord {
            phase: "observed".to_owned(),
            token: fresh_token(),
            pid: Some(owner.pid),
            created: Some(owner.created),
            program: Some(owner.program),
            note: format!(
                "adopted the live successor controller for {}",
                successor.run.display()
            ),
            ..base.clone()
        };
        write_json_atomic(&marker, &observed)?;
        run.cursor.effect(
            EffectKind::ContinuationRecorded,
            format!(
                "successor spec={} run={} decision={decision} adopted-live",
                successor.spec.display(),
                successor.run.display()
            ),
        );
        run.store.save_cursor(&run.cursor)?;
        notes.push(format!(
            "controller: adopted the live successor controller for {}; it is not started twice",
            successor.run.display()
        ));
        return reconcile_observed_continuation(run, observed, notes);
    }
    write_json_atomic(&marker, &base)?;
    run.cursor.effect(
        EffectKind::ContinuationRecorded,
        format!(
            "successor spec={} run={} decision={decision} intent",
            successor.spec.display(),
            successor.run.display()
        ),
    );
    run.store.save_cursor(&run.cursor)?;
    release(run);
    let exe = std::env::current_exe()?;
    let mut command = Command::new(&exe);
    command
        .arg("improve")
        .arg("start")
        .arg("--run")
        .arg(&successor.run)
        .arg("--spec")
        .arg(&successor.spec)
        .arg("--supervision")
        .arg("continuous");
    if let Some(check) = &successor.integration_check {
        command.arg("--integration-check").arg(check);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            reacquire(run)?;
            let failed = ContinuationRecord {
                phase: "intent".to_owned(),
                token: fresh_token(),
                note: format!("the successor controller could not be started: {error}"),
                ..base.clone()
            };
            write_json_atomic(&marker, &failed)?;
            run.cursor.condition = Some(format!(
                "the declared successor spec {} could not be started: {error}; the experiment is complete and no model work is dispatched here",
                successor.spec.display()
            ));
            run.store.save_cursor(&run.cursor)?;
            notes.push(format!(
                "controller: the successor controller could not be started: {error}"
            ));
            return Ok(());
        }
    };
    let user = process_service::current_user()?;
    let observed = match ServiceProcess::observe(child.id(), &exe, 0, &user) {
        Ok(observed) => observed,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            reacquire(run)?;
            let failed = ContinuationRecord {
                phase: "intent".to_owned(),
                token: fresh_token(),
                note: format!(
                    "the successor controller could not be observed and was stopped: {error}"
                ),
                ..base.clone()
            };
            write_json_atomic(&marker, &failed)?;
            run.cursor.condition = Some(format!(
                "the successor controller could not be observed and was stopped: {error}"
            ));
            run.store.save_cursor(&run.cursor)?;
            notes.push(format!(
                "controller: the successor controller could not be observed and was stopped: {error}"
            ));
            return Ok(());
        }
    };
    reacquire(run)?;
    let observed = ContinuationRecord {
        phase: "observed".to_owned(),
        token: fresh_token(),
        pid: Some(observed.identity().pid),
        created: Some(observed.identity().creation_time),
        program: Some(exe),
        note: format!("successor controller {} observed", successor.run.display()),
        ..base
    };
    write_json_atomic(&marker, &observed)?;
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!(
        "controller: successor controller for {} is observed (pid {}); it is not started twice",
        successor.run.display(),
        observed.pid.unwrap_or_default()
    ));
    wait_for_successor_child(run, child, &observed, &successor, notes)
}

/// True when the retained marker belongs to the currently declared successor.
fn declared_matches(record: &ContinuationRecord, declared: Option<&SuccessorRecord>) -> bool {
    let Some(declared) = declared else {
        return false;
    };
    record.run.as_deref() == Some(declared.run.as_path())
        && record.spec.as_deref() == Some(declared.spec.as_path())
}

fn record_idle_continuation(
    run: &mut Run,
    already_recorded: bool,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let marker = run.store.root().join(CONTINUATION_FILE);
    if already_recorded {
        run.cursor.effect(
            EffectKind::ContinuationRecorded,
            "idle: no successor declared",
        );
        run.store.save_cursor(&run.cursor)?;
        notes.push(
            "controller: continuation was already recorded; it is not started again".to_owned(),
        );
        return Ok(());
    }
    if run.cursor.phase == Phase::ActivationConfirmed {
        run.cursor.condition = Some(
            "no independently specified successor was declared; the run is idle and no model work is started to invent one"
                .to_owned(),
        );
    }
    run.cursor.effect(
        EffectKind::ContinuationRecorded,
        "idle: no successor declared",
    );
    run.store.save_cursor(&run.cursor)?;
    write_json_atomic(
        &marker,
        &ContinuationRecord {
            schema: 1,
            successor_started: false,
            note: "no successor declared".to_owned(),
            phase: "idle".to_owned(),
            token: fresh_token(),
            spec: None,
            spec_sha256: None,
            run: None,
            lineage: None,
            lineage_sha256: None,
            decision: None,
            pid: None,
            created: None,
            program: None,
            exit_code: None,
        },
    )?;
    notes
        .push("controller: idle; no successor was specified and no model call was made".to_owned());
    Ok(())
}

/// The recorded successor controller without a child handle in this process
/// (a resumed controller): follow its identity to exit, or reconcile it under
/// a stop through its own run owner.
fn reconcile_observed_continuation(
    run: &mut Run,
    record: ContinuationRecord,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let marker = run.store.root().join(CONTINUATION_FILE);
    let successor = match record.run.clone() {
        Some(path) => path,
        None => {
            let completed = ContinuationRecord {
                phase: "completed".to_owned(),
                token: fresh_token(),
                note: "the legacy successor marker recorded no run path or process identity; its outcome is not re-started".to_owned(),
                ..record
            };
            write_json_atomic(&marker, &completed)?;
            notes.push(
                "controller: the legacy continuation marker recorded no identity; it is retained as completed-unknown"
                    .to_owned(),
            );
            return Ok(());
        }
    };
    let declared = successor_record(run)?;
    let identity = match (record.pid, record.created, record.program.clone()) {
        (Some(pid), Some(created), Some(program)) => Some((pid, created, program)),
        _ => None,
    };
    if let Some((pid, created, program)) = identity {
        let user = process_service::current_user()?;
        let identity = ProcessIdentity {
            pid,
            creation_time: created,
        };
        if ServiceProcess::inspect(identity, &program, &user)?.is_some() {
            run.cursor.condition = Some(format!(
                "waiting for the recorded successor controller {pid}; it is not started twice and stop stays available"
            ));
            run.store.save_cursor(&run.cursor)?;
            release(run);
            loop {
                if stop_requested(run.store.root()) {
                    if let Some(successor) = &declared {
                        stop_successor_run(successor, notes)?;
                    }
                    let deadline = Instant::now() + SUCCESSOR_WAIT;
                    while Instant::now() < deadline
                        && ServiceProcess::inspect(identity, &program, &user)?.is_some()
                    {
                        thread::sleep(POLL);
                    }
                    let exited = ServiceProcess::inspect(identity, &program, &user)?.is_none();
                    reacquire(run)?;
                    let stopped = ContinuationRecord {
                        phase: "stopped".to_owned(),
                        token: fresh_token(),
                        exit_code: None,
                        note: format!(
                            "stopped under request through the successor's own owner; controller {} {}",
                            pid,
                            if exited {
                                "was observed to exit"
                            } else {
                                "was still running when the bounded wait ended"
                            }
                        ),
                        ..record
                    };
                    write_json_atomic(&marker, &stopped)?;
                    run.cursor.condition = Some(
                        "the successor run was stopped through its own owner; no dependent dispatch remains here"
                            .to_owned(),
                    );
                    run.cursor.effect(
                        EffectKind::ContinuationRecorded,
                        "successor stopped through its own owner",
                    );
                    run.store.save_cursor(&run.cursor)?;
                    notes.push(
                        "controller: the recorded successor was stopped through its own owner"
                            .to_owned(),
                    );
                    return Ok(());
                }
                if ServiceProcess::inspect(identity, &program, &user)?.is_none() {
                    break;
                }
                thread::sleep(POLL);
            }
            reacquire(run)?;
        }
    }
    let completed = ContinuationRecord {
        phase: "completed".to_owned(),
        token: fresh_token(),
        exit_code: record.exit_code,
        note: "the recorded successor controller is not running; its exact exit status was not observed by this controller"
            .to_owned(),
        ..record
    };
    write_json_atomic(&marker, &completed)?;
    run.cursor.effect(
        EffectKind::ContinuationRecorded,
        "successor completed (exit status unobserved)",
    );
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!(
        "controller: the recorded successor for {} is not running; it is retained as completed with an unobserved exit status",
        successor.display()
    ));
    Ok(())
}

fn successor_record(run: &Run) -> io::Result<Option<SuccessorRecord>> {
    let path = run.store.root().join(SUCCESSOR_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    read_json(&path, MAX_RUN_SPEC_BYTES).map(Some)
}

fn live_successor_owner(run_path: &Path) -> io::Result<Option<OwnerRecord>> {
    let path = run_path.join(harness_core::improvement_loop::OWNER_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let record: OwnerRecord = read_json(&path, 4096)?;
    Ok(matches!(owner_is_live(&record)?, Some(true)).then_some(record))
}

/// Stops the successor's own run through its existing stop owner, which
/// resolves its executors and retains unknown effects, instead of only
/// killing the controller process here.
fn stop_successor_run(successor: &SuccessorRecord, notes: &mut Vec<String>) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut command = Command::new(&exe);
    command
        .arg("improve")
        .arg("stop")
        .arg("--run")
        .arg(&successor.run)
        .arg("--reason")
        .arg("the predecessor controller was stopped; the successor is suspended through its own owner");
    match command.output() {
        Ok(output) if output.status.success() => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let summary = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .take(2)
                .collect::<Vec<_>>()
                .join(" ");
            notes.push(format!("controller: successor stop owner: {summary}"));
        }
        Ok(output) => notes.push(format!(
            "controller: the successor stop owner refused ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => notes.push(format!(
            "controller: the successor stop owner could not be run: {error}"
        )),
    }
    Ok(())
}

/// Waits for the successor controller this process spawned. A stop is
/// resolved through the successor's own run owner before the controller is
/// reaped, so a successor executor is never left without its stop path.
fn wait_for_successor_child(
    run: &mut Run,
    mut child: std::process::Child,
    record: &ContinuationRecord,
    successor: &SuccessorRecord,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let marker = run.store.root().join(CONTINUATION_FILE);
    // The successor runs as its own owned run; this run keeps status and stop
    // usable while waiting, so the mutation guard is released here.
    if run.guard.is_some() {
        release(run);
    }
    loop {
        if stop_requested(run.store.root()) {
            stop_successor_run(successor, notes)?;
            let deadline = Instant::now() + SUCCESSOR_WAIT;
            let mut status = None;
            while Instant::now() < deadline {
                if let Some(current) = child.try_wait()? {
                    status = Some(current);
                    break;
                }
                thread::sleep(POLL);
            }
            let exit_code = match status {
                Some(status) => {
                    let _ = child.wait();
                    status.code()
                }
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    None
                }
            };
            reacquire(run)?;
            let stopped = ContinuationRecord {
                phase: "stopped".to_owned(),
                token: fresh_token(),
                exit_code,
                note: format!(
                    "stopped under request; the successor run owner was invoked, the controller exit code is {}",
                    exit_code.map_or("unknown".to_owned(), |code| code.to_string())
                ),
                ..record.clone()
            };
            write_json_atomic(&marker, &stopped)?;
            run.cursor.condition = Some(
                "the successor run was stopped through its own owner; no dependent dispatch remains here"
                    .to_owned(),
            );
            run.cursor.effect(
                EffectKind::ContinuationRecorded,
                "successor stopped through its own owner",
            );
            run.store.save_cursor(&run.cursor)?;
            notes.push(
                "controller: stop consumed; the successor was suspended through its own owner before this controller leaves"
                    .to_owned(),
            );
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            reacquire(run)?;
            let completed = ContinuationRecord {
                phase: "completed".to_owned(),
                token: fresh_token(),
                exit_code: status.code(),
                note: format!(
                    "successor controller {} exited with {}",
                    successor.run.display(),
                    status
                ),
                ..record.clone()
            };
            write_json_atomic(&marker, &completed)?;
            run.cursor.effect(
                EffectKind::ContinuationRecorded,
                format!(
                    "successor run={} exited={}",
                    successor.run.display(),
                    status
                ),
            );
            run.store.save_cursor(&run.cursor)?;
            notes.push(format!(
                "controller: successor run {} exited {status}",
                successor.run.display()
            ));
            return Ok(());
        }
        thread::sleep(POLL);
    }
}

fn rejected_build_overrides() -> Vec<String> {
    std::env::vars()
        .filter_map(|(key, value)| {
            if value.is_empty() {
                return None;
            }
            let upper = key.to_ascii_uppercase();
            let rejected = upper.starts_with("RUSTC")
                || upper == "RUSTFLAGS"
                || upper.starts_with("CARGO_BUILD_")
                || upper.starts_with("CARGO_TARGET_")
                || upper.starts_with("CARGO_PROFILE_")
                || upper.starts_with("CARGO_ALIAS_")
                || matches!(
                    upper.as_str(),
                    "CARGO_ENCODED_RUSTFLAGS" | "CARGO_INCREMENTAL"
                );
            rejected.then_some(key)
        })
        .collect()
}
