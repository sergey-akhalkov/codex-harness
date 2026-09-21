//! Native harness-rtk acceptance against real child processes in an owned TEMP
//! copy. The console-stdout bypass runs under a native ConPTY session, so it
//! needs no interactive terminal of its own.
#![cfg(windows)]

use harness_core::{
    console::{ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline, StopReason},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
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

fn rtk_fixture() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_harness-rtk-fixture"));
    assert!(
        path.is_file(),
        "cargo test builds the fixture double; missing {}",
        path.display()
    );
    path
}

/// Owned adapter copy whose `rtk.exe` dependency and whose allowlisted source
/// command are the inert fixture double: no RTK download and no network.
fn staged(root: &Path) -> (PathBuf, PathBuf) {
    let directory = root.join("bin");
    fs::create_dir_all(&directory).unwrap();
    fs::copy(adapter(), directory.join("harness-rtk.exe")).unwrap();
    fs::copy(rtk_fixture(), directory.join("rtk.exe")).unwrap();
    let command = directory.join("git.exe");
    fs::copy(rtk_fixture(), &command).unwrap();
    (directory.join("harness-rtk.exe"), command)
}

fn packed_handle(stdout: &str) -> String {
    let line = stdout
        .lines()
        .find(|line| line.starts_with("[rtk pack: "))
        .expect("compact output must carry the pack footer");
    line["[rtk pack: ".len()..]
        .split(' ')
        .next()
        .unwrap()
        .to_owned()
}

fn command_output(command: &Path, args: &[&str], workspace: &Path) -> Vec<u8> {
    let output = Command::new(command)
        .args(args)
        .current_dir(workspace)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn pack_file(home: &Path, handle: &str) -> PathBuf {
    home.join("harness/rtk/pack").join(format!("{handle}.log"))
}

/// The single retained raw archive, which is what a whole-file raw re-read emits.
fn only_raw_archive(home: &Path) -> PathBuf {
    let mut entries: Vec<PathBuf> = fs::read_dir(home.join("harness/rtk/raw"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1, "one retained raw archive");
    entries.remove(0)
}

fn present_locator(stdout: &[u8]) -> bool {
    let text = String::from_utf8_lossy(stdout);
    text.contains("[rtk pack:") || text.contains("[rtk raw:")
}

/// Numbered content lines of a recall response, in emitted order.
fn numbered_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| {
            line.split_once(": ")
                .is_some_and(|(number, _)| number.parse::<u32>().is_ok())
        })
        .collect()
}

fn window_bounds(text: &str) -> (usize, usize, usize) {
    let line = text
        .lines()
        .find(|line| line.starts_with("window: lines "))
        .expect("recall must report its window");
    let window = &line["window: lines ".len()..];
    let (range, total) = window.split_once(" of ").expect("window totals");
    let (first, last) = range.split_once('-').expect("window range");
    (
        first.parse().unwrap(),
        last.parse().unwrap(),
        total.parse().unwrap(),
    )
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
    assert!(
        !first
            .stdout
            .windows(10)
            .any(|window| window == b"[rtk pack:"),
        "exec is a raw passthrough and mints no observation handle"
    );
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

#[test]
fn compact_mints_a_handle_and_recall_returns_the_exact_window() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let expected = command_output(&command, &["log", "-n", "300"], &workspace);
    let compact = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "300"],
        &workspace,
        &home,
        None,
    );
    assert!(
        compact.status.success(),
        "{}",
        String::from_utf8_lossy(&compact.stderr)
    );
    let text = String::from_utf8(compact.stdout.clone()).unwrap();
    assert!(
        text.starts_with(&format!(
            "fixture-pipe git-log: 300 lines, {} bytes\n",
            expected.len()
        )),
        "{text}"
    );
    let handle = packed_handle(&text);
    assert!(text.contains("sha256:"), "{text}");
    let retained: Vec<PathBuf> = fs::read_dir(home.join("harness/rtk/raw"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(retained.len(), 1);
    assert!(
        text.contains(&format!("[rtk raw: {}]", retained[0].display())),
        "{text}"
    );
    assert_eq!(fs::read(&retained[0]).unwrap(), expected);

    // The command double is gone, so a recall that returns content cannot have
    // rerun the source command.
    fs::remove_file(&command).unwrap();
    let window = invoke(
        &binary,
        &["recall", &handle, "--offset", "5", "--limit", "10"],
        &workspace,
        &home,
        None,
    );
    assert_eq!(
        window.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&window.stderr)
    );
    let recalled = String::from_utf8(window.stdout).unwrap();
    assert!(recalled.contains("source: "), "{recalled}");
    assert!(
        recalled.contains("digest: sha256 ") && recalled.contains(" verified"),
        "{recalled}"
    );
    assert!(recalled.contains("stored: 300 lines"), "{recalled}");
    assert!(recalled.contains("window: lines 5-14 of 300"), "{recalled}");
    let expected = String::from_utf8(expected).unwrap();
    for (position, line) in expected.lines().enumerate().skip(4).take(10) {
        assert!(
            recalled.contains(&format!("{}: {line}", position + 1)),
            "{recalled}"
        );
    }
    assert!(!recalled.contains("15: "), "{recalled}");
    assert!(recalled.contains("--offset 15"), "{recalled}");

    // Defaults are offset 1 and limit 200, with the next window named.
    let defaults = invoke(&binary, &["recall", &handle], &workspace, &home, None);
    let defaults = String::from_utf8(defaults.stdout).unwrap();
    assert!(
        defaults.contains("window: lines 1-200 of 300"),
        "{defaults}"
    );
    assert!(defaults.contains("--offset 201"), "{defaults}");
}

