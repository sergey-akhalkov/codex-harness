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
use harness_core::board_hypothesis;
use harness_core::build_identity;
use harness_core::improvement_activation::{
    self, ActivationOutcome, ActivationRequest, CheckSpec, IntegrationOutcome, IntegrationReceipt,
    IntegrationRequest,
};
use harness_core::improvement_experiment::{
    Arm, CorroborationRequirement, CorroborationSelection, CorroborationStatus, ExperimentBindings,
    RetainedTask, TaskRetention, retain_completed_task, retained_tasks_from_board,
    select_corroboration,
};
use harness_core::improvement_loop::{
    AttemptState, ComparisonArm, EffectKind, MAX_RUN_SPEC_BYTES, OwnerRecord, Phase, RunSpec,
    RunStore, current_process_identity, owner_is_live, read_json, write_json_atomic,
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
/// The decision-boundary consumption receipts: the completed real task's
/// retention, the declared corroboration selection and the unadopted
/// reconciliation of the hypothesis' own change. Each is written once per
/// decision, so a later pass never repeats the same external action.
const RETENTION_FILE: &str = "retention.json";
const RETAINED_DIR: &str = "retained";
const RETAINED_INDEX_FILE: &str = "retained-tasks.json";
const CORROBORATION_FILE: &str = "corroboration.json";
const RECONCILE_FILE: &str = "reconcile.json";
/// Bound on one recorded consumption reason; the receipt names the exact
/// identity, never a long transcript.
const MAX_CONSUMPTION_DETAIL: usize = 512;
/// Bound on the run-local retained-task index used for corroboration
/// selection; the durable owner records stay on the Beads cards.
const MAX_RETAINED_TASKS: usize = 64;
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
    /// The successor spec's own hypothesis card, read from its validated
    /// independent run inputs before it is started; never inferred here.
    #[serde(default)]
    hypothesis: Option<String>,
    /// The evaluation workload the successor's own independently specified
    /// plan declares. It is that spec's own C and is never inherited from
    /// this run's experiment.
    #[serde(default)]
    workload_card: Option<String>,
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
    // Routed consumption at the decision boundary: the merged owners retain
    // the completed real task, select declared corroboration units and
    // reconcile an unadopted decision with its own change's actual task
    // state. Each is recorded once; none of them may change the decision the
    // comparison owner published.
    consume_decision_evidence(run, &evaluation, notes)?;
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
        removal_required: run
            .cursor
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate.removal_required),
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

// ---------------------------------------------------------------------------
// Decision-boundary consumption.
//
// The controller routes a settled decision to the merged owners instead of
// duplicating them: the experiment owner retains the completed real task's
// replayable pre-solution inputs under its existing Beads card, the
// corroboration selector picks applicable independent retained units for the
// declared adoption scope, and the feedback owner reconciles an unadopted
// decision with the hypothesis' own OpenSpec change. Every receipt is
// identity-only: no solution, patch, answer or conversation content enters
// run state, the board or the main specifications. Each action is recorded
// once, so a later pass never repeats the same external action.
// ---------------------------------------------------------------------------

/// The run-local receipt of the completed real task's retention. It carries
/// the identity references and the retained pre-solution replay locator; it
/// has no field for a solution or an answer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RetentionReceipt {
    pub(super) schema: u32,
    /// `retained` or `unavailable`.
    pub(super) status: String,
    pub(super) owner: String,
    pub(super) case_id: String,
    pub(super) experiment: String,
    /// The committed source revision the completed real task ran.
    pub(super) revision: String,
    /// The frozen root commit materialized from that revision.
    pub(super) frozen: String,
    /// Content digest over the retained snapshot's tree entries.
    pub(super) tree: String,
    /// The retained pristine pre-solution copy used for replay.
    pub(super) replay: Option<PathBuf>,
    pub(super) reason: Option<String>,
}

