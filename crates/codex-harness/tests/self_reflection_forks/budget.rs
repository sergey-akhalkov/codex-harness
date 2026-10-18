//! Bounded cumulative reflection cost through the accepted-stage controller.
//!
//! These checks use the real accepted closure, native fork runner and fixture
//! app-server. They prove that declared allowances survive the second stage and
//! a repeated resume, and that unknown usage never unlocks dependent work.

use super::*;
use harness_core::{
    board_cli,
    delivery_reflection::{
        AcceptanceBinding, ActivationRequest, DeliveryState, Finding, JournalPolicy, Purpose,
        ReflectionController, ReflectionLimits, StageState, acceptance_metadata,
    },
};

const BUDGET_ACTOR: &str = "reflection-fixture-lead";

fn accepted_with_limits(
    workspace: &Workspace,
    limits: ReflectionLimits,
) -> (ReflectionController, Value, String) {
    let (controller, task) = super::accepted::prepare(workspace);
    controller
        .activate(&ActivationRequest {
            lead_session: LEAD_THREAD.to_owned(),
            run: None,
            boundary: None,
            journal: JournalPolicy::RequireExisting,
            limits: Some(limits),
        })
        .unwrap();
    let binding = AcceptanceBinding::new(
        &task,
        "accepted-run",
        "accepted-assignment",
        "accepted-result",
        "accepted-revision",
        vec![EXECUTOR_SOURCE.to_owned()],
        LEAD_THREAD,
        BUDGET_ACTOR,
        Purpose::Ordinary,
    )
    .with_turns(
        Some(EXECUTOR_SOURCE.to_owned()),
        Some(EXECUTOR_SOURCE_TURN.to_owned()),
        Some(LEAD_SOURCE_TURN.to_owned()),
    );
    board_cli::close_accepted(
        Path::new("bd.exe"),
        &workspace.checkout,
        BUDGET_ACTOR,
        &task,
        &acceptance_metadata(&binding).unwrap(),
    )
    .unwrap();
    let polled = controller.poll().unwrap();
    assert_eq!(polled.accepted, 1, "{polled:?}");
    let acceptance = polled.jobs[0].acceptance_id.clone();
    let mut request = workspace.request("accepted-run");
    request["task"] = json!(task);
    request["acceptance"] = json!(acceptance);
    request["leadSource"]["thread"] = json!(LEAD_THREAD);
    workspace.lead_state.lock().unwrap().source_turn = Some(LEAD_SOURCE_TURN.to_owned());
    (controller, request, acceptance)
}

fn run_budget_chain(
    workspace: &Workspace,
    request: &Value,
    executor_outcome: &str,
    lead_outcome: &str,
    executor_result: &str,
    lead_result: &str,
) -> ConsoleOutcome {
    wait_session(spawn_chain_in_board(
        workspace,
        request,
        executor_outcome,
        lead_outcome,
        executor_result,
        lead_result,
        Some(&workspace.checkout),
    ))
}

#[test]
fn token_exhaustion_blocks_the_lead_and_resume_keeps_the_first_stage_usage() {
    let workspace = Workspace::new("budget-token-exhaustion", LeadMode::Delivered);
    let limits = ReflectionLimits {
        max_attempts_per_stage: 1,
        max_turns_per_stage: 1,
        max_tokens_per_stage: 120,
        max_tokens_per_record: 120,
        ..ReflectionLimits::default()
    };
    let (controller, request, acceptance) = accepted_with_limits(&workspace, limits);
    let first = run_budget_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&first, "token allowance across both stages");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert_eq!(record.executor.usage.attempts, 1);
    assert_eq!(record.executor.usage.turns, 1);
    assert_eq!(record.executor.usage.tokens, 120);
    assert_eq!(record.lead.state, StageState::Pending);
    assert_eq!(record.lead.usage.attempts, 0);
    assert_eq!(record.lead.usage.turns, 0);
    assert_eq!(record.lead.usage.tokens, 0);
    assert_eq!(record.delivery, DeliveryState::Pending);
    let retained = record.executor.outcome.as_ref().unwrap();
    assert_eq!(retained.finding, Finding::Findings);
    assert_eq!(retained.summary, EXECUTOR_RESULT);

    // The exact new turn is billed in full; the copied source's historical
    // cumulative total is retained as evidence, not charged a second time.
    let executor = stage(&workspace, "executor-stage");
    assert_eq!(executor["usageKnown"], json!(true), "{executor}");
    assert_eq!(executor["usage"]["input_tokens"], json!(100), "{executor}");
    assert_eq!(executor["usage"]["output_tokens"], json!(20), "{executor}");
    assert_eq!(executor["usage"]["total_tokens"], json!(120), "{executor}");
    assert_eq!(
        executor["cumulativeThread"]["total_tokens"],
        json!(570),
        "{executor}"
    );
    assert_eq!(
        executor["copiedSourceTurns"],
        json!([EXECUTOR_SOURCE_TURN]),
        "{executor}"
    );

    let calls = request_counts(&workspace);
    let repeated = run_budget_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&repeated, "token allowance across both stages");
    assert_eq!(request_counts(&workspace), calls);
    let repeated_record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(repeated_record, record);
    assert_eq!(
        repeated_record.executor.outcome.as_ref().unwrap().summary,
        EXECUTOR_RESULT
    );
}

