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

pub(super) fn native(workspace: &Workspace, program: &str, args: &[&str]) -> std::process::Output {
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

pub(super) fn prepare(workspace: &Workspace) -> (ReflectionController, String) {
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
    (controller, task["id"].as_str().unwrap().to_owned())
}

pub(super) fn accepted(workspace: &Workspace) -> (ReflectionController, Value, String) {
    let (controller, task) = prepare(workspace);
    controller
        .activate(&ActivationRequest {
            lead_session: LEAD_THREAD.to_owned(),
            run: None,
            boundary: None,
            journal: JournalPolicy::RequireExisting,
            limits: None,
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
    let chain = read_json(&workspace.state.join("chain.json"));
    assert_eq!(
        chain["acceptedWork"],
        serde_json::to_value(&record.binding).unwrap()
    );
    for role in ["executor-self-reflection", "lead-self-reflection"] {
        let turns = fixture_requests(&workspace, role, "turn/start");
        assert_eq!(turns.len(), 1);
        let prompt = turns[0]["params"]["input"][0]["text"].as_str().unwrap();
        let references = prompt
            .split_once("<<<ACCEPTED WORK\n")
            .unwrap()
            .1
            .split_once("\nACCEPTED WORK>>>")
            .unwrap()
            .0;
        let references: Value = serde_json::from_str(references).unwrap();
        assert_eq!(references, serde_json::to_value(&record.binding).unwrap());
    }
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

#[test]
fn occupied_shared_capacity_keeps_the_closure_pending_then_freed_capacity_runs_it() {
    let workspace = Workspace::new("accepted-capacity", LeadMode::Delivered);
    let (controller, request, acceptance) = accepted(&workspace);
    let before = controller.record(&acceptance).unwrap().unwrap();
    let config = harness_core::orchestration_config::load(&kit_root()).unwrap();
    let pool_size = harness_core::orchestration_config::worktree_pool_size(&config);
    assert!(pool_size >= config.max_concurrent_executors);
    let program = std::env::current_exe().unwrap().canonicalize().unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    let identity = harness_core::process_service::ServiceProcess::observe(
        std::process::id(),
        &program,
        0,
        &user,
    )
    .unwrap()
    .identity();
    let pool =
        harness_core::task_worktree::pool_state_dir(&workspace.home, &workspace.checkout).unwrap();
    fs::create_dir_all(&pool).unwrap();
    // Model-free native lease fixture: the test process actually owns every
    // recorded live PID/creation identity. No worktree position is allocated.
    let occupy_capacity = || {
        (1..=config.max_concurrent_executors)
            .map(|index| {
                let path = pool.join(format!("lease-{index}.json"));
                fs::write(
                    &path,
                    serde_json::to_vec(&json!({
                        "schema": 1, "owner": format!("ordinary-fixture-{index}"),
                        "index": index, "path": workspace.checkout,
                        "pid": identity.pid, "created": identity.creation_time, "program": program,
                    }))
                    .unwrap(),
                )
                .unwrap();
                path
            })
            .collect::<Vec<_>>()
    };
    let leases = occupy_capacity();
    let blocked = run_accepted(&workspace, &request, "complete");
    assert_refused(&blocked, "reflection concurrency is full");
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), before);
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
    assert!(!workspace.state.join("executor-stage.json").exists());
    for path in leases {
        fs::remove_file(path).unwrap();
    }
    let completed = run_accepted(&workspace, &request, "complete");
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap().delivery,
        DeliveryState::Consumed
    );
    let retained = controller.record(&acceptance).unwrap().unwrap();
    let calls = request_counts(&workspace);
    let occupied_again = occupy_capacity();
    let replay = run_accepted(&workspace, &request, "complete");
    assert_eq!(replay.outcome.exit_code, 0, "{}", replay.transcript);
    assert_eq!(
        request_counts(&workspace),
        calls,
        "reuse starts no model work"
    );
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), retained);
    for path in occupied_again {
        fs::remove_file(path).unwrap();
    }
    for index in 1..=pool_size {
        assert!(
            harness_core::task_worktree::load_slot_record(
                &workspace.home,
                &workspace.checkout,
                index
            )
            .unwrap()
            .is_none()
        );
    }
    assert!(!fs::read_dir(pool).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("reflection-lease-") && name.ends_with(".json"))
    }));
}
