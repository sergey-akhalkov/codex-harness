use super::*;
use harness_core::delivery_reflection::{DeliveryState, StageState as RecordedStageState};
use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

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

fn process_image(pid: u32) -> Option<PathBuf> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = [0u16; 32 * 1024];
    let mut length = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
    unsafe {
        CloseHandle(handle);
    }
    (ok != 0 && length != 0)
        .then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))
}

fn process_running(pid: u32) -> bool {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut exit_code = 0u32;
    let ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
    unsafe {
        CloseHandle(handle);
    }
    ok != 0 && exit_code as i32 == STILL_ACTIVE
}

/// Every reflection frontend owned by `root`.
///
/// The ordinary manager launches one foreground stage runner in its own
/// console, and that runner owns the native frontend. The frontend is
/// therefore a grandchild of the manager, not its direct child: ownership is
/// the manager's process tree, not one parent link.
fn reflection_processes(root: u32, expected: &Path) -> Vec<u32> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_null() || snapshot as isize == -1 {
        return Vec::new();
    }
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut present = unsafe { Process32FirstW(snapshot, &mut entry) };
    let mut processes = Vec::new();
    while present != 0 {
        processes.push((entry.th32ProcessID, entry.th32ParentProcessID));
        present = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    unsafe {
        CloseHandle(snapshot);
    }
    let mut descendants = vec![root];
    let mut found = Vec::new();
    let mut next = 0;
    while next < descendants.len() {
        let parent = descendants[next];
        next += 1;
        for &(pid, parent_pid) in &processes {
            if parent_pid == parent && pid != root {
                descendants.push(pid);
                if process_image(pid).is_some_and(|image| image == expected) {
                    found.push(pid);
                }
            }
        }
    }
    found
}

fn reflection_frontends(root: u32) -> Vec<u32> {
    reflection_processes(root, &frontend_double())
}

fn reflection_backends(workspace: &Workspace, root: u32) -> Vec<u32> {
    reflection_processes(root, &workspace.home.join("harness/bin/codex.exe"))
}

/// Ends the app-server double serving an in-flight reflection turn. The
/// manager, its visible frontend and the retained native identities stay
/// owned; the host observes a control-transport failure instead of waiting for
/// the production 300-second bound.
pub(super) fn terminate_reflection_backend(workspace: &Workspace, root: u32) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(
            process_running(root),
            "the owning manager pid {root} exited before its app-server double was established"
        );
        let backends = reflection_backends(workspace, root);
        if let [pid] = backends.as_slice() {
            println!("reflection app-server pid {pid} is live under owning manager pid {root}");
            terminate_pid(*pid);
            wait_until(
                || !process_running(*pid),
                Duration::from_secs(5),
                "the test-owned reflection app-server did not exit",
            );
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the owning manager pid {root} did not expose exactly one reflection app-server within 20s: {backends:?}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Waits for the manager root to end after it persisted the unknown submitted
/// turn. The caller already owns the console session whose job reaps any
/// remaining descendants.
pub(super) fn wait_for_owned_manager_exit(root: u32) {
    wait_until(
        || !process_running(root),
        Duration::from_secs(20),
        "the test-owned reflection manager did not exit",
    );
}

fn wait_for_reflection_frontend(root: u32, previous: Option<u32>) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(
            process_running(root),
            "the owning manager pid {root} exited before its reflection frontend was established"
        );
        if let Some(pid) = reflection_frontends(root)
            .into_iter()
            .find(|pid| Some(*pid) != previous)
        {
            println!(
                "reflection frontend pid {pid} is live under owning manager pid {root}{}",
                previous.map_or_else(String::new, |lost| format!(" after losing pid {lost}"))
            );
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "the ordinary manager pid {root} did not attach another reflection frontend within 20s"
        );
        std::thread::sleep(Duration::from_millis(25));
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
    wait_for_submitted_stage_in(&workspace.state)
}

