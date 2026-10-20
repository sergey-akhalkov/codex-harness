//! Connect native accepted-closure records to the existing fork runner.
//! Activation and ordinary acceptance remain with their lifecycle owners.

use super::*;
use harness_core::delivery_reflection::{
    BudgetLimit, DeliveryState as RecordedDeliveryState, LifecycleStatus, ReflectionController,
    ReflectionRecord, Stage, StageAdmission, StageRecord, StageState as RecordedStageState,
    UsageDelta,
};

pub(super) fn execute(request: Request, board: &Path) -> io::Result<i32> {
    execute_inner(request, board, false)
}

/// The explicit `delivery-improvement process` entry point. Only this route may
/// reconcile the exact acknowledged pre-fork rejection described by
/// [`retained_pre_fork_rejection`]; the low-level runner keeps its refusal.
pub(super) fn execute_with_pre_fork_recovery(request: Request, board: &Path) -> io::Result<i32> {
    execute_inner(request, board, true)
}

fn execute_inner(
    request: Request,
    board: &Path,
    recover_pre_fork_rejection: bool,
) -> io::Result<i32> {
    let controller = ReflectionController::new(
        crate::feedback_cli::discover_bd_in(&request.codex_home)?,
        board,
        &request.codex_home,
    );
    super::gate::ReflectionGate::load(&request.source)?.ensure_enabled()?;
    let polled = controller.poll()?;
    if let Some(recovery) = polled.recovery {
        return Err(invalid(format!(
            "accepted board requires reconciliation before reflection: {}",
            recovery.detail
        )));
    }
    let record = controller.record(&request.acceptance)?.ok_or_else(|| {
        invalid("the requested acceptance is not a retained eligible local closure")
    })?;
    let state = controller
        .stored_state()?
        .ok_or_else(|| invalid("the accepted reflection lifecycle disappeared"))?;
    if state.lifecycle != LifecycleStatus::Active {
        return Err(invalid(
            "delivery reflection is stopped; enable it under the current managed lead before processing retained work",
        ));
    }
    validate_request(&request, &record, &state.binding.lead_session)?;
    let (chain, _lease) = prepare_chain_with_acceptance(
        request,
        Some(record.binding.clone()),
        Some((&state.binding.lead_session, &state.retired_leads)),
    )?;
    check_binding(&controller, &chain)?;
    preflight_lead(&chain)?;
    let executor = run_recorded_stage(
        &controller,
        &chain,
        Stage::Executor,
        recover_pre_fork_rejection,
        |admission| run_executor(&chain, admission),
    )?;
    check_binding(&controller, &chain)?;
    // The executor result is retained before this fresh read. A concurrent
    // on-to-off change therefore leaves the lead stage pending instead of
    // starting another model call or discarding the executor outcome.
    super::gate::ReflectionGate::load(&chain.source)?.ensure_enabled()?;
    preflight_lead(&chain)?;
    wait_for_lead_turn(&chain)?;
    super::gate::ReflectionGate::load(&chain.source)?.ensure_enabled()?;
    let lead = run_recorded_stage(
        &controller,
        &chain,
        Stage::Lead,
        recover_pre_fork_rejection,
        |admission| run_lead(&chain, &executor, admission),
    )?;
    check_binding(&controller, &chain)?;
    let delivery = match deliver_synthesis(&chain, &executor, &lead) {
        Ok(delivery) => delivery,
        Err(error) => {
            if let Err(retention) =
                controller.mark_delivery_unknown(&chain.acceptance, error.to_string())
            {
                return Err(invalid(format!(
                    "{error}; retaining delivery uncertainty: {retention}"
                )));
            }
            return Err(error);
        }
    };
    if delivery.state == "delivered" {
        // The native delivery owner established the message in the real lead's
        // conversation. Submission or transport acknowledgement is insufficient.
        controller.observe_delivery(&chain.acceptance, &delivery.client_message_id)?;
    } else {
        controller.mark_delivery_unknown(
            &chain.acceptance,
            format!(
                "native delivery remains {}; retain and reconcile the existing message",
                delivery.state
            ),
        )?;
    }
    print_result(&chain, &executor, &lead, &delivery)
}