/// The run-local receipt of the declared corroboration selection. The
/// embedded selection is identity and replay references only.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CorroborationReceipt {
    pub(super) schema: u32,
    /// `selected` or `unavailable`.
    pub(super) status: String,
    /// The additional independent units the declared scope requires beyond
    /// the run's own declared plan unit.
    pub(super) required_units: u32,
    pub(super) selection: Option<CorroborationSelection>,
    pub(super) reason: Option<String>,
}

/// The run-local receipt of one unadopted decision's reconcile call. The
/// merged feedback owner keeps the change's task state authoritative; this
/// receipt records which exact identities were reconciled and what it did.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReconcileReceipt {
    pub(super) schema: u32,
    pub(super) item: String,
    pub(super) outcome: String,
    pub(super) action: String,
    pub(super) change: String,
    /// `retained`, `archived` or `refused`.
    pub(super) status: String,
    pub(super) exit_code: Option<i32>,
    pub(super) detail: Option<String>,
}

pub(super) fn retention_receipt(run: &Run) -> io::Result<Option<RetentionReceipt>> {
    load_optional(&run.store.root().join(RETENTION_FILE))
}

pub(super) fn corroboration_receipt(run: &Run) -> io::Result<Option<CorroborationReceipt>> {
    load_optional(&run.store.root().join(CORROBORATION_FILE))
}

pub(super) fn reconcile_receipt(run: &Run) -> io::Result<Option<ReconcileReceipt>> {
    load_optional(&run.store.root().join(RECONCILE_FILE))
}

/// Consumes the settled decision through the merged owners. None of these
/// steps may change the verdict the comparison owner published; a refusal is
/// recorded with its exact reason and never retried identically.
fn consume_decision_evidence(
    run: &mut Run,
    evaluation: &PolicyEvaluation,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    retain_completed_workload(run, notes)?;
    select_corroboration_units(run, notes)?;
    reconcile_unadopted_decision(run, evaluation, notes)?;
    Ok(())
}

