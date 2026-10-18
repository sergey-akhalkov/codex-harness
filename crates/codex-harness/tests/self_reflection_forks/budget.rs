//! Bounded cumulative reflection cost through the accepted-stage controller.
//!
//! These checks use the real accepted closure, native fork runner and fixture
//! app-server. They prove that declared token, turn, investigation and
//! supplied-context allowances survive the second stage and a repeated resume,
//! and that unknown usage never unlocks dependent work.

use super::*;
use harness_core::{
    board_cli,
    delivery_reflection::{
        AcceptanceBinding, ActivationRequest, BudgetLimit, DeliveryState, Finding, JournalPolicy,
        Purpose, ReflectionController, ReflectionLimits, Stage, StageState, UsageDelta,
        acceptance_metadata,
    },
};
use std::ffi::OsString;

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

fn run_budget_chain_with_investigations(
    workspace: &Workspace,
    request: &Value,
    executor_investigations: u64,
    lead_investigations: u64,
    executor_fork: &str,
    executor_turn: &str,
) -> ConsoleOutcome {
    let mut spec = chain_spec(
        workspace,
        request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
        Some(&workspace.checkout),
    );
    for (name, value) in [
        (
            "HARNESS_REFLECTION_FIXTURE_EXECUTOR_INVESTIGATIONS",
            executor_investigations.to_string(),
        ),
        (
            "HARNESS_REFLECTION_FIXTURE_LEAD_INVESTIGATIONS",
            lead_investigations.to_string(),
        ),
        (
            "HARNESS_REFLECTION_FIXTURE_EXECUTOR_FORK_THREAD",
            executor_fork.to_owned(),
        ),
        (
            "HARNESS_REFLECTION_FIXTURE_EXECUTOR_FORK_TURN",
            executor_turn.to_owned(),
        ),
    ] {
        spec.env.insert(name.into(), Some(value.into()));
    }
    wait_session(ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap())
}

fn install_fixture(workspace: &Workspace) {
    let bin = workspace.home.join("harness/bin");
    fs::create_dir_all(&bin).unwrap();
    fs::copy(fixture(), bin.join("codex.exe")).unwrap();
    fs::copy(
        workspace.home.join(format!("{PROFILE}.config.toml")),
        workspace.home.join("config.toml"),
    )
    .unwrap();
    let registry = workspace.home.join("harness/lead-endpoints");
    fs::create_dir_all(&registry).unwrap();
    fs::copy(
        &workspace.endpoint,
        registry.join(format!("{LEAD_THREAD}.json")),
    )
    .unwrap();
}

