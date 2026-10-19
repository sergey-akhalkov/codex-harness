//! Verified managed-lead succession after an accepted closure retained work.
//! These checks use the real CLI, lifecycle lock, native stage owner and
//! loopback lead endpoints; model turns remain synthetic fixture responses.

use super::*;
use harness_core::delivery_reflection::{DeliveryState, ReflectionController, StageState};

const NO_FINDING: &str = "NO FINDING";

fn registry(workspace: &Workspace, thread: &str) -> PathBuf {
    workspace
        .home
        .join("harness/lead-endpoints")
        .join(format!("{thread}.json"))
}

fn install_successor_registry(workspace: &Workspace) {
    fs::copy(
        &workspace.successor_endpoint,
        registry(workspace, SUCCESSOR_THREAD),
    )
    .unwrap();
}

fn remove_old_registry(workspace: &Workspace) {
    fs::remove_file(registry(workspace, LEAD_THREAD)).unwrap();
}

fn restore_old_registry(workspace: &Workspace) {
    fs::copy(&workspace.endpoint, registry(workspace, LEAD_THREAD)).unwrap();
}

fn process(workspace: &Workspace, thread: &str) -> ConsoleOutcome {
    process_with(workspace, thread, "complete")
}

fn process_with(workspace: &Workspace, thread: &str, lead_outcome: &str) -> ConsoleOutcome {
    let mut spec = CommandSpec::new(manager());
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
    spec.current_dir = Some(workspace.checkout.clone());
    apply_fixture_env(
        workspace,
        &mut spec,
        LEAD_THREAD,
        "complete",
        lead_outcome,
        NO_FINDING,
        LEAD_RESULT,
    );
    spec.env
        .insert("HARNESS_REFLECTION_HOST".into(), Some(thread.into()));
    spec.env
        .insert("CODEX_THREAD_ID".into(), Some(thread.into()));
    wait_session(ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap())
}

fn succeed(workspace: &Workspace, thread: &str) -> Value {
    lifecycle::success(lifecycle::command(workspace, "succeed", Some(thread)))
}

fn command_failure(output: std::process::Output) -> String {
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{text}");
    text
}

fn state_bytes(controller: &ReflectionController) -> Vec<u8> {
    fs::read(controller.state_file().unwrap()).unwrap()
}

fn acceptance_dir(controller: &ReflectionController, acceptance: &str) -> PathBuf {
    controller
        .state_file()
        .unwrap()
        .with_extension("")
        .join(acceptance)
}

fn chain_path(controller: &ReflectionController, acceptance: &str) -> PathBuf {
    acceptance_dir(controller, acceptance).join("chain.json")
}

fn delivery_path(controller: &ReflectionController, acceptance: &str) -> PathBuf {
    acceptance_dir(controller, acceptance).join("lead-delivery.json")
}

fn acceptance_stage(controller: &ReflectionController, acceptance: &str, role: &str) -> Value {
    read_json(&acceptance_dir(controller, acceptance).join(format!("{role}.json")))
}

fn lead_dispatches(server: &Server) -> usize {
    server
        .requests()
        .iter()
        .filter(|request| {
            matches!(
                request["method"].as_str(),
                Some("turn/start" | "turn/steer")
            )
        })
        .count()
}

