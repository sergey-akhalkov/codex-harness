//! Foreground surface runner for one reflection stage.
//!
//! The managed lead host's journal consumer is model-free and has no terminal.
//! A fresh stage still needs a visibly titled native frontend whose attachment
//! is verified before the model turn is submitted. Exactly one foreground
//! trampoline therefore runs the existing stage body in its own visible
//! console; the consumer waits for that process and reads its retained
//! `StageState`. Reuse, reconciliation and every refusal stay in the model-free
//! consumer, and no second stage is started when the trampoline's outcome is
//! unknown.

use super::*;
use harness_core::process::{
    Cancellation, CommandSpec, Deadline, ExclusiveFileLock, Job, Limits, ProcessIdentity,
};
use std::io::IsTerminal;

const RUNNER_SCHEMA: u32 = 1;
/// The trampoline owns the fork, attachment, turn and cleanup. This margin is
/// above the stage's own native turn bound and never extends model work.
const SURFACE_MARGIN: u64 = 120;
const SURFACE_CLEANUP: Duration = Duration::from_secs(10);

/// `StageAdmission` is serialized by its owner but does not implement
/// deserialization. This exact mirror carries the same public allowances into
/// the trampoline; the parent remains the only owner that begins a stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AdmissionState {
    stage_turns_used: u64,
    stage_turns_limit: u64,
    stage_tokens_used: u64,
    stage_tokens_limit: u64,
    stage_investigations_used: u64,
    stage_investigations_limit: u64,
    stage_context_bytes_used: u64,
    stage_context_bytes_limit: u64,
    record_investigations_used: u64,
    record_investigations_limit: u64,
    record_context_bytes_used: u64,
    record_context_bytes_limit: u64,
    record_tokens_used: u64,
    record_tokens_limit: u64,
}

impl From<&StageAdmission> for AdmissionState {
    fn from(admission: &StageAdmission) -> Self {
        Self {
            stage_turns_used: admission.stage_turns_used,
            stage_turns_limit: admission.stage_turns_limit,
            stage_tokens_used: admission.stage_tokens_used,
            stage_tokens_limit: admission.stage_tokens_limit,
            stage_investigations_used: admission.stage_investigations_used,
            stage_investigations_limit: admission.stage_investigations_limit,
            stage_context_bytes_used: admission.stage_context_bytes_used,
            stage_context_bytes_limit: admission.stage_context_bytes_limit,
            record_investigations_used: admission.record_investigations_used,
            record_investigations_limit: admission.record_investigations_limit,
            record_context_bytes_used: admission.record_context_bytes_used,
            record_context_bytes_limit: admission.record_context_bytes_limit,
            record_tokens_used: admission.record_tokens_used,
            record_tokens_limit: admission.record_tokens_limit,
        }
    }
}

impl From<&AdmissionState> for StageAdmission {
    fn from(state: &AdmissionState) -> Self {
        Self {
            stage_turns_used: state.stage_turns_used,
            stage_turns_limit: state.stage_turns_limit,
            stage_tokens_used: state.stage_tokens_used,
            stage_tokens_limit: state.stage_tokens_limit,
            stage_investigations_used: state.stage_investigations_used,
            stage_investigations_limit: state.stage_investigations_limit,
            stage_context_bytes_used: state.stage_context_bytes_used,
            stage_context_bytes_limit: state.stage_context_bytes_limit,
            record_investigations_used: state.record_investigations_used,
            record_investigations_limit: state.record_investigations_limit,
            record_context_bytes_used: state.record_context_bytes_used,
            record_context_bytes_limit: state.record_context_bytes_limit,
            record_tokens_used: state.record_tokens_used,
            record_tokens_limit: state.record_tokens_limit,
        }
    }
}

/// The exact inputs of one foreground stage. The child re-resolves the role and
/// profile and refuses title drift before starting; it never re-begins stage
/// admission or writes the accepted ledger.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StageRequest {
    schema: u32,
    role: String,
    chain: ChainState,
    prompt: String,
    state_path: PathBuf,
    result_path: PathBuf,
    native_title: String,
    admission: Option<AdmissionState>,
    coordinator: ProcessIdentity,
    coordinator_program: PathBuf,
}

/// Retained evidence that this stage owned a visible console and that the
/// native TUI replaced its caption with the prepared title.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SurfaceEvidence {
    schema: u32,
    role: String,
    identity: harness_core::process::ProcessIdentity,
    program: PathBuf,
    coordinator: ProcessIdentity,
    coordinator_program: PathBuf,
    native_title: String,
    caption_before: Option<String>,
    caption_after: Option<String>,
    console_attached: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StageResult {
    schema: u32,
    state: Option<StageState>,
    error: Option<String>,
    surface: SurfaceEvidence,
}