/// Drives the installed `delivery-improvement` interface, not the low-level
/// request runner, so continuation is proven exactly as an operator reaches it.
fn delivery_cli(
    workspace: &Workspace,
    action: &str,
    extra: &[&str],
    executor_investigations: u64,
    executor_fork: &str,
    executor_turn: &str,
) -> ConsoleOutcome {
    let mut spec = CommandSpec::new(manager());
    spec.args = vec![
        "delivery-improvement".into(),
        action.into(),
        "--project".into(),
        workspace.checkout.as_os_str().to_owned(),
        "--source".into(),
        kit_root().into_os_string(),
        "--codex-home".into(),
        workspace.home.as_os_str().to_owned(),
    ];
    spec.args
        .extend(extra.iter().map(|value| OsString::from(*value)));
    spec.current_dir = Some(workspace.checkout.clone());
    apply_fixture_env(
        workspace,
        &mut spec,
        LEAD_THREAD,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    spec.env
        .insert("HARNESS_REFLECTION_HOST".into(), Some(LEAD_THREAD.into()));
    spec.env
        .insert("CODEX_THREAD_ID".into(), Some(LEAD_THREAD.into()));
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_INVESTIGATIONS".into(),
        Some(executor_investigations.to_string().into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_LEAD_INVESTIGATIONS".into(),
        Some("0".into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_FORK_THREAD".into(),
        Some(executor_fork.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_FORK_TURN".into(),
        Some(executor_turn.into()),
    );
    wait_session(ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap())
}

fn status_json(workspace: &Workspace) -> Value {
    let output = Command::new(manager())
        .args(["delivery-improvement", "status", "--project"])
        .arg(&workspace.checkout)
        .arg("--source")
        .arg(kit_root())
        .arg("--codex-home")
        .arg(&workspace.home)
        .env("CODEX_HOME", &workspace.home)
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
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

#[test]
fn investigation_overrun_pauses_the_stage_and_retry_retains_prior_work() {
    let workspace = Workspace::new("budget-investigation-overrun", LeadMode::Delivered);
    let limits = ReflectionLimits {
        max_attempts_per_stage: 2,
        max_turns_per_stage: 2,
        max_tokens_per_stage: 240,
        max_tokens_per_record: 480,
        max_investigations_per_stage: 1,
        max_investigations_per_record: 2,
        ..ReflectionLimits::default()
    };
    let (controller, request, acceptance) = accepted_with_limits(&workspace, limits);
    let paused = run_budget_chain_with_investigations(
        &workspace,
        &request,
        2,
        0,
        EXECUTOR_FORK,
        EXECUTOR_FORK_TURN,
    );
    assert_refused(&paused, "stage_investigations observed 2 exceeds allowed 1");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));

    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Blocked);
    assert_eq!(record.executor.usage.attempts, 1);
    assert_eq!(record.executor.usage.turns, 1);
    assert_eq!(record.executor.usage.tokens, 120);
    assert_eq!(record.executor.usage.investigations, 2);
    assert!(record.executor.usage.context_bytes > 0, "{record:?}");
    assert!(record.executor.usage_reconciled);
    assert!(record.executor.work_reconciled);
    assert!(
        record
            .executor
            .usage
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("stage_investigations"))
    );
    let pause = record.executor.pause.as_ref().unwrap();
    assert_eq!(pause.limit, BudgetLimit::StageInvestigations);
    assert_eq!(pause.observed, 2);
    assert_eq!(pause.allowed, 1);
    assert!(pause.missing_fact.contains("investigation allowance"));
    assert!(
        pause
            .continuation_choice
            .contains("delivery-improvement continue"),
        "{}",
        pause.continuation_choice
    );
    assert!(
        pause.continuation_choice.contains("--stage executor"),
        "{}",
        pause.continuation_choice
    );
    assert!(!pause.continuation_choice.contains("retry_stage"));
    assert!(!pause.evidence.is_empty());
    assert_eq!(record.lead.state, StageState::Pending);
    assert_eq!(record.lead.usage.attempts, 0);
    assert_eq!(record.delivery, DeliveryState::Pending);

    let native = stage(&workspace, "executor-stage");
    assert_eq!(native["status"], json!("budget-paused"), "{native}");
    assert!(
        native["contextBasis"].as_str().is_some_and(|basis| {
            basis.contains("not provider tokens") && basis.contains("not proof")
        }),
        "{native}"
    );
    assert_eq!(native["usageKnown"], json!(true), "{native}");
    assert_eq!(native["usage"]["total_tokens"], json!(120), "{native}");
    assert_eq!(native["work"]["investigations"], json!(2), "{native}");
    assert!(
        native["work"]["context_bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0),
        "{native}"
    );
    assert!(
        native["sourceContextBytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0),
        "{native}"
    );
    assert_eq!(
        native["budgetPause"]["limit"],
        json!("stage_investigations"),
        "{native}"
    );

    let calls = request_counts(&workspace);
    let repeated = run_budget_chain_with_investigations(
        &workspace,
        &request,
        2,
        0,
        EXECUTOR_FORK,
        EXECUTOR_FORK_TURN,
    );
    assert_refused(&repeated, "requires explicit recovery");
    assert_eq!(request_counts(&workspace), calls);
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);

    let retry = controller
        .retry_stage(&acceptance, Stage::Executor)
        .unwrap_err();
    assert!(
        retry
            .to_string()
            .contains("stage_investigations observed 2"),
        "{retry}"
    );
    assert_eq!(request_counts(&workspace), calls);
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);

    let raised = ReflectionLimits {
        max_attempts_per_stage: 2,
        max_turns_per_stage: 2,
        max_tokens_per_stage: 240,
        max_tokens_per_record: 480,
        max_investigations_per_stage: 4,
        max_investigations_per_record: 8,
        ..ReflectionLimits::default()
    };
    controller
        .activate(&ActivationRequest {
            lead_session: LEAD_THREAD.to_owned(),
            run: None,
            boundary: None,
            journal: JournalPolicy::RequireExisting,
            limits: Some(raised),
        })
        .unwrap();
    let receipt = controller
        .retry_stage(&acceptance, Stage::Executor)
        .unwrap();
    assert_eq!(receipt.state, StageState::Pending);
    assert_eq!(receipt.usage.attempts, 1);
    assert_eq!(receipt.usage.tokens, 120);
    assert_eq!(receipt.usage.investigations, 2);
    assert_eq!(receipt.usage.last_error, None);

    let completed = run_budget_chain_with_investigations(
        &workspace,
        &request,
        2,
        0,
        "fixture-executor-reflection-fork-2",
        "fixture-executor-reflection-turn-2",
    );
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    assert_eq!(request_counts(&workspace), (2, 2, 1, 1));

    let archived = read_json(&workspace.state.join("executor-stage-attempt-1.json"));
    assert_eq!(archived["status"], json!("budget-paused"), "{archived}");
    assert_eq!(archived["work"]["investigations"], json!(2), "{archived}");
    let executor = stage(&workspace, "executor-stage");
    assert_eq!(
        executor["turnId"],
        json!("fixture-executor-reflection-turn-2"),
        "{executor}"
    );
    assert_eq!(
        executor["copiedSourceTurns"],
        json!([EXECUTOR_SOURCE_TURN]),
        "{executor}"
    );
    assert!(
        executor["contextBasis"]
            .as_str()
            .is_some_and(|basis| basis.contains("serialized fork thread record")),
        "{executor}"
    );

    let final_record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(final_record.executor.state, StageState::Retained);
    assert_eq!(final_record.executor.usage.attempts, 2);
    assert_eq!(final_record.executor.usage.turns, 2);
    assert_eq!(final_record.executor.usage.tokens, 240);
    assert_eq!(final_record.executor.usage.investigations, 4);
    assert!(final_record.executor.pause.is_none());
    assert_eq!(final_record.executor.usage.last_error, None);
    assert_eq!(final_record.lead.state, StageState::Retained);
    assert_eq!(final_record.lead.usage.tokens, 240);
    assert_eq!(final_record.delivery, DeliveryState::Consumed);
    assert_eq!(final_record.usage_totals().tokens, 480);
    assert_eq!(final_record.usage_totals().investigations, 4);
}

