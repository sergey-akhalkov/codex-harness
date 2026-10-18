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
    let mut command = Command::new(manager());
    command
        .args(["delivery-improvement", action, "--project"])
        .arg(&workspace.checkout)
        .arg("--source")
        .arg(kit_root())
        .arg("--codex-home")
        .arg(&workspace.home)
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
