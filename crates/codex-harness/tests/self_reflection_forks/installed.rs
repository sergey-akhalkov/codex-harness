//! The ordinary accepted reflection lifecycle through an immutable installed
//! bundle outside this checkout. The native board, retained delivery, installed
//! manager, installed launcher and registered upstream all participate; only
//! the model conversations are the existing canned fixture.

use super::reflection_install::{InstalledReflection, PRESERVED};
use super::*;
use harness_core::delivery_reflection::{
    DeliveryState, JournalOwnership, LifecycleStatus, ReflectionController, StageState,
};
use std::os::windows::fs::OpenOptionsExt;

#[test]
fn installed_acceptance_drives_reflections_disables_and_preserves_restored_state() {
    let workspace = Workspace::new("reflection-installed", LeadMode::Delivered);
    let installed = InstalledReflection::connect(&workspace);

    let (task, acceptance, activated) =
        lifecycle::normal_acceptance(&workspace, &installed.manager);
    assert_eq!(activated.lifecycle, LifecycleStatus::Active);
    assert_eq!(activated.binding.lead_session, LEAD_THREAD);
    assert_eq!(activated.journal, JournalOwnership::EnabledByController);
    assert_eq!(activated.records.len(), 1);
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    installed.assert_app_servers(&workspace.checkout);

    let controller = ReflectionController::new("bd.exe", &workspace.checkout, &workspace.home);
    let state = controller.stored_state().unwrap().unwrap();
    assert_eq!(state.lifecycle, LifecycleStatus::Stopped);
    assert_eq!(state.binding, activated.binding);
    assert_eq!(state.checkpoint, activated.checkpoint);
    assert_eq!(state.journal, activated.journal);
    assert_eq!(state.records.len(), 1);
    let record = state.records.get(&acceptance).unwrap().clone();
    assert_eq!(record.delivery, DeliveryState::Consumed);
    let retained = record.binding.result.clone();
    let board_before = board_show(&workspace, &task);
    let config_before = fs::read_to_string(workspace.home.join("config.toml")).unwrap();
    assert!(config_before.contains(PRESERVED), "{config_before}");

    let report = installed.disconnect();
    assert_eq!(report.status, "disconnected");
    assert!(
        !workspace
            .home
            .join("harness/bin/codex-harness.exe")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(workspace.home.join("config.toml")).unwrap(),
        config_before,
        "disconnect preserves the unrelated configuration byte for byte"
    );
    assert_eq!(
        controller.stored_state().unwrap().unwrap().records[&acceptance],
        record,
        "disconnect preserves the retained delivery"
    );

    let report = installed.reconnect();
    assert_eq!(report.status, "connected");
    assert!(report.runtime.unwrap().passed);
    installed.assert_installed(&workspace);
    assert_eq!(
        fs::read_to_string(workspace.home.join("config.toml")).unwrap(),
        config_before,
        "update restoration preserves the unrelated configuration"
    );
    assert_eq!(
        board_show(&workspace, &task),
        board_before,
        "the accepted board issue survives disconnection and restoration"
    );
    let restored = controller.stored_state().unwrap().unwrap();
    assert_eq!(restored.records.len(), 1);
    assert_eq!(restored.records[&acceptance], record);
    assert_eq!(restored.records[&acceptance].binding.result, retained);

    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    installed.assert_app_servers(&workspace.checkout);
}