#[test]
fn installed_continuation_resumes_the_exact_pause_without_resetting_costs() {
    let workspace = Workspace::new("budget-cli-continuation", LeadMode::Delivered);
    let limits = ReflectionLimits {
        max_attempts_per_stage: 2,
        max_turns_per_stage: 2,
        max_tokens_per_stage: 240,
        max_tokens_per_record: 480,
        max_investigations_per_stage: 1,
        max_investigations_per_record: 2,
        ..ReflectionLimits::default()
    };
    let (controller, request, acceptance) = accepted_with_limits(&workspace, limits);
    install_fixture(&workspace);
    assert_eq!(&request["acceptance"], &json!(acceptance));

    let paused = delivery_cli(
        &workspace,
        "process",
        &[],
        2,
        EXECUTOR_FORK,
        EXECUTOR_FORK_TURN,
    );
    assert_refused(&paused, "stage_investigations observed 2 exceeds allowed 1");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Blocked);
    assert_eq!(
        record.executor.pause.as_ref().unwrap().limit,
        BudgetLimit::StageInvestigations
    );
    assert!(record.executor.usage_reconciled);
    assert!(record.executor.work_reconciled);

    // Status is model-free and names the exact installed continuation,
    // including the raised allowance flags computed from the retained pause.
    let status = status_json(&workspace);
    let entry = status["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["acceptance"] == json!(acceptance))
        .unwrap();
    assert_eq!(entry["executor"]["state"], json!("blocked"), "{entry}");
    assert_eq!(entry["executor"]["usageReconciled"], json!(true), "{entry}");
    assert_eq!(entry["executor"]["workReconciled"], json!(true), "{entry}");
    assert_eq!(
        entry["executor"]["missingReconciliation"],
        json!([]),
        "{entry}"
    );
    let continuation = entry["executor"]["continuation"].as_str().unwrap();
    assert!(
        continuation.contains("delivery-improvement continue")
            && continuation.contains("--stage executor")
            && continuation.contains("--max-investigations-per-stage")
            && continuation.contains("--max-investigations-per-record"),
        "{continuation}"
    );
    assert!(
        entry["executor"]["nextAction"]
            .as_str()
            .is_some_and(|action| action.contains("--stage executor")),
        "{entry}"
    );

    // Unchanged allowances cannot resume the pause: no model request and no
    // state change.
    let unchanged = delivery_cli(
        &workspace,
        "continue",
        &["--acceptance", &acceptance, "--stage", "executor"],
        2,
        EXECUTOR_FORK,
        EXECUTOR_FORK_TURN,
    );
    assert_refused(&unchanged, "--max-investigations-per-stage");
    assert!(
        unchanged.transcript.contains("nothing was changed"),
        "{}",
        unchanged.transcript
    );
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);

    // A continuation for any other stage is refused before effects.
    let wrong_stage = delivery_cli(
        &workspace,
        "continue",
        &[
            "--acceptance",
            &acceptance,
            "--stage",
            "lead",
            "--max-investigations-per-stage",
            "4",
            "--max-investigations-per-record",
            "8",
        ],
        2,
        EXECUTOR_FORK,
        EXECUTOR_FORK_TURN,
    );
    assert_refused(&wrong_stage, "not a retained budget pause");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);

    // Reconciliation is never assumed: a one-sided pair is refused at parse
    // time, and an already-reconciled stage refuses a second charge.
    let one_sided = delivery_cli(
        &workspace,
        "continue",
        &[
            "--acceptance",
            &acceptance,
            "--stage",
            "executor",
            "--reconcile-usage-turns",
            "0",
        ],
        2,
        EXECUTOR_FORK,
        EXECUTOR_FORK_TURN,
    );
    assert_refused(&one_sided, "--reconcile-usage-tokens");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    let doubled = delivery_cli(
        &workspace,
        "continue",
        &[
            "--acceptance",
            &acceptance,
            "--stage",
            "executor",
            "--reconcile-usage-turns",
            "0",
            "--reconcile-usage-tokens",
            "0",
        ],
        2,
        EXECUTOR_FORK,
        EXECUTOR_FORK_TURN,
    );
    assert_refused(&doubled, "already reconciled");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);

    // The exact paused stage continues with its retained usage and the raised
    // cumulative allowances; the second native attempt and the lead follow.
    let continued = delivery_cli(
        &workspace,
        "continue",
        &[
            "--acceptance",
            &acceptance,
            "--stage",
            "executor",
            "--max-investigations-per-stage",
            "4",
            "--max-investigations-per-record",
            "8",
        ],
        2,
        "fixture-executor-reflection-fork-2",
        "fixture-executor-reflection-turn-2",
    );
    assert_eq!(continued.outcome.exit_code, 0, "{}", continued.transcript);
    assert_eq!(request_counts(&workspace), (2, 2, 1, 1));

    let state_dir = controller
        .state_file()
        .unwrap()
        .with_extension("")
        .join(&acceptance);
    let archived = read_json(&state_dir.join("executor-stage-attempt-1.json"));
    assert_eq!(archived["status"], json!("budget-paused"), "{archived}");
    assert_eq!(archived["work"]["investigations"], json!(2), "{archived}");

    let final_record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(final_record.executor.state, StageState::Retained);
    assert_eq!(final_record.executor.usage.attempts, 2);
    assert_eq!(final_record.executor.usage.turns, 2);
    assert_eq!(final_record.executor.usage.tokens, 240);
    assert_eq!(final_record.executor.usage.investigations, 4);
    assert!(final_record.executor.pause.is_none());
    assert_eq!(final_record.lead.state, StageState::Retained);
    assert_eq!(final_record.lead.usage.tokens, 240);
    assert_eq!(final_record.delivery, DeliveryState::Consumed);
    assert_eq!(final_record.usage_totals().tokens, 480);
    assert_eq!(final_record.usage_totals().investigations, 4);
    let raised = controller.stored_state().unwrap().unwrap().limits;
    assert_eq!(raised.max_investigations_per_stage, 4);
    assert_eq!(raised.max_investigations_per_record, 8);
}

