//! User-facing lifecycle commands consume the real board's accepted identity.

use super::*;
use harness_core::delivery_reflection::{
    AcceptanceBinding, DeliveryState, LifecycleStatus, ReflectionController, StageState,
    acceptance_metadata,
};

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

/// A disposable kit source with the role inputs needed by the real CLI. Tests
/// that change `self_reflection_enabled` must never edit the checkout that is
/// also serving unrelated test binaries.
pub(super) fn source_fixture(workspace: &Workspace, name: &str, enabled: bool) -> PathBuf {
    let source = workspace.root().join(format!("source-{name}"));
    let kit = kit_root();
    copy_tree(&kit.join("global"), &source.join("global"));
    copy_tree(&kit.join(".agents/skills"), &source.join(".agents/skills"));
    set_source_enabled(&source, enabled);
    source
}

pub(super) fn set_source_enabled(source: &Path, enabled: bool) {
    let path = source.join("global/orchestration.toml");
    let text = fs::read_to_string(&path).unwrap();
    let (from, to) = if enabled {
        (
            "self_reflection_enabled = false",
            "self_reflection_enabled = true",
        )
    } else {
        (
            "self_reflection_enabled = true",
            "self_reflection_enabled = false",
        )
    };
    if !text.contains(to) {
        assert_eq!(text.matches(from).count(), 1, "{}", path.display());
        fs::write(path, text.replace(from, to)).unwrap();
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
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
    manager_command_for_source(manager, &kit_root(), workspace, action, lead, extra)
}

fn manager_command_for_source(
    manager: &Path,
    source: &Path,
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
        .arg(source)
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

fn command_with_source(
    workspace: &Workspace,
    source: &Path,
    action: &str,
    lead: Option<&str>,
    extra: &[&str],
) -> std::process::Output {
    manager_command_for_source(&manager(), source, workspace, action, lead, extra)
        .current_dir(&workspace.checkout)
        .output()
        .unwrap()
}

fn command_from_source(
    manager: &Path,
    workspace: &Workspace,
    source: &Path,
    action: &str,
    lead: Option<&str>,
    extra: &[&str],
) -> std::process::Output {
    manager_command_for_source(manager, source, workspace, action, lead, extra)
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
    process_spec_from_source(workspace, manager, &kit_root(), lead_thread, action)
}

pub(super) fn process_spec_from_source(
    workspace: &Workspace,
    manager: &Path,
    source: &Path,
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
        source.as_os_str().to_owned(),
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
fn disabled_config_refuses_enable_before_lifecycle_mutation() {
    let workspace = Workspace::new("gate-enable-disabled", LeadMode::Delivered);
    let (controller, _task) = accepted::prepare(&workspace);
    install_fixture(&workspace);
    let source = source_fixture(&workspace, "enable-disabled", false);
    let before = controller.stored_state().unwrap();

    let output = command_with_source(&workspace, &source, "enable", Some(LEAD_THREAD), &[]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("self_reflection_enabled = false"), "{text}");
    assert!(
        text.contains("global/orchestration.toml") || text.contains("orchestration.toml"),
        "{text}"
    );
    assert_eq!(controller.stored_state().unwrap(), before);
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
}

#[test]
fn disabled_config_blocks_process_and_continuation_and_reports_the_effective_gate() {
    let workspace = Workspace::new("gate-disabled-process", LeadMode::Delivered);
    let (controller, _request, acceptance) = accepted::accepted(&workspace);
    install_fixture(&workspace);
    let source = source_fixture(&workspace, "disabled-process", false);
    let before = controller.stored_state().unwrap().unwrap();

    let status = success(command_with_source(
        &workspace,
        &source,
        "status",
        None,
        &[],
    ));
    assert_eq!(status["selfReflectionEnabled"], false, "{status}");
    assert_eq!(status["modelWork"], "stopped", "{status}");
    assert_eq!(
        status["configPath"],
        json!(fs::canonicalize(source.join("global/orchestration.toml")).unwrap()),
        "{status}"
    );
    assert!(
        status["modelWorkReason"]
            .as_str()
            .is_some_and(|reason| reason.contains("self_reflection_enabled = false")),
        "{status}"
    );

    let continuation = command_with_source(
        &workspace,
        &source,
        "continue",
        Some(LEAD_THREAD),
        &["--acceptance", &acceptance, "--stage", "executor"],
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&continuation.stdout),
        String::from_utf8_lossy(&continuation.stderr)
    );
    assert!(!continuation.status.success(), "{text}");
    assert!(text.contains("self_reflection_enabled = false"), "{text}");
    let unchanged = controller.stored_state().unwrap().unwrap();
    assert_eq!(unchanged.records, before.records);
    assert_eq!(unchanged.binding, before.binding);
    assert_eq!(unchanged.checkpoint, before.checkpoint);

    let processed = success(command_with_source(
        &workspace,
        &source,
        "process",
        Some(LEAD_THREAD),
        &[],
    ));
    assert_eq!(processed["selfReflectionEnabled"], false, "{processed}");
    assert_eq!(processed["modelWork"], "stopped", "{processed}");
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
    let after = controller.stored_state().unwrap().unwrap();
    assert_eq!(after.records, before.records);
    assert_eq!(after.binding, before.binding);
}

#[test]
fn disabled_config_refuses_manual_execution_before_any_fork_or_state_write() {
    let workspace = Workspace::new("gate-manual-disabled", LeadMode::Delivered);
    let source = source_fixture(&workspace, "manual-disabled", false);
    let state_dir = workspace.state.join("manual-disabled-run");
    let mut request = workspace.request("manual-disabled");
    request["source"] = json!(source);
    request["stateDir"] = json!(state_dir);
    fs::write(&workspace.request, serde_json::to_vec(&request).unwrap()).unwrap();

    let output = Command::new(manager())
        .args(["self-reflection", "--request"])
        .arg(&workspace.request)
        .current_dir(&workspace.checkout)
        .env("CODEX_HOME", &workspace.home)
        .env_remove("HARNESS_REFLECTION_SESSION")
        .env_remove("HARNESS_EXECUTOR_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("self_reflection_enabled = false"), "{text}");
    assert!(!state_dir.exists(), "{}", state_dir.display());
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
}

#[test]
fn disabled_period_closures_are_skipped_at_the_event_boundary_and_never_swept() {
    use harness_core::{
        board_cli,
        delivery_reflection::{AcceptanceBinding, Purpose, acceptance_metadata},
    };

    let workspace = Workspace::new("gate-disabled-period", LeadMode::Delivered);
    let (controller, task) = accepted::prepare(&workspace);
    install_fixture(&workspace);
    let source = source_fixture(&workspace, "disabled-period", true);
    success(command_with_source(
        &workspace,
        &source,
        "enable",
        Some(LEAD_THREAD),
        &[],
    ));
    let boundary = controller.stored_state().unwrap().unwrap().checkpoint;
    set_source_enabled(&source, false);

    let binding = AcceptanceBinding::new(
        &task,
        "disabled-run",
        "disabled-assignment",
        "disabled-result",
        "disabled-revision",
        vec![EXECUTOR_SOURCE.to_owned()],
        LEAD_THREAD,
        "reflection-fixture-lead",
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
        "reflection-fixture-lead",
        &task,
        &acceptance_metadata(&binding).unwrap(),
    )
    .unwrap();

    let skipped = success(command_with_source(
        &workspace,
        &source,
        "process",
        Some(LEAD_THREAD),
        &[],
    ));
    assert_eq!(skipped["selfReflectionEnabled"], false, "{skipped}");
    assert_eq!(skipped["retained"], 0, "{skipped}");
    let state = controller.stored_state().unwrap().unwrap();
    assert!(state.checkpoint > boundary, "{state:?}");
    assert!(state.records.is_empty(), "{state:?}");
    assert_eq!(state.last_poll.as_ref().unwrap().accepted, 0);
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));

    set_source_enabled(&source, true);
    let after = success(command_with_source(
        &workspace,
        &source,
        "process",
        Some(LEAD_THREAD),
        &[],
    ));
    assert_eq!(after["retained"], 0, "{after}");
    assert!(
        controller
            .stored_state()
            .unwrap()
            .unwrap()
            .records
            .is_empty()
    );
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
}

#[test]
fn normal_accept_command_drives_owned_consumer_after_exact_lead_turn_completes() {
    let workspace = Workspace::new("lifecycle-normal-accept", LeadMode::Delivered);
    install_fixture(&workspace);
    normal_acceptance(&workspace, &manager());
}

#[test]
fn ordinary_acceptance_reassigns_executor_task_and_closes_exactly_once() {
    let workspace = Workspace::new("ordinary-assignment", LeadMode::Delivered);
    install_fixture(&workspace);
    let prepared = prepare_ordinary_acceptance_setup(
        &workspace,
        &manager(),
        &kit_root(),
        false,
        Some(&kit_root()),
    );
    accepted::native_as(
        &workspace,
        "bd.exe",
        LEAD_THREAD,
        &["update", &prepared.task, "--assignee", "executor", "--json"],
    );

    let accepted = prepared.accept(&workspace);
    assert_eq!(accepted["status"], "accepted", "{accepted}");
    assert_eq!(accepted["duplicate"], false, "{accepted}");
    assert_eq!(accepted["run"], "accepted-run", "{accepted}");
    assert_eq!(
        accepted["assignment"], prepared.binding.assignment,
        "{accepted}"
    );
    assert_eq!(accepted["result"], prepared.binding.result, "{accepted}");
    assert_eq!(
        accepted["revision"], prepared.binding.revision,
        "{accepted}"
    );
    assert_eq!(
        accepted["acceptanceId"],
        prepared.binding.acceptance_id().unwrap(),
        "{accepted}"
    );
    assert_eq!(accepted["executorSession"], EXECUTOR_SOURCE, "{accepted}");
    assert_eq!(accepted["leadSession"], LEAD_THREAD, "{accepted}");
    assert_eq!(accepted["leadTurn"], LEAD_SOURCE_TURN, "{accepted}");
    assert_eq!(accepted["currentLeadTurn"], LEAD_SOURCE_TURN, "{accepted}");
    assert_eq!(accepted["executorTurn"], EXECUTOR_SOURCE_TURN, "{accepted}");
    assert_eq!(accepted["modelCalls"], 0, "{accepted}");
    assert_eq!(
        accepted["reflection"]["selfReflectionEnabled"], true,
        "{accepted}"
    );
    assert_eq!(
        accepted["reflection"]["automaticTrigger"], true,
        "{accepted}"
    );
    let shown = show_task(&workspace, &prepared.task);
    assert_eq!(shown["status"], "closed", "{shown}");
    assert_eq!(shown["close_reason"], "accepted", "{shown}");
    assert_eq!(shown["assignee"], LEAD_THREAD, "{shown}");
    let events = history_events(&workspace, &prepared.task);
    assert_eq!(accepted_closures(&events), 1, "{events:?}");
    assert!(
        events.iter().any(|event| event["event_type"] == "closed"
            && event["new_value"] == "accepted"
            && event["actor"] == LEAD_THREAD),
        "{events:?}"
    );

    let repeated = prepared.accept(&workspace);
    assert_eq!(repeated["duplicate"], true, "{repeated}");
    assert_eq!(
        repeated["acceptanceId"], accepted["acceptanceId"],
        "{repeated}"
    );
    assert_eq!(
        accepted_closures(&history_events(&workspace, &prepared.task)),
        1,
        "a repeated acceptance must not create another closure"
    );
}

#[test]
fn ordinary_acceptance_refuses_partial_close_then_recovers_through_lead_commands() {
    let workspace = Workspace::new("ordinary-partial-close", LeadMode::Delivered);
    install_fixture(&workspace);
    let source = source_fixture(&workspace, "ordinary-partial-close", true);
    let prepared =
        prepare_ordinary_acceptance_setup(&workspace, &manager(), &source, true, Some(&source));
    accepted::native_as(
        &workspace,
        "bd.exe",
        LEAD_THREAD,
        &["update", &prepared.task, "--assignee", "executor", "--json"],
    );
    write_acceptance_metadata(&workspace, &prepared);
    let before = show_task(&workspace, &prepared.task);
    assert_eq!(before["status"], "open", "{before}");
    assert_eq!(before["assignee"], "executor", "{before}");
    assert_eq!(
        accepted_closures(&history_events(&workspace, &prepared.task)),
        0,
        "the fixture must stop after metadata and before its close"
    );

    let refused = prepared.accept_raw(&workspace);
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(!refused.status.success(), "{text}");
    assert!(
        text.contains("open but already carries acceptance metadata"),
        "{text}"
    );
    assert!(text.contains("Never use --force"), "{text}");
    let unchanged = show_task(&workspace, &prepared.task);
    assert_eq!(unchanged["status"], "open", "{unchanged}");
    assert_eq!(unchanged["assignee"], "executor", "{unchanged}");
    assert_eq!(unchanged["metadata"], before["metadata"], "{unchanged}");
    assert_eq!(
        accepted_closures(&history_events(&workspace, &prepared.task)),
        0
    );

    // The supported recovery keeps the recorded metadata. The actual approving
    // lead conditionally reclaims the completed task and closes it with the
    // accepted reason; replay then validates rather than rewrites that record.
    set_source_enabled(&source, true);
    accepted::native_as(
        &workspace,
        "bd.exe",
        LEAD_THREAD,
        &[
            "update",
            &prepared.task,
            "--assignee",
            LEAD_THREAD,
            "--if-assignee",
            "executor",
            "--if-status",
            "open",
            "--json",
        ],
    );
    accepted::native_as(
        &workspace,
        "bd.exe",
        LEAD_THREAD,
        &["close", &prepared.task, "--reason", "accepted", "--json"],
    );

    let accepted = prepared.accept(&workspace);
    assert_eq!(accepted["status"], "accepted", "{accepted}");
    assert_eq!(accepted["duplicate"], true, "{accepted}");
    assert_eq!(accepted["leadTurn"], LEAD_SOURCE_TURN, "{accepted}");
    assert_eq!(
        accepted["reflection"]["selfReflectionEnabled"], false,
        "{accepted}"
    );
    assert_eq!(
        accepted["reflection"]["automaticTrigger"], false,
        "{accepted}"
    );
    assert_eq!(
        accepted["acceptanceId"],
        prepared.binding.acceptance_id().unwrap(),
        "{accepted}"
    );
    let events = history_events(&workspace, &prepared.task);
    assert_eq!(accepted_closures(&events), 1, "{events:?}");
    let repeated = prepared.accept(&workspace);
    assert_eq!(repeated["duplicate"], true, "{repeated}");
    assert_eq!(repeated["acceptanceId"], accepted["acceptanceId"]);
    assert_eq!(
        accepted_closures(&history_events(&workspace, &prepared.task)),
        1,
        "recovering the partial close must not duplicate the closure"
    );

    accepted::native_as(
        &workspace,
        "bd.exe",
        LEAD_THREAD,
        &[
            "reopen",
            &prepared.task,
            "--reason",
            "new evidence",
            "--json",
        ],
    );
    let refused = prepared.accept_raw(&workspace);
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(!refused.status.success(), "{text}");
    assert!(
        text.contains("open but already carries acceptance metadata"),
        "{text}"
    );
    let shown = show_task(&workspace, &prepared.task);
    assert_eq!(shown["status"], "open", "{shown}");
    assert_eq!(
        accepted_closures(&history_events(&workspace, &prepared.task)),
        1,
        "a reopened task must not gain a second accepted closure"
    );
}

#[test]
fn ordinary_acceptance_refuses_cancelled_then_reopened_work() {
    let workspace = Workspace::new("ordinary-cancelled-reopen", LeadMode::Delivered);
    install_fixture(&workspace);
    let prepared = prepare_ordinary_acceptance_setup(
        &workspace,
        &manager(),
        &kit_root(),
        false,
        Some(&kit_root()),
    );
    write_acceptance_metadata(&workspace, &prepared);
    accepted::native_as(
        &workspace,
        "bd.exe",
        LEAD_THREAD,
        &[
            "update",
            &prepared.task,
            "--assignee",
            LEAD_THREAD,
            "--json",
        ],
    );
    accepted::native_as(
        &workspace,
        "bd.exe",
        LEAD_THREAD,
        &["close", &prepared.task, "--reason", "cancelled", "--json"],
    );
    let cancelled = prepared.accept_raw(&workspace);
    let cancelled_text = format!(
        "{} {}",
        String::from_utf8_lossy(&cancelled.stdout),
        String::from_utf8_lossy(&cancelled.stderr)
    );
    assert!(!cancelled.status.success(), "{cancelled_text}");
    assert!(
        cancelled_text.contains("already closed with reason 'cancelled', not accepted"),
        "{cancelled_text}"
    );
    assert_eq!(show_task(&workspace, &prepared.task)["status"], "closed");
    assert_eq!(
        accepted_closures(&history_events(&workspace, &prepared.task)),
        0,
        "cancelled work must never become accepted"
    );
    accepted::native_as(
        &workspace,
        "bd.exe",
        LEAD_THREAD,
        &[
            "reopen",
            &prepared.task,
            "--reason",
            "cancelled work was reopened",
            "--json",
        ],
    );

    let refused = prepared.accept_raw(&workspace);
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(!refused.status.success(), "{text}");
    assert!(
        text.contains("open but already carries acceptance metadata"),
        "{text}"
    );
    assert_eq!(show_task(&workspace, &prepared.task)["status"], "open");
    assert!(
        history_events(&workspace, &prepared.task)
            .iter()
            .any(|event| event["event_type"] == "closed" && event["new_value"] == "cancelled"),
        "the fixture must record a cancelled closure before reopening"
    );
    assert_eq!(
        accepted_closures(&history_events(&workspace, &prepared.task)),
        0,
        "cancelled work must never become accepted"
    );
}

#[test]
fn ordinary_acceptance_refuses_mismatched_partial_metadata_without_rewriting() {
    let workspace = Workspace::new("ordinary-mismatched-partial", LeadMode::Delivered);
    install_fixture(&workspace);
    let prepared = prepare_ordinary_acceptance_setup(
        &workspace,
        &manager(),
        &kit_root(),
        false,
        Some(&kit_root()),
    );
    let mut mismatched = prepared.binding.clone();
    mismatched.result = "0".repeat(64);
    accepted::native_as(
        &workspace,
        "bd.exe",
        LEAD_THREAD,
        &["update", &prepared.task, "--assignee", "executor", "--json"],
    );
    for (key, value) in acceptance_metadata(&mismatched).unwrap() {
        let pair = format!("{key}={value}");
        accepted::native(
            &workspace,
            "bd.exe",
            &["update", &prepared.task, "--set-metadata", &pair, "--json"],
        );
    }
    let before = show_task(&workspace, &prepared.task);

    let refused = prepared.accept_raw(&workspace);
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(!refused.status.success(), "{text}");
    assert!(
        text.contains("open but already carries acceptance metadata"),
        "{text}"
    );
    let after = show_task(&workspace, &prepared.task);
    assert_eq!(after["status"], "open", "{after}");
    assert_eq!(after["assignee"], "executor", "{after}");
    assert_eq!(after["metadata"], before["metadata"], "{after}");
    assert_eq!(
        accepted_closures(&history_events(&workspace, &prepared.task)),
        0
    );
}

#[test]
fn running_host_observes_disable_between_stages_and_reuses_executor_after_reenable() {
    let workspace = Workspace::new("gate-dynamic-disable", LeadMode::Delivered);
    let (controller, _request, acceptance) = accepted::accepted(&workspace);
    install_fixture(&workspace);
    let source = source_fixture(&workspace, "dynamic-disable", true);
    workspace.lead_state.lock().unwrap().source_turn_active = true;

    let spec = process_spec_from_source(&workspace, &manager(), &source, LEAD_THREAD, "_serve");
    let session = ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap();
    let until = Instant::now() + Duration::from_secs(90);
    loop {
        if controller
            .record(&acceptance)
            .unwrap()
            .is_some_and(|record| record.executor.state == StageState::Retained)
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "executor reflection did not retain its result: {:?}; requests={:?}",
            controller.record(&acceptance).unwrap(),
            request_counts(&workspace)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    let pending = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(pending.lead.state, StageState::Pending);
    let state_dir = controller
        .state_file()
        .unwrap()
        .with_extension("")
        .join(&acceptance);
    let executor_before = fs::read(state_dir.join("executor-stage.json")).unwrap();
    let chain_before = read_json(&state_dir.join("chain.json"));

    set_source_enabled(&source, false);
    workspace.lead_state.lock().unwrap().source_turn_active = false;
    std::thread::sleep(Duration::from_millis(3_000));
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    let disabled = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(disabled.executor.state, StageState::Retained);
    assert_eq!(disabled.lead.state, StageState::Pending);
    assert_eq!(
        fs::read(state_dir.join("executor-stage.json")).unwrap(),
        executor_before
    );

    set_source_enabled(&source, true);
    loop {
        if controller.record(&acceptance).unwrap().unwrap().delivery == DeliveryState::Consumed {
            break;
        }
        assert!(
            Instant::now() < until,
            "exact accepted synthesis was not consumed after re-enable: {:?}",
            controller.record(&acceptance).unwrap()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    assert_eq!(
        fs::read(state_dir.join("executor-stage.json")).unwrap(),
        executor_before
    );
    let chain_after = read_json(&state_dir.join("chain.json"));
    assert_eq!(
        chain_after["executorSource"],
        chain_before["executorSource"]
    );
    assert_eq!(chain_after["executorStage"], chain_before["executorStage"]);
    assert_eq!(chain_after["acceptance"], chain_before["acceptance"]);

    success(command_from_source(
        &manager(),
        &workspace,
        &source,
        "disable",
        None,
        &[],
    ));
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
}

/// The ordinary accepted-work lifecycle, independent of where the manager and
/// launcher were installed. The producer is the public retained completion and
/// the only acceptance identity is the board's exact native acceptance.
pub(super) fn normal_acceptance(workspace: &Workspace, manager: &Path) -> (String, String) {
    normal_acceptance_with_source(workspace, manager, &kit_root())
}

struct PreparedOrdinaryAcceptance {
    controller: ReflectionController,
    task: String,
    retained_result: String,
    binding: AcceptanceBinding,
    manager: PathBuf,
    accept_source: Option<PathBuf>,
}

impl PreparedOrdinaryAcceptance {
    fn accept_raw(&self, workspace: &Workspace) -> std::process::Output {
        let mut command = Command::new(&self.manager);
        command
            .args(["executor", "accept", &self.task, "--project"])
            .arg(&workspace.checkout);
        if let Some(accept_source) = &self.accept_source {
            command.arg("--source").arg(accept_source);
        }
        command
            .arg("--codex-home")
            .arg(&workspace.home)
            .current_dir(&workspace.checkout)
            .env("CODEX_HOME", &workspace.home)
            .env("CODEX_THREAD_ID", LEAD_THREAD)
            .env_remove("HARNESS_REFLECTION_SESSION")
            .env_remove("HARNESS_EXECUTOR_SESSION")
            .env_remove("HARNESS_BOUNDED_LEAD_SESSION")
            .output()
            .unwrap()
    }

    fn accept(&self, workspace: &Workspace) -> Value {
        success(self.accept_raw(workspace))
    }
}

fn show_task(workspace: &Workspace, task: &str) -> Value {
    let shown = accepted::native(workspace, "bd.exe", &["show", task, "--json"]);
    let mut value: Value = serde_json::from_slice(&shown.stdout).unwrap();
    if let Value::Array(rows) = &value {
        value = rows.first().cloned().unwrap();
    }
    value
}

fn history_events(workspace: &Workspace, task: &str) -> Vec<Value> {
    let shown = accepted::native(
        workspace,
        "bd.exe",
        &["history", task, "--events", "--json"],
    );
    serde_json::from_slice(&shown.stdout).unwrap()
}

fn accepted_closures(events: &[Value]) -> usize {
    events
        .iter()
        .filter(|event| event["event_type"] == "closed" && event["new_value"] == "accepted")
        .count()
}

fn write_acceptance_metadata(workspace: &Workspace, prepared: &PreparedOrdinaryAcceptance) {
    for (key, value) in acceptance_metadata(&prepared.binding).unwrap() {
        let pair = format!("{key}={value}");
        accepted::native(
            workspace,
            "bd.exe",
            &["update", &prepared.task, "--set-metadata", &pair, "--json"],
        );
    }
}

fn prepare_ordinary_acceptance_setup(
    workspace: &Workspace,
    manager: &Path,
    source: &Path,
    disable_before_accept: bool,
    accept_source: Option<&Path>,
) -> PreparedOrdinaryAcceptance {
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
    success(command_from_source(
        manager,
        workspace,
        source,
        "enable",
        Some(LEAD_THREAD),
        &[],
    ));
    if disable_before_accept {
        set_source_enabled(source, false);
    }
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
    let pool_source = accept_source.unwrap_or(&workspace.checkout);
    let pool = task_worktree::pool_state_dir(&workspace.home, pool_source).unwrap();
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
    let retained_result = retained.result.as_ref().unwrap().sha256.clone();
    let binding = AcceptanceBinding::new(
        task.clone(),
        "accepted-run".to_owned(),
        retained
            .assignment
            .as_ref()
            .unwrap()
            .assignment_sha256
            .clone(),
        retained_result.clone(),
        retained.source.revision.clone().unwrap(),
        vec![EXECUTOR_SOURCE.to_owned()],
        LEAD_THREAD.to_owned(),
        LEAD_THREAD.to_owned(),
        Purpose::Ordinary,
    )
    .with_turns(
        Some(EXECUTOR_SOURCE.to_owned()),
        Some(EXECUTOR_SOURCE_TURN.to_owned()),
        Some(LEAD_SOURCE_TURN.to_owned()),
    )
    .with_self_reflection_enabled(!disable_before_accept);
    PreparedOrdinaryAcceptance {
        controller,
        task,
        retained_result,
        binding,
        manager: manager.to_path_buf(),
        accept_source: accept_source.map(Path::to_path_buf),
    }
}

fn prepare_ordinary_acceptance(
    workspace: &Workspace,
    manager: &Path,
    source: &Path,
    disable_before_accept: bool,
    accept_source: Option<&Path>,
) -> (ReflectionController, String, String, String, Value) {
    let prepared = prepare_ordinary_acceptance_setup(
        workspace,
        manager,
        source,
        disable_before_accept,
        accept_source,
    );
    let accepted = prepared.accept(workspace);
    assert_eq!(accepted["duplicate"], false);
    assert_eq!(accepted["status"], "accepted", "{accepted}");
    assert_eq!(accepted["purpose"], "ordinary", "{accepted}");
    assert_eq!(accepted["modelCalls"], 0, "{accepted}");
    for field in [
        "acceptanceId",
        "run",
        "assignment",
        "result",
        "resultLocator",
        "revision",
        "executorSession",
        "executorTurn",
        "leadSession",
        "leadTurn",
    ] {
        assert!(
            accepted[field]
                .as_str()
                .is_some_and(|value| !value.trim().is_empty()),
            "acceptance receipt lost {field}: {accepted}"
        );
    }
    let expected_permission = !disable_before_accept;
    assert_eq!(
        accepted["reflection"]["selfReflectionEnabled"], expected_permission,
        "{accepted}"
    );
    assert_eq!(
        accepted["reflection"]["automaticTrigger"], expected_permission,
        "{accepted}"
    );
    assert_eq!(accepted["leadTurn"], LEAD_SOURCE_TURN);
    assert_eq!(accepted["executorTurn"], EXECUTOR_SOURCE_TURN);
    let acceptance = accepted["acceptanceId"].as_str().unwrap();
    let repeated = prepared.accept(workspace);
    assert_eq!(repeated["duplicate"], true);
    assert_eq!(repeated["acceptanceId"], acceptance);
    assert_eq!(
        repeated["reflection"]["selfReflectionEnabled"], expected_permission,
        "{repeated}"
    );
    assert_eq!(
        repeated["reflection"]["automaticTrigger"], expected_permission,
        "{repeated}"
    );
    assert_eq!(request_counts(workspace), (0, 0, 0, 0));

    (
        prepared.controller,
        prepared.task,
        acceptance.to_owned(),
        prepared.retained_result,
        accepted,
    )
}

fn normal_acceptance_with_source(
    workspace: &Workspace,
    manager: &Path,
    source: &Path,
) -> (String, String) {
    let (controller, task, acceptance, retained_result, _accepted) =
        prepare_ordinary_acceptance(workspace, manager, source, false, Some(source));
    let acceptance = acceptance.as_str();

    let spec = process_spec_from_source(workspace, manager, source, LEAD_THREAD, "_serve");
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
    assert_eq!(record.binding.result, retained_result);
    assert_eq!(record.binding.lead_turn.as_deref(), Some(LEAD_SOURCE_TURN));
    success(command_from_source(
        manager,
        workspace,
        source,
        "disable",
        None,
        &[],
    ));
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
fn ordinary_acceptance_remains_usable_while_reflection_is_disabled() {
    let workspace = Workspace::new("gate-ordinary-acceptance", LeadMode::Delivered);
    install_fixture(&workspace);
    let source = source_fixture(&workspace, "ordinary-acceptance", true);
    let (controller, task, acceptance, _result, accepted) =
        prepare_ordinary_acceptance(&workspace, &manager(), &source, true, Some(&source));
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
    assert_eq!(accepted["reflection"]["selfReflectionEnabled"], false);
    assert_eq!(accepted["reflection"]["automaticTrigger"], false);

    let status = success(command_with_source(
        &workspace,
        &source,
        "status",
        None,
        &[],
    ));
    assert_eq!(status["selfReflectionEnabled"], false, "{status}");
    assert_eq!(status["modelWork"], "stopped", "{status}");

    // Re-enable before the first poll. The acceptance itself was recorded while
    // permission was false, so later permission must not make that closure
    // eligible for reflection.
    set_source_enabled(&source, true);
    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 0, "{poll:?}");
    assert_eq!(poll.model_calls, 0, "{poll:?}");
    assert!(poll.recovery.is_none(), "{poll:?}");
    assert!(
        poll.ignored >= 1,
        "the disabled-period acceptance was not ignored: {poll:?}"
    );
    assert!(
        poll.refusals.is_empty(),
        "the disabled-period acceptance added a refusal: {poll:?}"
    );
    assert!(
        controller.record(&acceptance).unwrap().is_none(),
        "a closure accepted while reflection was disabled became a reflection record"
    );

    let processed = success(command_with_source(
        &workspace,
        &source,
        "process",
        Some(LEAD_THREAD),
        &[],
    ));
    assert_eq!(processed["lifecycle"], "active", "{processed}");
    assert_eq!(processed["retained"], 0, "{processed}");
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
    assert!(
        controller
            .stored_state()
            .unwrap()
            .unwrap()
            .records
            .is_empty()
    );
    assert!(
        !acceptance.trim().is_empty(),
        "accepted closure identity was not retained"
    );
    assert!(!task.trim().is_empty());
}

#[test]
fn ordinary_acceptance_reads_the_installed_kit_permission_without_consumer_config() {
    for explicit_consumer in [false, true] {
        let workspace = Workspace::new(
            if explicit_consumer {
                "gate-installed-explicit"
            } else {
                "gate-installed-implicit"
            },
            LeadMode::Delivered,
        );
        install_fixture(&workspace);
        let source = source_fixture(&workspace, "installed-acceptance", true);
        fs::write(
            workspace.home.join("harness/installation.json"),
            serde_json::to_vec_pretty(&json!({
                "schemaVersion": 2,
                "settings": {"sourceRoot": source},
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(
            !workspace
                .checkout
                .join("global/orchestration.toml")
                .exists(),
            "the consumer fixture must not supply kit configuration"
        );

        let (controller, _task, acceptance, _result, accepted) = prepare_ordinary_acceptance(
            &workspace,
            &manager(),
            &source,
            true,
            explicit_consumer.then_some(workspace.checkout.as_path()),
        );
        assert_eq!(accepted["status"], "accepted", "{accepted}");
        assert_eq!(accepted["purpose"], "ordinary", "{accepted}");
        assert_eq!(accepted["modelCalls"], 0, "{accepted}");
        assert_eq!(
            accepted["reflection"]["selfReflectionEnabled"], false,
            "{accepted}"
        );
        assert_eq!(
            accepted["reflection"]["automaticTrigger"], false,
            "{accepted}"
        );

        set_source_enabled(&source, true);
        let poll = controller.poll().unwrap();
        assert_eq!(poll.accepted, 0, "{poll:?}");
        assert_eq!(poll.model_calls, 0, "{poll:?}");
        assert!(
            poll.ignored >= 1,
            "the installed-kit disabled acceptance was not ignored: {poll:?}"
        );
        assert!(
            poll.refusals.is_empty(),
            "the installed-kit disabled acceptance added a refusal: {poll:?}"
        );
        assert!(
            controller.record(&acceptance).unwrap().is_none(),
            "re-enabling the installed kit scheduled the disabled-period acceptance"
        );

        let processed = success(command_with_source(
            &workspace,
            &source,
            "process",
            Some(LEAD_THREAD),
            &[],
        ));
        assert_eq!(processed["retained"], 0, "{processed}");
        assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
    }
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

    let disabled_source = source_fixture(&workspace, "lifecycle-off-disabled", false);
    let before_disabled = controller.stored_state().unwrap().unwrap();
    let disabled_foreign = command_with_source(
        &workspace,
        &disabled_source,
        "process",
        Some("foreign-lead"),
        &[],
    );
    assert!(!disabled_foreign.status.success());
    assert!(String::from_utf8_lossy(&disabled_foreign.stderr).contains("real lead is unavailable"));
    let after_disabled = controller.stored_state().unwrap().unwrap();
    assert_eq!(after_disabled.checkpoint, before_disabled.checkpoint);
    assert_eq!(after_disabled.records, before_disabled.records);
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