/// Retains the completed real workload task through the experiment owner as
/// soon as one decision is settled: the frozen pre-solution copy is
/// re-materialized as an independent replayable copy and recorded under the
/// card that already owns the task. A missing frozen copy or owner record is
/// recorded as unavailable - never replaced by a summary - and the same exact
/// attempt is not repeated.
fn retain_completed_workload(run: &mut Run, notes: &mut Vec<String>) -> io::Result<()> {
    if run.store.root().join(RETENTION_FILE).is_file() {
        return Ok(());
    }
    if run.spec.comparison.is_none() {
        return Ok(());
    }
    let state = run.cursor.comparison.clone();
    let owner = state
        .as_ref()
        .and_then(|state| state.workload_card.clone())
        .unwrap_or_default();
    let experiment = run.cursor.experiment.clone();
    let bindings = match state.as_ref().and_then(|state| state.bindings.as_ref()) {
        Some(path) => match load_optional::<ExperimentBindings>(path) {
            Ok(bindings) => bindings,
            Err(error) => {
                return unavailable_retention(
                    run,
                    notes,
                    &owner,
                    "",
                    &experiment,
                    None,
                    format!(
                        "the retained comparison bindings at {} are unreadable: {error}",
                        path.display()
                    ),
                );
            }
        },
        None => None,
    };
    let Some(bindings) = bindings else {
        return unavailable_retention(
            run,
            notes,
            &owner,
            "",
            &experiment,
            None,
            "the prepared comparison bindings are not retained; a completed real task is never retained from a summary".to_owned(),
        );
    };
    let case_id = bindings.case_id.clone();
    if owner.is_empty() {
        return unavailable_retention(
            run,
            notes,
            &owner,
            &case_id,
            &experiment,
            None,
            "the run retains no workload card; retention never creates a card or a second task store"
                .to_owned(),
        );
    }
    let card =
        match board_hypothesis::load_card(&run.spec.board.bd, &run.spec.board.project, &owner) {
            Ok(card) => card,
            Err(error) => {
                return unavailable_retention(
                    run,
                    notes,
                    &owner,
                    &case_id,
                    &experiment,
                    None,
                    format!("the owning card {owner} is unreadable: {error}"),
                );
            }
        };
    let Some(admission) = board_hypothesis::parse_admission(&card.description) else {
        return unavailable_retention(
            run,
            notes,
            &owner,
            &case_id,
            &experiment,
            None,
            format!(
                "the owning card {owner} carries no recognized admission record; its mechanism and conditions are required before retention"
            ),
        );
    };
    let (Some(mechanism), Some(conditions)) = (admission.mechanism, admission.conditions) else {
        return unavailable_retention(
            run,
            notes,
            &owner,
            &case_id,
            &experiment,
            None,
            format!(
                "the owning card {owner} records no mechanism/conditions; retention never invents applicability"
            ),
        );
    };
    let workload = match bindings.arm(Arm::Baseline) {
        Ok(arm) => arm.workload.clone(),
        Err(error) => {
            return unavailable_retention(
                run,
                notes,
                &owner,
                &case_id,
                &experiment,
                None,
                format!("the completed task's frozen baseline arm is not bound: {error}"),
            );
        }
    };
    let retention = TaskRetention {
        owner: owner.clone(),
        case_id: case_id.clone(),
        experiment: experiment.clone(),
        mechanism,
        conditions,
        oracle: bindings.oracle.clone(),
        acceptance: bindings.acceptance.clone(),
    };
    let destination = run
        .store
        .root()
        .join(RETAINED_DIR)
        .join(retained_dir_name(&case_id));
    let retained = match retain_completed_task(&workload, &destination, &retention) {
        Ok(retained) => retained,
        Err(error) => {
            return unavailable_retention(
                run,
                notes,
                &owner,
                &case_id,
                &experiment,
                None,
                format!("the completed task's frozen copy could not be retained: {error}"),
            );
        }
    };
    let draft = board_hypothesis::RetentionDraft {
        case_id: retained.case_id.clone(),
        experiment: retained.experiment.clone(),
        mechanism: retained.mechanism.clone(),
        conditions: retained.conditions.clone(),
        revision: retained.replay.source_revision.clone(),
        frozen: retained.replay.revision.clone(),
        tree: retained.replay.tree_sha256.clone(),
        oracle: retained.oracle.clone(),
        acceptance: retained.acceptance.clone(),
        replay: retained.replay.path.display().to_string(),
        detail: Some("completed real task retained at the decision boundary".to_owned()),
    };
    let bounded = match board_hypothesis::BoundedRetention::try_from_draft(draft) {
        Ok(bounded) => bounded,
        Err(error) => {
            return unavailable_retention(
                run,
                notes,
                &owner,
                &case_id,
                &experiment,
                Some(retained.replay.path.clone()),
                format!("the retention references were refused: {error}"),
            );
        }
    };
    let recorded = match board_hypothesis::record_retention(
        &run.spec.board.bd,
        &run.spec.board.project,
        &owner,
        &bounded,
    ) {
        Ok(record) => record.recorded,
        Err(error) => {
            return unavailable_retention(
                run,
                notes,
                &owner,
                &case_id,
                &experiment,
                Some(retained.replay.path.clone()),
                format!("the owning card {owner} refused the retention record: {error}"),
            );
        }
    };
    if let Err(error) = append_retained_task(run, &retained) {
        notes.push(format!(
            "controller: the run-local retained-task index did not record case {case_id}: {error}; the durable owner record stays on {owner}"
        ));
    }
    write_json_atomic(
        &run.store.root().join(RETENTION_FILE),
        &RetentionReceipt {
            schema: 1,
            status: "retained".to_owned(),
            owner: owner.clone(),
            case_id: case_id.clone(),
            experiment: experiment.clone(),
            revision: retained.replay.source_revision.clone(),
            frozen: retained.replay.revision.clone(),
            tree: retained.replay.tree_sha256.clone(),
            replay: Some(retained.replay.path.clone()),
            reason: None,
        },
    )?;
    run.cursor.effect(
        EffectKind::ContinuationRecorded,
        format!(
            "retention case={case_id} owner={owner} replay={} tree={}",
            retained.replay.path.display(),
            &retained.replay.tree_sha256[..16.min(retained.replay.tree_sha256.len())]
        ),
    );
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!(
        "controller: retained the completed real task case={case_id} under {owner} ({}) as identity-only replayable pre-solution inputs",
        if recorded { "new record" } else { "existing record" }
    ));
    Ok(())
}

