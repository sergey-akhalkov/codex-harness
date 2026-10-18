//! Connect native accepted-closure records to the existing fork runner.
//! Activation and ordinary acceptance remain with their lifecycle owners.

use super::*;
use harness_core::delivery_reflection::{
    ReflectionController, ReflectionRecord, Stage, StageState as RecordedStageState,
};

pub(super) fn execute(request: Request, board: &Path) -> io::Result<i32> {
    let controller = ReflectionController::new(
        crate::feedback_cli::discover_bd_in(&request.codex_home)?,
        board,
        &request.codex_home,
    );
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
    validate_request(&request, &record)?;
    let (chain, _lease) = prepare_chain_with_acceptance(request, Some(record.binding.clone()))?;
    check_binding(&controller, &chain)?;
    preflight_lead(&chain)?;
    let executor = run_recorded_stage(&controller, &chain, Stage::Executor, || {
        run_executor(&chain)
    })?;
    check_binding(&controller, &chain)?;
    preflight_lead(&chain)?;
    wait_for_lead_turn(&chain)?;
    let lead = run_recorded_stage(&controller, &chain, Stage::Lead, || {
        run_lead(&chain, &executor)
    })?;
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

fn validate_request(request: &Request, record: &ReflectionRecord) -> io::Result<()> {
    let binding = &record.binding;
    if request.task != binding.task
        || request.run != binding.run
        || request.acceptance != record.acceptance_id
        || Some(request.executor_source.thread.as_str()) != binding.executor_session.as_deref()
        || Some(request.executor_source.last_turn.as_str()) != binding.executor_turn.as_deref()
        || request.lead_source.thread != binding.lead_session
        || request.lead_return.thread != binding.lead_session
        || Some(request.lead_source.last_turn.as_str()) != binding.lead_turn.as_deref()
    {
        return Err(invalid(
            "the request does not match the accepted task/run and exact executor/lead turn binding; no fork was started",
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

fn run_recorded_stage(
    controller: &ReflectionController,
    chain: &ChainState,
    stage: Stage,
    run: impl FnOnce() -> io::Result<StageState>,
) -> io::Result<StageState> {
    let record = controller
        .record(&chain.acceptance)?
        .ok_or_else(|| invalid("the accepted closure record disappeared; no stage was started"))?;
    let (recorded, state_path) = match stage {
        Stage::Executor => (
            &record.executor,
            chain.state_dir.join("executor-stage.json"),
        ),
        Stage::Lead => (&record.lead, chain.state_dir.join("lead-stage.json")),
    };
    match recorded.state {
        RecordedStageState::Pending => {}
        RecordedStageState::InFlight | RecordedStageState::Retained => {
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
        }
        other => {
            return Err(invalid(format!(
                "{} reflection is {other:?}; its existing attempt requires explicit recovery",
                stage.as_str()
            )));
        }
    }
    let role = match stage {
        Stage::Executor => Role::ExecutorSelfReflection,
        Stage::Lead => Role::LeadSelfReflection,
    };
    let _admission = admit_stage(chain, role)?;
    if recorded.state == RecordedStageState::Pending {
        controller.begin_stage(&chain.acceptance, stage)?;
    }
    let state = match run() {
        Ok(state) => state,
        Err(error) => {
            if recorded.state != RecordedStageState::Retained
                && let Err(retention) =
                    controller.mark_stage_unknown(&chain.acceptance, stage, error.to_string())
            {
                return Err(invalid(format!(
                    "{error}; retaining stage uncertainty: {retention}"
                )));
            }
            return Err(error);
        }
    };
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
    if !stage_usage_known(&state) {
        let detail =
            stage_unknown_reason(&state).unwrap_or("the native new-turn usage is unavailable");
        controller.retain_stage_unreconciled(&chain.acceptance, stage, outcome, usage, detail)?;
        return Err(invalid(format!(
            "{} result retained, but usage remains unknown: {detail}",
            stage.as_str()
        )));
    }
    controller.retain_stage(&chain.acceptance, stage, outcome, usage)?;
    Ok(state)
}
