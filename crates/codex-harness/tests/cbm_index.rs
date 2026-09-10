#![cfg(windows)]
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn harness(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    command.current_dir(root);
    command
}

fn rejected(output: &Output, root: &Path, files: usize) {
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read_dir(root).unwrap().count(), files);
}

fn cbm_index(root: &Path, request: &Path) -> Command {
    let mut command = harness(root);
    command
        .args(["dependencies", "cbm-index", "--executable"])
        .arg(root.join("missing.exe"))
        .arg("--cache")
        .arg(root.join("cache"))
        .arg("--runtime")
        .arg(root.join("runtime"))
        .arg("--account")
        .arg(root.join("account"))
        .arg("--arguments-file")
        .arg(request);
    command
}

fn cbm_tool(root: &Path, request: &Path, tool: &str) -> Command {
    let mut command = harness(root);
    command
        .args(["dependencies", "cbm-tool", "--executable"])
        .arg(root.join("missing.exe"))
        .arg("--cache")
        .arg(root.join("cache"))
        .arg("--tool")
        .arg(tool)
        .arg("--arguments-file")
        .arg(request);
    command
}

#[test]
fn invalid_request_fails_before_cache_account_or_runtime_creation() {
    let root = tempfile::tempdir().unwrap();
    let request = root.path().join("arguments.json");
    fs::write(&request, b"[]").unwrap();
    let output = cbm_index(root.path(), &request).output().unwrap();
    rejected(&output, root.path(), 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("bounded object arguments"));
}

#[test]
fn cbm_index_help_and_missing_options_do_not_launch_an_upstream_tool() {
    let root = tempfile::tempdir().unwrap();
    let help = harness(root.path())
        .args(["dependencies", "cbm-index", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("selected cache is modified"));
    let missing = harness(root.path())
        .args(["dependencies", "cbm-index", "--cache"])
        .output()
        .unwrap();
    rejected(&missing, root.path(), 0);
    let required = harness(root.path())
        .args(["dependencies", "cbm-index", "--executable"])
        .arg(root.path().join("missing.exe"))
        .arg("--cache")
        .arg(root.path().join("cache"))
        .arg("--runtime")
        .arg(root.path().join("runtime"))
        .arg("--account")
        .arg(root.path().join("account"))
        .output()
        .unwrap();
    rejected(&required, root.path(), 0);
}

#[test]
fn cbm_index_duplicate_and_unknown_switches_fail_before_spawn() {
    let root = tempfile::tempdir().unwrap();
    let duplicate = harness(root.path())
        .args([
            "dependencies",
            "cbm-index",
            "--cache",
            "cache",
            "--cache",
            "other",
        ])
        .output()
        .unwrap();
    rejected(&duplicate, root.path(), 0);
    assert!(
        String::from_utf8_lossy(&duplicate.stderr).contains("invalid dependency command options")
    );
    let unknown = harness(root.path())
        .args(["dependencies", "cbm-index", "--nope", "1"])
        .output()
        .unwrap();
    rejected(&unknown, root.path(), 0);
}

#[test]
fn cbm_index_malformed_and_duplicate_json_fail_before_spawn() {
    let root = tempfile::tempdir().unwrap();
    let request = root.path().join("arguments.json");
    fs::write(&request, b"{").unwrap();
    let malformed = cbm_index(root.path(), &request).output().unwrap();
    rejected(&malformed, root.path(), 1);
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("MCP malformed JSON output"));
    fs::write(&request, br#"{"a":1,"a":2}"#).unwrap();
    let duplicate = cbm_index(root.path(), &request).output().unwrap();
    rejected(&duplicate, root.path(), 1);
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("MCP malformed JSON output"));
}

#[test]
fn catalogue_rejects_incomplete_options_and_absent_account_without_creation() {
    let root = tempfile::tempdir().unwrap();
    let help = harness(root.path())
        .args(["dependencies", "cbm-catalogue", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("No tool is called"));
    let incomplete = harness(root.path())
        .args(["dependencies", "cbm-catalogue", "--executable"])
        .output()
        .unwrap();
    rejected(&incomplete, root.path(), 0);
    let absent = harness(root.path())
        .args(["dependencies", "cbm-catalogue", "--executable"])
        .arg(root.path().join("missing.exe"))
        .arg("--account")
        .arg(root.path().join("absent"))
        .output()
        .unwrap();
    rejected(&absent, root.path(), 0);
}

#[test]
fn cbm_tool_help_and_invalid_switches_do_not_launch_an_upstream_tool() {
    let root = tempfile::tempdir().unwrap();
    let help = harness(root.path())
        .args(["dependencies", "cbm-tool", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("indexing uses cbm-index"));
    let missing = harness(root.path())
        .args(["dependencies", "cbm-tool", "--executable"])
        .output()
        .unwrap();
    rejected(&missing, root.path(), 0);
    let required = harness(root.path())
        .args(["dependencies", "cbm-tool", "--executable"])
        .arg(root.path().join("missing.exe"))
        .arg("--cache")
        .arg(root.path().join("cache"))
        .arg("--tool")
        .arg("search_graph")
        .output()
        .unwrap();
    rejected(&required, root.path(), 0);
    let duplicate = harness(root.path())
        .args(["dependencies", "cbm-tool", "--tool", "a", "--tool", "b"])
        .output()
        .unwrap();
    rejected(&duplicate, root.path(), 0);
    assert!(
        String::from_utf8_lossy(&duplicate.stderr).contains("invalid dependency command options")
    );
    let unknown = harness(root.path())
        .args(["dependencies", "cbm-tool", "--nope", "1"])
        .output()
        .unwrap();
    rejected(&unknown, root.path(), 0);
}

#[test]
fn cbm_tool_rejects_nonobject_and_malformed_arguments_without_writes() {
    let root = tempfile::tempdir().unwrap();
    let request = root.path().join("arguments.json");
    fs::write(&request, b"[]").unwrap();
    let nonobject = cbm_tool(root.path(), &request, "search_graph")
        .output()
        .unwrap();
    rejected(&nonobject, root.path(), 1);
    assert!(String::from_utf8_lossy(&nonobject.stderr).contains("bounded object arguments"));
    fs::write(&request, b"{").unwrap();
    let malformed = cbm_tool(root.path(), &request, "search_graph")
        .output()
        .unwrap();
    rejected(&malformed, root.path(), 1);
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("MCP malformed JSON output"));
    fs::write(&request, br#"{"a":1,"a":2}"#).unwrap();
    let duplicate = cbm_tool(root.path(), &request, "search_graph")
        .output()
        .unwrap();
    rejected(&duplicate, root.path(), 1);
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("MCP malformed JSON output"));
}

#[test]
fn cbm_tool_rejects_index_repository_and_unknown_tool_without_writes() {
    let root = tempfile::tempdir().unwrap();
    let request = root.path().join("arguments.json");
    fs::write(&request, b"{}").unwrap();
    let index = cbm_tool(root.path(), &request, "index_repository")
        .output()
        .unwrap();
    rejected(&index, root.path(), 1);
    assert!(
        String::from_utf8_lossy(&index.stderr)
            .contains("CBM tool is unknown or requires the audited indexing path")
    );
    let unknown = cbm_tool(root.path(), &request, "not-a-cbm-tool")
        .output()
        .unwrap();
    rejected(&unknown, root.path(), 1);
    assert!(
        String::from_utf8_lossy(&unknown.stderr)
            .contains("CBM tool is unknown or requires the audited indexing path")
    );
}
