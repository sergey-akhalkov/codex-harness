//! Real beads closures through the CLI and both model-free native fork stages.

use super::*;
use harness_core::{
    board_cli,
    delivery_reflection::{
        AcceptanceBinding, ActivationRequest, DeliveryState, JournalPolicy, Purpose,
        ReflectionController, StageState, acceptance_metadata,
    },
};

const ACTOR: &str = "reflection-fixture-lead";

fn native(workspace: &Workspace, program: &str, args: &[&str]) -> std::process::Output {
    let output = Command::new(program)
        .args(args)
        .current_dir(&workspace.checkout)
        .env("BD_NON_INTERACTIVE", "1")
        .env("BEADS_ACTOR", ACTOR)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{program} {args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn accepted(workspace: &Workspace) -> (ReflectionController, Value, String) {
    native(workspace, "git", &["init", "-q", "--initial-branch=main"]);
    native(
        workspace,
        "git",
        &["config", "user.email", "fixture@example.test"],
    );
    native(workspace, "git", &["config", "user.name", "Fixture"]);
    fs::write(
        workspace.checkout.join("README.md"),
        "synthetic accepted task\n",
    )
    .unwrap();
    native(workspace, "git", &["add", "README.md"]);
    native(workspace, "git", &["commit", "-qm", "seed"]);
    native(
        workspace,
        "bd.exe",
        &[
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "rfl",
        ],
    );
    native(
        workspace,
        "bd.exe",
        &["config", "set", "events-journal", "true"],
    );
    let controller = ReflectionController::new("bd.exe", &workspace.checkout, &workspace.home);
    controller
        .activate(&ActivationRequest {
            lead_session: LEAD_THREAD.to_owned(),
            run: None,
            boundary: None,
            journal: JournalPolicy::RequireExisting,
            limits: None,
        })
        .unwrap();
    let output = native(
        workspace,
        "bd.exe",
        &[
            "create",
            "Accepted ordinary task",
            "--type",
            "task",
            "--json",
        ],
    );
    let task: Value = serde_json::from_slice(&output.stdout).unwrap();
    let task = task["id"].as_str().unwrap();
    let binding = AcceptanceBinding::new(
        task,
        "accepted-run",
        "accepted-assignment",
        "accepted-result",
        "accepted-revision",
        vec![EXECUTOR_SOURCE.to_owned()],
        LEAD_THREAD,
        ACTOR,
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
        ACTOR,
        task,
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

fn run_accepted(workspace: &Workspace, request: &Value, executor_outcome: &str) -> ConsoleOutcome {
    wait_session(spawn_chain_in_board(
        workspace,
        request,
        executor_outcome,
        "complete",
        "NO FINDING",
        LEAD_RESULT,
        Some(&workspace.checkout),
    ))
}

#[test]
fn native_accepted_closure_retains_both_stages_delivery_and_usage_without_replay() {
    let workspace = Workspace::new("accepted-closure", LeadMode::Delivered);
    let (controller, request, acceptance) = accepted(&workspace);
    let completed = run_accepted(&workspace, &request, "complete");
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert_eq!(record.lead.state, StageState::Retained);
    assert_eq!(record.delivery, DeliveryState::Consumed);
    assert_eq!(record.executor.usage.attempts, 1);
    assert_eq!(record.lead.usage.attempts, 1);
    assert!(record.executor.usage.tokens > 0 && record.lead.usage.tokens > 0);
    let calls = request_counts(&workspace);
    let replay = run_accepted(&workspace, &request, "complete");
    assert_eq!(replay.outcome.exit_code, 0, "{}", replay.transcript);
    assert_eq!(request_counts(&workspace), calls);
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);

    fs::remove_file(workspace.state.join("executor-stage.json")).unwrap();
    let missing = run_accepted(&workspace, &request, "complete");
    assert_refused(&missing, "native state is missing");
    assert_eq!(request_counts(&workspace), calls);
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);
}

#[test]
fn foreign_acceptance_refuses_before_fork_and_unknown_usage_blocks_the_lead() {
    let workspace = Workspace::new("accepted-refusals", LeadMode::Delivered);
    let (controller, request, acceptance) = accepted(&workspace);
    let mut foreign = request.clone();
    foreign["executorSource"]["lastTurn"] = json!("unaccepted-turn");
    let refused = run_accepted(&workspace, &foreign, "complete");
    assert_refused(&refused, "does not match the accepted task/run");
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
    assert_eq!(
        controller
            .record(&acceptance)
            .unwrap()
            .unwrap()
            .executor
            .usage
            .attempts,
        0
    );

    let unknown = run_accepted(&workspace, &request, "missing-usage");
    assert_refused(&unknown, "usage remains unknown");
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert!(!record.executor.usage_reconciled);
    assert_eq!(record.lead.state, StageState::Pending);
    assert_eq!(record.lead.usage.attempts, 0);
    let calls = request_counts(&workspace);
    let retry = run_accepted(&workspace, &request, "complete");
    assert_refused(&retry, "usage remains unknown");
    assert_eq!(request_counts(&workspace), calls);
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);
}
