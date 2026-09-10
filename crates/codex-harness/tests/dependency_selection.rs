#![cfg(windows)]
use std::{fs, process::Command};

#[test]
#[ignore = "explicit owned staged artifact and trusted digest; executes package runtime and exercises local selection/rollback"]
fn official_staged_artifact_selection_preserves_conflicts_and_rolls_back() {
    use serde_json::Value;
    use std::{ffi::OsString, path::PathBuf};
    let stage =
        PathBuf::from(std::env::var_os("HARNESS_DEPENDENCY_STAGE").expect("explicit stage"));
    let digest =
        std::env::var_os("HARNESS_DEPENDENCY_MANIFEST_SHA256").expect("explicit trusted digest");
    let slot = std::env::var("HARNESS_DEPENDENCY_SLOT").expect("explicit slot");
    assert!(["codebase-memory", "nuphus", "basedpyright"].contains(&slot.as_str()));
    let state = stage.parent().unwrap().parent().unwrap();
    let active = state.join(format!("dependency-{slot}.json"));
    assert!(
        !active.exists(),
        "acceptance requires an initially unselected owned slot"
    );
    assert!(
        !state
            .join(format!("dependency-{slot}-journal.json"))
            .exists()
    );
    let evidence = tempfile::Builder::new()
        .prefix("harness-dependency-selection-native-")
        .tempdir()
        .unwrap()
        .keep();
    println!("selection evidence {}", evidence.display());
    let invoke = |label: &str, options: &[OsString], success: bool| -> Option<Value> {
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .arg("dependencies")
            .args(options)
            .current_dir(&evidence)
            .output()
            .unwrap();
        fs::write(evidence.join(format!("{label}.stdout")), &output.stdout).unwrap();
        fs::write(evidence.join(format!("{label}.stderr")), &output.stderr).unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{label}: {}; {}",
            String::from_utf8_lossy(&output.stderr),
            evidence.display()
        );
        if success {
            assert!(output.stderr.is_empty());
            Some(serde_json::from_slice(&output.stdout).unwrap())
        } else {
            assert!(output.stdout.is_empty());
            None
        }
    };
    let mut select: Vec<OsString> = vec![
        "select".into(),
        "--state".into(),
        state.into(),
        "--slot".into(),
        slot.clone().into(),
        "--stage".into(),
        stage.clone().into(),
        "--manifest-sha256".into(),
        digest,
    ];
    if let Some(node) = std::env::var_os("HARNESS_DEPENDENCY_NODE") {
        select.extend([
            "--node".into(),
            node,
            "--node-sha256".into(),
            std::env::var_os("HARNESS_DEPENDENCY_NODE_SHA256").expect("explicit Node digest"),
        ]);
    }
    let selected: Vec<OsString> = vec![
        "selected".into(),
        "--state".into(),
        state.into(),
        "--slot".into(),
        slot.clone().into(),
    ];
    let initial = invoke("select", &select, true).unwrap();
    assert_eq!(initial["changed"], true);
    assert_eq!(initial["global_registration_changed"], false);
    assert_eq!(initial["candidate"]["status"], "runtime-verified");
    let receipt = initial["receipt_sha256"].as_str().unwrap();
    let inspected = invoke("selected", &selected, true).unwrap();
    assert_eq!(inspected["status"], "selected");
    assert_eq!(inspected["package_code_executed"], false);
    assert_eq!(inspected["candidate"]["package_code_executed"], false);
    assert_eq!(invoke("repeat", &select, true).unwrap()["changed"], false);

    // Both objects belong to this acceptance actor. A copying editor's
    // replacement retains bytes but gets a different file ID.
    let backup = tempfile::tempdir_in(state).unwrap();
    let original = backup.path().join("original");
    let bytes = fs::read(&active).unwrap();
    fs::rename(&active, &original).unwrap();
    fs::copy(&original, &active).unwrap();
    invoke("foreign-selected", &selected, false);
    invoke("foreign-select", &select, false);
    assert_eq!(fs::read(&active).unwrap(), bytes);
    assert_eq!(fs::read(&original).unwrap(), bytes);
    fs::remove_file(&active).unwrap();
    fs::rename(&original, &active).unwrap();
    let rollback: Vec<OsString> = vec![
        "rollback-selection".into(),
        "--state".into(),
        state.into(),
        "--slot".into(),
        slot.clone().into(),
        "--receipt-sha256".into(),
        receipt.into(),
    ];
    assert_eq!(
        invoke("rollback", &rollback, true).unwrap()["changed"],
        true
    );
    assert_eq!(
        invoke("absent", &selected, true).unwrap()["status"],
        "absent"
    );
    assert!(!active.exists());
    assert!(stage.join("manifest.json").is_file());
    let recovery: Vec<OsString> = vec![
        "recover-selection".into(),
        "--state".into(),
        state.into(),
        "--slot".into(),
        slot.into(),
    ];
    assert_eq!(
        invoke("idle-recovery", &recovery, true).unwrap()["changed"],
        false
    );
}

#[test]
fn selection_options_reject_ambiguous_or_untrusted_inputs_before_mutation() {
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("state");
    for options in [
        vec![
            "select",
            "--state",
            absent.to_str().unwrap(),
            "--slot",
            "../escape",
        ],
        vec![
            "validate",
            "--stage",
            "PRIVATE-CANDIDATE",
            "--manifest-sha256",
            "a",
            "--node",
            "PRIVATE-NODE",
        ],
        vec![
            "validate",
            "--stage",
            "PRIVATE-CANDIDATE",
            "--stage",
            "other",
        ],
        vec![
            "selected",
            "--state",
            absent.to_str().unwrap(),
            "--slot",
            "nuphus",
            "--node",
            "PRIVATE-NODE",
        ],
        vec![
            "recover-selection",
            "--state",
            absent.to_str().unwrap(),
            "--slot",
            "nuphus",
        ],
        vec![
            "rollback-selection",
            "--state",
            absent.to_str().unwrap(),
            "--slot",
            "nuphus",
            "--receipt-sha256",
            "PRIVATE-INVALID",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .arg("dependencies")
            .args(options)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-"));
        assert!(!absent.exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn selection_help_has_no_package_or_state_side_effects() {
    let root = tempfile::tempdir().unwrap();
    for operation in [
        "validate",
        "select",
        "selected",
        "recover-selection",
        "rollback-selection",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["dependencies", operation, "--help"])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert!(String::from_utf8_lossy(&output.stdout).contains("trusted preparation result"));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