#[test]
fn compact_falls_back_to_todays_output_when_packing_is_unavailable() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let expected = command_output(&command, &["log", "-n", "300"], &workspace);
    // A file where the pack area belongs makes every pack step fail.
    fs::create_dir_all(home.join("harness/rtk")).unwrap();
    fs::write(home.join("harness/rtk/pack"), b"not a directory").unwrap();
    let compact = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "300"],
        &workspace,
        &home,
        None,
    );
    assert!(
        compact.status.success(),
        "{}",
        String::from_utf8_lossy(&compact.stderr)
    );
    let retained: Vec<PathBuf> = fs::read_dir(home.join("harness/rtk/raw"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(retained.len(), 1);
    assert_eq!(
        String::from_utf8(compact.stdout).unwrap(),
        format!(
            // The filter's own line, its newline and today's footer locator.
            "fixture-pipe git-log: 300 lines, {} bytes\n\n[rtk raw: {}]\n",
            expected.len(),
            retained[0].display()
        ),
        "a pack failure must keep today's exact compact output"
    );
    assert_eq!(fs::read(&retained[0]).unwrap(), expected);
    assert!(
        String::from_utf8_lossy(&compact.stderr).contains("observation handle not issued"),
        "{}",
        String::from_utf8_lossy(&compact.stderr)
    );
}

#[test]
fn bypass_paths_never_present_a_handle() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let log = command_output(&command, &["log", "-n", "300"], &workspace);
    let status = command_output(&command, &["status"], &workspace);
    let disabled = invoke_with_env(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "300"],
        &workspace,
        &home,
        None,
        &[("HARNESS_RTK_DISABLE", "1")],
    );
    assert_eq!(disabled.stdout, log);
    assert!(!present_locator(&disabled.stdout));
    let unsupported = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "status"],
        &workspace,
        &home,
        None,
    );
    assert_eq!(unsupported.stdout, status);
    assert!(!present_locator(&unsupported.stdout));
    // Allowlisted, but below the retention floor, so it stays raw too.
    let small = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "status", "--short"],
        &workspace,
        &home,
        None,
    );
    assert_eq!(small.stdout, status);
    assert!(!present_locator(&small.stdout));
    // A present but failing filter dependency stays raw, and the `filter`
    // entry point mints nothing because it knows no source command.
    let payload = vec![b'x'; 600];
    let failed = invoke(
        &binary,
        &["filter", "fixture-fail"],
        &workspace,
        &home,
        Some(&payload),
    );
    assert!(failed.status.success());
    assert_eq!(failed.stdout, payload);
    assert!(!present_locator(&failed.stdout));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("raw passthrough"));
    // Oversize output on the compact path (30000 fixture lines = 4830000 bytes)
    // passes raw as well, retaining nothing at all.
    let bulk = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "30000"],
        &workspace,
        &home,
        None,
    );
    assert!(
        bulk.status.success(),
        "{}",
        String::from_utf8_lossy(&bulk.stderr)
    );
    assert_eq!(bulk.stdout.len(), 30000 * 161);
    assert!(!present_locator(&bulk.stdout));
    assert!(
        String::from_utf8_lossy(&bulk.stderr).contains("stdout exceeds 4 MiB"),
        "{}",
        String::from_utf8_lossy(&bulk.stderr)
    );
    assert!(
        !home.join("harness/rtk/pack/index.json").exists(),
        "bypass paths must not mint observations"
    );
}