#[test]
fn installed_in_flight_disable_and_failed_install_changes_preserve_pending_work() {
    let workspace = Workspace::new("reflection-installed-pending", LeadMode::Delivered);
    let installed = InstalledReflection::connect(&workspace);
    let (controller, task, acceptance, completion_manifest, retained_result) =
        prepare_installed_acceptance(&workspace, &installed);
    let unrelated = workspace.checkout.join("unrelated-preserved.txt");
    fs::write(&unrelated, "unrelated owned state stays untouched\n").unwrap();
    let source =
        PreservedSource::capture(&workspace, &task, &completion_manifest, &retained_result);

    let initial = controller.stored_state().unwrap().unwrap();
    assert_eq!(initial.lifecycle, LifecycleStatus::Active);
    assert!(
        initial.records.is_empty(),
        "the native server owns board-event ingestion"
    );

    let session = ConsoleSession::spawn(ConsoleSpec::new(installed_process_spec(
        &workspace,
        &installed,
        "mutator-window",
    )))
    .unwrap();
    wait_for_path(
        &workspace
            .fixture_dir
            .join("turn-executor-self-reflection.json"),
        Duration::from_secs(20),
    );
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if controller
            .record(&acceptance)
            .unwrap()
            .is_some_and(|record| record.executor.state == StageState::InFlight)
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "the executor turn did not retain an in-flight record: {:?}",
            controller.record(&acceptance).unwrap()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));

    let stopped = installed_success(
        installed_command(&installed, &workspace, "disable", &[])
            .output()
            .unwrap(),
    );
    assert_eq!(stopped["stopped"]["retained_records"], 1, "{stopped}");
    let during = controller.stored_state().unwrap().unwrap();
    assert_eq!(during.lifecycle, LifecycleStatus::Stopped);
    assert_eq!(
        during.records[&acceptance].executor.state,
        StageState::InFlight
    );
    assert_eq!(during.records[&acceptance].lead.state, StageState::Pending);
    source.assert_unchanged(&workspace, &task);

    fs::write(
        workspace
            .fixture_dir
            .join("release-executor-self-reflection.marker"),
        "release the already-started native turn",
    )
    .unwrap();
    workspace.lead_state.lock().unwrap().source_turn_active = false;
    let finished = wait_session(session);
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert!(
        finished
            .transcript
            .contains("delivery reflection is stopped"),
        "{}",
        finished.transcript
    );

    let retained = controller.stored_state().unwrap().unwrap();
    assert_eq!(retained.lifecycle, LifecycleStatus::Stopped);
    assert_eq!(
        retained.records[&acceptance].executor.state,
        StageState::InFlight
    );
    assert_eq!(
        retained.records[&acceptance].lead.state,
        StageState::Pending
    );
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    let chain = controller
        .state_file()
        .unwrap()
        .with_extension("")
        .join(&acceptance);
    assert!(!chain.join("lead-stage.json").exists());
    let executor_stage = fs::read(chain.join("executor-stage.json")).unwrap();
    let native: Value = serde_json::from_slice(&executor_stage).unwrap();
    assert_eq!(native["status"], "completed", "{native}");
    assert_eq!(native["result"], EXECUTOR_RESULT, "{native}");
    assert_eq!(native["turnId"], EXECUTOR_FORK_TURN, "{native}");
    assert_eq!(native["usageKnown"], true, "{native}");
    assert_eq!(native["usage"]["total_tokens"], 120, "{native}");
    source.assert_unchanged(&workspace, &task);

    let launch_path = workspace.home.join("harness/native-launch.json");
    let installation_path = workspace.home.join("harness/installation.json");
    let launch_before = fs::read(&launch_path).unwrap();
    let installation_before = fs::read(&installation_path).unwrap();
    let launch: Value = serde_json::from_slice(&launch_before).unwrap();
    let build = PathBuf::from(launch["build"].as_str().unwrap());
    let upstream = PathBuf::from(launch["upstream"]["executable"].as_str().unwrap());
    let update = {
        let _held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&installation_path)
            .unwrap();
        installed_update(&installed, &workspace, &build, &upstream)
            .output()
            .unwrap()
    };
    assert!(
        !update.status.success(),
        "locked installation metadata must refuse the update: {} {}",
        String::from_utf8_lossy(&update.stdout),
        String::from_utf8_lossy(&update.stderr)
    );
    assert_eq!(controller.stored_state().unwrap().unwrap(), retained);
    assert_eq!(fs::read(&installation_path).unwrap(), installation_before);
    assert_eq!(fs::read(&launch_path).unwrap(), launch_before);
    source.assert_unchanged(&workspace, &task);

    let launcher = workspace.home.join("harness/bin/codex.exe");
    let held_launcher = workspace.home.join("harness/bin/codex.exe.activation-held");
    fs::rename(&launcher, &held_launcher).unwrap();
    let activation = installed_command(&installed, &workspace, "enable", &[])
        .output()
        .unwrap();
    fs::rename(&held_launcher, &launcher).unwrap();
    assert!(!activation.status.success());
    assert!(
        String::from_utf8_lossy(&activation.stderr)
            .contains("the installed Codex launcher is missing"),
        "{}",
        String::from_utf8_lossy(&activation.stderr)
    );
    assert_eq!(controller.stored_state().unwrap().unwrap(), retained);
    source.assert_unchanged(&workspace, &task);
    installed.assert_installed(&workspace);

    let report = installed.disconnect();
    assert_eq!(report.status, "disconnected");
    assert!(!installed.manager.exists());
    assert_eq!(controller.stored_state().unwrap().unwrap(), retained);
    source.assert_unchanged(&workspace, &task);

    let report = installed.reconnect();
    assert_eq!(report.status, "connected");
    assert!(report.runtime.unwrap().passed);
    installed.assert_installed(&workspace);
    assert_eq!(controller.stored_state().unwrap().unwrap(), retained);
    source.assert_unchanged(&workspace, &task);

    installed_success(
        installed_command(&installed, &workspace, "enable", &[])
            .output()
            .unwrap(),
    );
    let resumed = controller.stored_state().unwrap().unwrap();
    assert_eq!(resumed.lifecycle, LifecycleStatus::Active);
    assert_eq!(resumed.records, retained.records);
    assert_eq!(resumed.journal, retained.journal);
    assert_eq!(resumed.checkpoint, retained.checkpoint);

    let session = ConsoleSession::spawn(ConsoleSpec::new(installed_process_spec(
        &workspace, &installed, "complete",
    )))
    .unwrap();
    let until = Instant::now() + Duration::from_secs(90);
    loop {
        if controller
            .record(&acceptance)
            .unwrap()
            .is_some_and(|record| record.delivery == DeliveryState::Consumed)
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "the retained executor result did not reach the owned lead: {:?}",
            controller.record(&acceptance).unwrap()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    assert_eq!(
        fs::read(chain.join("executor-stage.json")).unwrap(),
        executor_stage
    );
    installed_success(
        installed_command(&installed, &workspace, "disable", &[])
            .output()
            .unwrap(),
    );
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);

    let completed = controller.stored_state().unwrap().unwrap();
    let record = &completed.records[&acceptance];
    assert_eq!(completed.lifecycle, LifecycleStatus::Stopped);
    assert_eq!(record.executor.state, StageState::Retained);
    assert_eq!(record.lead.state, StageState::Retained);
    assert_eq!(record.delivery, DeliveryState::Consumed);
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    source.assert_unchanged(&workspace, &task);
    installed.assert_installed(&workspace);
    installed.assert_app_servers(&workspace.checkout);

    let replay = installed_command(&installed, &workspace, "process", &[])
        .output()
        .unwrap();
    installed_success(replay);
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    assert_eq!(controller.stored_state().unwrap().unwrap(), completed);
}

