#![cfg(windows)]
//! Explicit published-package lifecycle acceptance. No live registrations.
use serde_json::{Value, json};
use std::{collections::BTreeMap, ffi::OsStr, fs, path::Path, process::Command};

fn command(args: &[&OsStr]) -> Value {
    let result = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
fn snapshot(root: &Path) -> BTreeMap<std::path::PathBuf, (u64, std::time::SystemTime)> {
    let mut result = BTreeMap::new();
    if !root.exists() {
        return result;
    }
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let metadata = fs::symlink_metadata(entry.path()).unwrap();
            assert!(!metadata.is_symlink());
            result.insert(
                entry.path().strip_prefix(root).unwrap().to_owned(),
                (metadata.len(), metadata.modified().unwrap()),
            );
            if metadata.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    result
}

#[test]
#[ignore = "explicit official release download and bounded runtime probe; requires private CODEGRAPH_DEPENDENCY_ACCEPTANCE_OUTPUT"]
fn native_entry_stages_qualifies_selects_checks_and_rolls_back_published_codegraph() {
    let output = std::path::PathBuf::from(
        std::env::var_os("CODEGRAPH_DEPENDENCY_ACCEPTANCE_OUTPUT").expect("private output path"),
    );
    let owned = tempfile::tempdir().unwrap();
    let state = owned.path().join("dependency-state");
    let home = owned.path().join("home");
    let missing = command(&[
        "mcp".as_ref(),
        "prepare-codegraph".as_ref(),
        "--mode".as_ref(),
        "Check".as_ref(),
        "--codex-home".as_ref(),
        home.as_os_str(),
        "--dependency-state".as_ref(),
        state.as_os_str(),
    ]);
    assert_eq!(missing["status"], "missing");
    assert!(!home.exists() && !state.exists());
    let staged = command(&[
        "dependencies".as_ref(),
        "stage".as_ref(),
        "--package".as_ref(),
        "@colbymchenry/codegraph".as_ref(),
        "--version".as_ref(),
        "1.6.0".as_ref(),
        "--state".as_ref(),
        state.as_os_str(),
    ]);
    let mut report = json!({"missing_check":missing,"staged":staged});
    fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    let stage = std::path::PathBuf::from(staged["stage"].as_str().unwrap());
    let digest = staged["manifest_sha256"].as_str().unwrap();
    let selected = command(&[
        "dependencies".as_ref(),
        "select".as_ref(),
        "--state".as_ref(),
        state.as_os_str(),
        "--slot".as_ref(),
        "codegraph".as_ref(),
        "--stage".as_ref(),
        stage.as_os_str(),
        "--manifest-sha256".as_ref(),
        digest.as_ref(),
    ]);
    assert_eq!(selected["candidate"]["runtime_compatibility"], "probed");
    assert_eq!(
        selected["candidate"]["runtime"]["cleanup"]["job"]["active_processes"],
        0
    );
    let before = snapshot(owned.path());
    let inspected = command(&[
        "dependencies".as_ref(),
        "selected".as_ref(),
        "--state".as_ref(),
        state.as_os_str(),
        "--slot".as_ref(),
        "codegraph".as_ref(),
    ]);
    assert_eq!(inspected["status"], "selected");
    assert_eq!(inspected["package_code_executed"], false);
    let checked = command(&[
        "mcp".as_ref(),
        "prepare-codegraph".as_ref(),
        "--mode".as_ref(),
        "Check".as_ref(),
        "--codex-home".as_ref(),
        home.as_os_str(),
        "--dependency-state".as_ref(),
        state.as_os_str(),
    ]);
    assert_eq!(checked["status"], "prepared");
    assert_eq!(checked["read_only"], true);
    assert!(checked["qualification"].is_null());
    assert_eq!(
        before,
        snapshot(owned.path()),
        "read-only commands changed owned files"
    );
    let recovered = command(&[
        "dependencies".as_ref(),
        "recover-selection".as_ref(),
        "--state".as_ref(),
        state.as_os_str(),
        "--slot".as_ref(),
        "codegraph".as_ref(),
    ]);
    assert_eq!(recovered["changed"], false);
    let receipt = selected["receipt_sha256"].as_str().unwrap();
    let rollback = command(&[
        "dependencies".as_ref(),
        "rollback-selection".as_ref(),
        "--state".as_ref(),
        state.as_os_str(),
        "--slot".as_ref(),
        "codegraph".as_ref(),
        "--receipt-sha256".as_ref(),
        receipt.as_ref(),
    ]);
    let absent = command(&[
        "dependencies".as_ref(),
        "selected".as_ref(),
        "--state".as_ref(),
        state.as_os_str(),
        "--slot".as_ref(),
        "codegraph".as_ref(),
    ]);
    assert_eq!(absent["status"], "absent");
    assert!(
        stage.join("package/node.exe").is_file(),
        "rollback deleted the shared candidate"
    );
    report["selected"] = selected;
    report["checked"] = checked;
    report["inspected"] = inspected;
    report["recovered"] = recovered;
    report["rollback"] = rollback;
    report["absent"] = absent;
    fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
