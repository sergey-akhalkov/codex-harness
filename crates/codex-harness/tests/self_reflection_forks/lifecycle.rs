//! User-facing lifecycle commands consume the real board's accepted identity.

use super::*;
use harness_core::delivery_reflection::{DeliveryState, StageState};

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

fn command(workspace: &Workspace, action: &str, lead: Option<&str>) -> std::process::Output {
    command_with(workspace, action, lead, &[])
}

fn command_with(
    workspace: &Workspace,
    action: &str,
    lead: Option<&str>,
    extra: &[&str],
) -> std::process::Output {
    let mut command = Command::new(manager());
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
    command.output().unwrap()
}

fn success(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn process(workspace: &Workspace, request: &Value) -> ConsoleOutcome {
    let mut spec = chain_spec(
        workspace,
        request,
        "complete",
        "complete",
        "NO FINDING",
        LEAD_RESULT,
        None,
    );
    // The adapter must derive everything from native owners. A request file
    // prepared by the shared fixture helper is deliberately unavailable.
    fs::remove_file(&workspace.request).unwrap();
    spec.args = vec![
        "delivery-improvement".into(),
        "process".into(),
        "--project".into(),
        workspace.checkout.as_os_str().to_owned(),
        "--source".into(),
        kit_root().into_os_string(),
        "--codex-home".into(),
        workspace.home.as_os_str().to_owned(),
    ];
    spec.env
        .insert("CODEX_THREAD_ID".into(), Some(LEAD_THREAD.into()));
    wait_session(ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap())
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