#[test]
fn recall_clamps_the_requested_window_and_marks_truncation() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let compact = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "3000"],
        &workspace,
        &home,
        None,
    );
    let handle = packed_handle(&String::from_utf8(compact.stdout).unwrap());
    let clamped = invoke(
        &binary,
        &["recall", &handle, "--limit", "5000"],
        &workspace,
        &home,
        None,
    );
    assert_eq!(clamped.status.code(), Some(0));
    assert!(
        clamped.stdout.len() <= 256 * 1024,
        "a recall response stays within the emitted byte bound: {} bytes",
        clamped.stdout.len()
    );
    let text = String::from_utf8(clamped.stdout).unwrap();
    let (first, last, total) = window_bounds(&text);
    assert_eq!((first, total), (1, 3000), "{text}");
    assert!(last <= 2000, "the limit clamps to 2000 lines: {text}");
    assert!(
        last < 2000,
        "the byte clamp, not the line limit, must close this window: {text}"
    );
    assert!(
        text.contains("[rtk pack: window truncated at 262144 bytes]"),
        "{text}"
    );
    let content = numbered_lines(&text);
    assert_eq!(content.len(), last - first + 1, "{text}");
    assert!(text.contains(&format!("--offset {}", last + 1)), "{text}");

    // A window that fits inside the byte bound reports the observation's end.
    let small = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "300"],
        &workspace,
        &home,
        None,
    );
    let small = packed_handle(&String::from_utf8(small.stdout).unwrap());
    let complete = invoke(
        &binary,
        &["recall", &small, "--limit", "2000"],
        &workspace,
        &home,
        None,
    );
    let text = String::from_utf8(complete.stdout).unwrap();
    assert_eq!(window_bounds(&text), (1, 300, 300), "{text}");
    assert!(text.contains("next: end of observation"), "{text}");

    // An offset past the end reports that state instead of fabricating lines.
    let past = invoke(
        &binary,
        &["recall", &small, "--offset", "5000"],
        &workspace,
        &home,
        None,
    );
    assert_eq!(past.status.code(), Some(0));
    let text = String::from_utf8(past.stdout).unwrap();
    assert!(
        text.contains("window: none; offset 5000 is past the last stored line (300)"),
        "{text}"
    );
    assert!(numbered_lines(&text).is_empty(), "{text}");
}

#[test]
fn retention_evicts_the_oldest_observation_and_cleans_orphans() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let pack = home.join("harness/rtk/pack");
    fs::create_dir_all(&pack).unwrap();
    let orphan = pack.join("ob-0000000000000000000-1-000000.log");
    fs::write(&orphan, b"interrupted mint").unwrap();
    let mut handles = Vec::new();
    for _ in 0..66 {
        let compact = invoke(
            &binary,
            &["compact", command.to_str().unwrap(), "log", "-n", "12"],
            &workspace,
            &home,
            None,
        );
        assert!(
            compact.status.success(),
            "{}",
            String::from_utf8_lossy(&compact.stderr)
        );
        handles.push(packed_handle(&String::from_utf8(compact.stdout).unwrap()));
    }
    assert!(!orphan.exists(), "orphan pack files are cleaned up");
    let index: Value = serde_json::from_slice(&fs::read(pack.join("index.json")).unwrap()).unwrap();
    assert_eq!(
        index["entries"].as_array().unwrap().len(),
        64,
        "pack retention keeps 64 records"
    );
    let evicted = invoke(&binary, &["recall", &handles[0]], &workspace, &home, None);
    assert_eq!(evicted.status.code(), Some(2));
    assert!(evicted.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&evicted.stderr).into_owned();
    assert!(
        stderr.contains("unknown observation handle") && stderr.contains("[rtk raw:"),
        "{stderr}"
    );
    let newest = invoke(
        &binary,
        &["recall", handles.last().unwrap()],
        &workspace,
        &home,
        None,
    );
    assert_eq!(
        newest.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&newest.stderr)
    );
    // An index record whose content is gone is reported as unknown, not served.
    fs::remove_file(pack_file(&home, handles.last().unwrap())).unwrap();
    let missing = invoke(
        &binary,
        &["recall", handles.last().unwrap()],
        &workspace,
        &home,
        None,
    );
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
}