/// Runs one fresh stage in its own visible console, or delegates a model-free
/// continuation/refusal to the existing stage body in this process.
pub(super) fn run_stage_in_surface(
    chain: &ChainState,
    declaration: &StageDeclaration,
    source: &Boundary,
    prepared: &PreparedStage,
    state_path: &Path,
    prompt: String,
    admission: Option<&StageAdmission>,
) -> io::Result<StageState> {
    let retained = if state_path.exists() {
        Some(read_json::<StageState>(state_path, MAX_REQUEST_BYTES)?)
    } else {
        None
    };
    let fresh = retained
        .as_ref()
        .is_none_or(|state| state.status == StageStatus::NotStarted);
    // Only a fresh stage with a fresh admission can start model work. A
    // retained completed stage, a submitted turn and every refusal are handled
    // by the existing stage body without a terminal.
    if !fresh || admission.is_none() {
        return run_stage(
            chain,
            declaration,
            source,
            prepared,
            state_path,
            prompt,
            admission,
        );
    }
    launch_surface(
        chain,
        declaration,
        source,
        prepared,
        state_path,
        prompt,
        admission.expect("checked above"),
    )
}

fn launch_surface(
    chain: &ChainState,
    declaration: &StageDeclaration,
    source: &Boundary,
    prepared: &PreparedStage,
    state_path: &Path,
    prompt: String,
    admission: &StageAdmission,
) -> io::Result<StageState> {
    let directory = state_path
        .parent()
        .ok_or_else(|| invalid("the stage state path has no directory"))?;
    let request_path = directory.join(format!("{}-surface-request.json", declaration.role));
    let result_path = directory.join(format!("{}-surface-result.json", declaration.role));
    let evidence_path = state_path.with_extension("surface.json");
    // The coordinator holds this lease for the entire spawn/wait. The child
    // verifies that the lease is already held, so invoking the internal stage
    // mode directly cannot turn it into an uncoordinated model-work entry.
    let request_lease = ExclusiveFileLock::try_acquire(&request_path.with_extension("lock"))?
        .ok_or_else(|| {
            invalid(format!(
                "another foreground runner already owns the {} surface request; no second stage was started",
                declaration.role
            ))
        })?;
    remove_stale(&result_path)?;
    remove_stale(&evidence_path)?;
    let coordinator_program = std::env::current_exe()?.canonicalize()?;
    let user = harness_core::process_service::current_user()?;
    let coordinator = harness_core::process_service::ServiceProcess::observe(
        std::process::id(),
        &coordinator_program,
        0,
        &user,
    )?
    .identity();
    let request = StageRequest {
        schema: RUNNER_SCHEMA,
        role: declaration.role.clone(),
        chain: chain.clone(),
        prompt,
        state_path: state_path.to_owned(),
        result_path: result_path.clone(),
        native_title: prepared.native_title.clone(),
        admission: Some(AdmissionState::from(admission)),
        coordinator,
        coordinator_program: coordinator_program.clone(),
    };
    persist(&request_path, &request)?;
    let mut spec = CommandSpec::new(&coordinator_program);
    spec.args = vec![
        "self-reflection".into(),
        "--stage-runner".into(),
        request_path.clone().into_os_string(),
    ];
    spec.current_dir = Some(chain.source.clone());
    // This console is the stage's own surface: the native frontend inherits
    // it, and the runtime verifies the frontend PID and the TUI caption there.
    spec.new_console = Some(prepared.native_title.clone().into());
    spec.env.insert(
        "CODEX_HOME".into(),
        Some(chain.codex_home.as_os_str().to_owned()),
    );
    // The trampoline is an ordinary management process. A reflection marker
    // inherited from the consumer would (correctly) refuse it; executor and
    // lead-session markers could change its own nested command identity.
    for name in [
        REFLECTION_SESSION_ENV,
        "HARNESS_EXECUTOR_SESSION",
        "HARNESS_EXECUTOR_RUN",
        "HARNESS_CONTEXT_ROLE",
        "HARNESS_CONTEXT_REVISION",
        "HARNESS_ORIGINATING_LEAD",
        "HARNESS_LEAD_THREAD",
        "HARNESS_LEAD_RECIPIENT",
        "HARNESS_REFLECTION_HOST",
        "HARNESS_REFLECTION_STAGE",
        "CODEX_THREAD_ID",
        "CODEX_SESSION_ID",
    ] {
        spec.env.insert(name.into(), None);
    }
    let job = Job::new(Limits::default())?;
    let child = match job.spawn(&spec) {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_file(&request_path);
            return Err(invalid(format!(
                "the {} foreground surface did not start: {error}; no fork or turn was submitted",
                declaration.role
            )));
        }
    };
    println!(
        "{}: foreground surface pid {} title {:?}",
        declaration.role,
        child.identity().pid,
        prepared.native_title
    );
    let bound = Duration::from_secs(chain.bound_seconds.saturating_add(SURFACE_MARGIN));
    let waited = job.wait(
        &child,
        Deadline::after(bound)?,
        &Cancellation::default(),
        SURFACE_CLEANUP,
    );
    // The child exits before this drop, and Windows refuses the request-file
    // deletion while this lease is still held.
    drop(request_lease);
    if let Err(error) = fs::remove_file(&request_path)
        && error.kind() != io::ErrorKind::NotFound
    {
        println!(
            "{}: foreground request cleanup failed at {}: {error}",
            declaration.role,
            request_path.display()
        );
    }
    let outcome = match waited {
        Ok(outcome) => outcome,
        Err(error) => {
            return Err(invalid(format!(
                "the {} foreground surface could not be reaped: {error}; the durable stage state at {} is retained and no second runner was started",
                declaration.role,
                state_path.display()
            )));
        }
    };
    let result = if result_path.is_file() {
        Some(read_json::<StageResult>(&result_path, MAX_REQUEST_BYTES)?)
    } else {
        None
    };
    if outcome.reason != harness_core::process::StopReason::Exited || outcome.exit_code != 0 {
        let detail = result
            .as_ref()
            .and_then(|result| result.error.clone())
            .unwrap_or_else(|| format!("exit code {} ({:?})", outcome.exit_code, outcome.reason));
        return Err(invalid(format!(
            "the {} foreground surface ended without a completed stage: {detail}; its durable state at {} and result at {} were retained and no second runner was started",
            declaration.role,
            state_path.display(),
            result_path.display()
        )));
    }
    let result = result.ok_or_else(|| {
        invalid(format!(
            "the {} foreground surface exited without a retained result at {}; the durable stage state at {} is retained and no second runner was started",
            declaration.role,
            result_path.display(),
            state_path.display()
        ))
    })?;
    if result.schema != RUNNER_SCHEMA || result.surface.schema != RUNNER_SCHEMA {
        return Err(invalid(format!(
            "the {} foreground surface retained an unsupported result schema {} / surface schema {}",
            declaration.role, result.schema, result.surface.schema
        )));
    }
    if let Some(error) = result.error {
        return Err(invalid(format!(
            "the {} foreground surface refused or failed: {error}; its durable state at {} and result at {} were retained and no second runner was started",
            declaration.role,
            state_path.display(),
            result_path.display()
        )));
    }
    let state = result.state.ok_or_else(|| {
        invalid(format!(
            "the {} foreground surface retained neither a stage state nor an error",
            declaration.role
        ))
    })?;
    validate_stage(&state, chain, declaration, source, prepared)?;
    if result.surface.role != declaration.role
        || result.surface.native_title != prepared.native_title
        || result.surface.identity != child.identity()
        || result.surface.program != coordinator_program
        || result.surface.coordinator != coordinator
        || result.surface.coordinator_program != coordinator_program
        || !result.surface.console_attached
        || result.surface.caption_before.as_deref() != Some(prepared.native_title.as_str())
    {
        return Err(invalid(format!(
            "the {} foreground surface did not retain its exact visible identity: {:?}",
            declaration.role, result.surface
        )));
    }
    if let Err(error) = fs::remove_file(&result_path)
        && error.kind() != io::ErrorKind::NotFound
    {
        println!(
            "{}: foreground result cleanup failed at {}: {error}",
            declaration.role,
            result_path.display()
        );
    }
    Ok(state)
}