#[test]
fn successor_reuses_retained_executor_work_and_delivers_once_to_the_new_owner() {
    let workspace = Workspace::new("succession-delivery", LeadMode::Delivered);
    let (controller, _request, acceptance) = accepted::accepted(&workspace);
    lifecycle::install_fixture(&workspace);
    install_successor_registry(&workspace);

    // The original accepting turn is deliberately absent so the first owner
    // retains the executor result and stops before its pending lead stage.
    workspace.lead_state.lock().unwrap().source_turn = None;
    let first = process(&workspace, LEAD_THREAD);
    assert_refused(&first, "exact accepting lead turn");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    let retained = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(retained.executor.state, StageState::Retained);
    assert_eq!(retained.lead.state, StageState::Pending);
    assert_eq!(retained.delivery, DeliveryState::Pending);
    let lifecycle_before = controller.stored_state().unwrap().unwrap();
    let executor_before = acceptance_stage(&controller, &acceptance, "executor-stage");
    let chain_before = read_json(&chain_path(&controller, &acceptance));
    assert_eq!(chain_before["leadSource"]["thread"], LEAD_THREAD);
    assert_eq!(chain_before["leadReturn"]["thread"], LEAD_THREAD);

    remove_old_registry(&workspace);
    let receipt = succeed(&workspace, SUCCESSOR_THREAD);
    assert_eq!(receipt["changed"], true);
    assert_eq!(receipt["previous_lead_session"], LEAD_THREAD);
    assert_eq!(receipt["lead_session"], SUCCESSOR_THREAD);
    assert_eq!(receipt["revision"], lifecycle_before.binding.revision + 1);

    let completed = process(&workspace, SUCCESSOR_THREAD);
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    assert_eq!(lead_dispatches(&workspace.lead), 0);
    assert_eq!(lead_dispatches(&workspace.successor), 1);
    assert_eq!(
        acceptance_stage(&controller, &acceptance, "executor-stage"),
        executor_before
    );

    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert_eq!(record.lead.state, StageState::Retained);
    assert_eq!(record.delivery, DeliveryState::Consumed);
    assert_eq!(record.binding.lead_session, LEAD_THREAD);
    assert_eq!(
        record.executor.outcome.as_ref().unwrap().summary,
        NO_FINDING
    );
    assert_eq!(record.lead.outcome.as_ref().unwrap().summary, LEAD_RESULT);

    let lead_fork = fixture_requests(&workspace, "lead-self-reflection", "thread/fork");
    assert_eq!(lead_fork.len(), 1, "{lead_fork:?}");
    assert_eq!(lead_fork[0]["params"]["threadId"], LEAD_THREAD);
    assert_eq!(lead_fork[0]["params"]["lastTurnId"], LEAD_SOURCE_TURN);

    let delivery = read_json(&delivery_path(&controller, &acceptance));
    assert_eq!(delivery["state"], "delivered");
    {
        let successor = workspace.successor_state.lock().unwrap();
        assert!(successor.accepted);
        assert_eq!(
            successor.client_id.as_deref(),
            delivery["clientMessageId"].as_str()
        );
        assert!(
            successor
                .text
                .as_deref()
                .is_some_and(|text| text.contains(LEAD_RESULT)),
            "{:?}",
            successor.text
        );
    }

    let chain_after = read_json(&chain_path(&controller, &acceptance));
    assert_eq!(chain_after["leadSource"], chain_before["leadSource"]);
    assert_eq!(chain_after["acceptedWork"], chain_before["acceptedWork"]);
    let mut expected_chain = chain_before;
    expected_chain["leadReturn"] = chain_after["leadReturn"].clone();
    assert_eq!(chain_after, expected_chain);

    let replay = process(&workspace, SUCCESSOR_THREAD);
    assert_eq!(replay.outcome.exit_code, 0, "{}", replay.transcript);
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    assert_eq!(lead_dispatches(&workspace.successor), 1);
    assert_eq!(controller.record(&acceptance).unwrap().unwrap(), record);
    let settled = state_bytes(&controller);

    restore_old_registry(&workspace);
    let old_process = process(&workspace, LEAD_THREAD);
    assert_ne!(
        old_process.outcome.exit_code, 0,
        "{}",
        old_process.transcript
    );
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    assert_eq!(lead_dispatches(&workspace.successor), 1);
    assert_eq!(
        read_json(&delivery_path(&controller, &acceptance)),
        delivery
    );

    let old_succeed = command_failure(lifecycle::command(&workspace, "succeed", Some(LEAD_THREAD)));
    assert!(old_succeed.contains("cannot reclaim"), "{old_succeed}");
    assert_eq!(state_bytes(&controller), settled);

    let repeated = succeed(&workspace, SUCCESSOR_THREAD);
    assert_eq!(repeated["changed"], false);
    assert_eq!(state_bytes(&controller), settled);
}

