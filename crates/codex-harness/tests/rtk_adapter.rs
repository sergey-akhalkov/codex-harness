//! Native harness-rtk acceptance against real child processes in an owned TEMP copy.
#![cfg(windows)]

use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn adapter() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_codex-harness")).with_file_name("harness-rtk.exe");
    assert!(
        path.is_file(),
        "build the RTK adapter first: cargo build --locked -p harness-rtk --jobs 1; missing {}",
        path.display()
    );
    path
}

fn observer() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-observe"))
}

fn invoke(
    adapter: &Path,
    args: &[&str],
    cwd: &Path,
    home: &Path,
    data: Option<&[u8]>,
) -> std::process::Output {
    let mut command = Command::new(adapter);
    command.args(args).current_dir(cwd).env("CODEX_HOME", home);
    command.env_remove("HARNESS_RTK_DISABLE");
    command.env("PROCESS_CASE_ROOT", cwd);
    if data.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Some(bytes) = data {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn invoke_with_env(
    adapter: &Path,
    args: &[&str],
    cwd: &Path,
    home: &Path,
    data: Option<&[u8]>,
    extra: &[(&str, &str)],
) -> std::process::Output {
    let mut command = Command::new(adapter);
    command.args(args).current_dir(cwd).env("CODEX_HOME", home);
    command.env_remove("HARNESS_RTK_DISABLE");
    command.env("PROCESS_CASE_ROOT", cwd);
    for (key, value) in extra {
        command.env(*key, *value);
    }
    if data.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Some(bytes) = data {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn bash(command: &str) -> Value {
    json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": command }
    })
}

#[test]
fn exec_preserves_child_identity_and_runs_once() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let first = invoke(
        &adapter(),
        &[
            "exec",
            observer().to_str().unwrap(),
            "--fixture",
            "echo-args",
            "проверка-файл",
        ],
        &workspace,
        &home,
        None,
    );
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(stdout.contains("проверка-файл"), "{stdout}");
    assert!(!first.stdout.windows(9).any(|window| window == b"[rtk raw:"));
    let second = invoke(
        &adapter(),
        &[
            "exec",
            observer().to_str().unwrap(),
            "--fixture",
            "echo-args",
            "проверка-файл",
        ],
        &workspace,
        &home,
        None,
    );
    assert!(second.status.success());
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn hook_rewrites_literal_exec_and_ignores_malformed_and_stop() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let adapter = adapter();
    let local = root.path().join("harness-rtk.exe");
    fs::copy(&adapter, &local).unwrap();
    fs::write(
        local.with_file_name("rtk.exe"),
        b"not the real rtk dependency",
    )
    .unwrap();
    let rewritten = invoke(
        &local,
        &["hook"],
        &workspace,
        &home,
        Some(
            serde_json::to_vec(&bash("harness-rtk.exe exec git status --short"))
                .unwrap()
                .as_slice(),
        ),
    );
    assert!(rewritten.status.success());
    let value: Value = serde_json::from_slice(&rewritten.stdout).unwrap();
    assert_eq!(
        value["hookSpecificOutput"]["updatedInput"]["command"],
        "harness-rtk.exe compact git status --short"
    );
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "allow");
    let silent = vec![
        b"not-json".to_vec(),
        serde_json::to_vec(&json!({
            "hook_event_name": "Stop",
            "tool_name": "Bash",
            "tool_input": { "command": "harness-rtk.exe exec git log -n 80" }
        }))
        .unwrap(),
        serde_json::to_vec(&json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Write",
            "tool_input": { "command": "harness-rtk.exe exec git log -n 80" }
        }))
        .unwrap(),
        serde_json::to_vec(&bash("harness-rtk.exe compact git log -n 80")).unwrap(),
        serde_json::to_vec(&bash("harness-rtk.exe exec git log -n 80 | cat")).unwrap(),
    ];
    for payload in silent {
        let output = invoke(&adapter, &["hook"], &workspace, &home, Some(&payload));
        assert!(output.status.success());
        assert!(output.stdout.is_empty(), "{:?}", output.stdout);
    }
    let disabled = invoke_with_env(
        &local,
        &["hook"],
        &workspace,
        &home,
        Some(
            serde_json::to_vec(&bash("harness-rtk.exe exec git status --short"))
                .unwrap()
                .as_slice(),
        ),
        &[("HARNESS_RTK_DISABLE", "1")],
    );
    assert!(disabled.status.success());
    assert!(disabled.stdout.is_empty(), "{:?}", disabled.stdout);
    let missing_dir = root.path().join("missing-rtk");
    fs::create_dir(&missing_dir).unwrap();
    let missing = missing_dir.join("harness-rtk.exe");
    fs::copy(&adapter, &missing).unwrap();
    let missing_hook = invoke(
        &missing,
        &["hook"],
        &workspace,
        &home,
        Some(
            serde_json::to_vec(&bash("harness-rtk.exe exec git status --short"))
                .unwrap()
                .as_slice(),
        ),
    );
    assert!(missing_hook.status.success());
    assert!(missing_hook.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&missing_hook.stderr)
            .contains("dependency unavailable; original command unchanged")
    );
}

#[test]
fn filter_failure_and_oversize_stay_raw_without_locator() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let adapter = adapter();
    let failed = invoke(
        &adapter,
        &["filter", "not-a-filter"],
        &workspace,
        &home,
        Some(&[b'x'; 600]),
    );
    assert!(failed.status.success());
    assert_eq!(failed.stdout, vec![b'x'; 600]);
    assert!(String::from_utf8_lossy(&failed.stderr).contains("raw passthrough"));
    let over = vec![b'x'; 4 * 1024 * 1024 + 10];
    let oversized = invoke(
        &adapter,
        &["filter", "git-log"],
        &workspace,
        &home,
        Some(&over),
    );
    assert!(oversized.status.success());
    assert_eq!(oversized.stdout, over);
    assert!(
        !oversized
            .stdout
            .windows(9)
            .any(|window| window == b"[rtk raw:")
    );
    assert!(String::from_utf8_lossy(&oversized.stderr).contains("stdout exceeds 4 MiB"));
}

#[test]
fn compact_without_rtk_and_disable_stay_raw() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let adapter = adapter();
    let missing_dir = root.path().join("missing-rtk");
    fs::create_dir(&missing_dir).unwrap();
    let missing = missing_dir.join("harness-rtk.exe");
    fs::copy(&adapter, &missing).unwrap();
    let raw = invoke(
        &adapter,
        &[
            "exec",
            observer().to_str().unwrap(),
            "--fixture",
            "echo-args",
            "raw-passthrough",
        ],
        &workspace,
        &home,
        None,
    );
    let compact = invoke(
        &missing,
        &[
            "compact",
            observer().to_str().unwrap(),
            "--fixture",
            "echo-args",
            "raw-passthrough",
        ],
        &workspace,
        &home,
        None,
    );
    assert_eq!(compact.status.code(), raw.status.code());
    assert_eq!(compact.stdout, raw.stdout);
    assert!(
        !compact
            .stdout
            .windows(9)
            .any(|window| window == b"[rtk raw:")
    );
    let disabled = invoke_with_env(
        &adapter,
        &[
            "compact",
            observer().to_str().unwrap(),
            "--fixture",
            "echo-args",
            "raw-passthrough",
        ],
        &workspace,
        &home,
        None,
        &[("HARNESS_RTK_DISABLE", "1")],
    );
    assert_eq!(disabled.stdout, raw.stdout);
    assert!(
        !disabled
            .stdout
            .windows(9)
            .any(|window| window == b"[rtk raw:")
    );
}