fn remove_stale(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(invalid(format!(
            "the stale surface artifact {} could not be removed before a new stage: {error}",
            path.display()
        ))),
    }
}

/// The hidden child mode: one stage body in the console the parent created.
pub(super) fn execute_stage_runner(request_path: &Path) -> io::Result<i32> {
    let request: StageRequest = read_json(request_path, MAX_REQUEST_BYTES)?;
    if request.schema != RUNNER_SCHEMA {
        return Err(invalid(format!(
            "the foreground stage request schema {} is not {RUNNER_SCHEMA}",
            request.schema
        )));
    }
    // The coordinator holds this lease across spawn and wait. A direct
    // invocation can take it, which proves no coordinator is driving the
    // request; refuse before any fork or model turn.
    if let Some(lease) = ExclusiveFileLock::try_acquire(&request_path.with_extension("lock"))? {
        drop(lease);
        return Err(invalid(format!(
            "the foreground stage request at {} is not owned by a live coordinator; refusing an uncoordinated model stage",
            request_path.display()
        )));
    }
    let user = harness_core::process_service::current_user()?;
    if harness_core::process_service::ServiceProcess::inspect(
        request.coordinator,
        &request.coordinator_program,
        &user,
    )?
    .is_none()
    {
        return Err(invalid(format!(
            "the foreground stage coordinator {} is not the exact live process; refusing an orphaned model stage",
            request.coordinator.pid
        )));
    }
    let (role, declaration, source) = match request.role.as_str() {
        EXECUTOR_STAGE => (
            Role::ExecutorSelfReflection,
            &request.chain.executor_stage,
            &request.chain.executor_source,
        ),
        LEAD_STAGE => (
            Role::LeadSelfReflection,
            &request.chain.lead_stage,
            &request.chain.lead_source,
        ),
        other => {
            return Err(invalid(format!(
                "the foreground stage request names unknown role {other:?}"
            )));
        }
    };
    if declaration.role != request.role {
        return Err(invalid(format!(
            "the foreground stage request role {} does not match its declaration {}",
            request.role, declaration.role
        )));
    }
    // The request must be the durable chain's own stage, not a synthetic
    // relabel: the immutable chain and both retained artifact paths are
    // re-verified before the stage body runs.
    let chain_path = request.chain.state_dir.join("chain.json");
    let durable: ChainState = read_json(&chain_path, MAX_REQUEST_BYTES)?;
    if durable != request.chain {
        return Err(invalid(format!(
            "the {} foreground stage request drifts from the durable chain at {}; no fork or turn was submitted",
            request.role,
            chain_path.display()
        )));
    }
    let expected_state = match request.role.as_str() {
        EXECUTOR_STAGE => request.chain.state_dir.join("executor-stage.json"),
        LEAD_STAGE => request.chain.state_dir.join("lead-stage.json"),
        _ => unreachable!("the role was checked above"),
    };
    let expected_result = expected_state
        .parent()
        .map(|directory| directory.join(format!("{}-surface-result.json", declaration.role)));
    if request.state_path != expected_state
        || expected_result.as_deref() != Some(request.result_path.as_path())
    {
        return Err(invalid(format!(
            "the {} foreground stage request names artifacts outside its chain: state {} result {}",
            request.role,
            request.state_path.display(),
            request.result_path.display()
        )));
    }
    let prepared = prepare_stage(&request.chain, declaration, role)?;
    if prepared.native_title != request.native_title {
        return Err(invalid(format!(
            "the {} foreground surface title drifted from {:?} to {:?}; no fork or turn was submitted",
            request.role, request.native_title, prepared.native_title
        )));
    }
    let caption_before = harness_core::task_control::console_caption()?;
    let console_attached = io::stdout().is_terminal() && caption_before.is_some();
    if !console_attached {
        return Err(invalid(format!(
            "the {} foreground stage has no owned console; refusing model work outside a visible titled surface",
            request.role
        )));
    }
    let admission = request.admission.as_ref().map(StageAdmission::from);
    let outcome = run_stage(
        &request.chain,
        declaration,
        source,
        &prepared,
        &request.state_path,
        request.prompt,
        admission.as_ref(),
    );
    if harness_core::process_service::ServiceProcess::inspect(
        request.coordinator,
        &request.coordinator_program,
        &user,
    )?
    .is_none()
    {
        return Err(invalid(format!(
            "the {} foreground stage coordinator exited before its result was retained; the durable stage state at {} is preserved",
            request.role,
            request.state_path.display()
        )));
    }
    let caption_after = harness_core::task_control::console_caption().ok().flatten();
    let program = std::env::current_exe()?.canonicalize()?;
    let identity = harness_core::process_service::ServiceProcess::observe(
        std::process::id(),
        &program,
        0,
        &user,
    )?
    .identity();
    let surface = SurfaceEvidence {
        schema: RUNNER_SCHEMA,
        role: request.role.clone(),
        identity,
        program,
        coordinator: request.coordinator,
        coordinator_program: request.coordinator_program.clone(),
        native_title: prepared.native_title.clone(),
        caption_before,
        caption_after,
        console_attached,
    };
    persist(&request.state_path.with_extension("surface.json"), &surface)?;
    let result = match &outcome {
        Ok(state) => StageResult {
            schema: RUNNER_SCHEMA,
            state: Some(state.clone()),
            error: None,
            surface,
        },
        Err(error) => StageResult {
            schema: RUNNER_SCHEMA,
            state: None,
            error: Some(error.to_string()),
            surface,
        },
    };
    persist(&request.result_path, &result)?;
    match outcome {
        Ok(_) => Ok(0),
        Err(error) => {
            eprintln!("self-reflection: {} surface failed: {error}", request.role);
            Ok(1)
        }
    }
}