fn validate_request(
    request: &Request,
    record: &ReflectionRecord,
    current_lead: &str,
) -> io::Result<()> {
    let binding = &record.binding;
    if request.task != binding.task
        || request.run != binding.run
        || request.acceptance != record.acceptance_id
        || Some(request.executor_source.thread.as_str()) != binding.executor_session.as_deref()
        || Some(request.executor_source.last_turn.as_str()) != binding.executor_turn.as_deref()
        || request.lead_source.thread != binding.lead_session
        || request.lead_return.thread != current_lead
        || Some(request.lead_source.last_turn.as_str()) != binding.lead_turn.as_deref()
    {
        return Err(invalid(
            "the request does not match the accepted task/run and exact immutable executor/lead source turns, or its return thread is not the current lifecycle owner; no fork was started",
        ));
    }
    Ok(())
}

fn check_binding(controller: &ReflectionController, chain: &ChainState) -> io::Result<()> {
    let checked =
        controller.check_active_binding(&chain.lead_return.thread, Some(&chain.acceptance))?;
    if !checked.is_active() {
        return Err(invalid(format!(
            "accepted closure is no longer eligible: {}",
            checked.detail
        )));
    }
    Ok(())
}

fn recorded_stage(record: &ReflectionRecord, stage: Stage) -> &StageRecord {
    match stage {
        Stage::Executor => &record.executor,
        Stage::Lead => &record.lead,
    }
}

fn stage_state_path(state_dir: &Path, stage: Stage) -> PathBuf {
    state_dir.join(match stage {
        Stage::Executor => "executor-stage.json",
        Stage::Lead => "lead-stage.json",
    })
}

/// Proves that the retained blocked stage is its original fork paused before
/// the first turn. Only this shape may resume the existing attempt; a pause
/// after a submitted turn, a missing native artifact or any identity drift
/// keeps the ordinary explicit-retry path.
pub(super) fn resumable_pre_turn_pause(
    controller: &ReflectionController,
    record: &ReflectionRecord,
    stage: Stage,
    declaration: &StageDeclaration,
) -> io::Result<bool> {
    let recorded = recorded_stage(record, stage);
    if recorded.state != RecordedStageState::Blocked || recorded.pause.is_none() {
        return Ok(false);
    }
    let state_dir = controller
        .state_file()?
        .with_extension("")
        .join(&record.acceptance_id);
    let path = stage_state_path(&state_dir, stage);
    if !path.is_file() {
        return Ok(false);
    }
    let native: StageState = read_json(&path, MAX_REQUEST_BYTES)?;
    let (role, source_thread, source_turn) = match stage {
        Stage::Executor => (
            EXECUTOR_STAGE,
            record.binding.executor_session.as_deref(),
            record.binding.executor_turn.as_deref(),
        ),
        Stage::Lead => (
            LEAD_STAGE,
            Some(record.binding.lead_session.as_str()),
            record.binding.lead_turn.as_deref(),
        ),
    };
    let (Some(source_thread), Some(source_turn)) = (source_thread, source_turn) else {
        return Ok(false);
    };
    let usage = stage_usage_delta(&native);
    let pre_turn_pause = native.stage == role
        && native.task == record.binding.task
        && native.run == record.binding.run
        && native.acceptance == record.acceptance_id
        && native.source_thread == source_thread
        && native.source_last_turn == source_turn
        && native.status == StageStatus::BudgetPaused
        && native.turn_id.is_none()
        && native.fork.is_some()
        && native.budget_pause.as_ref().is_some_and(|pause| {
            matches!(
                pause.limit,
                BudgetLimit::StageContextBytes | BudgetLimit::RecordContextBytes
            )
        })
        && native.usage_known
        && usage.turns == 0
        && usage.tokens == 0
        && native.work_known
        && native.source_context_bytes > 0
        && native.work.context_bytes == native.source_context_bytes
        && recorded.usage.attempts >= 1
        && recorded.usage.turns == 0
        && recorded.usage.tokens == 0
        && recorded.usage.investigations == 0
        && recorded.usage.context_bytes == native.source_context_bytes;
    if !pre_turn_pause {
        return Ok(false);
    }
    let (Some(executor_session), Some(executor_turn), Some(lead_turn)) = (
        record.binding.executor_session.as_deref(),
        record.binding.executor_turn.as_deref(),
        record.binding.lead_turn.as_deref(),
    ) else {
        return Ok(false);
    };
    let executor_source = Boundary {
        thread: executor_session.to_owned(),
        last_turn: executor_turn.to_owned(),
    };
    let lead_source = Boundary {
        thread: record.binding.lead_session.clone(),
        last_turn: lead_turn.to_owned(),
    };
    let visible_title = stage_visible_title(
        declaration,
        &record.binding.task,
        &record.binding.run,
        &executor_source,
        &lead_source,
    );
    if native.role_revision != declaration.revision
        || native.profile != declaration.profile
        || native.model != declaration.model
        || native.provider != declaration.provider
        || native.effort != declaration.effort
        || native.visible_title != visible_title
    {
        return Err(invalid(format!(
            "the retained zero-turn {} pause names a different role revision, model, provider, effort or visible identity than the current accepted-stage declaration; refusing to resume a drifted native attempt; nothing was changed",
            stage.as_str()
        )));
    }
    Ok(true)
}

