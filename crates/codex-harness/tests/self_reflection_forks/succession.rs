//! Verified managed-lead succession after an accepted closure retained work.
//! These checks use the real CLI, lifecycle lock, native stage owner and
//! loopback lead endpoints; model turns remain synthetic fixture responses.

use super::*;
use harness_core::delivery_reflection::{
    DeliveryState, LifecycleStatus, ReflectionController, StageState,
};

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

/// Re-registers the retained lead with a fresh live owner process so a retired
/// owner can attempt ordinary commands through the real managed-lead route.
fn install_live_old_registry(workspace: &Workspace) {
    let owner = workspace.spawn_owner_process();
    workspace.set_endpoint_process(&workspace.endpoint, &owner);
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

/// Activates the shared acceptance fixture, then records the exact live owner
/// process through the ordinary current-owner lifecycle route. The endpoint
/// file is updated before it is installed in the managed-lead registry, so
/// removing that record later is distinct from the process exiting.
fn accepted_with_owner(workspace: &Workspace) -> (ReflectionController, String) {
    let (controller, _request, acceptance) = accepted::accepted(workspace);
    let owner = workspace.spawn_owner_process();
    workspace.set_endpoint_process(&workspace.endpoint, &owner);
    controller
        .retain_owner_process(LEAD_THREAD, &owner)
        .unwrap();
    (controller, acceptance)
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

fn lifecycle_files(workspace: &Workspace) -> Vec<PathBuf> {
    let directory = workspace.home.join("harness/delivery-reflection");
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|value| value == "json"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn successor_turn_available(workspace: &Workspace) {
    let mut state = workspace.successor_state.lock().unwrap();
    state.source_turn = Some(LEAD_SOURCE_TURN.to_owned());
    state.source_turn_active = true;
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
    let (controller, acceptance) = accepted_with_owner(&workspace);
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

    // The registration is gone, but the exact retained owner process is still
    // live: this is not exit evidence and must refuse with no state change.
    remove_old_registry(&workspace);
    let settled_before_succession = state_bytes(&controller);
    let live = command_failure(lifecycle::command(
        &workspace,
        "succeed",
        Some(SUCCESSOR_THREAD),
    ));
    assert!(live.contains("still live"), "{live}");
    assert_eq!(state_bytes(&controller), settled_before_succession);
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));

    // A real exit plus the absent registration is the positive proof.
    workspace.stop_owner_process();
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

    install_live_old_registry(&workspace);
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
    let (controller, _acceptance) = accepted_with_owner(&workspace);
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
    let live_retained = command_failure(lifecycle::command(
        &workspace,
        "succeed",
        Some(SUCCESSOR_THREAD),
    ));
    assert!(live_retained.contains("still live"), "{live_retained}");
    assert_eq!(state_bytes(&controller), settled);

    workspace.stop_owner_process();
    let transferred = succeed(&workspace, SUCCESSOR_THREAD);
    assert_eq!(transferred["changed"], true);
    let transferred_state = state_bytes(&controller);

    install_live_old_registry(&workspace);
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
    let (controller, acceptance) = accepted_with_owner(&workspace);
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
    workspace.stop_owner_process();
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
    let (controller, acceptance) = accepted_with_owner(&workspace);
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
    workspace.stop_owner_process();
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
    let (controller, acceptance) = accepted_with_owner(&workspace);
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
    workspace.stop_owner_process();
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

#[test]
fn successor_refuses_an_incomplete_retired_source_checkpoint_before_any_lead_fork() {
    let workspace = Workspace::new("succession-source-checkpoint", LeadMode::Delivered);
    let (controller, acceptance) = accepted_with_owner(&workspace);
    lifecycle::install_fixture(&workspace);
    install_successor_registry(&workspace);

    // Retain the executor result under the original owner without running the
    // pending lead stage.
    workspace.lead_state.lock().unwrap().source_turn = None;
    let first = process(&workspace, LEAD_THREAD);
    assert_refused(&first, "exact accepting lead turn");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    assert_eq!(
        controller
            .record(&acceptance)
            .unwrap()
            .unwrap()
            .executor
            .state,
        StageState::Retained
    );

    remove_old_registry(&workspace);
    workspace.stop_owner_process();
    assert_eq!(succeed(&workspace, SUCCESSOR_THREAD)["changed"], true);

    // The fixture recovers its fork slot from this marker. With the outcome
    // below it reports the retired source checkpoint as inProgress, which the
    // existing native source-completion validation must refuse before any
    // successor lead fork or turn is started.
    let fork = json!({
        "forkThread": LEAD_FORK,
        "cwd": workspace.checkout,
        "model": MODEL,
        "modelProvider": PROVIDER,
        "reasoningEffort": EFFORT,
        "config": {},
    });
    fs::write(
        workspace.fixture_dir.join("fork-lead-self-reflection.json"),
        serde_json::to_vec_pretty(&fork).unwrap(),
    )
    .unwrap();

    let refused = process_with(&workspace, SUCCESSOR_THREAD, "source-checkpoint-changed");
    assert_refused(&refused, "not completed");
    assert_eq!(request_counts(&workspace), (1, 1, 0, 0));
    assert_eq!(lead_dispatches(&workspace.successor), 0);
    assert!(
        fixture_requests(&workspace, "lead-self-reflection", "thread/fork").is_empty(),
        "the successor forked the retired source before completing it"
    );
    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, StageState::Retained);
    assert_ne!(record.delivery, DeliveryState::Consumed);
}

#[test]
fn successor_acceptance_automatically_transfers_an_inactive_owners_lifecycle() {
    let workspace = Workspace::new("succession-automatic-inactive", LeadMode::Delivered);
    let (controller, retained_acceptance) = accepted_with_owner(&workspace);
    lifecycle::install_fixture(&workspace);
    install_successor_registry(&workspace);
    successor_turn_available(&workspace);

    let before = controller.stored_state().unwrap().unwrap();
    assert_eq!(before.lifecycle, LifecycleStatus::Active, "{before:?}");
    assert_eq!(before.binding.lead_session, LEAD_THREAD, "{before:?}");
    assert_eq!(before.records.len(), 1, "{before:?}");
    let retained_record = before.records[&retained_acceptance].clone();
    let source = lifecycle::source_fixture(&workspace, "succession-automatic-inactive", true);
    let pending = lifecycle::prepare_additional_ordinary_acceptance(
        &workspace,
        &manager(),
        &source,
        SUCCESSOR_THREAD,
        "successor-accepted-run",
        2,
    );

    // Remove the old registration and positively end its exact retained owner.
    // The successor is already registered, so accepting ordinary work is the
    // only activation input and must use the verified succession path.
    remove_old_registry(&workspace);
    workspace.stop_owner_process();
    let accepted = pending.accept(&workspace);
    assert_eq!(accepted["status"], "accepted", "{accepted}");
    assert_eq!(accepted["modelCalls"], 0, "{accepted}");
    assert_eq!(
        accepted["reflection"]["automaticTrigger"], true,
        "{accepted}"
    );
    assert_eq!(
        lifecycle::show_task(&workspace, &pending.task)["status"],
        "closed"
    );

    let after = controller.stored_state().unwrap().unwrap();
    assert_eq!(after.lifecycle, LifecycleStatus::Active, "{after:?}");
    assert_eq!(after.binding.id, before.binding.id, "{after:?}");
    assert_eq!(after.binding.lead_session, SUCCESSOR_THREAD, "{after:?}");
    assert_eq!(
        after.binding.revision,
        before.binding.revision + 1,
        "{after:?}"
    );
    assert_eq!(after.checkpoint, before.checkpoint, "{after:?}");
    assert_eq!(after.records, before.records, "{after:?}");
    assert_eq!(after.journal, before.journal, "{after:?}");
    assert!(before.source.same_scope(&after.source), "{after:?}");
    assert!(
        after.retired_leads.iter().any(|lead| lead == LEAD_THREAD),
        "the inactive owner was not retired: {after:?}"
    );
    assert_eq!(after.records[&retained_acceptance], retained_record);
    assert_eq!(
        lifecycle_files(&workspace),
        vec![controller.state_file().unwrap()]
    );
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
}

#[test]
fn successor_acceptance_refuses_a_live_owner_before_closing_the_new_task() {
    let workspace = Workspace::new("succession-automatic-live", LeadMode::Delivered);
    let (controller, _acceptance) = accepted_with_owner(&workspace);
    lifecycle::install_fixture(&workspace);
    install_successor_registry(&workspace);
    successor_turn_available(&workspace);

    let before = state_bytes(&controller);
    let source = lifecycle::source_fixture(&workspace, "succession-automatic-live", true);
    let pending = lifecycle::prepare_additional_ordinary_acceptance(
        &workspace,
        &manager(),
        &source,
        SUCCESSOR_THREAD,
        "live-owner-run",
        2,
    );
    let failure = command_failure(pending.accept_raw(&workspace));
    assert!(
        failure.to_lowercase().contains("live"),
        "the refusal did not name the retained live owner: {failure}"
    );
    assert_eq!(state_bytes(&controller), before);
    let shown = lifecycle::show_task(&workspace, &pending.task);
    assert_eq!(shown["status"], "open", "{shown}");
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
}