#[test]
fn fork_context_overrun_pauses_before_the_turn_and_restart_sends_no_request() {
    let workspace = Workspace::new("budget-context-overrun", LeadMode::Delivered);
    let limits = ReflectionLimits {
        max_attempts_per_stage: 1,
        max_turns_per_stage: 1,
        max_context_bytes_per_stage: 1,
        max_context_bytes_per_record: 1_000_000,
        ..ReflectionLimits::default()
    };
    let (controller, request, acceptance) = accepted_with_limits(&workspace, limits);
    let paused = run_budget_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&paused, "stage_context_bytes observed");
    assert_eq!(request_counts(&workspace), (1, 0, 0, 0));

    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Blocked);
    assert_eq!(record.executor.usage.attempts, 1);
    assert_eq!(
        record.executor.usage.turns, 0,
        "no turn was submitted before the context pause"
    );
    assert_eq!(record.executor.usage.tokens, 0);
    assert_eq!(record.executor.usage.investigations, 0);
    assert!(record.executor.usage.context_bytes > 0, "{record:?}");
    assert!(record.executor.usage_reconciled);
    assert!(record.executor.work_reconciled);
    let pause = record.executor.pause.as_ref().unwrap();
    assert_eq!(pause.limit, BudgetLimit::StageContextBytes);
    assert_eq!(pause.observed, record.executor.usage.context_bytes);
    assert_eq!(pause.allowed, 1);
    assert!(pause.missing_fact.contains("supplied source context"));
    assert!(
        pause
            .continuation_choice
            .contains("--max-context-bytes-per-stage")
    );
    assert_eq!(record.lead.state, StageState::Pending);
    assert_eq!(record.delivery, DeliveryState::Pending);

    let native = stage(&workspace, "executor-stage");
    assert_eq!(native["status"], json!("budget-paused"), "{native}");
    assert_eq!(native["turnId"], Value::Null, "{native}");
    assert_eq!(native["usageKnown"], json!(true), "{native}");
    assert_eq!(native["usage"]["total_tokens"], json!(0), "{native}");
    assert_eq!(native["work"]["investigations"], json!(0), "{native}");
    assert!(
        native["sourceContextBytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0),
        "{native}"
    );
    assert_eq!(
        native["budgetPause"]["limit"],
        json!("stage_context_bytes"),
        "{native}"
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
    assert_refused(&repeated, "requires explicit recovery");
    assert_eq!(request_counts(&workspace), calls);
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);
}