fn unavailable_retention(
    run: &mut Run,
    notes: &mut Vec<String>,
    owner: &str,
    case_id: &str,
    experiment: &str,
    replay: Option<PathBuf>,
    reason: String,
) -> io::Result<()> {
    let reason = bounded_consumption(reason);
    write_json_atomic(
        &run.store.root().join(RETENTION_FILE),
        &RetentionReceipt {
            schema: 1,
            status: "unavailable".to_owned(),
            owner: owner.to_owned(),
            case_id: case_id.to_owned(),
            experiment: experiment.to_owned(),
            revision: String::new(),
            frozen: String::new(),
            tree: String::new(),
            replay,
            reason: Some(reason.clone()),
        },
    )?;
    run.cursor.effect(
        EffectKind::ContinuationRecorded,
        format!("retention unavailable: {reason}"),
    );
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!(
        "controller: the completed real task was not retained: {reason}"
    ));
    Ok(())
}

/// A stable, bounded directory name for one retained task identity.
fn retained_dir_name(case_id: &str) -> String {
    let digest = build_identity::hash_bytes(case_id.as_bytes());
    let mut name: String = case_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    if name.is_empty() {
        name.push_str("case");
    }
    format!("{name}-{}", &digest[..12])
}

/// The run-local retained-task index: complete identity and replay
/// references of the tasks this run retained, used by corroboration
/// selection. It never holds a solution, patch or conversation.
fn append_retained_task(run: &Run, retained: &RetainedTask) -> io::Result<()> {
    let path = run.store.root().join(RETAINED_INDEX_FILE);
    let mut tasks: Vec<RetainedTask> = if path.is_file() {
        read_json(&path, MAX_RUN_SPEC_BYTES)?
    } else {
        Vec::new()
    };
    let duplicate = tasks.iter().any(|task| {
        task.case_id == retained.case_id && task.replay.tree_sha256 == retained.replay.tree_sha256
    });
    if !duplicate {
        if tasks.len() >= MAX_RETAINED_TASKS {
            return Err(invalid(format!(
                "the retained-task index already holds {MAX_RETAINED_TASKS} tasks; the durable records stay on their Beads cards"
            )));
        }
        tasks.push(retained.clone());
        write_json_atomic(&path, &tasks)?;
    }
    Ok(())
}

