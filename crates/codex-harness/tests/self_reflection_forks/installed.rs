//! The ordinary accepted reflection lifecycle through an immutable installed
//! bundle outside this checkout. The native board, retained delivery, installed
//! manager, installed launcher and registered upstream all participate; only
//! the model conversations are the existing canned fixture.

use super::reflection_install::{InstalledReflection, PRESERVED};
use super::*;
use harness_core::delivery_reflection::{DeliveryState, ReflectionController};

#[test]
fn installed_acceptance_drives_reflections_disables_and_preserves_restored_state() {
    let workspace = Workspace::new("reflection-installed", LeadMode::Delivered);
    let installed = InstalledReflection::connect(&workspace);

    let (task, acceptance) = lifecycle::normal_acceptance(&workspace, &installed.manager);
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    installed.assert_app_servers(&workspace.checkout);

    let controller = ReflectionController::new("bd.exe", &workspace.checkout, &workspace.home);
    let state = controller.stored_state().unwrap().unwrap();
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