pub(super) fn wait_for_stage_status_in(state_dir: &Path, status: &str, description: &str) -> Value {
    let path = state_dir.join("executor-stage.json");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(bytes) = fs::read(&path)
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            && value["status"] == status
        {
            return value;
        }
        assert!(Instant::now() < deadline, "{description} within 20s");
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub(super) fn wait_for_submitted_stage_in(state_dir: &Path) -> Value {
    wait_for_stage_status_in(
        state_dir,
        "turn-submitted",
        "the executor stage did not retain its submitted turn",
    )
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
    assert_exact_reflection_frontend_in(workspace, &workspace.state)
}

fn assert_exact_reflection_frontend_in(workspace: &Workspace, state_dir: &Path) {
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
    let mut tokens = Vec::new();
    for entry in fs::read_dir(state_dir).unwrap() {
        let path = entry.unwrap().path();
        let retained = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.ends_with("-native")
                    || name.ends_with("-reconcile")
                    || name.ends_with("-recover")
            });
        let token = path.join("endpoint.token");
        if retained && token.is_file() {
            tokens.push((token.clone(), fs::read_to_string(token).unwrap()));
        }
    }
    assert!(
        !tokens.is_empty(),
        "no reflection endpoint token was retained under {}",
        state_dir.display()
    );
    for (path, token) in tokens {
        assert!(
            !argv.contains(token.trim()),
            "frontend argv leaked the token retained at {}",
            path.display()
        );
    }

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