/// An unknown stage whose ledger records no model work at all. Only this shape
/// can be reconciled from an acknowledged pre-fork rejection; a stage that
/// already recorded a turn or tokens is never treated as zero work.
fn recorded_no_model_work(slot: &StageRecord) -> bool {
    slot.state == RecordedStageState::Unknown
        && slot.usage.attempts >= 1
        && slot.usage.turns == 0
        && slot.usage.tokens == 0
        && slot.usage.investigations == 0
        && slot.usage.context_bytes == 0
        && slot.outcome.is_none()
        && slot.pause.is_none()
}

/// Selects the exact retained stage the explicit process may reconcile: an
/// unknown stage with no recorded model work whose native artifact carries an
/// acknowledged pre-fork rejection. It performs no write and starts no work.
pub(super) fn retained_pre_fork_rejection(
    controller: &ReflectionController,
    record: &ReflectionRecord,
) -> Option<Stage> {
    if record.delivery == RecordedDeliveryState::Consumed {
        return None;
    }
    let state_dir = controller
        .state_file()
        .ok()?
        .with_extension("")
        .join(&record.acceptance_id);
    [Stage::Executor, Stage::Lead].into_iter().find(|stage| {
        if !recorded_no_model_work(recorded_stage(record, *stage)) {
            return false;
        }
        fs::read(stage_state_path(&state_dir, *stage))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StageState>(&bytes).ok())
            .is_some_and(|native| pre_fork_rejection(&native).is_ok())
    })
}

/// Reconciles exactly one acknowledged native `thread/fork` rejection before
/// any fork or turn existed. The native artifact is checked against the
/// retained chain identity and must contain no result, fork, turn, usage or
/// work signal. The old stage and surface evidence are preserved before the
/// ordinary runner starts the next attempt.
fn recover_pre_fork_rejection(
    controller: &ReflectionController,
    chain: &ChainState,
    stage: Stage,
    state_path: &Path,
) -> io::Result<()> {
    let (declaration, source, role) = match stage {
        Stage::Executor => (
            &chain.executor_stage,
            &chain.executor_source,
            Role::ExecutorSelfReflection,
        ),
        Stage::Lead => (
            &chain.lead_stage,
            &chain.lead_source,
            Role::LeadSelfReflection,
        ),
    };
    let prepared = prepare_stage(chain, declaration, role)?;
    let native: StageState = read_json(state_path, MAX_REQUEST_BYTES)?;
    validate_stage(&native, chain, declaration, source, &prepared)?;
    let _rejection = pre_fork_rejection(&native)?;

    let state = controller.stored_state()?.ok_or_else(|| {
        invalid("the accepted closure reflection state disappeared; no recovery was started")
    })?;
    let record = state.records.get(&chain.acceptance).ok_or_else(|| {
        invalid("the accepted closure record disappeared; no recovery was started")
    })?;
    if record.delivery == RecordedDeliveryState::Consumed {
        return Err(invalid(format!(
            "{} reflection belongs to an already consumed delivery; no stage may run again",
            stage.as_str()
        )));
    }
    let slot = recorded_stage(record, stage);
    if !recorded_no_model_work(slot) {
        return Err(invalid(format!(
            "{} reflection is {:?} with attempts {}, turns {}, tokens {}, investigations {} and context {}; only an unknown stage with no recorded model work can reconcile an acknowledged pre-fork rejection",
            stage.as_str(),
            slot.state,
            slot.usage.attempts,
            slot.usage.turns,
            slot.usage.tokens,
            slot.usage.investigations,
            slot.usage.context_bytes,
        )));
    }
    if slot.usage.attempts >= state.limits.max_attempts_per_stage {
        return Err(invalid(format!(
            "{} reflection reached its {} attempt allowance; the acknowledged pre-fork rejection cannot start another attempt",
            stage.as_str(),
            state.limits.max_attempts_per_stage,
        )));
    }
    let attempt = slot.usage.attempts;
    if !slot.usage_reconciled {
        controller.record_unknown_usage(&chain.acceptance, stage, UsageDelta::default())?;
    }
    if !slot.work_reconciled {
        controller.record_observed_work(&chain.acceptance, stage, StageWork::default())?;
    }
    let archive_stem = format!(
        "{}-attempt-{attempt}",
        state_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("stage")
    );
    let archived = archive_surface_evidence(state_path, declaration, &archive_stem)?;
    controller.retry_stage(&chain.acceptance, stage)?;
    println!(
        "{}: reconciled the acknowledged native thread/fork rejection before any fork or turn: preserved attempt {attempt} and surface evidence{}; recorded zero usage and zero observed work, and admitted exactly one continuation",
        stage.as_str(),
        archived
            .iter()
            .map(|path| format!(" {}", path.display()))
            .collect::<String>()
    );
    Ok(())
}

