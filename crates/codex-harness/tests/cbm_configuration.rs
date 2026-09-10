#![cfg(windows)]
use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn missing_cache_has_native_defaults_without_creation_or_process_start() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("not-created");
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["dependencies", "resource-check", "--cache"])
        .arg(&missing)
        .current_dir(root.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["policy_active"], false);
    assert_eq!(report["configuration"]["auto_watch"], true);
    assert_eq!(report["configuration"]["ui_enabled"], true);
    assert_eq!(report["package_code_executed"], false);
    assert!(!missing.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn malformed_private_configuration_and_options_are_not_echoed() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.json"), b"PRIVATE-CONFIGURATION").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["dependencies", "resource-check", "--cache"])
        .arg(root.path())
        .current_dir(root.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-CONFIGURATION"));
    assert_eq!(
        fs::read(root.path().join("config.json")).unwrap(),
        b"PRIVATE-CONFIGURATION"
    );
}