#[derive(Debug)]
struct PreservedSource {
    board: Value,
    board_config: Value,
    files: Vec<(PathBuf, Vec<u8>)>,
}

impl PreservedSource {
    fn capture(
        workspace: &Workspace,
        task: &str,
        completion_manifest: &Path,
        retained_result: &Path,
    ) -> Self {
        let files = [
            workspace.home.join("config.toml"),
            workspace.root().join("executor-result.txt"),
            completion_manifest.to_owned(),
            retained_result.to_owned(),
            workspace.checkout.join("unrelated-preserved.txt"),
        ]
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path).unwrap_or_else(|error| {
                panic!("read preserved source {}: {error}", path.display())
            });
            (path, bytes)
        })
        .collect();
        Self {
            board: board_show(workspace, task),
            board_config: board_config(workspace),
            files,
        }
    }

    fn assert_unchanged(&self, workspace: &Workspace, task: &str) {
        assert_eq!(board_show(workspace, task), self.board);
        assert_eq!(board_config(workspace), self.board_config);
        for (path, expected) in &self.files {
            assert_eq!(
                fs::read(path).unwrap(),
                *expected,
                "{} changed outside the retained reflection lifecycle",
                path.display()
            );
        }
    }
}

fn prepare_installed_acceptance(
    workspace: &Workspace,
    installed: &InstalledReflection,
) -> (ReflectionController, String, String, PathBuf, PathBuf) {
    use harness_core::{
        board_cli,
        delivery_reflection::{Purpose, board_source_identity},
        task_worktree::{self, AssignmentSubmission, CompletionFacts, SlotAssignmentBinding},
    };

    let (controller, task) = accepted::prepare(workspace);
    {
        let mut lead = workspace.lead_state.lock().unwrap();
        lead.source_turn = Some(LEAD_SOURCE_TURN.to_owned());
        lead.source_turn_active = true;
    }
    installed_success(
        installed_command(installed, workspace, "enable", &[])
            .output()
            .unwrap(),
    );
    board_cli::set_purpose_metadata(
        Path::new("bd.exe"),
        &workspace.checkout,
        &task,
        Purpose::Ordinary,
    )
    .unwrap();
    accepted::native(workspace, "git", &["add", "."]);
    accepted::native(
        workspace,
        "git",
        &["commit", "--allow-empty", "-qm", "fixture board ready"],
    );
    let assignment = harness_core::build_identity::hash_bytes(
        b"synthetic in-flight installed structured assignment",
    );
    let result = workspace.root().join("executor-result.txt");
    fs::write(
        &result,
        "Synthetic in-flight installed executor result with completed checks.",
    )
    .unwrap();
    let pool = task_worktree::pool_state_dir(&workspace.home, &kit_root()).unwrap();
    fs::create_dir_all(&pool).unwrap();
    let retained = task_worktree::retain_completion(
        &pool,
        &CompletionFacts {
            run_generation: "installed-in-flight-run".to_owned(),
            slot: 1,
            owner: "fixture-installed-executor".to_owned(),
            checkout: workspace.checkout.clone(),
            task: Some("fixture-durable-task".to_owned()),
            handle: Some("fixture-task-handle".to_owned()),
            assignment: Some(SlotAssignmentBinding {
                source: board_source_identity(Path::new("bd.exe"), &workspace.checkout).unwrap(),
                issue: task.clone(),
                assignment_sha256: assignment.clone(),
                purpose: Some("ordinary".to_owned()),
                purpose_source: "board".to_owned(),
            }),
            submitted_assignment: Some(AssignmentSubmission::structured(assignment)),
            role: Some("executor".to_owned()),
            state: "completed".to_owned(),
            exit_code: Some(0),
            session: Some(EXECUTOR_SOURCE.to_owned()),
            turn: Some(EXECUTOR_SOURCE_TURN.to_owned()),
            result: Some(result),
        },
    )
    .unwrap();
    assert!(retained.eligibility.eligible, "{retained:?}");
    assert_eq!(retained.slot, 1);
    let retained_result = retained.result.as_ref().unwrap().locator.clone();
    let completion_manifest = pool
        .join("completions")
        .join("installed-in-flight-run")
        .join("completion.json");
    assert!(completion_manifest.is_file());

    let accepted = installed_success(
        Command::new(&installed.manager)
            .args(["executor", "accept", &task, "--project"])
            .arg(&workspace.checkout)
            .arg("--source")
            .arg(kit_root())
            .arg("--codex-home")
            .arg(&workspace.home)
            .current_dir(&workspace.checkout)
            .env("CODEX_HOME", &workspace.home)
            .env("CODEX_THREAD_ID", LEAD_THREAD)
            .env_remove("HARNESS_REFLECTION_SESSION")
            .env_remove("HARNESS_EXECUTOR_SESSION")
            .env_remove("HARNESS_BOUNDED_LEAD_SESSION")
            .output()
            .unwrap(),
    );
    assert_eq!(accepted["duplicate"], false);
    assert_eq!(
        accepted["reflection"]["automaticTrigger"], true,
        "{accepted}"
    );
    assert_eq!(accepted["leadTurn"], LEAD_SOURCE_TURN);
    assert_eq!(accepted["executorTurn"], EXECUTOR_SOURCE_TURN);
    assert_eq!(request_counts(workspace), (0, 0, 0, 0));
    (
        controller,
        task,
        accepted["acceptanceId"].as_str().unwrap().to_owned(),
        completion_manifest,
        retained_result,
    )
}