#[test]
fn record_investigation_overrun_pauses_the_lead_and_rerun_sends_no_request() {
    let workspace = Workspace::new("budget-record-investigations", LeadMode::Delivered);
    let limits = ReflectionLimits {
        max_attempts_per_stage: 1,
        max_turns_per_stage: 1,
        max_tokens_per_stage: 240,
        max_tokens_per_record: 480,
        max_investigations_per_stage: 2,
        max_investigations_per_record: 2,
        ..ReflectionLimits::default()
    };
    let (controller, request, acceptance) = accepted_with_limits(&workspace, limits);
    let paused = run_budget_chain_with_investigations(
        &workspace,
        &request,
        1,
        2,
        EXECUTOR_FORK,
        EXECUTOR_FORK_TURN,
    );
    assert_refused(
        &paused,
        "record_investigations observed 3 exceeds allowed 2",
    );
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));

    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert_eq!(record.executor.usage.tokens, 120);
    assert_eq!(record.executor.usage.investigations, 1);
    assert!(record.executor.usage_reconciled);
    assert!(record.executor.work_reconciled);
    assert_eq!(record.lead.state, StageState::Blocked);
    assert_eq!(record.lead.usage.attempts, 1);
    assert_eq!(record.lead.usage.turns, 1);
    assert_eq!(record.lead.usage.tokens, 240);
    assert_eq!(record.lead.usage.investigations, 2);
    assert!(record.lead.usage.context_bytes > 0, "{record:?}");
    assert!(record.lead.usage_reconciled);
    assert!(record.lead.work_reconciled);
    let pause = record.lead.pause.as_ref().unwrap();
    assert_eq!(pause.limit, BudgetLimit::RecordInvestigations);
    assert_eq!(pause.observed, 3);
    assert_eq!(pause.allowed, 2);
    assert!(
        pause
            .missing_fact
            .contains("cumulative investigation allowance")
    );
    assert!(
        pause
            .continuation_choice
            .contains("--max-investigations-per-record")
    );
    assert!(!pause.evidence.is_empty());
    assert_eq!(record.delivery, DeliveryState::Pending);

    let native = stage(&workspace, "lead-stage");
    assert_eq!(native["status"], json!("budget-paused"), "{native}");
    assert_eq!(native["usageKnown"], json!(true), "{native}");
    assert_eq!(native["usage"]["total_tokens"], json!(240), "{native}");
    assert_eq!(native["work"]["investigations"], json!(2), "{native}");
    assert!(
        native["sourceContextBytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0),
        "{native}"
    );
    assert!(
        native["contextBasis"]
            .as_str()
            .is_some_and(|basis| basis.contains("serialized fork thread record")),
        "{native}"
    );
    assert!(
        native["budgetPause"]["evidence"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "{native}"
    );

    let calls = request_counts(&workspace);
    let repeated = run_budget_chain_with_investigations(
        &workspace,
        &request,
        1,
        2,
        EXECUTOR_FORK,
        EXECUTOR_FORK_TURN,
    );
    assert_refused(&repeated, "requires explicit recovery");
    assert_eq!(request_counts(&workspace), calls);
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);
}