fn archive_surface_evidence(
    state_path: &Path,
    declaration: &StageDeclaration,
    archive_stem: &str,
) -> io::Result<Vec<PathBuf>> {
    let directory = state_path
        .parent()
        .ok_or_else(|| invalid("the stage state path has no directory"))?;
    let mut archived = Vec::new();
    for (source, suffix) in [
        (state_path.with_extension("surface.json"), ".surface.json"),
        (
            directory.join(format!("{}-surface-result.json", declaration.role)),
            "-surface-result.json",
        ),
    ] {
        if !source.is_file() {
            continue;
        }
        let mut destination = directory.join(format!("{archive_stem}{suffix}"));
        if destination.exists() {
            destination = directory.join(format!("{archive_stem}-{}{suffix}", now_ms()));
        }
        fs::rename(&source, &destination).map_err(|error| {
            invalid(format!(
                "the prior {} surface evidence at {} could not be preserved at {} before recovery: {error}",
                declaration.role,
                source.display(),
                destination.display()
            ))
        })?;
        archived.push(destination);
    }
    Ok(archived)
}

fn run_recorded_stage(
    controller: &ReflectionController,
    chain: &ChainState,
    stage: Stage,
    allow_pre_fork_recovery: bool,
    run: impl FnOnce(Option<&StageAdmission>) -> io::Result<StageState>,
) -> io::Result<StageState> {
    let mut record = controller
        .record(&chain.acceptance)?
        .ok_or_else(|| invalid("the accepted closure record disappeared; no stage was started"))?;
    let state_path = stage_state_path(&chain.state_dir, stage);
    match recorded_stage(&record, stage).state {
        RecordedStageState::Pending => {}
        RecordedStageState::InFlight | RecordedStageState::Retained => {
            let recorded = recorded_stage(&record, stage);
            if !state_path.is_file() {
                return Err(invalid(format!(
                    "{} reflection has a recorded attempt but its native state is missing at {}; preserve the record and reconcile instead of starting another fork",
                    stage.as_str(),
                    state_path.display()
                )));
            }
            let native: StageState = read_json(&state_path, MAX_REQUEST_BYTES)?;
            if native.status == StageStatus::NotStarted
                || (recorded.state == RecordedStageState::Retained && !stage_is_completed(&native))
            {
                return Err(invalid(
                    "the recorded attempt lacks its retained native completion; no new fork was started",
                ));
            }
            if recorded.state == RecordedStageState::Retained && !recorded.usage_reconciled {
                return Err(invalid(
                    "retained reflection usage remains unknown; reconcile its native cost before continuing",
                ));
            }
            if recorded.state == RecordedStageState::Retained && !recorded.work_reconciled {
                return Err(invalid(
                    "retained reflection observed work remains unknown; reconcile its investigation/context work before continuing",
                ));
            }
        }
        RecordedStageState::Unknown if allow_pre_fork_recovery => {
            if !state_path.is_file() {
                return Err(invalid(format!(
                    "{} reflection is unknown but its native state is missing at {}; preserve the record and reconcile instead of starting another fork",
                    stage.as_str(),
                    state_path.display()
                )));
            }
            recover_pre_fork_rejection(controller, chain, stage, &state_path)?;
            record = controller.record(&chain.acceptance)?.ok_or_else(|| {
                invalid("the accepted closure record disappeared during pre-fork recovery")
            })?;
            if recorded_stage(&record, stage).state != RecordedStageState::Pending {
                return Err(invalid(format!(
                    "{} reflection did not return to pending after reconciling the acknowledged pre-fork rejection; no new fork was started",
                    stage.as_str()
                )));
            }
        }
        other => {
            return Err(invalid(format!(
                "{} reflection is {other:?}; its existing attempt requires explicit recovery",
                stage.as_str()
            )));
        }
    }
    let recorded = recorded_stage(&record, stage);
    let role = match stage {
        Stage::Executor => Role::ExecutorSelfReflection,
        Stage::Lead => Role::LeadSelfReflection,
    };
    let starts_new_attempt = recorded.state == RecordedStageState::Pending;
    let _admission = admit_stage(chain, role, starts_new_attempt)?;
    if starts_new_attempt && state_path.is_file() {
        let prior: StageState = read_json(&state_path, MAX_REQUEST_BYTES)?;
        if prior.status != StageStatus::NotStarted {
            let stem = state_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("stage");
            let attempt = format!("{stem}-attempt-{}", recorded.usage.attempts);
            let archive_stem = if state_path
                .with_file_name(format!("{attempt}.json"))
                .exists()
            {
                format!("{attempt}-{}", now_ms())
            } else {
                attempt
            };
            let archive = state_path.with_file_name(format!("{archive_stem}.json"));
            fs::rename(&state_path, &archive).map_err(|error| {
                invalid(format!(
                    "the prior {} native attempt could not be preserved at {} before retry: {error}",
                    stage.as_str(),
                    archive.display()
                ))
            })?;
            let stage_role = match stage {
                Stage::Executor => chain.executor_stage.role.as_str(),
                Stage::Lead => chain.lead_stage.role.as_str(),
            };
            for kind in ["native", "reconcile"] {
                let source = state_path
                    .parent()
                    .ok_or_else(|| invalid("the stage state path has no directory"))?
                    .join(format!("{stage_role}-{kind}"));
                if !source.is_dir() {
                    continue;
                }
                let archive = source.with_file_name(format!("{archive_stem}-{kind}"));
                fs::rename(&source, &archive).map_err(|error| {
                    invalid(format!(
                        "the prior {} {kind} attempt could not be preserved at {} before retry: {error}",
                        stage.as_str(),
                        archive.display()
                    ))
                })?;
            }
        }
    }
    let admission = if starts_new_attempt {
        controller.begin_stage(&chain.acceptance, stage)?.admission
    } else {
        // Recovery reuses the retained attempt: it must not begin a new one,
        // but it still needs the cumulative stage/record allowance captured
        // before the lost surface so a restored turn cannot exceed it.
        let state = controller.stored_state()?.ok_or_else(|| {
            invalid("the accepted closure reflection state disappeared; no stage was admitted")
        })?;
        StageAdmission::new(&state.limits, &recorded.usage, &record.usage_totals())
    };
    let state = match run(Some(&admission)) {
        Ok(state) => state,
        Err(error) => {
            let native = fs::read(&state_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<StageState>(&bytes).ok());
            if native
                .as_ref()
                .is_some_and(|native| native.status == StageStatus::TurnSubmitted)
            {
                // The native turn identity is retained. Keep the accepted
                // ledger in flight so the ordinary process can restore the
                // surface and reconcile that exact turn instead of refusing
                // the record as a generic unknown attempt.
                return Err(error);
            }
            let pre_submission_loss_retained = native.as_ref().is_some_and(|native| {
                native.status == StageStatus::VisibilityLost
                    && native.turn_id.is_none()
                    && native.fork.is_some()
            });
            let pre_turn_pause_retained = native.as_ref().is_some_and(|native| {
                native.status == StageStatus::BudgetPaused
                    && native.turn_id.is_none()
                    && native.fork.is_some()
            });
            if (pre_submission_loss_retained || pre_turn_pause_retained)
                && (starts_new_attempt || recorded.state == RecordedStageState::InFlight)
            {
                // The exact fork is known and no turn was submitted. Keep the
                // accepted ledger in flight so the ordinary process can
                // restore visibility and admit the initial turn once, instead
                // of turning a safely retryable loss into an unknown attempt.
                return Err(error);
            }
            if recorded.state != RecordedStageState::Retained {
                let usage = submitted_stage_usage(&state_path);
                let (work, work_reconciled) = submitted_stage_work(&state_path);
                if let Err(retention) = controller.mark_stage_unknown_with_work_and_usage(
                    &chain.acceptance,
                    stage,
                    error.to_string(),
                    usage,
                    work,
                    work_reconciled,
                ) {
                    return Err(invalid(format!(
                        "{error}; retaining stage uncertainty: {retention}"
                    )));
                }
            }
            return Err(error);
        }
    };
    if let Some(pause) = stage_budget_pause(&state).cloned() {
        let usage = stage_usage_delta(&state);
        let work = stage_work(&state);
        let usage_reconciled = stage_usage_known(&state);
        let work_reconciled = stage_work_known(&state);
        let usage_unknown_reason = (!usage_reconciled)
            .then(|| stage_unknown_reason(&state))
            .flatten();
        let work_unknown_reason = (!work_reconciled)
            .then_some(state.work_unknown_reason.as_deref())
            .flatten();
        controller.pause_stage(
            &chain.acceptance,
            stage,
            pause.clone(),
            usage,
            work,
            usage_reconciled,
            work_reconciled,
            usage_unknown_reason,
            work_unknown_reason,
        )?;
        return Err(invalid(format!(
            "{} evidence is retained at {}; no dependent work was admitted",
            pause.summary(),
            state_path.display()
        )));
    }
    let outcome = stage_outcome(&state);
    if recorded.state == RecordedStageState::Retained {
        let retained = recorded
            .outcome
            .as_ref()
            .ok_or_else(|| invalid("retained stage lacks its outcome"))?;
        if !stage_is_completed(&state)
            || !stage_usage_known(&state)
            || retained.finding != outcome.finding
            || retained.summary != outcome.summary.trim()
            || retained.coverage != outcome.coverage
            || retained.session != outcome.session
        {
            return Err(invalid(
                "retained native result differs from the accepted closure ledger; no result was reused",
            ));
        }
        return Ok(state);
    }
    let usage = stage_usage_delta(&state);
    let work = stage_work(&state);
    let mut details = Vec::new();
    if !stage_usage_known(&state) {
        details.push(
            stage_unknown_reason(&state)
                .unwrap_or("the native new-turn usage is unavailable")
                .to_owned(),
        );
    }
    if !stage_work_known(&state) {
        details.push(
            state
                .work_unknown_reason
                .as_deref()
                .unwrap_or("the native investigation/context work is unavailable")
                .to_owned(),
        );
    }
    let detail = details.join("; ");
    if !detail.is_empty() {
        if stage_usage_known(&state) {
            controller.retain_stage_with_work_reconciliation(
                &chain.acceptance,
                stage,
                outcome,
                usage,
                true,
                work,
                false,
                detail.clone(),
            )?;
        } else {
            controller.retain_stage_unreconciled_with_work(
                &chain.acceptance,
                stage,
                outcome,
                usage,
                work,
                stage_work_known(&state),
                detail.clone(),
            )?;
        }
        return Err(invalid(format!(
            "{} result retained, but usage remains unknown: {detail}",
            stage.as_str()
        )));
    }
    controller.retain_stage_with_work(&chain.acceptance, stage, outcome, usage, work)?;
    Ok(state)
}

/// Accounts for a turn submitted before the stage ended without a reconciled
/// completion. Token fields stay exactly as the native state recorded them; an
/// unknown total is not replaced with an estimate.
fn submitted_stage_usage(state_path: &Path) -> UsageDelta {
    let Ok(bytes) = fs::read(state_path) else {
        return UsageDelta::default();
    };
    let Ok(state) = serde_json::from_slice::<StageState>(&bytes) else {
        return UsageDelta::default();
    };
    UsageDelta {
        turns: u64::from(state.turn_id.is_some()),
        tokens: stage_usage_delta(&state).tokens,
    }
}

fn submitted_stage_work(state_path: &Path) -> (StageWork, bool) {
    let Ok(bytes) = fs::read(state_path) else {
        return (StageWork::default(), false);
    };
    let Ok(state) = serde_json::from_slice::<StageState>(&bytes) else {
        return (StageWork::default(), false);
    };
    (state.work, state.work_known)
}