/// Selects the additional independent corroboration units the declared
/// adoption scope requires, before the outcome is treated as supporting a
/// broader claim. Selection is identity-only and order-independent; too few
/// applicable, independent, replayable retained tasks leave the broader claim
/// explicitly inconclusive rather than turning an inapplicable workload into
/// evidence or repeating an identical measurement.
fn select_corroboration_units(run: &mut Run, notes: &mut Vec<String>) -> io::Result<()> {
    if run.store.root().join(CORROBORATION_FILE).is_file() {
        return Ok(());
    }
    let Some(comparison) = run.spec.comparison.as_ref() else {
        return Ok(());
    };
    let declared = match comparison.declared_policy() {
        Ok(declared) => declared,
        Err(error) => {
            return unavailable_corroboration(
                run,
                notes,
                0,
                format!("the declared comparison policy is unusable: {error}"),
            );
        }
    };
    let required_total = declared.policy.stopping.required_units;
    if required_total <= 1 {
        return Ok(());
    }
    let additional = required_total - 1;
    let item = run
        .cursor
        .candidate
        .as_ref()
        .map(|candidate| candidate.hypothesis.clone())
        .unwrap_or_else(|| run.spec.hypothesis_item.clone());
    let record = board_hypothesis::load_card(&run.spec.board.bd, &run.spec.board.project, &item)
        .ok()
        .and_then(|card| board_hypothesis::parse_admission(&card.description));
    let (Some(mechanism), Some(conditions)) = (
        record.as_ref().and_then(|record| record.mechanism.clone()),
        record.as_ref().and_then(|record| record.conditions.clone()),
    ) else {
        return unavailable_corroboration(
            run,
            notes,
            additional,
            format!(
                "hypothesis card {item} records no mechanism/conditions; units applicable to the declared claim cannot be selected"
            ),
        );
    };
    // Only the declared plan's own unit identities are excluded, never the
    // owning card: the admission owner reuses one hypothesis card per
    // mechanism/conditions identity, so this run's own unit and every
    // applicable independent prior unit are recorded under that same card.
    // Excluding the card would exclude every applicable prior unit and leave
    // the broader claim inconclusive exactly when an applicable unit exists.
    let mut excluded: Vec<String> = Vec::new();
    if let Some(state) = run.cursor.comparison.as_ref()
        && let Some(path) = state.bindings.as_ref()
        && let Ok(Some(bindings)) = load_optional::<ExperimentBindings>(path)
    {
        excluded.push(bindings.case_id.clone());
    }
    // A decision boundary interrupted after retention but before selection
    // resumes with this run's unit already recorded on its card; its recorded
    // case id keeps that exact unit out of its own corroboration even when the
    // declared bindings are no longer readable.
    if let Ok(Some(receipt)) = retention_receipt(run)
        && receipt.status == "retained"
        && !receipt.case_id.is_empty()
    {
        excluded.push(receipt.case_id.clone());
    }
    // Retained units are discovered from their durable Beads owners, so a
    // later run can corroborate on tasks an earlier run retained. Records
    // whose artifacts are missing or changed stay unsupported with their
    // exact reason instead of being reconstructed from a summary.
    let candidates = match retained_tasks_from_board(&run.spec.board.bd, &run.spec.board.project) {
        Ok(discovery) => {
            for unsupported in &discovery.unsupported {
                notes.push(format!(
                    "retained unit {} ({}) not selectable: {}",
                    unsupported.item, unsupported.case_id, unsupported.reason
                ));
            }
            discovery.tasks
        }
        Err(error) => {
            return unavailable_corroboration(
                run,
                notes,
                additional,
                format!("retained-task discovery through the board is unavailable: {error}"),
            );
        }
    };
    let requirement = CorroborationRequirement {
        mechanism,
        conditions,
        required_units: additional,
        excluded,
    };
    let selection = match select_corroboration(&candidates, &requirement) {
        Ok(selection) => selection,
        Err(error) => {
            return unavailable_corroboration(
                run,
                notes,
                additional,
                format!("corroboration selection refused: {error}"),
            );
        }
    };
    write_json_atomic(
        &run.store.root().join(CORROBORATION_FILE),
        &CorroborationReceipt {
            schema: 1,
            status: "selected".to_owned(),
            required_units: additional,
            selection: Some(selection.clone()),
            reason: None,
        },
    )?;
    let state = if selection.is_ready() {
        "ready"
    } else {
        "inconclusive"
    };
    run.cursor.effect(
        EffectKind::ContinuationRecorded,
        format!(
            "corroboration required=+{additional} status={state} units={} excluded={}",
            selection.units.len(),
            selection.excluded.len()
        ),
    );
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!(
        "controller: the declared adoption scope requires {additional} additional independent corroboration unit(s); selection is {state} with {} applicable unit(s){}",
        selection.units.len(),
        match &selection.status {
            CorroborationStatus::Inconclusive(reason) => format!(" ({reason})"),
            CorroborationStatus::Ready => String::new(),
        }
    ));
    Ok(())
}

