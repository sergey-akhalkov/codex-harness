#![cfg(windows)]
use std::{fs, process::Command};

#[test]
fn invalid_python_hash_does_not_create_state_or_execute_tools() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .current_dir(root.path())
        .args(["dependencies", "stage-python", "--uv"])
        .arg(root.path().join("missing-uv.exe"))
        .args(["--uv-sha256", "invalid", "--python"])
        .arg(root.path().join("missing-python.exe"))
        .arg("--python-sha256")
        .arg("0".repeat(64))
        .arg("--state")
        .arg(root.path().join("state"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn python_stage_help_and_duplicate_options_preserve_empty_working_directory() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .current_dir(root.path())
        .args(["dependencies", "stage-python", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("empty offline UV environment"));
    let duplicate = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .current_dir(root.path())
        .args(["dependencies", "stage-python", "--uv", "a", "--uv", "b"])
        .output()
        .unwrap();
    assert_eq!(duplicate.status.code(), Some(2));
    assert!(duplicate.stdout.is_empty());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}
