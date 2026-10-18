//! User-facing lifecycle commands consume the real board's accepted identity.

use super::*;
use harness_core::delivery_reflection::{DeliveryState, LifecycleStatus, StageState};

pub(super) fn install_fixture(workspace: &Workspace) {
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

pub(super) fn command(
    workspace: &Workspace,
    action: &str,
    lead: Option<&str>,
) -> std::process::Output {
    command_with(workspace, action, lead, &[])
}

fn manager_command(
    manager: &Path,
    workspace: &Workspace,
    action: &str,
    lead: Option<&str>,
    extra: &[&str],
) -> Command {
    let mut command = Command::new(manager);
    command
        .args(["delivery-improvement", action, "--project"])
        .arg(&workspace.checkout)
        .arg("--source")
        .arg(kit_root())
        .arg("--codex-home")
        .arg(&workspace.home)
        .args(extra)
        .env("CODEX_HOME", &workspace.home)
        .env_remove("HARNESS_REFLECTION_SESSION")
        .env_remove("HARNESS_EXECUTOR_SESSION")
        .env_remove("CODEX_THREAD_ID");
    if let Some(lead) = lead {
        command.env("CODEX_THREAD_ID", lead);
    }
    command
}

fn command_with(
    workspace: &Workspace,
    action: &str,
    lead: Option<&str>,
    extra: &[&str],
) -> std::process::Output {
    manager_command(&manager(), workspace, action, lead, extra)
        .output()
        .unwrap()
}

fn command_from(
    manager: &Path,
    workspace: &Workspace,
    action: &str,
    lead: Option<&str>,
    extra: &[&str],
) -> std::process::Output {
    manager_command(manager, workspace, action, lead, extra)
        .current_dir(&workspace.checkout)
        .output()
        .unwrap()
}

pub(super) fn success(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn process(workspace: &Workspace, request: &Value) -> ConsoleOutcome {
    wait_session(
        ConsoleSession::spawn(ConsoleSpec::new(process_spec(
            workspace, request, "process",
        )))
        .unwrap(),
    )
}

fn process_spec(workspace: &Workspace, request: &Value, action: &str) -> CommandSpec {
    let lead_thread = request["leadSource"]["thread"]
        .as_str()
        .unwrap_or(LEAD_THREAD);
    process_spec_from(workspace, &manager(), lead_thread, action)
}

/// Derived native inputs must suffice without a caller-authored request.

pub(super) fn process_spec_from(
    workspace: &Workspace,
    manager: &Path,
    lead_thread: &str,
    action: &str,
) -> CommandSpec {
    let mut spec = CommandSpec::new(manager);
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
    spec.current_dir = Some(workspace.checkout.clone());
    apply_fixture_env(
        workspace,
        &mut spec,
        lead_thread,
        "complete",
        "complete",
        "NO FINDING",
        LEAD_RESULT,
    );
    spec.env
        .insert("HARNESS_REFLECTION_HOST".into(), Some(lead_thread.into()));
    spec.env
        .insert("CODEX_THREAD_ID".into(), Some(LEAD_THREAD.into()));
    spec
}

#[test]
fn normal_cli_derives_the_chain_and_retains_completed_delivery_without_replay() {
    let workspace = Workspace::new("lifecycle-process", LeadMode::Delivered);
    let (controller, request, acceptance) = accepted::accepted(&workspace);
    install_fixture(&workspace);
    let before = controller.stored_state().unwrap().unwrap();
    let enabled = success(command(&workspace, "enable", Some(LEAD_THREAD)));
    assert_eq!(enabled["checkpoint"], before.checkpoint);
    assert_eq!(enabled["revision"], before.binding.revision);
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));

    let completed = process(&workspace, &request);
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert_eq!(record.lead.state, StageState::Retained);
    assert_eq!(record.delivery, DeliveryState::Consumed);
    let state = controller
        .state_file()
        .unwrap()
        .with_extension("")
        .join(&acceptance);
    let chain = read_json(&state.join("chain.json"));
    assert_eq!(chain["task"], request["task"]);
    assert_eq!(chain["executorSource"], request["executorSource"]);
    assert_eq!(chain["leadSource"], request["leadSource"]);
    assert_eq!(chain["executorStage"]["profile"], PROFILE);
    assert_eq!(chain["leadStage"]["profile"], "default");
    assert_eq!(
        chain["acceptedWork"],
        serde_json::to_value(&record.binding).unwrap()
    );
    let calls = request_counts(&workspace);
    let replay = process(&workspace, &request);
    assert_eq!(replay.outcome.exit_code, 0, "{}", replay.transcript);
    assert_eq!(request_counts(&workspace), calls);
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);
    let status = success(command(&workspace, "status", None));
    assert_eq!(status["pending"], 0);
    assert_eq!(status["retained"], 1);
}