/// The ordinary delivery-improvement process derives its request and state
/// paths from the retained board acceptance. Losing only its owned reflection
/// frontend after a submitted native turn - while the owning manager stays
/// alive - must restore visibility in the same manager and recover that exact
/// turn without a caller-authored request, a second fork/turn, or a repeated
/// lead delivery.
#[test]
fn native_normal_process_frontend_only_visibility_loss_recovers_exact_accepted_turn_without_request()
 {
    let workspace = Workspace::new("normal-process-frontend-loss-recovery", LeadMode::Delivered);
    let (controller, _request, acceptance) = accepted::accepted(&workspace);
    lifecycle::install_fixture(&workspace);
    let before = controller.stored_state().unwrap().unwrap();
    let enabled = lifecycle::success(lifecycle::command(&workspace, "enable", Some(LEAD_THREAD)));
    assert_eq!(enabled["checkpoint"], before.checkpoint);
    assert_eq!(enabled["revision"], before.binding.revision);
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
    assert!(
        !workspace.request.exists(),
        "ordinary process must not require a caller-authored request"
    );
    let state_dir = controller
        .state_file()
        .unwrap()
        .with_extension("")
        .join(&acceptance);

    let mut first_spec =
        lifecycle::process_spec_from(&workspace, &manager(), LEAD_THREAD, "process");
    first_spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_OUTCOME".into(),
        Some("mutator-window".into()),
    );
    first_spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_RESULT".into(),
        Some(EXECUTOR_RESULT.into()),
    );
    let session = ConsoleSession::spawn(ConsoleSpec::new(first_spec)).unwrap();
    let pid = session.identity().pid;
    let submitted = wait_for_submitted_stage_in(&state_dir);
    let accepted_work_before = read_json(&state_dir.join("chain.json"))["acceptedWork"].clone();
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

    let frontend = wait_for_reflection_frontend(pid, None);
    assert!(
        process_running(pid),
        "the ordinary manager exited before only its reflection frontend was closed"
    );
    terminate_pid(frontend);
    wait_until(
        || !process_running(frontend),
        Duration::from_secs(5),
        "the test-owned reflection frontend did not exit",
    );
    assert!(
        process_running(pid),
        "closing only the reflection frontend also ended the ordinary manager"
    );
    let restored_frontend = wait_for_reflection_frontend(pid, Some(frontend));
    assert!(
        process_running(pid),
        "the restored reflection frontend did not belong to a live ordinary manager"
    );
    assert_ne!(restored_frontend, frontend);
    assert_eq!(read_json(&state_dir.join("executor-stage.json")), submitted);
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap(),
        in_flight,
        "frontend-only loss changed the accepted ledger"
    );
    assert!(
        !workspace.request.exists(),
        "ordinary process must derive the retained request instead of writing one"
    );

    fs::write(
        workspace
            .fixture_dir
            .join("release-executor-self-reflection.marker"),
        "release",
    )
    .unwrap();
    wait_until(
        || {
            fs::read(state_dir.join("executor-stage.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .is_some_and(|stage| stage["status"] == "completed")
        },
        Duration::from_secs(20),
        "the ordinary process did not reconcile the exact retained executor turn",
    );
    assert_exact_reflection_frontend_in(&workspace, &state_dir);
    let recovered = wait_session(session);
    assert_eq!(recovered.outcome.exit_code, 0, "{}", recovered.transcript);

    let executor = read_json(&state_dir.join("executor-stage.json"));
    assert_eq!(executor["status"], "completed", "{executor}");
    assert_eq!(executor["fork"]["forkThread"], EXECUTOR_FORK, "{executor}");
    assert_eq!(executor["turnId"], EXECUTOR_FORK_TURN, "{executor}");
    assert_eq!(executor["result"], EXECUTOR_RESULT, "{executor}");
    assert_actual_route(&executor);
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
    assert!(
        resumes.len() <= 1,
        "recovery resumed more than once: {resumes:?}"
    );
    for resume in resumes {
        assert_eq!(resume["params"]["threadId"], EXECUTOR_FORK, "{resume}");
    }
    assert_eq!(
        executor_rollout(&workspace.home),
        (rollout_path, rollout_bytes),
        "recovery changed the original turn's rollout cost evidence"
    );
    assert_eq!(
        read_json(&state_dir.join("chain.json"))["acceptedWork"],
        accepted_work_before,
        "recovery changed the accepted context"
    );

    let lead = read_json(&state_dir.join("lead-stage.json"));
    assert_eq!(lead["status"], "completed", "{lead}");
    assert_eq!(lead["fork"]["forkThread"], LEAD_FORK, "{lead}");
    assert_eq!(lead["turnId"], LEAD_FORK_TURN, "{lead}");
    assert_eq!(lead["result"], LEAD_RESULT, "{lead}");
    assert_eq!(lead["profile"], "default", "{lead}");
    assert_eq!(lead["provider"], PROVIDER, "{lead}");
    assert_eq!(lead["model"], MODEL, "{lead}");
    assert_eq!(lead["effort"], EFFORT, "{lead}");
    assert_eq!(
        lead["roleRevision"].as_str(),
        Some(role_facts().lead_revision.as_str()),
        "{lead}"
    );
    assert!(
        lead["visibleTitle"]
            .as_str()
            .is_some_and(|title| title.contains("profile=default")
                && title.contains(&format!("provider={PROVIDER}"))
                && title.contains(&format!("model={MODEL}"))
                && title.contains(&format!("effort={EFFORT}"))),
        "{lead}"
    );
    let delivery = read_json(&state_dir.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "delivered", "{delivery}");
    assert_eq!(delivery["by"], "clientId", "{delivery}");
    let lead_requests = workspace.lead.requests_for("turn/start");
    assert_eq!(lead_requests.len(), 1, "{lead_requests:?}");
    assert_eq!(
        lead_requests[0]["params"]["clientUserMessageId"], delivery["clientMessageId"],
        "{lead_requests:?}"
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
    assert_eq!(record.lead.usage.attempts, 1);
    assert_eq!(record.lead.usage.turns, 1);
    assert_eq!(record.lead.usage.tokens, 240);
    assert_eq!(record.usage_totals().tokens, 360);
    assert_eq!(controller.stored_state().unwrap().unwrap().records.len(), 1);
    assert!(
        !workspace.request.exists(),
        "ordinary process must not write a caller-authored request"
    );
}

/// The ordinary delivery-improvement process derives its request and state
/// paths from the retained board acceptance. Losing its owning manager after a
/// submitted native turn must recover that exact turn without a caller-authored
/// request, a second fork/turn, or a repeated lead delivery.
#[test]
fn native_normal_process_visibility_loss_recovers_exact_accepted_turn_without_request() {
    let workspace = Workspace::new("normal-process-visibility-recovery", LeadMode::Delivered);
    let (controller, _request, acceptance) = accepted::accepted(&workspace);
    lifecycle::install_fixture(&workspace);
    let before = controller.stored_state().unwrap().unwrap();
    let enabled = lifecycle::success(lifecycle::command(&workspace, "enable", Some(LEAD_THREAD)));
    assert_eq!(enabled["checkpoint"], before.checkpoint);
    assert_eq!(enabled["revision"], before.binding.revision);
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
    assert!(
        !workspace.request.exists(),
        "ordinary process must not require a caller-authored request"
    );
    let state_dir = controller
        .state_file()
        .unwrap()
        .with_extension("")
        .join(&acceptance);

    let mut first_spec =
        lifecycle::process_spec_from(&workspace, &manager(), LEAD_THREAD, "process");
    first_spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_OUTCOME".into(),
        Some("mutator-window".into()),
    );
    first_spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_RESULT".into(),
        Some(EXECUTOR_RESULT.into()),
    );
    let session = ConsoleSession::spawn(ConsoleSpec::new(first_spec)).unwrap();
    let pid = session.identity().pid;
    let submitted = wait_for_submitted_stage_in(&state_dir);
    let accepted_work_before = read_json(&state_dir.join("chain.json"))["acceptedWork"].clone();
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
    assert_eq!(read_json(&state_dir.join("executor-stage.json")), submitted);
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap(),
        in_flight,
        "the owning process loss changed the accepted ledger"
    );
    assert!(
        !workspace.request.exists(),
        "ordinary process must derive the retained request instead of writing one"
    );

    let _ = fs::remove_file(workspace.home.join("frontend-argv.txt"));
    let _ = fs::remove_file(workspace.home.join("frontend-release"));
    workspace.lead_state.lock().unwrap().source_turn_active = true;
    let mut recovering_spec =
        lifecycle::process_spec_from(&workspace, &manager(), LEAD_THREAD, "process");
    recovering_spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_RESULT".into(),
        Some(EXECUTOR_RESULT.into()),
    );
    let recovering = ConsoleSession::spawn(ConsoleSpec::new(recovering_spec)).unwrap();
    wait_until(
        || {
            fs::read(state_dir.join("executor-stage.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .is_some_and(|stage| stage["status"] == "completed")
        },
        Duration::from_secs(20),
        "the ordinary process did not recover the submitted executor stage",
    );
    assert_exact_reflection_frontend_in(&workspace, &state_dir);
    workspace.lead_state.lock().unwrap().source_turn_active = false;
    let recovered = wait_session(recovering);
    assert_eq!(recovered.outcome.exit_code, 0, "{}", recovered.transcript);

    let executor = read_json(&state_dir.join("executor-stage.json"));
    assert_eq!(executor["status"], "completed", "{executor}");
    assert_eq!(executor["fork"]["forkThread"], EXECUTOR_FORK, "{executor}");
    assert_eq!(executor["turnId"], EXECUTOR_FORK_TURN, "{executor}");
    assert_eq!(executor["result"], EXECUTOR_RESULT, "{executor}");
    assert_actual_route(&executor);
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
        read_json(&state_dir.join("chain.json"))["acceptedWork"],
        accepted_work_before,
        "recovery changed the accepted context"
    );

    let lead = read_json(&state_dir.join("lead-stage.json"));
    assert_eq!(lead["status"], "completed", "{lead}");
    assert_eq!(lead["fork"]["forkThread"], LEAD_FORK, "{lead}");
    assert_eq!(lead["turnId"], LEAD_FORK_TURN, "{lead}");
    assert_eq!(lead["result"], LEAD_RESULT, "{lead}");
    assert_eq!(lead["profile"], "default", "{lead}");
    assert_eq!(lead["provider"], PROVIDER, "{lead}");
    assert_eq!(lead["model"], MODEL, "{lead}");
    assert_eq!(lead["effort"], EFFORT, "{lead}");
    assert_eq!(
        lead["roleRevision"].as_str(),
        Some(role_facts().lead_revision.as_str()),
        "{lead}"
    );
    assert!(
        lead["visibleTitle"]
            .as_str()
            .is_some_and(|title| title.contains("profile=default")
                && title.contains(&format!("provider={PROVIDER}"))
                && title.contains(&format!("model={MODEL}"))
                && title.contains(&format!("effort={EFFORT}"))),
        "{lead}"
    );
    let delivery = read_json(&state_dir.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "delivered", "{delivery}");
    assert_eq!(delivery["by"], "clientId", "{delivery}");
    let lead_requests = workspace.lead.requests_for("turn/start");
    assert_eq!(lead_requests.len(), 1, "{lead_requests:?}");
    assert_eq!(
        lead_requests[0]["params"]["clientUserMessageId"], delivery["clientMessageId"],
        "{lead_requests:?}"
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
    assert_eq!(record.lead.usage.attempts, 1);
    assert_eq!(record.lead.usage.turns, 1);
    assert_eq!(record.lead.usage.tokens, 240);
    assert_eq!(record.usage_totals().tokens, 360);
    assert_eq!(controller.stored_state().unwrap().unwrap().records.len(), 1);
    assert!(
        !workspace.request.exists(),
        "ordinary process must not write a caller-authored request"
    );
}

/// A pre-submission surface loss leaves a known fork with no turn. The
/// ordinary owner must resume that exact fork, verify the replacement surface,
/// then admit its initial turn once - never fork again and never replay an
/// ambiguous submission.
#[test]
fn native_normal_process_pre_submission_visibility_loss_recovers_exact_retained_fork() {
    let workspace = Workspace::new("normal-process-pre-submission-loss", LeadMode::Delivered);
    let (controller, _request, acceptance) = accepted::accepted(&workspace);
    lifecycle::install_fixture(&workspace);
    lifecycle::success(lifecycle::command(&workspace, "enable", Some(LEAD_THREAD)));
    let state_dir = controller
        .state_file()
        .unwrap()
        .with_extension("")
        .join(&acceptance);

    // The owned frontend double exits as soon as this marker exists. The
    // ordinary process still creates and measures the exact fork, then loses
    // its surface before any turn is submitted.
    fs::write(
        workspace.home.join("frontend-release"),
        "pre-submission surface loss",
    )
    .unwrap();
    let mut first_spec =
        lifecycle::process_spec_from(&workspace, &manager(), LEAD_THREAD, "process");
    first_spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_OUTCOME".into(),
        Some("mutator-window".into()),
    );
    first_spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_RESULT".into(),
        Some(EXECUTOR_RESULT.into()),
    );
    let session = ConsoleSession::spawn(ConsoleSpec::new(first_spec)).unwrap();
    let lost = wait_for_stage_status_in(
        &state_dir,
        "visibility-lost",
        "the ordinary process did not retain the pre-submission surface loss",
    );
    assert_eq!(lost["fork"]["forkThread"], EXECUTOR_FORK, "{lost}");
    assert!(lost["turnId"].is_null(), "{lost}");
    assert!(
        lost["error"].as_str().is_some_and(|error| {
            error.contains("no turn was submitted") && error.contains("retryable")
        }),
        "{lost}"
    );
    let accepted_work_before = read_json(&state_dir.join("chain.json"))["acceptedWork"].clone();
    let work_before = lost["work"].clone();
    let source_context_before = lost["sourceContextBytes"].clone();
    assert_eq!(
        request_counts(&workspace),
        (1, 0, 0, 0),
        "the pre-submission loss submitted model work"
    );
    assert!(
        fixture_requests(&workspace, "executor-self-reflection", "thread/start").is_empty(),
        "the first pass started a new thread instead of forking the source"
    );
    let in_flight = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(in_flight.executor.state, RecordedStageState::InFlight);
    assert_eq!(in_flight.executor.usage.attempts, 1);
    assert_eq!(in_flight.executor.usage.turns, 0);
    let finished = wait_session(session);
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert_eq!(read_json(&state_dir.join("executor-stage.json")), lost);
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap(),
        in_flight,
        "the pre-submission surface loss changed the accepted ledger"
    );
    assert!(
        !workspace.request.exists(),
        "ordinary process must derive the retained request instead of writing one"
    );

    // A second pre-submission loss must remain safely retryable: the owner
    // resumes the known fork again, submits no turn, and keeps the ledger in
    // flight for another ordinary-process attempt.
    let still_lost_spec =
        lifecycle::process_spec_from(&workspace, &manager(), LEAD_THREAD, "process");
    let still_lost =
        wait_session(ConsoleSession::spawn(ConsoleSpec::new(still_lost_spec)).unwrap());
    assert_ne!(still_lost.outcome.exit_code, 0, "{}", still_lost.transcript);
    let retried_loss = read_json(&state_dir.join("executor-stage.json"));
    assert_eq!(retried_loss["status"], "visibility-lost", "{retried_loss}");
    assert_eq!(
        retried_loss["fork"]["forkThread"], EXECUTOR_FORK,
        "{retried_loss}"
    );
    assert!(retried_loss["turnId"].is_null(), "{retried_loss}");
    assert!(
        retried_loss["error"].as_str().is_some_and(|error| {
            error.contains("pre-submission reflection recovery")
                && error.contains("No turn was submitted")
        }),
        "{retried_loss}"
    );
    assert_eq!(
        request_counts(&workspace),
        (1, 0, 0, 0),
        "repeated pre-submission loss submitted model work"
    );
    let retried_in_flight = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(
        retried_in_flight.executor.state,
        RecordedStageState::InFlight
    );
    assert_eq!(retried_in_flight.executor.usage.attempts, 1);
    assert_eq!(retried_in_flight.executor.usage.turns, 0);
    let retry_resumes = fixture_requests(&workspace, "executor-self-reflection", "thread/resume");
    assert_eq!(retry_resumes.len(), 1, "{retry_resumes:?}");
    assert_eq!(
        retry_resumes[0]["params"]["threadId"], EXECUTOR_FORK,
        "{retry_resumes:?}"
    );

    fs::remove_file(workspace.home.join("frontend-release")).unwrap();
    let mut recovering_spec =
        lifecycle::process_spec_from(&workspace, &manager(), LEAD_THREAD, "process");
    recovering_spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_RESULT".into(),
        Some(EXECUTOR_RESULT.into()),
    );
    let recovering = ConsoleSession::spawn(ConsoleSpec::new(recovering_spec)).unwrap();
    wait_until(
        || {
            fs::read(state_dir.join("executor-stage.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .is_some_and(|stage| stage["status"] == "completed")
        },
        Duration::from_secs(20),
        "the ordinary process did not complete the recovered pre-submission fork",
    );
    assert_exact_reflection_frontend_in(&workspace, &state_dir);
    let recovered = wait_session(recovering);
    assert_eq!(recovered.outcome.exit_code, 0, "{}", recovered.transcript);

    let executor = read_json(&state_dir.join("executor-stage.json"));
    assert_eq!(executor["status"], "completed", "{executor}");
    assert_eq!(executor["fork"]["forkThread"], EXECUTOR_FORK, "{executor}");
    assert_eq!(executor["turnId"], EXECUTOR_FORK_TURN, "{executor}");
    assert_eq!(executor["result"], EXECUTOR_RESULT, "{executor}");
    assert_actual_route(&executor);
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
        "recovery forked or submitted the executor twice"
    );
    let executor_turns = fixture_requests(&workspace, "executor-self-reflection", "turn/start");
    assert_eq!(executor_turns.len(), 1, "{executor_turns:?}");
    assert_eq!(
        executor_turns[0]["params"]["threadId"], EXECUTOR_FORK,
        "{executor_turns:?}"
    );
    assert!(
        fixture_requests(&workspace, "executor-self-reflection", "thread/start").is_empty(),
        "recovery started a new thread"
    );
    let resumes = fixture_requests(&workspace, "executor-self-reflection", "thread/resume");
    assert_eq!(resumes.len(), 2, "{resumes:?}");
    for resume in &resumes {
        assert_eq!(resume["params"]["threadId"], EXECUTOR_FORK, "{resumes:?}");
    }
    executor_rollout(&workspace.home);
    assert_eq!(
        read_json(&state_dir.join("chain.json"))["acceptedWork"],
        accepted_work_before,
        "recovery changed the accepted context"
    );

    let lead = read_json(&state_dir.join("lead-stage.json"));
    assert_eq!(lead["status"], "completed", "{lead}");
    assert_eq!(lead["fork"]["forkThread"], LEAD_FORK, "{lead}");
    assert_eq!(lead["turnId"], LEAD_FORK_TURN, "{lead}");
    assert_eq!(lead["result"], LEAD_RESULT, "{lead}");
    assert_eq!(lead["profile"], "default", "{lead}");
    assert_eq!(lead["provider"], PROVIDER, "{lead}");
    assert_eq!(lead["model"], MODEL, "{lead}");
    assert_eq!(lead["effort"], EFFORT, "{lead}");
    assert_eq!(
        lead["roleRevision"].as_str(),
        Some(role_facts().lead_revision.as_str()),
        "{lead}"
    );
    let delivery = read_json(&state_dir.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "delivered", "{delivery}");
    assert_eq!(delivery["by"], "clientId", "{delivery}");
    let lead_requests = workspace.lead.requests_for("turn/start");
    assert_eq!(lead_requests.len(), 1, "{lead_requests:?}");
    assert_eq!(
        lead_requests[0]["params"]["clientUserMessageId"], delivery["clientMessageId"],
        "{lead_requests:?}"
    );

    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, RecordedStageState::Retained);
    assert_eq!(record.lead.state, RecordedStageState::Retained);
    assert_eq!(record.delivery, DeliveryState::Consumed);
    assert_eq!(
        record.executor.usage.attempts, 1,
        "pre-submission recovery began a second stage attempt"
    );
    assert_eq!(record.executor.usage.turns, 1);
    assert_eq!(record.executor.usage.tokens, 120);
    assert_eq!(record.executor.usage.investigations, 0);
    assert_eq!(
        record.executor.usage.context_bytes,
        executor["work"]["context_bytes"].as_u64().unwrap()
    );
    assert_eq!(record.lead.usage.attempts, 1);
    assert_eq!(record.lead.usage.turns, 1);
    assert_eq!(record.lead.usage.tokens, 240);
    assert_eq!(record.usage_totals().tokens, 360);
    assert_eq!(controller.stored_state().unwrap().unwrap().records.len(), 1);
    assert!(
        !workspace.request.exists(),
        "ordinary process must not write a caller-authored request"
    );
}