fn board_show(workspace: &Workspace, task: &str) -> Value {
    let output = Command::new("bd.exe")
        .args(["show", task, "--json"])
        .current_dir(&workspace.checkout)
        .env("BD_NON_INTERACTIVE", "1")
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

fn installed_process_spec(
    workspace: &Workspace,
    installed: &InstalledReflection,
    executor_outcome: &str,
) -> CommandSpec {
    let mut spec = CommandSpec::new(installed.manager.as_path());
    spec.args = vec![
        "delivery-improvement".into(),
        "_serve".into(),
        "--project".into(),
        workspace.checkout.as_os_str().to_owned(),
        "--source".into(),
        kit_root().into_os_string(),
        "--codex-home".into(),
        workspace.home.as_os_str().to_owned(),
    ];
    spec.current_dir = Some(workspace.checkout.clone());
    apply_fixture_env(
        workspace,
        &mut spec,
        LEAD_THREAD,
        executor_outcome,
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    spec.env
        .insert("HARNESS_REFLECTION_HOST".into(), Some(LEAD_THREAD.into()));
    spec.env
        .insert("CODEX_THREAD_ID".into(), Some(LEAD_THREAD.into()));
    spec
}

fn installed_command(
    installed: &InstalledReflection,
    workspace: &Workspace,
    action: &str,
    extra: &[&str],
) -> Command {
    let mut command = Command::new(&installed.manager);
    command
        .args(["delivery-improvement", action, "--project"])
        .arg(&workspace.checkout)
        .arg("--source")
        .arg(kit_root())
        .arg("--codex-home")
        .arg(&workspace.home)
        .args(extra)
        .current_dir(&workspace.checkout)
        .env("CODEX_HOME", &workspace.home)
        .env("CODEX_THREAD_ID", LEAD_THREAD)
        .env_remove("HARNESS_REFLECTION_SESSION")
        .env_remove("HARNESS_EXECUTOR_SESSION")
        .env_remove("HARNESS_BOUNDED_LEAD_SESSION");
    command
}

fn installed_update(
    installed: &InstalledReflection,
    workspace: &Workspace,
    build: &Path,
    upstream: &Path,
) -> Command {
    let root = build.parent().expect("installed build has a private root");
    let mut command = Command::new(&installed.manager);
    command
        .args(["update", "--core-only", "--source"])
        .arg(kit_root())
        .arg("--build")
        .arg(build)
        .arg("--codex-home")
        .arg(&workspace.home)
        .arg("--user-home")
        .arg(root.join("user"))
        .arg("--dependency-user-home")
        .arg(root.join("dependency-user"))
        .arg("--upstream")
        .arg(upstream)
        .args(["--timeout-seconds", "30", "--path-scope", "Process"])
        .current_dir(&workspace.checkout)
        .env("CODEX_HOME", &workspace.home)
        .env_remove("HARNESS_REFLECTION_SESSION")
        .env_remove("HARNESS_EXECUTOR_SESSION")
        .env_remove("HARNESS_BOUNDED_LEAD_SESSION");
    command
}

fn installed_success(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "installed command failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "installed command did not return JSON: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn board_config(workspace: &Workspace) -> Value {
    let output = Command::new("bd.exe")
        .args(["config", "list", "--json"])
        .current_dir(&workspace.checkout)
        .env("BD_NON_INTERACTIVE", "1")
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