fn unavailable_corroboration(
    run: &mut Run,
    notes: &mut Vec<String>,
    required_units: u32,
    reason: String,
) -> io::Result<()> {
    let reason = bounded_consumption(reason);
    write_json_atomic(
        &run.store.root().join(CORROBORATION_FILE),
        &CorroborationReceipt {
            schema: 1,
            status: "unavailable".to_owned(),
            required_units,
            selection: None,
            reason: Some(reason.clone()),
        },
    )?;
    run.cursor.effect(
        EffectKind::ContinuationRecorded,
        format!("corroboration unavailable: {reason}"),
    );
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!(
        "controller: corroboration selection was not performed: {reason}"
    ));
    Ok(())
}

/// Routes one unadopted decision to the merged reconcile owner for the
/// hypothesis card's own linked change. Retention reads the change's actual
/// task state and writes nothing; archival uses the supported
/// non-synchronizing path only when every required task is done, so an
/// unfinished required task is never closed through an experiment outcome
/// and no unadopted delta reaches the main specifications. An adopted
/// decision is refused here: its delta synchronizes through the
/// adoption/integration owner.
fn reconcile_unadopted_decision(
    run: &mut Run,
    evaluation: &PolicyEvaluation,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    if run.store.root().join(RECONCILE_FILE).is_file() {
        return Ok(());
    }
    let outcome = match evaluation.decision {
        PolicyDecision::Reject => "reject",
        PolicyDecision::Inconclusive => "inconclusive",
        PolicyDecision::Adopt => return Ok(()),
    };
    let item = run
        .cursor
        .candidate
        .as_ref()
        .map(|candidate| candidate.hypothesis.clone())
        .unwrap_or_else(|| run.spec.hypothesis_item.clone());
    let change = run
        .cursor
        .candidate
        .as_ref()
        .map(|candidate| candidate.change.clone())
        .unwrap_or_else(|| run.spec.specification.change.clone());
    let action = if evaluation.decision == PolicyDecision::Reject {
        "archive"
    } else {
        "retain"
    };
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("feedback")
        .arg("hypothesis-reconcile")
        .arg("--project")
        .arg(&run.spec.board.project)
        .arg("--bd")
        .arg(&run.spec.board.bd)
        .arg("--item")
        .arg(&item)
        .arg("--outcome")
        .arg(outcome)
        .arg("--action")
        .arg(action)
        .arg("--change")
        .arg(&change)
        .arg("--openspec-project")
        .arg(&run.spec.specification.project)
        .arg("--planning-root")
        .arg(&run.spec.specification.planning_root);
    if let Some(store) = &run.spec.specification.store {
        command.arg("--store").arg(store);
    }
    let receipt = match command.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let code = output.status.code();
            let status = if stdout.contains("action=archived") {
                "archived"
            } else if matches!(code, Some(0 | 1)) {
                "retained"
            } else {
                "refused"
            };
            let detail = stdout
                .lines()
                .chain(stderr.lines())
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| bounded_consumption(line.to_owned()));
            ReconcileReceipt {
                schema: 1,
                item: item.clone(),
                outcome: outcome.to_owned(),
                action: action.to_owned(),
                change: change.clone(),
                status: status.to_owned(),
                exit_code: code,
                detail,
            }
        }
        Err(error) => ReconcileReceipt {
            schema: 1,
            item: item.clone(),
            outcome: outcome.to_owned(),
            action: action.to_owned(),
            change: change.clone(),
            status: "refused".to_owned(),
            exit_code: None,
            detail: Some(bounded_consumption(format!(
                "the reconcile owner could not be started: {error}"
            ))),
        },
    };
    write_json_atomic(&run.store.root().join(RECONCILE_FILE), &receipt)?;
    run.cursor.effect(
        EffectKind::Reconciled,
        format!(
            "outcome={} item={} change={} action={} status={}",
            receipt.outcome, receipt.item, receipt.change, receipt.action, receipt.status
        ),
    );
    run.store.save_cursor(&run.cursor)?;
    notes.push(match receipt.status.as_str() {
        "archived" => format!(
            "controller: reconciled the unadopted decision {outcome}: the completed change {change} was archived through the non-synchronizing path and the main specifications kept their exact content"
        ),
        "retained" => format!(
            "controller: reconciled the unadopted decision {outcome}: change {change} stays retained; unfinished required tasks were not closed and no unadopted delta reached the main specifications"
        ),
        _ => format!(
            "controller: the reconcile owner refused the unadopted decision {outcome} for change {change}: {}",
            receipt.detail.as_deref().unwrap_or("no detail")
        ),
    });
    Ok(())
}