#[test]
fn duplicate_enable_reuses_the_live_binding_and_retained_records() {
    let workspace = Workspace::new("lifecycle-duplicate-enable", LeadMode::Delivered);
    let (controller, _request, acceptance) = accepted::accepted(&workspace);
    install_fixture(&workspace);
    let initial = controller.stored_state().unwrap().unwrap();
    assert_eq!(initial.lifecycle, LifecycleStatus::Active);
    assert_eq!(initial.records.len(), 1);
    let retained = initial.records.get(&acceptance).unwrap().clone();

    let first = success(command(&workspace, "enable", Some(LEAD_THREAD)));
    assert_eq!(first["reused"].as_bool(), Some(true), "{first}");
    assert_eq!(
        first["binding_id"].as_str(),
        Some(initial.binding.id.as_str()),
        "{first}"
    );
    assert_eq!(
        first["revision"].as_u64(),
        Some(initial.binding.revision),
        "{first}"
    );
    assert_eq!(
        first["checkpoint"].as_u64(),
        Some(initial.checkpoint),
        "{first}"
    );
    assert_eq!(
        first["bound_records"].as_u64(),
        Some(initial.records.len() as u64),
        "{first}"
    );
    let after_first = controller.stored_state().unwrap().unwrap();

    // A repeated enable of the same live binding is an exact no-op: the same
    // generation, checkpoint and retained records remain, and no fixture
    // request (fork or turn) is issued.
    let second = success(command(&workspace, "enable", Some(LEAD_THREAD)));
    assert_eq!(second, first);
    let after_second = controller.stored_state().unwrap().unwrap();
    assert_eq!(after_second, after_first);
    assert_eq!(after_second.records.get(&acceptance), Some(&retained));
    assert_eq!(after_second.binding, initial.binding);
    assert_eq!(after_second.checkpoint, initial.checkpoint);
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
    let status = success(command(&workspace, "status", None));
    assert_eq!(status["retained"].as_u64(), Some(1), "{status}");
}

#[test]
fn process_without_a_new_accepted_outcome_is_model_free_and_preserves_state() {
    let workspace = Workspace::new("lifecycle-no-new-outcome", LeadMode::Delivered);
    let (controller, _task) = accepted::prepare(&workspace);
    install_fixture(&workspace);
    success(command(&workspace, "enable", Some(LEAD_THREAD)));
    let before = controller.stored_state().unwrap().unwrap();
    assert_eq!(before.lifecycle, LifecycleStatus::Active);
    assert!(before.records.is_empty(), "{before:?}");
    let before_json = serde_json::to_value(&before).unwrap();
    let state_entries = || {
        fs::read_dir(&workspace.state)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>()
    };
    assert!(state_entries().is_empty());

    // The fixture board has a committed open task but no accepted closure.
    // The first explicit process poll may advance the journal cursor over
    // non-eligible board events; after that, repeated processing is model-free
    // and changes only its own poll stamp. No stage or delivery result file
    // appears and no reflection record is created.
    let mut after_first = None;
    for iteration in 0..2 {
        let status = success(command(&workspace, "process", Some(LEAD_THREAD)));
        assert_eq!(status["pending"].as_u64(), Some(0), "{status}");
        assert_eq!(status["retained"].as_u64(), Some(0), "{status}");
        assert_eq!(status["unfinished"].as_u64(), Some(0), "{status}");
        assert_eq!(status["undelivered"].as_u64(), Some(0), "{status}");
        assert_eq!(status["lifecycle"].as_str(), Some("active"), "{status}");

        let state = controller.stored_state().unwrap().unwrap();
        let poll = state.last_poll.as_ref().expect("explicit poll stamp");
        assert_eq!(poll.accepted, 0);
        assert_eq!(poll.duplicates, 0);
        assert_eq!(poll.model_calls, 0);
        let mut state_without_poll = serde_json::to_value(&state).unwrap();
        state_without_poll["last_poll"] = before_json["last_poll"].clone();
        if iteration == 0 {
            state_without_poll["checkpoint"] = before_json["checkpoint"].clone();
            assert_eq!(state_without_poll, before_json);
            after_first = Some((serde_json::to_value(&state).unwrap(), poll.checkpoint));
        } else {
            let (first, first_checkpoint) = after_first.as_ref().unwrap();
            assert_eq!(poll.checkpoint, *first_checkpoint);
            let mut first_without_poll = first.clone();
            first_without_poll["last_poll"] = before_json["last_poll"].clone();
            assert_eq!(state_without_poll, first_without_poll);
        }
        assert!(state_entries().is_empty());
        assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
    }
}