#[test]
fn recall_withholds_content_when_the_digest_does_not_match() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let compact = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "12"],
        &workspace,
        &home,
        None,
    );
    let handle = packed_handle(&String::from_utf8(compact.stdout).unwrap());
    let log = pack_file(&home, &handle);
    let mut bytes = fs::read(&log).unwrap();
    bytes[0] = b'X';
    fs::write(&log, &bytes).unwrap();
    let output = invoke(&binary, &["recall", &handle], &workspace, &home, None);
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "no observation content may be returned: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("digest check") && stderr.contains(&handle) && stderr.contains("[rtk raw:"),
        "{stderr}"
    );
}

#[test]
fn recall_reports_usage_and_unknown_handles_with_exit_codes() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    for (args, code, needle) in [
        (vec!["recall"], 1, "handle is required"),
        (vec!["recall", "ob-1-2-3"], 2, "unknown observation handle"),
        (
            vec!["recall", "ob-1-2-3", "--offset", "0"],
            1,
            "positive integer",
        ),
        (
            vec!["recall", "ob-1-2-3", "--limit", "many"],
            1,
            "positive integer",
        ),
        (
            vec!["recall", "ob-1-2-3", "--depth", "1"],
            1,
            "unknown option",
        ),
        (
            vec!["recall", "ob-1-2-3", "ob-4-5-6"],
            1,
            "exactly one observation handle",
        ),
    ] {
        let output = invoke(&binary, &args, &workspace, &home, None);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert_eq!(output.status.code(), Some(code), "{args:?}: {stderr}");
        assert!(output.stdout.is_empty(), "{args:?}: {}", output.status);
        assert!(stderr.contains(needle), "{args:?}: {stderr}");
    }
}

#[test]
fn terminal_stdout_bypasses_compression_and_retention() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let log = command_output(&command, &["log", "-n", "40"], &workspace);
    // A real console on stdout: ConPTY hands the adapter a terminal, so the
    // compact entry must pass the output through without retaining anything.
    let mut spec = CommandSpec::new(&binary);
    spec.args = vec![
        "compact".into(),
        command.clone().into_os_string(),
        "log".into(),
        "-n".into(),
        "40".into(),
    ];
    spec.current_dir = Some(workspace.clone());
    spec.env
        .insert("CODEX_HOME".into(), Some(home.clone().into_os_string()));
    spec.env.insert("HARNESS_RTK_DISABLE".into(), None);
    let session = ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap();
    let outcome = session
        .wait(
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(outcome.outcome.reason, StopReason::Exited);
    assert_eq!(outcome.outcome.exit_code, 0);
    let transcript = outcome.transcript;
    assert!(
        transcript.contains("fixture-log line 1 of 40")
            && transcript.contains("fixture-log line 40 of 40"),
        "{transcript}"
    );
    assert!(
        !transcript.contains("fixture-pipe"),
        "a terminal stdout stays raw: {transcript}"
    );
    assert!(!transcript.contains("[rtk pack:"), "{transcript}");
    assert!(!transcript.contains("[rtk raw:"), "{transcript}");
    assert!(
        !home.join("harness/rtk").exists(),
        "a terminal stdout retains neither raw archives nor observations"
    );
    // Control: the same run with a piped stdout compresses and packs, so the
    // difference is the terminal rather than the fixture setup.
    let piped = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "40"],
        &workspace,
        &home,
        None,
    );
    assert!(
        piped.status.success(),
        "{}",
        String::from_utf8_lossy(&piped.stderr)
    );
    let piped = String::from_utf8(piped.stdout).unwrap();
    assert!(piped.contains("fixture-pipe git-log: 40 lines"), "{piped}");
    assert!(piped.contains("[rtk pack:"), "{piped}");
    let handle = packed_handle(&piped);
    assert_eq!(fs::read(pack_file(&home, &handle)).unwrap(), log);
}