#[test]
fn low_turn_and_attempt_allowances_retain_both_stages_and_resume_without_recount() {
    let workspace = Workspace::new("budget-turn-attempt", LeadMode::Delivered);
    let limits = ReflectionLimits {
        max_attempts_per_stage: 1,
        max_turns_per_stage: 1,
        max_tokens_per_stage: 1_000,
        max_tokens_per_record: 1_000,
        ..ReflectionLimits::default()
    };
    let (controller, request, acceptance) = accepted_with_limits(&workspace, limits);
    let completed = run_budget_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        "NO FINDING",
        LEAD_RESULT,
    );
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert_eq!(record.lead.state, StageState::Retained);
    assert_eq!(record.delivery, DeliveryState::Consumed);
    assert_eq!(record.executor.usage.attempts, 1);
    assert_eq!(record.lead.usage.attempts, 1);
    assert_eq!(record.executor.usage.turns, 1);
    assert_eq!(record.lead.usage.turns, 1);
    assert_eq!(record.executor.usage.tokens, 120);
    assert_eq!(record.lead.usage.tokens, 240);
    assert_eq!(record.usage_totals().tokens, 360);
    assert_eq!(
        record.executor.outcome.as_ref().unwrap().finding,
        Finding::NoFinding
    );
    assert_eq!(
        record.lead.outcome.as_ref().unwrap().finding,
        Finding::Findings
    );

    let calls = request_counts(&workspace);
    assert_eq!(calls, (1, 1, 1, 1));
    let replay = run_budget_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        "NO FINDING",
        LEAD_RESULT,
    );
    assert_eq!(replay.outcome.exit_code, 0, "{}", replay.transcript);
    assert_eq!(request_counts(&workspace), calls);
    let replay_record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(replay_record, record);
    assert_eq!(
        replay_record.executor.outcome.as_ref().unwrap().summary,
        "NO FINDING"
    );
    assert_eq!(
        replay_record.lead.outcome.as_ref().unwrap().summary,
        LEAD_RESULT
    );
}

#[test]
fn unknown_new_turn_usage_stays_explicit_and_blocks_dependent_work() {
    let workspace = Workspace::new("budget-unknown-usage", LeadMode::Delivered);
    let limits = ReflectionLimits {
        max_attempts_per_stage: 1,
        max_turns_per_stage: 1,
        max_tokens_per_stage: 120,
        max_tokens_per_record: 240,
        ..ReflectionLimits::default()
    };
    let (controller, request, acceptance) = accepted_with_limits(&workspace, limits);
    let first = run_budget_chain(
        &workspace,
        &request,
        "missing-usage",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&first, "usage remains unknown");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert!(!record.executor.usage_reconciled);
    assert_eq!(record.executor.usage.attempts, 1);
    assert_eq!(record.executor.usage.turns, 1);
    assert_eq!(record.executor.usage.tokens, 0);
    assert!(
        record
            .executor
            .usage
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("total_tokens")),
        "{record:?}"
    );
    assert_eq!(record.lead.state, StageState::Pending);
    assert_eq!(record.lead.usage.attempts, 0);
    assert_eq!(record.delivery, DeliveryState::Pending);
    assert_eq!(
        record.executor.outcome.as_ref().unwrap().summary,
        EXECUTOR_RESULT
    );

    let calls = request_counts(&workspace);
    let repeated = run_budget_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&repeated, "retained reflection usage remains unknown");
    assert_eq!(request_counts(&workspace), calls);
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);
}

#[test]
fn failed_executor_stage_stays_unknown_and_resume_starts_no_model_work() {
    let workspace = Workspace::new("budget-failed-stage", LeadMode::Delivered);
    let limits = ReflectionLimits {
        max_attempts_per_stage: 1,
        max_turns_per_stage: 1,
        ..ReflectionLimits::default()
    };
    let (controller, request, acceptance) = accepted_with_limits(&workspace, limits);
    let failed = run_budget_chain(
        &workspace,
        &request,
        "failed",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&failed, "ended as failed");
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Unknown);
    assert_eq!(record.executor.usage.attempts, 1);
    assert_eq!(
        record.executor.usage.turns, 1,
        "a submitted failed turn is charged even when its outcome is unknown"
    );
    assert_eq!(record.executor.usage.tokens, 0);
    assert!(!record.executor.usage_reconciled);
    assert!(
        record
            .executor
            .usage
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("ended as failed")),
        "{record:?}"
    );
    assert_eq!(record.lead.state, StageState::Pending);
    assert_eq!(record.lead.usage.attempts, 0);
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));

    let calls = request_counts(&workspace);
    let repeated = run_budget_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&repeated, "requires explicit recovery");
    assert_eq!(request_counts(&workspace), calls);
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);
}