#[test]
fn normal_accept_command_drives_owned_consumer_after_exact_lead_turn_completes() {
    let workspace = Workspace::new("lifecycle-normal-accept", LeadMode::Delivered);
    install_fixture(&workspace);
    normal_acceptance(&workspace, &manager());
}

/// The ordinary accepted-work lifecycle, independent of where the manager and
/// launcher were installed. The producer is the public retained completion and
/// the only acceptance identity is the board's exact native acceptance.
pub(super) fn normal_acceptance(workspace: &Workspace, manager: &Path) -> (String, String) {
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
    success(command_from(
        manager,
        workspace,
        "enable",
        Some(LEAD_THREAD),
        &[],
    ));
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
    let assignment = harness_core::build_identity::hash_bytes(b"synthetic structured assignment");
    let result = workspace.root().join("executor-result.txt");
    fs::write(
        &result,
        "Synthetic accepted executor result with completed checks.",
    )
    .unwrap();
    let pool = task_worktree::pool_state_dir(&workspace.home, &kit_root()).unwrap();
    fs::create_dir_all(&pool).unwrap();
    let retained = task_worktree::retain_completion(
        &pool,
        &CompletionFacts {
            run_generation: "accepted-run".to_owned(),
            slot: 1,
            owner: "fixture-ordinary-executor".to_owned(),
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

    let accept = || {
        let output = Command::new(manager)
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
            .unwrap();
        success(output)
    };
    let accepted = accept();
    assert_eq!(accepted["duplicate"], false);
    assert_eq!(accepted["reflection"]["automaticTrigger"], true);
    assert_eq!(accepted["leadTurn"], LEAD_SOURCE_TURN);
    assert_eq!(accepted["executorTurn"], EXECUTOR_SOURCE_TURN);
    let acceptance = accepted["acceptanceId"].as_str().unwrap();
    let repeated = accept();
    assert_eq!(repeated["duplicate"], true);
    assert_eq!(repeated["acceptanceId"], acceptance);
    assert_eq!(request_counts(workspace), (0, 0, 0, 0));

    let spec = process_spec_from(workspace, manager, LEAD_THREAD, "_serve");
    let session = ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap();
    let until = Instant::now() + Duration::from_secs(90);
    loop {
        if controller
            .record(acceptance)
            .unwrap()
            .is_some_and(|record| record.executor.state == StageState::Retained)
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "executor reflection did not retain its result: {:?}; requests={:?}",
            controller.record(acceptance).unwrap(),
            request_counts(workspace)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(request_counts(workspace), (1, 1, 0, 0));
    assert_eq!(
        controller.record(acceptance).unwrap().unwrap().lead.state,
        StageState::Pending
    );
    workspace.lead_state.lock().unwrap().source_turn_active = false;
    loop {
        if controller.record(acceptance).unwrap().unwrap().delivery == DeliveryState::Consumed {
            break;
        }
        assert!(
            Instant::now() < until,
            "exact accepted synthesis was not consumed: {:?}",
            controller.record(acceptance).unwrap()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(request_counts(workspace), (1, 1, 1, 1));
    let record = controller.record(acceptance).unwrap().unwrap();
    assert_eq!(record.binding.result, retained.result.unwrap().sha256);
    assert_eq!(record.binding.lead_turn.as_deref(), Some(LEAD_SOURCE_TURN));
    success(command_from(manager, workspace, "disable", None, &[]));
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert_eq!(controller.stored_state().unwrap().unwrap().records.len(), 1);
    assert_eq!(request_counts(workspace), (1, 1, 1, 1));
    assert!(
        !workspace.request.exists(),
        "the installed chain must derive its identity without a caller-authored request"
    );
    (task, acceptance.to_owned())
}

#[test]
fn normal_cli_reconciles_a_queued_delivery_without_repeating_stages() {
    let workspace = Workspace::new("lifecycle-queued", LeadMode::Queued);
    let (controller, request, acceptance) = accepted::accepted(&workspace);
    install_fixture(&workspace);
    let first = process(&workspace, &request);
    assert_eq!(first.outcome.exit_code, 3, "{}", first.transcript);
    let retained = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(retained.executor.state, StageState::Retained);
    assert_eq!(retained.lead.state, StageState::Retained);
    assert_ne!(retained.delivery, DeliveryState::Consumed);
    let status = success(command(&workspace, "status", None));
    assert_eq!(status["pending"], 0);
    assert_eq!(status["unfinished"], 1);
    assert_eq!(status["undelivered"], 1);
    let calls = request_counts(&workspace);
    let second = process(&workspace, &request);
    assert_eq!(second.outcome.exit_code, 3, "{}", second.transcript);
    assert_eq!(request_counts(&workspace), calls);
    assert_ne!(
        controller.record(&acceptance).unwrap().unwrap().delivery,
        DeliveryState::Consumed
    );
    workspace.lead_state.lock().unwrap().delivery_visible = true;
    let delivered = process(&workspace, &request);
    assert_eq!(delivered.outcome.exit_code, 0, "{}", delivered.transcript);
    assert_eq!(request_counts(&workspace), calls);
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap().delivery,
        DeliveryState::Consumed
    );
}

#[test]
fn owned_consumer_observes_queued_delivery_and_stops_after_disable() {
    let workspace = Workspace::new("lifecycle-owned-queue", LeadMode::Queued);
    let (controller, request, acceptance) = accepted::accepted(&workspace);
    install_fixture(&workspace);
    let state_path = controller.state_file().unwrap();
    let mut spec = process_spec(&workspace, &request, "_serve");
    spec.env
        .insert("HARNESS_REFLECTION_HOST".into(), Some(LEAD_THREAD.into()));
    let session = ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap();
    let await_delivery = |expected: DeliveryState| {
        let until = Instant::now() + Duration::from_secs(90);
        loop {
            let state: harness_core::delivery_reflection::ReflectionState =
                serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
            let record = &state.records[&acceptance];
            if record.delivery == expected {
                break;
            }
            assert!(
                Instant::now() < until,
                "expected {expected:?}; observed {:?}",
                record.delivery
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    await_delivery(DeliveryState::Unknown);
    let calls = request_counts(&workspace);
    assert_eq!(calls, (1, 1, 1, 1));
    workspace.lead_state.lock().unwrap().delivery_visible = true;
    await_delivery(DeliveryState::Consumed);
    assert_eq!(request_counts(&workspace), calls);
    success(command(&workspace, "disable", None));
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap().delivery,
        DeliveryState::Consumed
    );
}

#[test]
fn foreign_lead_is_refused_and_disabled_processing_keeps_pending_work() {
    let workspace = Workspace::new("lifecycle-off", LeadMode::Delivered);
    let (controller, _, acceptance) = accepted::accepted(&workspace);
    install_fixture(&workspace);
    let before = controller.stored_state().unwrap().unwrap();
    let missing = command(&workspace, "enable", None);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("CODEX_THREAD_ID"));
    let foreign = command(&workspace, "process", Some("foreign-lead"));
    assert!(!foreign.status.success());
    assert!(String::from_utf8_lossy(&foreign.stderr).contains("real lead is unavailable"));
    assert_eq!(controller.stored_state().unwrap().unwrap(), before);
    success(command(&workspace, "disable", None));
    success(command(&workspace, "disable", None));
    let off = success(command(&workspace, "process", None));
    assert_eq!(off["lifecycle"], "stopped");
    assert_eq!(off["retained"], 1);
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap(),
        before.records[&acceptance]
    );
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
}

#[test]
fn native_journal_recovery_is_explicit_and_preserves_retained_work() {
    use harness_core::delivery_reflection::{ActivationRequest, JournalPolicy};

    let workspace = Workspace::new("lifecycle-recovery", LeadMode::Delivered);
    let (controller, _, _) = accepted::accepted(&workspace);
    install_fixture(&workspace);
    let board = |args: &[&str]| {
        let output = Command::new("bd.exe")
            .args(args)
            .current_dir(&workspace.checkout)
            .env("BD_NON_INTERACTIVE", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let initial = controller.stored_state().unwrap().unwrap();
    board(&["config", "set", "events-journal", "false"]);
    board(&[
        "create",
        "Unjournaled synthetic board write",
        "--type",
        "task",
    ]);
    assert!(controller.poll().unwrap().recovery.is_some());
    let recovering = controller.stored_state().unwrap().unwrap();
    let resolution = [
        "--resolution",
        "rebaseline-current",
        "--evidence",
        "Owned board inspected; unjournaled write was unrelated; retained task bindings reconcile.",
    ];
    let foreign = command_with(&workspace, "recover", Some("foreign-lead"), &resolution);
    assert!(!foreign.status.success());
    let missing_evidence = command_with(
        &workspace,
        "recover",
        Some(LEAD_THREAD),
        &["--resolution", "rebaseline-current"],
    );
    assert!(!missing_evidence.status.success());
    let stray_option = command_with(
        &workspace,
        "disable",
        None,
        &["--resolution", "resume-retained"],
    );
    assert!(!stray_option.status.success());
    assert_eq!(controller.stored_state().unwrap().unwrap(), recovering);
    board(&["config", "set", "events-journal", "true"]);
    let recovered = success(command_with(
        &workspace,
        "recover",
        Some(LEAD_THREAD),
        &resolution,
    ));
    assert!(recovered["revision"].as_u64().unwrap() > initial.binding.revision);
    let retained = controller.stored_state().unwrap().unwrap();
    assert!(retained.recovery.is_none());
    assert_eq!(retained.records, initial.records);
    assert_eq!(
        retained.last_recovery_evidence.as_deref(),
        Some(resolution[3])
    );
    board(&["config", "set", "events-journal-retain-days", "0"]);
    board(&["config", "set", "events-journal-retain-rows", "0"]);
    board(&["config", "set", "events-journal-auto-prune", "false"]);
    let floor = retained.checkpoint + 2;
    for title in ["Gap first write", "Gap second write", "Gap retained write"] {
        board(&["create", title, "--type", "task"]);
    }
    board(&["events", "prune", "--before", &floor.to_string(), "--json"]);
    let gap = controller.poll().unwrap();
    assert_eq!(gap.recovery.as_ref().unwrap().floor, Some(floor));
    let resumed = success(command_with(
        &workspace,
        "recover",
        Some(LEAD_THREAD),
        &["--resolution", "resume-retained"],
    ));
    assert_eq!(resumed["checkpoint"], floor - 1);
    assert!(controller.poll().unwrap().recovery.is_none());
    assert_eq!(
        controller.stored_state().unwrap().unwrap().records,
        initial.records
    );
    assert!(
        !command(&workspace, "restore-journal", None)
            .status
            .success()
    );
    success(command(&workspace, "disable", None));
    let restored = success(command(&workspace, "restore-journal", None));
    assert_eq!(
        restored["journal_restored"], false,
        "preexisting journal must remain enabled"
    );

    // Reuse the stopped native board to exercise controller-owned restoration.
    // Activation preserves records while taking ownership only of the missing setting.
    board(&["config", "unset", "events-journal"]);
    controller
        .activate(&ActivationRequest {
            lead_session: LEAD_THREAD.to_owned(),
            run: None,
            boundary: None,
            journal: JournalPolicy::EnableIfMissing,
            limits: None,
        })
        .unwrap();
    success(command(&workspace, "disable", None));
    let restored = success(command(&workspace, "restore-journal", None));
    assert_eq!(restored["journal_restored"], true);
    let repeated = success(command(&workspace, "restore-journal", None));
    assert_eq!(repeated["journal_restored"], false);
    assert_eq!(
        controller.stored_state().unwrap().unwrap().records,
        initial.records
    );
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
}