#[test]
fn succession_refuses_live_unverifiable_old_and_invalid_current_identity() {
    let workspace = Workspace::new("succession-refusals", LeadMode::Delivered);
    let (controller, _request, _acceptance) = accepted::accepted(&workspace);
    lifecycle::install_fixture(&workspace);
    let settled = state_bytes(&controller);

    let missing = command_failure(lifecycle::command(
        &workspace,
        "succeed",
        Some(SUCCESSOR_THREAD),
    ));
    assert!(
        missing.contains("no lead endpoint is registered"),
        "{missing}"
    );
    assert_eq!(state_bytes(&controller), settled);

    install_successor_registry(&workspace);
    let live = command_failure(lifecycle::command(
        &workspace,
        "succeed",
        Some(SUCCESSOR_THREAD),
    ));
    assert!(live.contains("still live"), "{live}");
    assert_eq!(state_bytes(&controller), settled);

    let successor_record = registry(&workspace, SUCCESSOR_THREAD);
    let successor_original = fs::read(&successor_record).unwrap();
    let mut stale: Value = serde_json::from_slice(&successor_original).unwrap();
    let created = stale["process"]["creationTime"]
        .as_u64()
        .expect("fixture endpoint records a process creation time");
    stale["process"]["creationTime"] = json!(created - 1);
    fs::write(
        &successor_record,
        serde_json::to_vec_pretty(&stale).unwrap(),
    )
    .unwrap();
    let stale_current = command_failure(lifecycle::command(
        &workspace,
        "succeed",
        Some(SUCCESSOR_THREAD),
    ));
    assert!(stale_current.contains("no longer live"), "{stale_current}");
    assert_eq!(state_bytes(&controller), settled);
    fs::write(&successor_record, successor_original).unwrap();

    let old_record = registry(&workspace, LEAD_THREAD);
    fs::write(&old_record, b"not-json").unwrap();
    let malformed = command_failure(lifecycle::command(
        &workspace,
        "succeed",
        Some(SUCCESSOR_THREAD),
    ));
    assert!(malformed.contains("unreadable or malformed"), "{malformed}");
    assert_eq!(state_bytes(&controller), settled);

    let mut processless = read_json(&workspace.endpoint);
    processless.as_object_mut().unwrap().remove("process");
    fs::write(
        &old_record,
        serde_json::to_vec_pretty(&processless).unwrap(),
    )
    .unwrap();
    let unverifiable = command_failure(lifecycle::command(
        &workspace,
        "succeed",
        Some(SUCCESSOR_THREAD),
    ));
    assert!(
        unverifiable.contains("records no app-server process"),
        "{unverifiable}"
    );
    assert_eq!(state_bytes(&controller), settled);
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));

    remove_old_registry(&workspace);
    let transferred = succeed(&workspace, SUCCESSOR_THREAD);
    assert_eq!(transferred["changed"], true);
    let transferred_state = state_bytes(&controller);

    restore_old_registry(&workspace);
    let reclaimed = command_failure(lifecycle::command(&workspace, "succeed", Some(LEAD_THREAD)));
    assert!(reclaimed.contains("cannot reclaim"), "{reclaimed}");
    assert_eq!(state_bytes(&controller), transferred_state);

    let repeated = succeed(&workspace, SUCCESSOR_THREAD);
    assert_eq!(repeated["changed"], false);
    assert_eq!(state_bytes(&controller), transferred_state);
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
}

#[test]
fn changed_route_after_succession_refuses_before_any_reflection_fork() {
    let workspace = Workspace::new("succession-route", LeadMode::Delivered);
    let (controller, _request, acceptance) = accepted::accepted(&workspace);
    lifecycle::install_fixture(&workspace);
    install_successor_registry(&workspace);

    // Establish the retained chain with the original owner before changing the
    // effective profile route. The pending lead stage must not be launched.
    workspace.lead_state.lock().unwrap().source_turn = None;
    let first = process(&workspace, LEAD_THREAD);
    assert_refused(&first, "exact accepting lead turn");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    let retained = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(retained.executor.state, StageState::Retained);
    assert_eq!(retained.lead.state, StageState::Pending);

    remove_old_registry(&workspace);
    assert_eq!(succeed(&workspace, SUCCESSOR_THREAD)["changed"], true);

    fs::write(
        workspace.home.join(format!("{PROFILE}.config.toml")),
        format!(
            "model = \"other-model\"\nmodel_provider = \"{PROVIDER}\"\nmodel_reasoning_effort = \"{EFFORT}\"\n"
        ),
    )
    .unwrap();
    let refused = process(&workspace, SUCCESSOR_THREAD);
    assert_refused(&refused, "request drifts from the retained chain");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    assert_eq!(lead_dispatches(&workspace.successor), 0);
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert_eq!(record.lead.state, StageState::Pending);
    assert_eq!(record.delivery, DeliveryState::Pending);
}