fn bounded_consumption(detail: String) -> String {
    if detail.len() <= MAX_CONSUMPTION_DETAIL {
        return detail;
    }
    let mut cut = MAX_CONSUMPTION_DETAIL;
    while !detail.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}...", &detail[..cut])
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
        removal_required: run
            .cursor
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate.removal_required),
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
    // The activation owner's verified workload lineage - the exact retained
    // implementations the integrated candidate carries - is consumed here
    // instead of being lost with the integration receipt: an adopted
    // baseline keeps its trace to the independently accepted solution, while
    // a run that never integrated records an empty lineage.
    let workload_lineage =
        load_optional::<IntegrationReceipt>(&run.store.root().join(INTEGRATION_FILE))?
            .map(|receipt| receipt.workload_lineage)
            .unwrap_or_default();
    let workload_lineage_count = workload_lineage.len();
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
            "workload_lineage": workload_lineage,
        }),
    )?;
    notes.push(format!(
        "controller: retained lineage at {} without rewriting frozen inputs ({} verified workload solution(s))",
        path.display(),
        workload_lineage_count
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
        hypothesis: None,
        workload_card: None,
        pid: None,
        created: None,
        program: None,
        exit_code: None,
    };
    // Grounded selection: the successor must be a valid independently
    // specified run, not merely a file path. Its own declared experiment (the
    // workload C it evaluates on) is read from that specification and
    // recorded; nothing is inferred from this run's workload.
    let successor_spec = match RunSpec::load(&successor.spec) {
        Ok(spec) => spec,
        Err(error) => {
            let note = format!(
                "the declared successor spec {} is not a valid independent run specification: {error}; no successor controller is started and nothing is manufactured for it",
                successor.spec.display()
            );
            write_json_atomic(
                &marker,
                &ContinuationRecord {
                    phase: "intent".to_owned(),
                    token: fresh_token(),
                    note: note.clone(),
                    ..base.clone()
                },
            )?;
            run.cursor.condition = Some(note.clone());
            run.cursor
                .effect(EffectKind::ContinuationRecorded, format!("refused: {note}"));
            run.store.save_cursor(&run.cursor)?;
            notes.push(format!("controller: {note}"));
            return Ok(());
        }
    };
    let base = ContinuationRecord {
        hypothesis: Some(successor_spec.hypothesis_item.clone()),
        workload_card: successor_spec
            .comparison
            .as_ref()
            .map(|comparison| comparison.workload_card.clone()),
        ..base
    };
    if run
        .spec
        .comparison
        .as_ref()
        .map(|comparison| comparison.workload_card.as_str())
        == base.hypothesis.as_deref()
    {
        notes.push(format!(
            "controller: the independently specified successor investigates {} - this run's evaluated workload - against its own declared workload {}",
            base.hypothesis.as_deref().unwrap_or_default(),
            base.workload_card.as_deref().unwrap_or("none")
        ));
    }
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
            hypothesis: None,
            workload_card: None,
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