#[test]
fn turn_exhaustion_blocks_a_second_attempt_after_reconciliation() {
    let workspace = Workspace::new("budget-turn-exhaustion", LeadMode::Delivered);
    let limits = ReflectionLimits {
        max_attempts_per_stage: 2,
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
    assert_eq!(record.executor.usage.turns, 1);
    assert!(!record.executor.usage_reconciled);
    assert!(
        record.executor.work_reconciled,
        "the fork context was measured before the failed turn"
    );
    assert!(record.executor.usage.context_bytes > 0, "{record:?}");
    let calls = request_counts(&workspace);
    assert_eq!(calls, (1, 1, 0, 0));

    controller
        .record_unknown_usage(
            &acceptance,
            Stage::Executor,
            UsageDelta {
                turns: 0,
                tokens: 0,
            },
        )
        .unwrap();
    let reconciled = controller.record(&acceptance).unwrap().unwrap();
    assert!(reconciled.executor.usage_reconciled);
    assert!(reconciled.executor.work_reconciled);
    assert_eq!(reconciled.executor.usage.last_error, None);
    assert_eq!(reconciled.executor.usage.turns, 1);

    let receipt = controller
        .retry_stage(&acceptance, Stage::Executor)
        .unwrap();
    assert_eq!(receipt.state, StageState::Pending);
    let error = controller
        .begin_stage(&acceptance, Stage::Executor)
        .unwrap_err();
    assert!(
        error.to_string().contains("reached its 1 turn allowance"),
        "{error}"
    );
    assert_eq!(request_counts(&workspace), calls);
    let after = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(after.executor.state, StageState::Pending);
    assert_eq!(after.executor.usage.attempts, 1);
    assert_eq!(after.executor.usage.turns, 1);
}