#[test]
fn unknown_delivery_is_retained_and_never_resent_to_the_successor() {
    let workspace = Workspace::new("succession-unknown-delivery", LeadMode::Indeterminate);
    let (controller, _request, acceptance) = accepted::accepted(&workspace);
    lifecycle::install_fixture(&workspace);
    install_successor_registry(&workspace);

    let first = process(&workspace, LEAD_THREAD);
    assert_ne!(first.outcome.exit_code, 0, "{}", first.transcript);
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    assert_eq!(lead_dispatches(&workspace.lead), 1);
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert_eq!(record.lead.state, StageState::Retained);
    assert_eq!(record.delivery, DeliveryState::Unknown);
    let retained_delivery = fs::read(delivery_path(&controller, &acceptance)).unwrap();
    assert_eq!(
        read_json(&delivery_path(&controller, &acceptance))["state"],
        "indeterminate"
    );

    remove_old_registry(&workspace);
    assert_eq!(succeed(&workspace, SUCCESSOR_THREAD)["changed"], true);
    let successor_process = process(&workspace, SUCCESSOR_THREAD);
    assert_ne!(
        successor_process.outcome.exit_code, 0,
        "{}",
        successor_process.transcript
    );
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    assert_eq!(lead_dispatches(&workspace.lead), 1);
    assert_eq!(lead_dispatches(&workspace.successor), 0);
    assert_eq!(
        fs::read(delivery_path(&controller, &acceptance)).unwrap(),
        retained_delivery
    );
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap().delivery,
        DeliveryState::Unknown
    );

    let repeated = process(&workspace, SUCCESSOR_THREAD);
    assert_ne!(repeated.outcome.exit_code, 0, "{}", repeated.transcript);
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    assert_eq!(lead_dispatches(&workspace.lead), 1);
    assert_eq!(lead_dispatches(&workspace.successor), 0);
    assert_eq!(
        fs::read(delivery_path(&controller, &acceptance)).unwrap(),
        retained_delivery
    );
}

#[test]
fn in_flight_lead_attempt_is_restored_not_relaunched_under_the_successor() {
    let workspace = Workspace::new("succession-in-flight", LeadMode::Delivered);
    let (controller, _request, acceptance) = accepted::accepted(&workspace);
    lifecycle::install_fixture(&workspace);
    install_successor_registry(&workspace);

    let interrupted = process_with(&workspace, LEAD_THREAD, "visibility-lost");
    assert_ne!(
        interrupted.outcome.exit_code, 0,
        "{}",
        interrupted.transcript
    );
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert_eq!(record.lead.state, StageState::InFlight);
    let retained = acceptance_stage(&controller, &acceptance, "lead-stage");
    assert_eq!(retained["status"], "turn-submitted");
    assert_eq!(retained["fork"]["forkThread"], LEAD_FORK);
    assert_eq!(retained["turnId"], LEAD_FORK_TURN);
    let retained_counts = request_counts(&workspace);

    remove_old_registry(&workspace);
    assert_eq!(succeed(&workspace, SUCCESSOR_THREAD)["changed"], true);
    let resumed = process_with(&workspace, SUCCESSOR_THREAD, "visibility-lost");
    assert_ne!(resumed.outcome.exit_code, 0, "{}", resumed.transcript);
    assert_eq!(request_counts(&workspace), retained_counts);
    assert_eq!(lead_dispatches(&workspace.successor), 0);
    let current = acceptance_stage(&controller, &acceptance, "lead-stage");
    assert_eq!(current["stage"], retained["stage"]);
    assert_eq!(current["fork"], retained["fork"]);
    assert_eq!(current["turnId"], retained["turnId"]);
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap().lead.state,
        StageState::InFlight
    );
}