#[test]
fn packed_observation_outlives_its_process_and_the_raw_keep_window() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let expected = command_output(&command, &["log", "-n", "12"], &workspace);
    let first = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "12"],
        &workspace,
        &home,
        None,
    );
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let text = String::from_utf8(first.stdout).unwrap();
    let handle = packed_handle(&text);
    let locator = text
        .lines()
        .find(|line| line.starts_with("[rtk raw: "))
        .expect("the raw locator stays present next to the handle");
    let raw = PathBuf::from(&locator["[rtk raw: ".len()..locator.len() - 1]);
    assert!(raw.is_file());
    let packed = pack_file(&home, &handle);
    assert_eq!(fs::read(&packed).unwrap(), expected);
    // A later session: 33 further compressed runs push this observation out of
    // the 32-file raw keep window while pack retention is untouched.
    for _ in 0..33 {
        let later = invoke(
            &binary,
            &["compact", command.to_str().unwrap(), "log", "-n", "12"],
            &workspace,
            &home,
            None,
        );
        assert!(later.status.success());
    }
    assert!(
        !raw.is_file(),
        "the raw locator of the first footer is pruned by its own keep window"
    );
    assert_eq!(
        fs::read(&packed).unwrap(),
        expected,
        "pack retention is independent of the raw keep window"
    );
    let recalled = invoke(
        &binary,
        &["recall", &handle, "--offset", "3", "--limit", "2"],
        &workspace,
        &home,
        None,
    );
    assert_eq!(
        recalled.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&recalled.stderr)
    );
    let recalled = String::from_utf8(recalled.stdout).unwrap();
    assert!(
        recalled.contains(&format!("[rtk pack: {handle}]")),
        "{recalled}"
    );
    assert!(recalled.contains("source: "), "{recalled}");
    assert!(recalled.contains("stored: 12 lines"), "{recalled}");
    assert!(recalled.contains("window: lines 3-4 of 12"), "{recalled}");
    let expected = String::from_utf8(expected).unwrap();
    for (position, line) in expected.lines().enumerate().skip(2).take(2) {
        assert!(
            recalled.contains(&format!("{}: {line}", position + 1)),
            "{recalled}"
        );
    }
}

/// Paired measurement behind the adoption evidence: the added footer line, the
/// bounded recall windows and the whole-file raw re-read they replace. Run with
/// `--nocapture` to read the numbers recorded in the change notes; only bytes
/// and avoided reruns are claimed, never tokens or quota.
#[test]
fn byte_accounting_for_bounded_recall_and_footer_overhead() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let expected = command_output(&command, &["log", "-n", "3000"], &workspace);
    assert_eq!(
        expected.len(),
        3000 * 161,
        "the fixture observation keeps a byte-stable width"
    );
    let compact = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "3000"],
        &workspace,
        &home,
        None,
    );
    assert!(
        compact.status.success(),
        "{}",
        String::from_utf8_lossy(&compact.stderr)
    );
    let text = String::from_utf8(compact.stdout).unwrap();
    let handle = packed_handle(&text);
    let raw = only_raw_archive(&home);
    let raw_bytes = fs::metadata(&raw).unwrap().len();
    assert_eq!(
        raw_bytes,
        expected.len() as u64,
        "a whole-file raw re-read emits exactly the retained archive"
    );
    assert!(
        text.contains(&format!("[rtk raw: {}]", raw.display())),
        "{text}"
    );
    let overhead = text
        .lines()
        .find(|line| line.starts_with("[rtk pack: "))
        .expect("the compressed footer carries the pack line")
        .len()
        + 1;
    assert!(overhead < 256, "one extra footer line: {overhead} bytes");
    // The source command double is gone before the recall windows, so serving
    // content there means no rerun happened.
    fs::remove_file(&command).unwrap();
    let first = invoke(&binary, &["recall", &handle], &workspace, &home, None);
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_window = first.stdout.len();
    let first_text = String::from_utf8(first.stdout).unwrap();
    assert_eq!(window_bounds(&first_text), (1, 200, 3000), "{first_text}");
    assert_eq!(numbered_lines(&first_text).len(), 200);
    let second = invoke(
        &binary,
        &["recall", &handle, "--offset", "201"],
        &workspace,
        &home,
        None,
    );
    assert_eq!(second.status.code(), Some(0));
    let second_window = second.stdout.len();
    let second_text = String::from_utf8(second.stdout).unwrap();
    assert_eq!(
        window_bounds(&second_text),
        (201, 400, 3000),
        "{second_text}"
    );
    assert!(first_window <= 256 * 1024 && second_window <= 256 * 1024);
    assert!(
        raw_bytes as usize >= 8 * first_window,
        "bounded recall must stay materially smaller than the whole-file re-read: \
         {first_window} bytes of {raw_bytes}"
    );
    println!(
        "rtk pack byte accounting: raw re-read {raw_bytes} B; added footer line {overhead} B; \
         recall windows {first_window} B and {second_window} B for 200 lines each, served with \
         the source command double removed (0 reruns)"
    );
}
