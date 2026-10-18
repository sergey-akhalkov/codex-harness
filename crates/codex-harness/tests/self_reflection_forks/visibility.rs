use super::*;
use harness_core::delivery_reflection::{DeliveryState, StageState as RecordedStageState};

fn terminate_pid(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        assert!(!handle.is_null(), "OpenProcess({pid})");
        assert_ne!(TerminateProcess(handle, 1), 0, "TerminateProcess({pid})");
        assert_ne!(CloseHandle(handle), 0, "CloseHandle({pid})");
    }
}

fn wait_until(mut ready: impl FnMut() -> bool, timeout: Duration, description: &str) {
    let deadline = Instant::now() + timeout;
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "{description} did not happen within {timeout:?}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn wait_for_submitted_stage(workspace: &Workspace) -> Value {
    let path = workspace.state.join("executor-stage.json");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(bytes) = fs::read(&path)
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            && value["status"] == "turn-submitted"
        {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "the executor stage did not retain its submitted turn within 20s"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn executor_rollout(home: &Path) -> (PathBuf, Vec<u8>) {
    let sessions = home.join("sessions");
    let mut paths = fs::read_dir(&sessions)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("executor-self-reflection-"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths.len(), 1, "executor rollout paths: {paths:?}");
    let path = paths.pop().unwrap();
    let bytes = fs::read(&path).unwrap();
    (path, bytes)
}

/// Lose the owning manager after the fixture has accepted the one model turn
/// and written its rollout, leaving the exact submitted identity recoverable.
fn lose_submitted_stage(name: &str, run: &str) -> (Workspace, Value, Value, PathBuf, Vec<u8>) {
    let workspace = Workspace::new(name, LeadMode::Delivered);
    let request = workspace.request(run);
    let session = spawn_chain(
        &workspace,
        &request,
        "mutator-window",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    let pid = session.identity().pid;
    let _ = wait_for_submitted_stage(&workspace);
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    let (rollout_path, rollout_bytes) = executor_rollout(&workspace.home);
    terminate_pid(pid);
    let lost = wait_session(session);
    assert_ne!(lost.outcome.exit_code, 0, "{}", lost.transcript);
    let submitted = stage(&workspace, "executor-stage");
    assert_eq!(submitted["status"], "turn-submitted", "{submitted}");
    assert_eq!(submitted["fork"]["forkThread"], EXECUTOR_FORK);
    assert_eq!(submitted["turnId"], EXECUTOR_FORK_TURN);
    (workspace, request, submitted, rollout_path, rollout_bytes)
}

fn assert_exact_reflection_frontend(workspace: &Workspace) {
    let argv = fs::read_to_string(workspace.home.join("frontend-argv.txt")).unwrap();
    let args = argv.lines().collect::<Vec<_>>();
    assert!(args.contains(&"--remote"), "{argv}");
    assert!(args.contains(&"--remote-auth-token-env"), "{argv}");
    assert!(args.contains(&"HARNESS_EXECUTOR_FRONTEND_TOKEN"), "{argv}");
    assert!(args.contains(&"features.worktrees=false"), "{argv}");
    assert!(args.contains(&"agents.enabled=false"), "{argv}");
    assert!(!args.contains(&"--no-alt-screen"), "{argv}");
    assert!(!args.contains(&"--sandbox"), "{argv}");
    assert!(!argv.contains(TASK), "{argv}");
    let resume = args
        .iter()
        .position(|arg| *arg == "resume")
        .unwrap_or_else(|| panic!("frontend did not resume a thread: {argv}"));
    assert_eq!(args.get(resume + 1), Some(&EXECUTOR_FORK), "{argv}");
    let token = fs::read_to_string(
        workspace
            .state
            .join("executor-self-reflection-reconcile/endpoint.token"),
    )
    .unwrap();
    assert!(
        !argv.contains(token.trim()),
        "frontend argv leaked its token"
    );

    let title = fs::read_to_string(workspace.home.join("frontend-title.txt")).unwrap();
    assert!(title.starts_with("ERefl ds/deepseek/"), "{title}");
    assert!(title.contains("/max#"), "{title}");
}

fn assert_actual_route(stage: &Value) {
    assert_eq!(stage["profile"], PROFILE, "{stage}");
    assert_eq!(stage["provider"], PROVIDER, "{stage}");
    assert_eq!(stage["model"], MODEL, "{stage}");
    assert_eq!(stage["effort"], EFFORT, "{stage}");
    assert_eq!(
        stage["roleRevision"].as_str(),
        Some(role_facts().executor_revision.as_str()),
        "{stage}"
    );
    assert!(
        stage["visibleTitle"].as_str().is_some_and(|title| title
            .contains(&format!("profile={PROFILE}"))
            && title.contains(&format!("provider={PROVIDER}"))
            && title.contains(&format!("model={MODEL}"))
            && title.contains(&format!("effort={EFFORT}"))),
        "{stage}"
    );
}

#[test]
fn native_visibility_recovery_restores_exact_submitted_turn_without_replay() {
    let (workspace, request, submitted, rollout_path, rollout_bytes) =
        lose_submitted_stage("visibility-recover", "run-visibility-recover");
    assert_eq!(submitted["sourceThread"], EXECUTOR_SOURCE, "{submitted}");
    assert_eq!(
        submitted["sourceLastTurn"], EXECUTOR_SOURCE_TURN,
        "{submitted}"
    );
    assert_actual_route(&submitted);
    let work_before = submitted["work"].clone();
    let source_context_before = submitted["sourceContextBytes"].clone();
    let _ = fs::remove_file(workspace.home.join("frontend-argv.txt"));
    let _ = fs::remove_file(workspace.home.join("frontend-release"));

    let recovered = run_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(recovered.outcome.exit_code, 0, "{}", recovered.transcript);

    let executor = stage(&workspace, "executor-stage");
    assert_eq!(executor["status"], "completed", "{executor}");
    assert_eq!(executor["fork"]["forkThread"], EXECUTOR_FORK, "{executor}");
    assert_eq!(executor["turnId"], EXECUTOR_FORK_TURN, "{executor}");
    assert_eq!(executor["result"], EXECUTOR_RESULT, "{executor}");
    assert_eq!(executor["work"], work_before, "{executor}");
    assert_eq!(
        executor["sourceContextBytes"], source_context_before,
        "{executor}"
    );
    assert_actual_route(&executor);
    assert_usage(
        &executor,
        120,
        570,
        "executor-self-reflection-new-turn",
        EXECUTOR_SOURCE_TURN,
    );

    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    let resumes = fixture_requests(&workspace, "executor-self-reflection", "thread/resume");
    assert_eq!(resumes.len(), 1, "{resumes:?}");
    assert_eq!(resumes[0]["params"]["threadId"], EXECUTOR_FORK);
    assert!(
        fixture_requests(&workspace, "executor-self-reflection", "thread/start").is_empty(),
        "recovery started a new thread"
    );
    assert_eq!(
        fixture_requests(&workspace, "executor-self-reflection", "thread/name/set").len(),
        1,
        "recovery relabelled the retained fork"
    );
    assert_eq!(
        executor_rollout(&workspace.home),
        (rollout_path, rollout_bytes),
        "recovery changed the original turn's rollout cost evidence"
    );
}

#[test]
fn native_visibility_recovery_loss_preserves_submitted_identity_and_recovers() {
    let (workspace, request, submitted, rollout_path, rollout_bytes) =
        lose_submitted_stage("visibility-retry", "run-visibility-retry");
    let before = request_counts(&workspace);
    let reads_before =
        fixture_requests(&workspace, "executor-self-reflection", "thread/read").len();
    let resumes_before =
        fixture_requests(&workspace, "executor-self-reflection", "thread/resume").len();
    let _ = fs::remove_file(workspace.home.join("frontend-argv.txt"));
    let _ = fs::remove_file(workspace.home.join("frontend-release"));

    let retrying = spawn_chain(
        &workspace,
        &request,
        "unknown-hold",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    wait_for_path(
        &workspace.home.join("frontend-argv.txt"),
        Duration::from_secs(20),
    );
    wait_until(
        || {
            fixture_requests(&workspace, "executor-self-reflection", "thread/resume").len()
                > resumes_before
        },
        Duration::from_secs(20),
        "recovery did not resume the retained fork",
    );
    wait_until(
        || {
            fixture_requests(&workspace, "executor-self-reflection", "thread/read").len()
                > reads_before
        },
        Duration::from_secs(20),
        "reconciliation did not begin reading the retained turn",
    );
    assert_exact_reflection_frontend(&workspace);
    fs::write(
        workspace.home.join("frontend-release"),
        "second visibility loss",
    )
    .unwrap();
    let lost = wait_session(retrying);
    assert_refused(
        &lost,
        "the restored reflection surface closed before reconciliation",
    );

    let retained = stage(&workspace, "executor-stage");
    assert_eq!(retained["status"], "turn-submitted", "{retained}");
    assert_eq!(retained["fork"]["forkThread"], EXECUTOR_FORK, "{retained}");
    assert_eq!(retained["turnId"], EXECUTOR_FORK_TURN, "{retained}");
    assert_eq!(retained["usage"], submitted["usage"], "{retained}");
    assert_eq!(retained["work"], submitted["work"], "{retained}");
    assert_eq!(
        retained["sourceContextBytes"], submitted["sourceContextBytes"],
        "{retained}"
    );
    assert!(
        retained["error"].as_str().is_some_and(|error| error
            .contains("native visibility restoration")
            && error.contains(EXECUTOR_FORK)
            && error.contains(EXECUTOR_FORK_TURN)
            && error.contains("remains recoverable")),
        "{retained}"
    );
    assert_eq!(request_counts(&workspace), before);
    assert_eq!(
        executor_rollout(&workspace.home),
        (rollout_path.clone(), rollout_bytes.clone()),
        "the failed restoration modified the retained turn's cost evidence"
    );

    fs::remove_file(workspace.home.join("frontend-release")).unwrap();
    let recovered = run_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(recovered.outcome.exit_code, 0, "{}", recovered.transcript);
    let executor = stage(&workspace, "executor-stage");
    assert_eq!(executor["status"], "completed", "{executor}");
    assert_eq!(executor["fork"]["forkThread"], EXECUTOR_FORK, "{executor}");
    assert_eq!(executor["turnId"], EXECUTOR_FORK_TURN, "{executor}");
    assert_eq!(executor["result"], EXECUTOR_RESULT, "{executor}");
    assert_usage(
        &executor,
        120,
        570,
        "executor-self-reflection-new-turn",
        EXECUTOR_SOURCE_TURN,
    );
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    assert_eq!(
        fixture_requests(&workspace, "executor-self-reflection", "thread/resume").len(),
        resumes_before + 2
    );
    assert!(
        fixture_requests(&workspace, "executor-self-reflection", "thread/start").is_empty(),
        "recovery started a new thread"
    );
    assert_eq!(
        executor_rollout(&workspace.home),
        (rollout_path, rollout_bytes),
        "recovery changed the original turn's rollout cost evidence"
    );
}

#[test]
fn native_accepted_visibility_loss_recovers_exact_submitted_turn_without_second_fork_or_turn() {
    let workspace = Workspace::new("accepted-visibility-recovery", LeadMode::Delivered);
    let (controller, request, acceptance) = accepted::accepted(&workspace);

    let session = spawn_chain_in_board(
        &workspace,
        &request,
        "mutator-window",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
        Some(&workspace.checkout),
    );
    let pid = session.identity().pid;
    let submitted = wait_for_submitted_stage(&workspace);
    let accepted_work_before =
        read_json(&workspace.state.join("chain.json"))["acceptedWork"].clone();
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    assert_eq!(
        submitted["fork"]["forkThread"], EXECUTOR_FORK,
        "{submitted}"
    );
    assert_eq!(submitted["turnId"], EXECUTOR_FORK_TURN, "{submitted}");
    let work_before = submitted["work"].clone();
    let source_context_before = submitted["sourceContextBytes"].clone();
    let in_flight = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(in_flight.executor.state, RecordedStageState::InFlight);
    let (rollout_path, rollout_bytes) = executor_rollout(&workspace.home);

    terminate_pid(pid);
    let lost = wait_session(session);
    assert_ne!(lost.outcome.exit_code, 0, "{}", lost.transcript);
    assert_eq!(stage(&workspace, "executor-stage"), submitted);
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap(),
        in_flight,
        "the owning process loss changed the accepted ledger"
    );

    let _ = fs::remove_file(workspace.home.join("frontend-argv.txt"));
    let _ = fs::remove_file(workspace.home.join("frontend-release"));
    workspace.lead_state.lock().unwrap().source_turn_active = true;
    let recovering = spawn_chain_in_board(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
        Some(&workspace.checkout),
    );
    wait_until(
        || {
            fs::read(workspace.state.join("executor-stage.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .is_some_and(|stage| stage["status"] == "completed")
        },
        Duration::from_secs(20),
        "the accepted executor stage was not recovered",
    );
    assert_exact_reflection_frontend(&workspace);
    workspace.lead_state.lock().unwrap().source_turn_active = false;
    let recovered = wait_session(recovering);
    assert_eq!(recovered.outcome.exit_code, 0, "{}", recovered.transcript);

    let executor = stage(&workspace, "executor-stage");
    assert_eq!(executor["status"], "completed", "{executor}");
    assert_eq!(executor["fork"]["forkThread"], EXECUTOR_FORK, "{executor}");
    assert_eq!(executor["turnId"], EXECUTOR_FORK_TURN, "{executor}");
    assert_eq!(executor["result"], EXECUTOR_RESULT, "{executor}");
    assert_eq!(executor["work"], work_before, "{executor}");
    assert_eq!(
        executor["sourceContextBytes"], source_context_before,
        "{executor}"
    );
    assert_usage(
        &executor,
        120,
        570,
        "executor-self-reflection-new-turn",
        EXECUTOR_SOURCE_TURN,
    );
    assert_eq!(
        request_counts(&workspace),
        (1, 1, 1, 1),
        "recovery forked or submitted the executor a second time"
    );
    assert!(
        fixture_requests(&workspace, "executor-self-reflection", "thread/start").is_empty(),
        "recovery started a new thread"
    );
    let resumes = fixture_requests(&workspace, "executor-self-reflection", "thread/resume");
    assert_eq!(resumes.len(), 1, "{resumes:?}");
    assert_eq!(
        resumes[0]["params"]["threadId"], EXECUTOR_FORK,
        "{resumes:?}"
    );
    assert_eq!(
        executor_rollout(&workspace.home),
        (rollout_path, rollout_bytes),
        "recovery changed the original turn's rollout cost evidence"
    );
    assert_eq!(
        read_json(&workspace.state.join("chain.json"))["acceptedWork"],
        accepted_work_before,
        "recovery changed the accepted context"
    );
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, RecordedStageState::Retained);
    assert_eq!(record.lead.state, RecordedStageState::Retained);
    assert_eq!(record.delivery, DeliveryState::Consumed);
    assert_eq!(record.executor.usage.attempts, 1);
    assert_eq!(record.executor.usage.turns, 1);
    assert_eq!(record.executor.usage.tokens, 120);
    assert_eq!(record.executor.usage.investigations, 0);
    assert_eq!(
        record.executor.usage.context_bytes,
        executor["work"]["context_bytes"].as_u64().unwrap()
    );
}
