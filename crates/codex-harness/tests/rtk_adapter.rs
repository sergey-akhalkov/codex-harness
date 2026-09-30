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
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
    time::Instant,
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
/// commands are the inert fixture double: no RTK download and no network.
fn staged(root: &Path) -> (PathBuf, PathBuf) {
    let directory = root.join("bin");
    fs::create_dir_all(&directory).unwrap();
    fs::copy(adapter(), directory.join("harness-rtk.exe")).unwrap();
    fs::copy(rtk_fixture(), directory.join("rtk.exe")).unwrap();
    let command = directory.join("git.exe");
    fs::copy(rtk_fixture(), &command).unwrap();
    fs::copy(rtk_fixture(), directory.join("cargo.exe")).unwrap();
    (directory.join("harness-rtk.exe"), command)
}

fn cargo_double(binary: &Path) -> PathBuf {
    binary.with_file_name("cargo.exe")
}

/// Invoke the adapter with a hard wall-clock bound, so a pipe deadlock fails
/// the test instead of hanging the suite.
fn invoke_bounded(
    adapter: &Path,
    args: &[&str],
    cwd: &Path,
    home: &Path,
    extra: &[(&str, &str)],
    timeout: Duration,
) -> std::process::Output {
    let mut command = Command::new(adapter);
    command.args(args).current_dir(cwd).env("CODEX_HOME", home);
    command.env_remove("HARNESS_RTK_DISABLE");
    command.env("PROCESS_CASE_ROOT", cwd);
    for (key, value) in extra {
        command.env(*key, *value);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes).unwrap()
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes).unwrap()
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("adapter invocation exceeded {timeout:?}: {args:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    std::process::Output {
        status,
        stdout: out.join().unwrap(),
        stderr: err.join().unwrap(),
    }
}

/// The bounded local decision records as the adapter's own reader reports them.
fn diagnostics(binary: &Path, workspace: &Path, home: &Path) -> Vec<Value> {
    let output = invoke(
        binary,
        &["diagnostics", "--json", "--last", "32"],
        workspace,
        home,
        None,
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["records"].as_array().cloned().unwrap_or_default()
}

fn record_for<'a>(records: &'a [Value], command: &str) -> &'a Value {
    records
        .iter()
        .rev()
        .find(|record| record["command"] == command)
        .unwrap_or_else(|| panic!("no decision record for `{command}`: {records:#?}"))
}

fn fixture_ledger(root: &Path) -> PathBuf {
    root.join("ledger.txt")
}

fn ledger_runs(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

/// A compiler-shaped stream well above the retention floor: two Cargo status
/// lines plus warning blocks whose text must survive verbatim.
fn warning_stream() -> String {
    let mut text = String::from("    Checking synthetic-demo v0.1.0 (/fixture/demo)\n");
    text.push_str("warning: unused variable: `unused_factor`\n");
    text.push_str(" --> src/lib.rs:7:9\n");
    text.push_str("  |\n");
    text.push_str("7 |     let unused_factor = 3;\n");
    text.push_str("  |         ^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_unused_factor`\n");
    text.push_str("  |\n");
    text.push_str(
        "  = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default\n",
    );
    text.push_str("  = note: this synthetic block is long enough that retention repays its cost\n");
    text.push_str("warning: unused import: `std::fmt`\n");
    text.push_str(" --> src/lib.rs:1:5\n");
    text.push_str("  |\n");
    text.push_str("1 | use std::fmt;\n");
    text.push_str("  |     ^^^^^^^^^\n");
    text.push_str("  |\n");
    text.push_str(
        "  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default\n",
    );
    text.push_str("warning: `synthetic-demo` (lib) generated 2 warnings (run `cargo fix --lib -p synthetic-demo` to apply 2 suggestions)\n");
    text.push_str("    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s\n");
    text
}

/// A failing stream: one Cargo status line plus a rustc error block that must
/// survive verbatim with no test summary anywhere.
fn compile_failure_stream() -> String {
    let mut text = String::from("    Checking synthetic-broken v0.1.0 (/fixture/broken)\n");
    text.push_str("error[E0308]: mismatched types\n");
    text.push_str(" --> broken/src/lib.rs:2:5\n");
    text.push_str("  |\n");
    text.push_str("1 | pub fn broken() -> u32 {\n");
    text.push_str("  |                    --- expected `u32` because of return type\n");
    text.push_str("2 |     \"not a number\"\n");
    text.push_str("  |     ^^^^^^^^^^^^^^ expected `u32`, found `&str`\n");
    text.push_str("  |\n");
    text.push_str(
        "  = note: the synthetic failure block is long enough that retaining it repays its cost\n",
    );
    text.push_str(
        "  = note: and it must survive the presentation byte-verbatim on the failing path\n",
    );
    text.push_str("For more information about this error, try `rustc --explain E0308`.\n");
    text.push_str("error: could not compile `synthetic-broken` (lib) due to 1 previous error\n");
    text
}

/// A block the adapter must not recognize: it is kept byte-verbatim.
fn unknown_block() -> String {
    let mut text = String::from("== synthetic tool block ==\n");
    for index in 1..=8 {
        text.push_str(&format!(
            "unrecognized diagnostic detail line {index} that no cargo status word starts\n"
        ));
    }
    text.push_str("== end synthetic tool block ==\n");
    text
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

const BARRIER_ENV: &str = "HARNESS_RTK_TEST_BARRIER_DIR";

/// Holds one test barrier release latch. The adapter announces its arrival on
/// stderr and then blocks on this file's exclusive lock; dropping the guard (or
/// the test process exiting) releases the child, so a lost test cannot wedge it.
struct Barrier {
    _release: fs::File,
}

impl Barrier {
    fn arm(directory: &Path, point: &str) -> Barrier {
        let release = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(format!("{point}.release")))
            .unwrap();
        release
            .try_lock()
            .expect("the test owns the barrier release latch");
        Barrier { _release: release }
    }

    fn release(self) {}
}

/// Kills a child that never reaches its barrier, so a broken regression fails
/// the suite instead of hanging it.
struct Watchdog {
    done: Arc<AtomicBool>,
    killer: Option<std::thread::JoinHandle<()>>,
}

impl Watchdog {
    fn arm(pid: u32) -> Watchdog {
        let done = Arc::new(AtomicBool::new(false));
        let killer = std::thread::spawn({
            let done = Arc::clone(&done);
            move || {
                for _ in 0..240 {
                    if done.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        });
        Watchdog {
            done,
            killer: Some(killer),
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        if let Some(killer) = self.killer.take() {
            let _ = killer.join();
        }
    }
}

/// A barrier-driven adapter invocation: stdout is drained continuously while
/// the test blocks on stderr for the announced barrier point.
struct BarrierChild {
    child: std::process::Child,
    stderr: std::io::BufReader<std::process::ChildStderr>,
    stdout: std::thread::JoinHandle<Vec<u8>>,
    _watchdog: Watchdog,
}

impl BarrierChild {
    fn spawn(
        adapter: &Path,
        args: &[&str],
        cwd: &Path,
        home: &Path,
        extra: &[(&str, &str)],
    ) -> BarrierChild {
        let mut command = Command::new(adapter);
        command.args(args).current_dir(cwd).env("CODEX_HOME", home);
        command.env_remove("HARNESS_RTK_DISABLE");
        command.env("PROCESS_CASE_ROOT", cwd);
        for (key, value) in extra {
            command.env(*key, *value);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let watchdog = Watchdog::arm(child.id());
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut stdout = stdout;
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).map(|_| bytes).unwrap()
        });
        BarrierChild {
            child,
            stderr: std::io::BufReader::new(stderr),
            stdout: reader,
            _watchdog: watchdog,
        }
    }

    /// Event wait, not a sleep: the adapter announces the point before it
    /// blocks, so a blocking line read observes it deterministically.
    fn wait_for_barrier(&mut self, point: &str) {
        let needle = format!("rtk: test barrier {point} reached");
        let mut line = String::new();
        loop {
            line.clear();
            let read = self.stderr.read_line(&mut line).unwrap();
            assert!(read > 0, "the child exited before barrier {point}");
            if line.contains(&needle) {
                return;
            }
        }
    }

    fn wait(mut self) -> (std::process::ExitStatus, Vec<u8>, String) {
        let status = self.child.wait().unwrap();
        let stdout = self.stdout.join().unwrap();
        let mut stderr = self.stderr;
        let mut rest = String::new();
        stderr.read_to_string(&mut rest).unwrap();
        (status, stdout, rest)
    }

    fn kill(mut self) -> (std::process::ExitStatus, Vec<u8>) {
        let _ = self.child.kill();
        let status = self.child.wait().unwrap();
        let stdout = self.stdout.join().unwrap();
        (status, stdout)
    }
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
fn compact_tolerates_the_pinned_rtk_startup_banner_but_not_other_stderr() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());

    // The real pinned `rtk.exe` announces on stderr that its own global hook
    // is absent; the kit configuration never installs that hook, so a banner
    // like this must not disable compression or handle minting.
    let compact = invoke_with_env(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "300"],
        &workspace,
        &home,
        None,
        &[("HARNESS_RTK_FIXTURE_BANNER", "1")],
    );
    assert!(
        compact.status.success(),
        "{}",
        String::from_utf8_lossy(&compact.stderr)
    );
    let text = String::from_utf8(compact.stdout.clone()).unwrap();
    assert!(text.starts_with("fixture-pipe git-log:"), "{text}");
    let handle = packed_handle(&text);
    assert!(pack_file(&home, &handle).is_file());

    // Any other stderr content stays a filter diagnostic: the run keeps the
    // raw locator and mints no handle.
    let diagnostics = invoke_with_env(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "300"],
        &workspace,
        &home,
        None,
        &[("HARNESS_RTK_FIXTURE_STDERR", "filter diagnostics")],
    );
    assert!(
        diagnostics.status.success(),
        "{}",
        String::from_utf8_lossy(&diagnostics.stderr)
    );
    let raw = String::from_utf8(diagnostics.stdout.clone()).unwrap();
    let expected = command_output(&command, &["log", "-n", "300"], &workspace);
    assert_eq!(raw.as_bytes(), expected.as_slice());
    assert!(!raw.contains("[rtk pack:"), "{raw}");
}

#[test]
fn a_filter_output_that_does_not_shrink_stays_raw_without_packing() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let expected = command_output(&command, &["log", "-n", "300"], &workspace);

    let output = invoke_with_env(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "300"],
        &workspace,
        &home,
        None,
        &[("HARNESS_RTK_FIXTURE_INFLATE", "1")],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.contains("[rtk pack:"), "{text}");
    // No observation was packed and no orphaned handle reached the index.
    let pack_root = home.join("harness/rtk/pack");
    assert!(!pack_root.join("index.json").is_file());
    let orphaned = pack_root
        .read_dir()
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0);
    assert_eq!(orphaned, 0);
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
        !home.join("harness/rtk/raw").exists() && !home.join("harness/rtk/pack").exists(),
        "a terminal stdout retains neither raw archives nor observations"
    );
    // The bypass itself stays inspectable: the record names why nothing was
    // compressed, which is what a hook rewrite can never establish on its own.
    let records = diagnostics(&binary, &workspace, &home);
    assert_eq!(
        records.len(),
        1,
        "the interactive invocation is recorded: {records:#?}"
    );
    assert_eq!(records[0]["reason"], "interactive");
    assert_eq!(records[0]["presentation_bytes"], Value::Null);
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

/// Task 1.1: every documented verification form reaches compression, and the
/// child still runs exactly once per invocation.
#[test]
fn cargo_corpus_forms_enter_the_documented_compression_paths() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    // Repeated eight times: the complete accounted presentation (both bodies,
    // both locators, both handles) must genuinely shrink before a run is
    // labeled applied.
    let stdout = warning_stream().repeat(8);
    let stderr = warning_stream().repeat(8);
    let ledger = fixture_ledger(root.path());
    let forms: [(&str, &[&str]); 6] = [
        (
            "cargo-test",
            &[
                "test",
                "-p",
                "synthetic-demo",
                "--locked",
                "--jobs",
                "1",
                "--",
                "--test-threads=1",
            ],
        ),
        (
            "cargo-test",
            &[
                "test",
                "--workspace",
                "--locked",
                "--jobs",
                "1",
                "--",
                "--test-threads=1",
                "sign_check",
            ],
        ),
        (
            "cargo-check",
            &["check", "--workspace", "--locked", "--jobs", "1"],
        ),
        (
            "cargo-build",
            &[
                "build",
                "-p",
                "synthetic-demo",
                "--release",
                "-j",
                "2",
                "--locked",
            ],
        ),
        (
            "cargo-clippy",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--locked",
                "--jobs",
                "1",
                "--",
                "-D",
                "warnings",
            ],
        ),
        (
            "cargo-check",
            &["check", "-j4", "--target-dir", "target/owned", "--locked"],
        ),
    ];
    for (index, (selection, args)) in forms.iter().enumerate() {
        let mut full: Vec<&str> = vec!["compact", cargo.to_str().unwrap()];
        full.extend_from_slice(args);
        let output = invoke_bounded(
            &binary,
            &full,
            &workspace,
            &home,
            &[
                ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", stdout.as_str()),
                ("HARNESS_RTK_FIXTURE_CARGO_STDERR", stderr.as_str()),
                ("HARNESS_RTK_FIXTURE_LEDGER", ledger.to_str().unwrap()),
            ],
            Duration::from_secs(30),
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let out = String::from_utf8_lossy(&output.stdout).into_owned();
        let err = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            out.contains("[rtk raw: ") && out.contains("[rtk pack: "),
            "{args:?} must compress stdout through its recorded path: {out}"
        );
        assert!(
            out.contains("[rtk raw[stderr]: ") && out.contains("[rtk pack[stderr]: "),
            "{args:?} must compress stderr and name that stream: {out}"
        );
        assert!(
            err.contains("warning: unused variable: `unused_factor`")
                && !err.contains("    Checking synthetic-demo"),
            "{args:?}: recognized progress lines leave, diagnostics stay verbatim: {err}"
        );
        assert_eq!(
            ledger_runs(&ledger),
            index + 1,
            "{args:?}: the native command must run exactly once"
        );
        let records = diagnostics(&binary, &workspace, &home);
        let command = format!("{} {}", cargo.display(), args.join(" "));
        let record = record_for(&records, &command);
        assert_eq!(record["selection"], *selection, "{args:?}: {record}");
        assert_eq!(record["decision"], "applied", "{args:?}: {record}");
        let raw_total = record["streams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|stream| stream["raw_bytes"].as_u64().unwrap())
            .sum::<u64>();
        assert!(
            record["presentation_bytes"].as_u64().unwrap() < raw_total,
            "{args:?}: an applied run must deliver fewer bytes than its raw originals: {record}"
        );
    }
}

/// Task 1.1: unsupported commands, flags and machine formats are bypassed with
/// a recorded reason instead of a guessed summary.
#[test]
fn cargo_machine_formats_and_unsupported_forms_stay_raw() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    let stdout = warning_stream();
    let stderr = warning_stream();
    let ledger = fixture_ledger(root.path());
    // The inert double has no role for `fmt` or `run`, so those two forms exit
    // with the double's own code and no output; the adapter must pass both the
    // code and the empty streams through unchanged.
    let forms: [(&[&str], &str, i32, bool); 10] = [
        (
            &["test", "--message-format=json", "--locked"],
            "machine-format",
            0,
            true,
        ),
        (&["check", "--json"], "machine-format", 0, true),
        (&["fmt", "--all"], "unsupported-command", 2, false),
        (&["run", "--release"], "unsupported-command", 2, false),
        (&["test", "-v"], "unsupported-flags", 0, true),
        (
            &["check", "an-extra-positional"],
            "unsupported-flags",
            0,
            true,
        ),
        (&["test", "-p"], "unsupported-flags", 0, true),
        (
            &["build", "--", "-Zunstable-options"],
            "unsupported-flags",
            0,
            true,
        ),
        (&["clippy", "--", "--fix"], "unsupported-flags", 0, true),
        (&["test", "--", "--nocapture"], "unsupported-flags", 0, true),
    ];
    for (index, (args, reason, code, passthrough)) in forms.iter().enumerate() {
        let mut full: Vec<&str> = vec!["compact", cargo.to_str().unwrap()];
        full.extend_from_slice(args);
        let output = invoke_bounded(
            &binary,
            &full,
            &workspace,
            &home,
            &[
                ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", stdout.as_str()),
                ("HARNESS_RTK_FIXTURE_CARGO_STDERR", stderr.as_str()),
                ("HARNESS_RTK_FIXTURE_LEDGER", ledger.to_str().unwrap()),
            ],
            Duration::from_secs(30),
        );
        assert_eq!(
            output.status.code(),
            Some(*code),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected = if *passthrough {
            stdout.clone()
        } else {
            String::new()
        };
        assert_eq!(
            String::from_utf8(output.stdout.clone()).unwrap(),
            expected,
            "{args:?}: a bypassed stream stays byte-raw"
        );
        let expected = if *passthrough {
            stderr.clone()
        } else {
            // The inert double reports its own missing role on stderr; that is
            // the child's output and the adapter must pass it through as-is.
            format!("rtk fixture: unsupported fixture role {}\n", args[0])
        };
        assert_eq!(
            String::from_utf8(output.stderr.clone()).unwrap(),
            expected,
            "{args:?}: a bypassed stream carries no footer or telemetry"
        );
        assert_eq!(
            ledger_runs(&ledger),
            index + 1,
            "{args:?}: the native command must still run exactly once"
        );
        let records = diagnostics(&binary, &workspace, &home);
        let command = format!("{} {}", cargo.display(), args.join(" "));
        let record = record_for(&records, &command);
        assert_eq!(record["decision"], "bypassed", "{args:?}: {record}");
        assert_eq!(record["reason"], *reason, "{args:?}: {record}");
    }
}

/// Task 1.2: a compile failure that appears only on stderr keeps its text, its
/// stream identity and its nonzero child result, and stays recallable.
#[test]
fn cargo_stderr_only_compile_failure_keeps_stream_and_exit() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    // Repeated eight times so the complete presentation (body, locator, handle
    // and the failure notice) genuinely shrinks before the stream is applied.
    let failure = compile_failure_stream().repeat(8);
    let ledger = fixture_ledger(root.path());
    let output = invoke_bounded(
        &binary,
        &[
            "compact",
            cargo.to_str().unwrap(),
            "check",
            "-p",
            "synthetic-broken",
            "--locked",
        ],
        &workspace,
        &home,
        &[
            // No test summary and no stdout at all: a compile error on stderr.
            ("HARNESS_RTK_FIXTURE_CARGO_STDERR", failure.as_str()),
            ("HARNESS_RTK_FIXTURE_CARGO_EXIT", "101"),
            ("HARNESS_RTK_FIXTURE_LEDGER", ledger.to_str().unwrap()),
        ],
        Duration::from_secs(30),
    );
    assert_eq!(
        output.status.code(),
        Some(101),
        "the child's own exit status is preserved: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(err.contains("error[E0308]: mismatched types"), "{err}");
    assert!(
        err.contains("error: could not compile `synthetic-broken` (lib) due to 1 previous error"),
        "{err}"
    );
    assert!(
        !err.contains("    Checking synthetic-broken"),
        "recognized progress lines are elided: {err}"
    );
    assert!(
        !err.contains("test result"),
        "no test counts may be invented: {err}"
    );
    assert!(
        out.contains("[rtk raw[stderr]: ") && out.contains("[rtk pack[stderr]: "),
        "the stderr archive and handle are named: {out}"
    );
    assert!(
        !out.contains("[rtk raw: "),
        "an empty stdout mints no stdout locator: {out}"
    );
    assert!(
        out.contains("[rtk cargo exit: 101]"),
        "a compacted failure never looks successful: {out}"
    );
    let record = {
        let records = diagnostics(&binary, &workspace, &home);
        record_for(
            &records,
            &format!("{} check -p synthetic-broken --locked", cargo.display()),
        )
        .clone()
    };
    assert_eq!(record["decision"], "applied", "{record}");
    assert_eq!(record["exit_code"], 101, "{record}");
    assert_eq!(record["streams"][0]["stream"], "stdout", "{record}");
    assert_eq!(record["streams"][0]["reason"], "empty-output", "{record}");
    assert_eq!(record["streams"][1]["stream"], "stderr", "{record}");
    assert_eq!(record["streams"][1]["decision"], "applied", "{record}");

    // Detail comes back with its stream identity while the command is gone.
    let handle = out
        .lines()
        .find(|line| line.starts_with("[rtk pack[stderr]: "))
        .map(|line| {
            line["[rtk pack[stderr]: ".len()..]
                .split(' ')
                .next()
                .unwrap()
                .to_owned()
        })
        .expect("the stderr footer carries its handle");
    fs::remove_file(&cargo).unwrap();
    let recalled = invoke(&binary, &["recall", &handle], &workspace, &home, None);
    assert_eq!(
        recalled.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&recalled.stderr)
    );
    let recalled = String::from_utf8(recalled.stdout).unwrap();
    assert!(recalled.contains("digest: sha256 "), "{recalled}");
    assert!(recalled.contains("[stderr]"), "{recalled}");
    for line in failure.lines().take(6) {
        assert!(recalled.contains(line), "{recalled}");
    }
    assert_eq!(ledger_runs(&ledger), 1, "recall never reruns the command");
}

/// Task 1.2: both pipes are drained concurrently, so a command that fills its
/// stderr pipe before writing stdout neither deadlocks nor loses a stream.
#[test]
fn cargo_large_simultaneous_streams_are_drained() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    let ledger = fixture_ledger(root.path());
    let output = invoke_bounded(
        &binary,
        &[
            "compact",
            cargo.to_str().unwrap(),
            "test",
            "--locked",
            "--jobs",
            "1",
        ],
        &workspace,
        &home,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT_BYTES", "200000"),
            ("HARNESS_RTK_FIXTURE_CARGO_STDERR_BYTES", "300000"),
            ("HARNESS_RTK_FIXTURE_LEDGER", ledger.to_str().unwrap()),
        ],
        Duration::from_secs(60),
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        (299_900..300_100).contains(&output.stderr.len()),
        "the whole stderr stream arrives: {} bytes",
        output.stderr.len()
    );
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(out.contains("[rtk pack: "), "{out}");
    assert_eq!(ledger_runs(&ledger), 1);
    let records = diagnostics(&binary, &workspace, &home);
    let record = record_for(
        &records,
        &format!("{} test --locked --jobs 1", cargo.display()),
    );
    assert_eq!(record["streams"][0]["decision"], "applied", "{record}");
    assert!(
        record["streams"][0]["raw_bytes"].as_u64().unwrap() >= 200_000,
        "{record}"
    );
    assert_eq!(record["streams"][1]["decision"], "bypassed", "{record}");
    assert_eq!(record["streams"][1]["reason"], "non-shrinking", "{record}");
    let stderr_bytes = record["streams"][1]["raw_bytes"].as_u64().unwrap();
    assert!(
        (300_000..300_100).contains(&stderr_bytes),
        "both streams are measured where they were captured: {record}"
    );
}

/// Task 1.2: exceeding the capture bound keeps the raw output usable and live,
/// names the limit and retains nothing it cannot deliver.
#[test]
fn cargo_oversized_stream_passes_raw_with_an_explicit_limit() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    let output = invoke_bounded(
        &binary,
        &[
            "compact",
            cargo.to_str().unwrap(),
            "build",
            "--locked",
            "--jobs",
            "1",
        ],
        &workspace,
        &home,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT_BYTES", "4200000"),
            ("HARNESS_RTK_FIXTURE_CARGO_STDERR_BYTES", "120000"),
        ],
        Duration::from_secs(120),
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.len() >= 4_200_000,
        "oversized stdout still arrives whole: {} bytes",
        output.stdout.len()
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("[rtk raw"),
        "an oversized stream mints no locator"
    );
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        err.contains("stdout exceeds 4 MiB; raw passthrough without retention"),
        "{err}"
    );
    assert!(
        err.len() >= 120_000,
        "stderr still arrives: {} bytes",
        err.len()
    );
    assert!(
        !home.join("harness/rtk/pack/index.json").exists(),
        "an oversized run packs nothing"
    );
    let records = diagnostics(&binary, &workspace, &home);
    let record = record_for(
        &records,
        &format!("{} build --locked --jobs 1", cargo.display()),
    );
    assert_eq!(record["decision"], "bypassed", "{record}");
    assert_eq!(record["reason"], "oversize", "{record}");
    assert_eq!(record["streams"][0]["raw_bytes"], Value::Null, "{record}");
    assert_eq!(record["presentation_bytes"], Value::Null, "{record}");
}

/// Task 1.2/1.4: blocks the adapter does not recognize stay byte-verbatim, a
/// short stream stays raw, and a stream whose complete presentation cannot pay
/// for its locator and handle falls back to the raw bytes as well.
#[test]
fn cargo_unknown_blocks_and_short_streams_stay_raw() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    // Enough recognized progress lines to pay for the footer, then a block the
    // adapter must not touch.
    let mut unknown = String::new();
    for _ in 0..32 {
        unknown.push_str("   Compiling synthetic-demo v0.1.0\n");
    }
    unknown.push_str(&unknown_block());
    let output = invoke_bounded(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", unknown.as_str()),
            ("HARNESS_RTK_FIXTURE_CARGO_STDERR", "   Compiling demo\n"),
        ],
        Duration::from_secs(30),
    );
    assert_eq!(output.status.code(), Some(0));
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        out.contains(&unknown_block()),
        "an unrecognized block is kept verbatim: {out}"
    );
    assert!(
        !out.contains("Compiling"),
        "recognized progress lines are elided: {out}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "   Compiling demo\n",
        "a short stream stays raw"
    );
    let short = "   Compiling demo\n".len() as u64;
    let records = diagnostics(&binary, &workspace, &home);
    let record = record_for(&records, &format!("{} check --locked", cargo.display()));
    assert_eq!(record["streams"][0]["decision"], "applied", "{record}");
    assert_eq!(record["streams"][1]["reason"], "short-output", "{record}");
    assert_eq!(record["streams"][1]["raw_bytes"], short, "{record}");
    assert_eq!(record["streams"][1]["presented_bytes"], short, "{record}");

    // One recognized line cannot pay for the locator and handle, so the
    // complete presentation does not shrink: the raw bytes stay, including the
    // status-like text, and the fallback publishes nothing new.
    let almost = format!("   Compiling synthetic-demo v0.1.0\n{}", unknown_block());
    let output = invoke_bounded(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", almost.as_str()),
            ("HARNESS_RTK_FIXTURE_CARGO_STDERR", "   Compiling demo\n"),
        ],
        Duration::from_secs(30),
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        almost,
        "a non-shrinking complete presentation keeps the raw bytes"
    );
    let records = diagnostics(&binary, &workspace, &home);
    let record = record_for(&records, &format!("{} check --locked", cargo.display()));
    assert_eq!(record["decision"], "bypassed", "{record}");
    assert_eq!(record["streams"][0]["reason"], "non-shrinking", "{record}");
    assert_eq!(record["streams"][0]["handle"], Value::Null, "{record}");
    let index: Value =
        serde_json::from_slice(&fs::read(home.join("harness/rtk/pack/index.json")).unwrap())
            .unwrap();
    assert!(
        index["entries"].as_array().unwrap().len() == 1,
        "the raw fallback publishes nothing new: {index}"
    );
}

/// Task 1.2: a failing or timing-out formatter keeps the original output
/// usable, names the presentation problem and never reruns the command.
#[test]
fn cargo_filter_failure_and_timeout_keep_raw_output() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    let stdout = warning_stream();
    let ledger = fixture_ledger(root.path());
    let cases: [((&str, &str), &str, &str); 2] = [
        (
            ("HARNESS_RTK_FIXTURE_STDERR", "filter diagnostics"),
            "filter-failed",
            "filter failed or returned diagnostics; raw passthrough",
        ),
        (
            ("HARNESS_RTK_FIXTURE_PIPE_DELAY_MS", "2600"),
            "filter-timeout",
            "filter timed out; raw passthrough",
        ),
    ];
    for (extra, reason, needle) in cases {
        let output = invoke_bounded(
            &binary,
            &["compact", cargo.to_str().unwrap(), "test", "--locked"],
            &workspace,
            &home,
            &[
                ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", stdout.as_str()),
                ("HARNESS_RTK_FIXTURE_LEDGER", ledger.to_str().unwrap()),
                extra,
            ],
            Duration::from_secs(60),
        );
        assert_eq!(
            output.status.code(),
            Some(0),
            "{reason}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout.clone()).unwrap(),
            stdout,
            "{reason}: the original output stays usable"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(needle),
            "{reason}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let records = diagnostics(&binary, &workspace, &home);
        let record = record_for(&records, &format!("{} test --locked", cargo.display()));
        assert_eq!(record["streams"][0]["reason"], reason, "{record}");
    }
    assert_eq!(
        ledger_runs(&ledger),
        2,
        "each invocation runs the native command exactly once"
    );
}

/// Task 1.2: unavailable retention storage keeps the run raw and usable.
#[test]
fn cargo_unavailable_retention_keeps_raw_output() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    let stdout = warning_stream();
    let stderr = warning_stream();
    // A file where the raw capture directory belongs makes every save_raw fail.
    fs::create_dir_all(home.join("harness/rtk")).unwrap();
    fs::write(home.join("harness/rtk/raw"), b"not a directory").unwrap();
    let output = invoke_bounded(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", stdout.as_str()),
            ("HARNESS_RTK_FIXTURE_CARGO_STDERR", stderr.as_str()),
        ],
        Duration::from_secs(30),
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8(output.stdout.clone()).unwrap(), stdout);
    let err = String::from_utf8(output.stderr.clone()).unwrap();
    assert!(
        err.ends_with(&stderr),
        "the raw stream still arrives after its diagnostic: {err}"
    );
    assert!(
        err.matches("raw capture unavailable; raw passthrough")
            .count()
            == 2,
        "each stream reports the retention problem: {err}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("[rtk raw"),
        "no locator may name storage that does not exist"
    );
    let records = diagnostics(&binary, &workspace, &home);
    let record = record_for(&records, &format!("{} check --locked", cargo.display()));
    assert_eq!(
        record["streams"][0]["reason"], "retention-unavailable",
        "{record}"
    );
    assert_eq!(
        record["streams"][1]["reason"], "retention-unavailable",
        "{record}"
    );
    assert_eq!(record["decision"], "bypassed", "{record}");
}

/// Task 1.2: a long run keeps bounded progress visibility while its output is
/// held back for presentation.
#[test]
fn cargo_long_run_reports_bounded_progress() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    // Large enough that the compact presentation still shrinks once the
    // emitted progress notices are part of the accounted bytes.
    let stdout = warning_stream().repeat(8);
    let output = invoke_bounded(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", stdout.as_str()),
            ("HARNESS_RTK_FIXTURE_CARGO_DELAY_MS", "1200"),
            ("HARNESS_RTK_PROGRESS_SECONDS", "1"),
        ],
        Duration::from_secs(30),
    );
    assert_eq!(output.status.code(), Some(0));
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        err.contains("compact capture still running") && err.contains("bytes captured so far"),
        "{err}"
    );
    assert!(
        err.matches("progress notice").count() <= 20,
        "the notice count stays bounded: {err}"
    );
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(out.contains("[rtk pack: "), "the run still compacts: {out}");
    let records = diagnostics(&binary, &workspace, &home);
    let record = record_for(&records, &format!("{} check --locked", cargo.display()));
    assert_eq!(record["decision"], "applied", "{record}");
    assert!(
        record["adapter_bytes"].as_u64().unwrap() > 0,
        "the progress notices are measured into the ledger: {record}"
    );
    let raw_total = record["streams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stream| stream["raw_bytes"].as_u64().unwrap_or(0))
        .sum::<u64>();
    assert!(
        record["presentation_bytes"].as_u64().unwrap() < raw_total,
        "applied bytes shrink on the same basis diagnostics report: {record}"
    );
    // Zero disables the notice instead of printing one per tick.
    let quiet = invoke_bounded(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", stdout.as_str()),
            ("HARNESS_RTK_FIXTURE_CARGO_DELAY_MS", "1200"),
            ("HARNESS_RTK_PROGRESS_SECONDS", "0"),
        ],
        Duration::from_secs(30),
    );
    assert!(
        !String::from_utf8_lossy(&quiet.stderr).contains("compact capture still running"),
        "HARNESS_RTK_PROGRESS_SECONDS=0 disables the notice"
    );
}

/// Task 1.3: the decision, the concrete reason, the measured stream bytes and
/// the complete presentation size are recorded, while tokens stay unavailable.
#[test]
fn compression_decisions_and_byte_evidence_are_recorded() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    // Repeated four times: the complete presentation must genuinely shrink.
    let stdout = warning_stream().repeat(4);
    let plain = format!(
        "{}fixture stderr without a cargo status word\n",
        "x".repeat(600)
    );
    let applied = invoke_with_env(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        None,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", stdout.as_str()),
            ("HARNESS_RTK_FIXTURE_CARGO_STDERR", plain.as_str()),
        ],
    );
    assert_eq!(applied.status.code(), Some(0));
    let records = diagnostics(&binary, &workspace, &home);
    let record = record_for(&records, &format!("{} check --locked", cargo.display())).clone();
    assert_eq!(record["decision"], "applied", "{record}");
    assert_eq!(record["streams"][0]["decision"], "applied", "{record}");
    assert_eq!(record["streams"][1]["decision"], "bypassed", "{record}");
    assert_eq!(record["streams"][1]["reason"], "non-shrinking", "{record}");
    assert_eq!(record["streams"][1]["raw_bytes"], plain.len(), "{record}");
    assert_eq!(
        record["streams"][1]["presented_bytes"],
        plain.len(),
        "{record}"
    );
    assert_eq!(
        record["presentation_bytes"].as_u64().unwrap(),
        (applied.stdout.len() + applied.stderr.len()) as u64,
        "the record accounts for every emitted byte: {record}"
    );
    assert_eq!(
        record["presentation_bytes"].as_u64().unwrap(),
        record["streams"][0]["presented_bytes"].as_u64().unwrap()
            + record["streams"][1]["presented_bytes"].as_u64().unwrap()
            + record["footer_bytes"].as_u64().unwrap()
            + record["adapter_bytes"].as_u64().unwrap(),
        "{record}"
    );
    let raw_total = record["streams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stream| stream["raw_bytes"].as_u64().unwrap())
        .sum::<u64>();
    assert!(
        record["presentation_bytes"].as_u64().unwrap() < raw_total,
        "an applied run shrinks on the same basis diagnostics report: {record}"
    );
    assert_eq!(
        record["adapter_bytes"], 0,
        "no adapter notice was emitted in this run: {record}"
    );
    assert_eq!(record["tokens"], Value::Null, "{record}");
    assert_eq!(record["token_measurement"], "unavailable", "{record}");
    assert!(
        record["streams"][1]["raw_bytes"].as_u64().unwrap() > 0,
        "raw bytes are measured where the stream was captured: {record}"
    );

    // The inspected-invocation line is opt-in and never touches the payload.
    let announced = invoke_with_env(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        None,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", stdout.as_str()),
            ("HARNESS_RTK_FIXTURE_CARGO_STDERR", plain.as_str()),
            ("HARNESS_RTK_DIAGNOSTIC", "1"),
        ],
    );
    let announced_err = String::from_utf8_lossy(&announced.stderr).into_owned();
    assert!(
        announced_err.contains("B -> ") && announced_err.contains("tokens unavailable"),
        "{announced_err}"
    );
    assert!(
        !announced_err.contains("[rtk raw") && !announced_err.contains("[rtk pack"),
        "the notice stays out of the payload channels: {announced_err}"
    );

    // A bypassed invocation carries the reason and no invented measurement.
    let bypassed = invoke(
        &binary,
        &[
            "compact",
            cargo.to_str().unwrap(),
            "test",
            "--message-format=json",
        ],
        &workspace,
        &home,
        None,
    );
    assert_eq!(bypassed.status.code(), Some(0));
    assert!(
        !String::from_utf8_lossy(&bypassed.stdout).contains("[rtk"),
        "a machine-format bypass receives no footer"
    );
    let records = diagnostics(&binary, &workspace, &home);
    let record = record_for(
        &records,
        &format!("{} test --message-format=json", cargo.display()),
    );
    assert_eq!(record["reason"], "machine-format", "{record}");
    assert_eq!(record["presentation_bytes"], Value::Null, "{record}");
    assert_eq!(record["streams"].as_array().unwrap().len(), 0, "{record}");

    // The human rendering names the byte evidence and the unavailable tokens.
    let rendered = invoke(&binary, &["diagnostics"], &workspace, &home, None);
    let rendered = String::from_utf8(rendered.stdout).unwrap();
    assert!(rendered.contains("[rtk diagnostics: "), "{rendered}");
    assert!(rendered.contains("tokens unavailable"), "{rendered}");
    assert!(
        rendered.contains("presentation unavailable"),
        "an uncaptured invocation reports no measurement instead of zero: {rendered}"
    );
}

/// Task 1.2/1.3: both retained streams are recallable with their identity
/// after the command is gone.
#[test]
fn recall_serves_both_cargo_streams_without_rerunning() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    // Repeated eight times: both complete stream presentations must shrink
    // before each is labeled applied and packed.
    let stdout = warning_stream().repeat(8);
    let stderr = compile_failure_stream().repeat(8);
    let ledger = fixture_ledger(root.path());
    let output = invoke_with_env(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        None,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", stdout.as_str()),
            ("HARNESS_RTK_FIXTURE_CARGO_STDERR", stderr.as_str()),
            ("HARNESS_RTK_FIXTURE_LEDGER", ledger.to_str().unwrap()),
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out = String::from_utf8(output.stdout).unwrap();
    let handles: Vec<String> = out
        .lines()
        .filter(|line| line.starts_with("[rtk pack"))
        .map(|line| {
            let rest = line.split_once(": ").unwrap().1;
            rest.split(' ').next().unwrap().to_owned()
        })
        .collect();
    assert_eq!(handles.len(), 2, "one handle per retained stream: {out}");
    fs::remove_file(&cargo).unwrap();
    let first = invoke(&binary, &["recall", &handles[0]], &workspace, &home, None);
    assert_eq!(first.status.code(), Some(0));
    let first = String::from_utf8(first.stdout).unwrap();
    assert!(first.contains("source: "), "{first}");
    assert!(
        !first.contains("[stderr]"),
        "stdout keeps its own identity: {first}"
    );
    for line in stdout.lines().take(3) {
        assert!(first.contains(line), "{first}");
    }
    let second = invoke(&binary, &["recall", &handles[1]], &workspace, &home, None);
    assert_eq!(second.status.code(), Some(0));
    let second = String::from_utf8(second.stdout).unwrap();
    assert!(second.contains("[stderr]"), "{second}");
    for line in stderr.lines().take(3) {
        assert!(second.contains(line), "{second}");
    }
    assert_eq!(ledger_runs(&ledger), 1, "recall never reruns the command");
}

/// Task 1.3: the local record stays bounded and survives damaged or missing
/// storage without inventing an outcome.
#[test]
fn diagnostics_records_stay_bounded_and_readable() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    // An unsupported command still records why compression did not apply.
    for _ in 0..33 {
        let output = invoke(
            &binary,
            &[
                "compact",
                cargo_double(&binary).to_str().unwrap(),
                "test",
                "-v",
            ],
            &workspace,
            &home,
            None,
        );
        assert_eq!(output.status.code(), Some(0));
    }
    let records = diagnostics(&binary, &workspace, &home);
    assert_eq!(records.len(), 32, "the local record stays bounded");
    assert_eq!(records[0]["reason"], "unsupported-flags");
    let clamped = invoke(
        &binary,
        &["diagnostics", "--json", "--last", "100"],
        &workspace,
        &home,
        None,
    );
    let value: Value = serde_json::from_slice(&clamped.stdout).unwrap();
    assert_eq!(value["records"].as_array().unwrap().len(), 32);
    let last = invoke(
        &binary,
        &["diagnostics", "--json", "--last", "2"],
        &workspace,
        &home,
        None,
    );
    let value: Value = serde_json::from_slice(&last.stdout).unwrap();
    assert_eq!(value["records"].as_array().unwrap().len(), 2);
    for (args, code, needle) in [
        (vec!["diagnostics", "--last", "0"], 1, "positive integer"),
        (vec!["diagnostics", "--depth", "1"], 1, "unknown option"),
    ] {
        let output = invoke(&binary, &args, &workspace, &home, None);
        assert_eq!(output.status.code(), Some(code), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(needle),
            "{args:?}"
        );
    }
    // A damaged record is reported instead of being guessed.
    fs::write(
        home.join("harness/rtk/diagnostics/records.json"),
        b"not json",
    )
    .unwrap();
    let damaged = invoke(&binary, &["diagnostics"], &workspace, &home, None);
    assert_eq!(damaged.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&damaged.stderr).contains("unreadable"));
    // Nothing recorded yet stays a clean, explicit answer.
    let fresh = tempfile::tempdir().unwrap();
    let fresh_home = fresh.path().join("codex");
    fs::create_dir(&fresh_home).unwrap();
    let empty = invoke(&binary, &["diagnostics"], &workspace, &fresh_home, None);
    assert_eq!(empty.status.code(), Some(0));
    assert!(empty.stdout.is_empty());
    assert!(String::from_utf8_lossy(&empty.stderr).contains("no compact-run decision records yet"));
}

/// A01: two real processes publish into one observation store at a controlled
/// interleaving. The second writer starts while the first holds the store
/// transaction open, and below the retention limits both successful handles
/// must recall their exact originals with valid digests.
#[test]
fn two_process_publication_keeps_both_handles_below_limits() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    let barrier = root.path().join("barrier");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    fs::create_dir(&barrier).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    let ledger = fixture_ledger(root.path());
    let first_stdout = warning_stream().repeat(6);
    let second_stdout = compile_failure_stream().repeat(6);

    // The first writer is parked on the store boundary itself: the second
    // writer runs a complete publication before the first enters its
    // transaction. The declared invariant is that every advertised handle
    // recalls its exact original; on the pre-fix code the second writer's
    // retention deletes the first writer's content exactly here.
    let latch = Barrier::arm(&barrier, "store-before-lock");
    let mut first = BarrierChild::spawn(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        &[
            (BARRIER_ENV, barrier.to_str().unwrap()),
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", first_stdout.as_str()),
            ("HARNESS_RTK_FIXTURE_LEDGER", ledger.to_str().unwrap()),
        ],
    );
    first.wait_for_barrier("store-before-lock");
    let second = invoke_with_env(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        None,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", second_stdout.as_str()),
            ("HARNESS_RTK_FIXTURE_LEDGER", ledger.to_str().unwrap()),
        ],
    );
    latch.release();
    let (first_status, first_out, first_err) = first.wait();
    assert_eq!(first_status.code(), Some(0), "{first_err}");
    assert_eq!(
        second.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let first_text = String::from_utf8(first_out).unwrap();
    let second_text = String::from_utf8(second.stdout).unwrap();
    let first_handle = packed_handle(&first_text);
    let second_handle = packed_handle(&second_text);
    assert_ne!(
        first_handle, second_handle,
        "each stream keeps its own handle"
    );
    assert_eq!(ledger_runs(&ledger), 2, "each process ran its Cargo once");

    // Every advertised handle must recall its own exact original: this is the
    // invariant that the pre-fix publication/retention interleaving broke.
    let recalled = invoke(&binary, &["recall", &first_handle], &workspace, &home, None);
    assert_eq!(
        recalled.status.code(),
        Some(0),
        "the first writer's advertised handle must recall its original: {}",
        String::from_utf8_lossy(&recalled.stderr)
    );
    let recalled_text = String::from_utf8_lossy(&recalled.stdout).into_owned();
    assert!(
        recalled_text.contains("digest: sha256 ") && recalled_text.contains(" verified"),
        "{recalled_text}"
    );
    for line in first_stdout.lines().take(4) {
        assert!(recalled_text.contains(line), "{recalled_text}");
    }
    let recalled = invoke(
        &binary,
        &["recall", &second_handle],
        &workspace,
        &home,
        None,
    );
    assert_eq!(
        recalled.status.code(),
        Some(0),
        "the second writer's advertised handle must recall its original: {}",
        String::from_utf8_lossy(&recalled.stderr)
    );
    let recalled_text = String::from_utf8_lossy(&recalled.stdout).into_owned();
    for line in second_stdout.lines().take(4) {
        assert!(recalled_text.contains(line), "{recalled_text}");
    }
    assert_eq!(
        fs::read(pack_file(&home, &first_handle)).unwrap(),
        first_stdout.as_bytes(),
        "the first observation keeps its exact original"
    );
    assert_eq!(
        fs::read(pack_file(&home, &second_handle)).unwrap(),
        second_stdout.as_bytes(),
        "neither writer removes the other's content"
    );
    let index: Value =
        serde_json::from_slice(&fs::read(home.join("harness/rtk/pack/index.json")).unwrap())
            .unwrap();
    assert_eq!(index["entries"].as_array().unwrap().len(), 2, "{index}");
    let staging = home.join("harness/rtk/pack/staging");
    let leftovers = fs::read_dir(&staging)
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0);
    assert_eq!(leftovers, 0, "no writer leaves staging leftovers behind");
}

/// A01: while one writer holds its store transaction open, a second writer's
/// bounded store access expires. The waiting run keeps its raw output usable
/// and never advertises a handle for content it did not commit; the holder
/// completes normally afterwards.
#[test]
fn store_contention_is_bounded_and_never_advertises_an_unusable_handle() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    let barrier = root.path().join("barrier");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    fs::create_dir(&barrier).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    let first_stdout = warning_stream().repeat(6);
    let second_stdout = warning_stream().repeat(6);

    let latch = Barrier::arm(&barrier, "store-staged");
    let mut first = BarrierChild::spawn(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        &[
            (BARRIER_ENV, barrier.to_str().unwrap()),
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", first_stdout.as_str()),
        ],
    );
    first.wait_for_barrier("store-staged");
    let started = Instant::now();
    let second = invoke_with_env(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        None,
        &[("HARNESS_RTK_FIXTURE_CARGO_STDOUT", second_stdout.as_str())],
    );
    let waited = started.elapsed();
    println!("rtk store contention: bounded publication attempt returned after {waited:?}");
    latch.release();
    let (first_status, first_out, first_err) = first.wait();

    // The holder completes normally, and everything it advertised is real.
    assert_eq!(first_status.code(), Some(0), "{first_err}");
    let first_text = String::from_utf8(first_out).unwrap();
    let handle = packed_handle(&first_text);
    let recalled = invoke(&binary, &["recall", &handle], &workspace, &home, None);
    assert_eq!(
        recalled.status.code(),
        Some(0),
        "the holder's advertised handle must recall its original: {}",
        String::from_utf8_lossy(&recalled.stderr)
    );
    assert!(
        String::from_utf8(recalled.stdout)
            .unwrap()
            .contains("digest: sha256 ")
    );

    assert_eq!(second.status.code(), Some(0));
    let second_text = String::from_utf8_lossy(&second.stdout).into_owned();
    assert!(
        !second_text.contains("[rtk pack:"),
        "no handle may be advertised for uncommitted content: {second_text}"
    );
    assert!(
        second_text.contains("[rtk raw: "),
        "the original stays reachable through its locator: {second_text}"
    );
    let second_err = String::from_utf8_lossy(&second.stderr).into_owned();
    assert!(
        second_err.contains("observation store is busy")
            && second_err.contains("observation handle not issued"),
        "{second_err}"
    );
    assert!(
        waited >= Duration::from_millis(1500),
        "the bounded store access is a bounded wait, not an immediate failure: {waited:?}"
    );
    let locator = second_text
        .lines()
        .find(|line| line.starts_with("[rtk raw: "))
        .expect("the waiting run still reports its raw locator");
    let raw_path = PathBuf::from(&locator["[rtk raw: ".len()..locator.len() - 1]);
    assert_eq!(
        fs::read(&raw_path).unwrap(),
        second_stdout.as_bytes(),
        "the waiting run's original is byte-exact"
    );

    let index: Value =
        serde_json::from_slice(&fs::read(home.join("harness/rtk/pack/index.json")).unwrap())
            .unwrap();
    assert_eq!(
        index["entries"].as_array().unwrap().len(),
        1,
        "only the completed writer is committed: {index}"
    );
}

/// A01: a writer that dies before its commit leaves only recoverable staging; a
/// writer that dies after the commit boundary leaves committed evidence that
/// still reads. Both interleavings preserve previously committed observations.
#[test]
fn interrupted_publication_recovers_before_and_after_the_commit() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    let barrier = root.path().join("barrier");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    fs::create_dir(&barrier).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    let pack = home.join("harness/rtk/pack");
    let payload = |count: usize| warning_stream().repeat(count);

    let baseline = payload(6);
    let first = invoke_with_env(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        None,
        &[("HARNESS_RTK_FIXTURE_CARGO_STDOUT", baseline.as_str())],
    );
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let baseline_handle = packed_handle(&String::from_utf8(first.stdout).unwrap());

    // Crash before the commit boundary: the transaction is interrupted while
    // the staged payload exists, so no handle was ever published.
    let latch = Barrier::arm(&barrier, "store-staged");
    let interrupted = payload(7);
    let mut crashed = BarrierChild::spawn(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        &[
            (BARRIER_ENV, barrier.to_str().unwrap()),
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", interrupted.as_str()),
        ],
    );
    crashed.wait_for_barrier("store-staged");
    let (status, crashed_out) = crashed.kill();
    assert_ne!(status.code(), Some(0), "the writer did not complete");
    assert!(
        crashed_out.is_empty(),
        "an interrupted writer advertises no handle: {}",
        String::from_utf8_lossy(&crashed_out)
    );
    latch.release();

    // The next operation recovers: staging is empty and the previously
    // committed observation still reads.
    let recovered = payload(8);
    let second = invoke_with_env(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        None,
        &[("HARNESS_RTK_FIXTURE_CARGO_STDOUT", recovered.as_str())],
    );
    assert_eq!(
        second.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_handle = packed_handle(&String::from_utf8(second.stdout).unwrap());
    let leftovers = fs::read_dir(pack.join("staging"))
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0);
    assert_eq!(leftovers, 0, "interrupted staging is recovered");
    let recalled = invoke(
        &binary,
        &["recall", &baseline_handle],
        &workspace,
        &home,
        None,
    );
    assert_eq!(
        recalled.status.code(),
        Some(0),
        "committed evidence survives the interrupted writer: {}",
        String::from_utf8_lossy(&recalled.stderr)
    );
    assert_eq!(
        fs::read(pack_file(&home, &baseline_handle)).unwrap(),
        baseline.as_bytes()
    );

    // Crash after the commit boundary: the observation is committed before the
    // writer is killed, so it stays readable even though its author died.
    let mut before: Vec<String> = fs::read_dir(&pack)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .strip_suffix(".log")
                .map(str::to_owned)
        })
        .collect();
    before.sort();
    let latch = Barrier::arm(&barrier, "store-committed");
    let committed = payload(9);
    let mut crashed = BarrierChild::spawn(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        &[
            (BARRIER_ENV, barrier.to_str().unwrap()),
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", committed.as_str()),
        ],
    );
    crashed.wait_for_barrier("store-committed");
    let (status, crashed_out) = crashed.kill();
    assert_ne!(status.code(), Some(0));
    assert!(
        crashed_out.is_empty(),
        "the killed writer never got to advertise its handle"
    );
    latch.release();
    let mut after: Vec<String> = fs::read_dir(&pack)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .strip_suffix(".log")
                .map(str::to_owned)
        })
        .collect();
    after.sort();
    let new_handles: Vec<String> = after
        .iter()
        .filter(|name| !before.contains(name))
        .cloned()
        .collect();
    assert_eq!(
        new_handles.len(),
        1,
        "the committed observation is the only new content: {after:?}"
    );
    let handle = &new_handles[0];
    assert_eq!(
        fs::read(pack_file(&home, handle)).unwrap(),
        committed.as_bytes(),
        "the committed bytes are complete after the writer's death"
    );
    let recalled = invoke(&binary, &["recall", handle], &workspace, &home, None);
    assert_eq!(
        recalled.status.code(),
        Some(0),
        "a committed observation survives its writer: {}",
        String::from_utf8_lossy(&recalled.stderr)
    );
    let recalled_text = String::from_utf8(recalled.stdout).unwrap();
    for line in committed.lines().take(4) {
        assert!(recalled_text.contains(line), "{recalled_text}");
    }
    // The store stays usable: a later writer still publishes and recalls.
    let later = payload(10);
    let third = invoke_with_env(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        None,
        &[("HARNESS_RTK_FIXTURE_CARGO_STDOUT", later.as_str())],
    );
    assert_eq!(third.status.code(), Some(0));
    let third_handle = packed_handle(&String::from_utf8(third.stdout).unwrap());
    assert!(
        third_handle != *handle && third_handle != second_handle,
        "each publication keeps its own handle"
    );
    assert_eq!(
        invoke(&binary, &["recall", &third_handle], &workspace, &home, None)
            .status
            .code(),
        Some(0)
    );
}

/// A01: a recall that overlaps legitimate eviction either serves verified
/// committed content or reports explicit expiry - never a partial window and
/// never a deadlock.
#[test]
fn recall_overlapping_eviction_reports_expiry_without_mixed_content() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    let barrier = root.path().join("barrier");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    fs::create_dir(&barrier).unwrap();
    let (binary, command) = staged(root.path());
    let target = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "300"],
        &workspace,
        &home,
        None,
    );
    assert_eq!(
        target.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&target.stderr)
    );
    let handle = packed_handle(&String::from_utf8(target.stdout).unwrap());

    let latch = Barrier::arm(&barrier, "recall-indexed");
    let mut reader = BarrierChild::spawn(
        &binary,
        &["recall", &handle],
        &workspace,
        &home,
        &[(BARRIER_ENV, barrier.to_str().unwrap())],
    );
    reader.wait_for_barrier("recall-indexed");
    // The reader already holds its verified index record; 64 further
    // publications now push that record out of the 64-record retention window.
    let mut newest = String::new();
    for _ in 0..64 {
        let output = invoke(
            &binary,
            &["compact", command.to_str().unwrap(), "log", "-n", "12"],
            &workspace,
            &home,
            None,
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        newest = packed_handle(&String::from_utf8(output.stdout).unwrap());
    }
    latch.release();
    let (status, stdout, stderr) = reader.wait();
    assert_eq!(status.code(), Some(2), "{stderr}");
    assert!(
        stdout.is_empty(),
        "an evicted observation serves no partial window: {}",
        String::from_utf8_lossy(&stdout)
    );
    assert!(stderr.contains("unknown observation handle"), "{stderr}");
    // The store stays coherent for everything it still names.
    let recalled = invoke(&binary, &["recall", &newest], &workspace, &home, None);
    assert_eq!(
        recalled.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&recalled.stderr)
    );
    assert!(
        String::from_utf8(recalled.stdout)
            .unwrap()
            .contains("digest: sha256 ")
    );
}

/// A01: an unusable store write path keeps the command output usable and issues
/// no handle, instead of failing the run or advertising missing content.
#[test]
fn store_write_failure_keeps_raw_output_and_issues_no_handle() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let expected = command_output(&command, &["log", "-n", "300"], &workspace);
    // A file where the staging area belongs makes every store write fail.
    let pack = home.join("harness/rtk/pack");
    fs::create_dir_all(&pack).unwrap();
    fs::write(pack.join("staging"), b"not a directory").unwrap();
    let output = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "300"],
        &workspace,
        &home,
        None,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("fixture-pipe git-log: 300 lines"), "{text}");
    assert!(
        !text.contains("[rtk pack:"),
        "no handle may be issued for unreachable content: {text}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("observation handle not issued"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !pack.join("index.json").exists(),
        "no store state is invented"
    );
    let locator = text
        .lines()
        .find(|line| line.starts_with("[rtk raw: "))
        .expect("the raw locator stays present");
    let raw_path = PathBuf::from(&locator["[rtk raw: ".len()..locator.len() - 1]);
    assert_eq!(
        fs::read(&raw_path).unwrap(),
        expected,
        "the delivered original is byte-exact"
    );
    let records = diagnostics(&binary, &workspace, &home);
    let record = record_for(&records, &format!("{} log -n 300", command.display()));
    assert_eq!(record["decision"], "applied", "{record}");
    assert_eq!(record["streams"][0]["handle"], Value::Null, "{record}");
}

/// W2/N01: malformed or unknown index metadata never authorizes destructive
/// retention. Publication and recall refuse, committed evidence survives and
/// outside sentinels stay byte-identical.
#[test]
fn invalid_index_metadata_preserves_evidence_and_outside_sentinels() {
    let cases = [
        "parent-relative handle",
        "absolute handle",
        "separator handle",
        "unsupported schema",
        "malformed digest",
        "size accounting overflow",
        "missing field",
    ];
    for case in cases {
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
        assert_eq!(compact.status.code(), Some(0), "{case}");
        let valid_handle = packed_handle(&String::from_utf8(compact.stdout).unwrap());
        let pack = home.join("harness/rtk/pack");
        let index_path = pack.join("index.json");
        let mut index: Value = serde_json::from_slice(&fs::read(&index_path).unwrap()).unwrap();
        let crafted = |handle: &str, bytes: u64| {
            json!({
                "handle": handle,
                "source": "synthetic sentinel",
                "digest": "0".repeat(64),
                "bytes": bytes,
                "lines": 1,
                "created": 0
            })
        };
        let mut sentinel: Option<PathBuf> = None;
        match case {
            "parent-relative handle" => {
                // A naive reader would delete `pack/../../sentinel.log`.
                index["entries"]
                    .as_array_mut()
                    .unwrap()
                    .push(crafted(r"..\..\sentinel", 200 * 1024 * 1024));
                let path = home.join("harness/sentinel.log");
                fs::write(&path, b"parent-relative sentinel").unwrap();
                sentinel = Some(path);
            }
            "absolute handle" => {
                let path = root.path().join("absolute-sentinel.log");
                fs::write(&path, b"absolute sentinel").unwrap();
                let handle = path.to_string_lossy().trim_end_matches(".log").to_owned();
                index["entries"]
                    .as_array_mut()
                    .unwrap()
                    .push(crafted(&handle, 200 * 1024 * 1024));
                sentinel = Some(path);
            }
            "separator handle" => {
                fs::create_dir_all(pack.join("sub")).unwrap();
                let path = pack.join("sub").join("sentinel.log");
                fs::write(&path, b"separator sentinel").unwrap();
                index["entries"]
                    .as_array_mut()
                    .unwrap()
                    .push(crafted(r"sub\sentinel", 200 * 1024 * 1024));
                sentinel = Some(path);
            }
            "unsupported schema" => index["schema"] = json!(2),
            "malformed digest" => {
                let mut record = crafted("ob-0000000000000000001-42-000001", 1);
                record["digest"] = json!("00ff");
                index["entries"].as_array_mut().unwrap().push(record);
            }
            "size accounting overflow" => {
                index["entries"].as_array_mut().unwrap().extend([
                    crafted("ob-0000000000000000001-42-000001", u64::MAX),
                    crafted("ob-0000000000000000002-42-000002", u64::MAX),
                ]);
            }
            "missing field" => {
                let mut record = crafted("ob-0000000000000000001-42-000001", 1);
                record.as_object_mut().unwrap().remove("lines");
                index["entries"].as_array_mut().unwrap().push(record);
            }
            _ => unreachable!(),
        }
        let mutated = serde_json::to_vec(&index).unwrap();
        fs::write(&index_path, &mutated).unwrap();
        let valid_pack = pack_file(&home, &valid_handle);
        let valid_bytes = fs::read(&valid_pack).unwrap();

        let attempt = invoke(
            &binary,
            &["compact", command.to_str().unwrap(), "log", "-n", "12"],
            &workspace,
            &home,
            None,
        );
        assert_eq!(attempt.status.code(), Some(0), "{case}");
        let text = String::from_utf8_lossy(&attempt.stdout).into_owned();
        assert!(
            !text.contains("[rtk pack:"),
            "{case}: invalid metadata must not authorize a handle: {text}"
        );
        assert!(text.contains("[rtk raw: "), "{case}: {text}");
        let stderr = String::from_utf8_lossy(&attempt.stderr).into_owned();
        assert!(
            stderr.contains("pack index") && stderr.contains("observation handle not issued"),
            "{case}: the invalid metadata is named: {stderr}"
        );
        assert_eq!(
            fs::read(&index_path).unwrap(),
            mutated,
            "{case}: the invalid index is left untouched"
        );
        assert_eq!(
            fs::read(&valid_pack).unwrap(),
            valid_bytes,
            "{case}: committed evidence survives"
        );
        if let Some(path) = &sentinel {
            assert!(
                path.is_file(),
                "{case}: the outside sentinel is not removed"
            );
        }
        // Recall must not claim success from an unusable index.
        let recall = invoke(&binary, &["recall", &valid_handle], &workspace, &home, None);
        assert_eq!(recall.status.code(), Some(1), "{case}");
        assert!(recall.stdout.is_empty(), "{case}");
        assert!(
            String::from_utf8_lossy(&recall.stderr).contains("pack index"),
            "{case}: {}",
            String::from_utf8_lossy(&recall.stderr)
        );
    }
}

/// W2/N01: recovery and retention never follow a reparse point. A junction with
/// an observation-shaped name inside the store stays a junction and its outside
/// target survives.
#[test]
fn store_recovery_never_follows_reparse_sentinels() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, command) = staged(root.path());
    let first = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "12"],
        &workspace,
        &home,
        None,
    );
    assert_eq!(first.status.code(), Some(0));
    let first_handle = packed_handle(&String::from_utf8(first.stdout).unwrap());

    // A junction named like an orphaned observation record points outside.
    let pack = home.join("harness/rtk/pack");
    let target = home.join("outside");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("inside.txt"), b"reparse target").unwrap();
    let junction = pack.join("ob-0000000000000000000-1-000000.log");
    // `mklink` is a cmd builtin and rejects forward slashes in a link path as
    // switches, so both paths go in as separate, backslash-normalized args.
    let created = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(junction.display().to_string().replace('/', "\\"))
        .arg(target.display().to_string().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "junction creation: {}{}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
    assert!(
        fs::symlink_metadata(&junction)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the fixture is a reparse point"
    );

    // A normal publication recovers the store: the junction is skipped, the
    // outside target is untouched and both committed observations still read.
    let second = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "12"],
        &workspace,
        &home,
        None,
    );
    assert_eq!(
        second.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_handle = packed_handle(&String::from_utf8(second.stdout).unwrap());
    assert!(
        fs::symlink_metadata(&junction)
            .unwrap()
            .file_type()
            .is_symlink(),
        "recovery leaves the reparse point in place"
    );
    assert_eq!(
        fs::read(target.join("inside.txt")).unwrap(),
        b"reparse target",
        "the outside target survives"
    );
    for handle in [&first_handle, &second_handle] {
        let recalled = invoke(&binary, &["recall", handle], &workspace, &home, None);
        assert_eq!(
            recalled.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&recalled.stderr)
        );
    }
}

/// A02/N07: a status-only Cargo stream cannot pay for its own locator and
/// handle, so the complete presentation stays raw; the same deep path still
/// compacts when the removed progress is large enough, and the applied ledger
/// is exactly the emitted presentation diagnostics report.
#[test]
fn complete_presentation_accounting_falls_back_to_raw_when_metadata_outweighs_removal() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    // A long home makes the raw locator itself a material part of the ledger.
    let home = root.path().join("d".repeat(210)).join("e".repeat(110));
    fs::create_dir(&workspace).unwrap();
    fs::create_dir_all(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    let ledger = fixture_ledger(root.path());
    let status_line = "    Checking synthetic-demo v0.1.0\n";

    // 15 removed lines (525 bytes) do not cover the ~580 byte footer: raw wins.
    let almost = status_line.repeat(15);
    let output = invoke_with_env(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        None,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", almost.as_str()),
            ("HARNESS_RTK_FIXTURE_LEDGER", ledger.to_str().unwrap()),
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        almost,
        "the complete presentation does not shrink, so the raw bytes stay"
    );
    let records = diagnostics(&binary, &workspace, &home);
    let record = record_for(&records, &format!("{} check --locked", cargo.display()));
    assert_eq!(record["decision"], "bypassed", "{record}");
    assert_eq!(record["reason"], "non-shrinking", "{record}");
    assert_eq!(
        record["presentation_bytes"].as_u64().unwrap(),
        almost.len() as u64,
        "{record}"
    );
    assert!(
        !home.join("harness/rtk/pack/index.json").exists(),
        "a raw fallback publishes nothing"
    );

    // 40 removed lines (1000 bytes) cover the same footer: applied, and the
    // reported presentation is exactly what the adapter emitted.
    let paid = status_line.repeat(40);
    let output = invoke_with_env(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        None,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", paid.as_str()),
            ("HARNESS_RTK_FIXTURE_LEDGER", ledger.to_str().unwrap()),
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout.clone()).unwrap();
    assert!(text.contains("[rtk pack: "), "{text}");
    let handle = packed_handle(&text);
    let records = diagnostics(&binary, &workspace, &home);
    let record = record_for(&records, &format!("{} check --locked", cargo.display()));
    assert_eq!(record["decision"], "applied", "{record}");
    assert_eq!(
        record["presentation_bytes"].as_u64().unwrap(),
        output.stdout.len() as u64,
        "the ledger is the emitted presentation: {record}"
    );
    assert!(
        record["presentation_bytes"].as_u64().unwrap() < paid.len() as u64,
        "an applied run delivers fewer bytes than its raw original: {record}"
    );
    assert_eq!(
        ledger_runs(&ledger),
        2,
        "each run executed Cargo exactly once"
    );
    let recalled = invoke(&binary, &["recall", &handle], &workspace, &home, None);
    assert_eq!(
        recalled.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&recalled.stderr)
    );
    assert!(
        String::from_utf8(recalled.stdout)
            .unwrap()
            .contains("stored: 40 lines")
    );
}

/// A02/N07: the non-Cargo compact route obeys the same complete-presentation
/// rule for its raw locator. A long path flips a barely-compressible stream to
/// raw while the identical command under a short home still compacts.
#[test]
fn non_cargo_compact_route_accounts_for_its_long_locator() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let short_home = root.path().join("short-codex");
    let long_home = root.path().join("g".repeat(215)).join("h".repeat(215));
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&short_home).unwrap();
    fs::create_dir_all(&long_home).unwrap();
    let (binary, command) = staged(root.path());
    let expected = command_output(&command, &["log", "-n", "4"], &workspace);
    assert!(expected.len() >= 500);

    // Short home: the fixture's small compaction pays for a short locator.
    let compact = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "4"],
        &workspace,
        &short_home,
        None,
    );
    assert_eq!(
        compact.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&compact.stderr)
    );
    let text = String::from_utf8(compact.stdout).unwrap();
    assert!(text.contains("[rtk pack: "), "{text}");
    let handle = packed_handle(&text);

    // Long home: the same body cannot pay for the long locator, so the run
    // stays byte-raw and publishes nothing.
    let raw = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "4"],
        &workspace,
        &long_home,
        None,
    );
    assert_eq!(
        raw.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&raw.stderr)
    );
    assert_eq!(
        raw.stdout, expected,
        "a long locator that outweighs the compaction keeps the raw bytes"
    );
    assert!(
        !long_home.join("harness/rtk/pack/index.json").exists(),
        "the long-path fallback publishes nothing"
    );
    let records = diagnostics(&binary, &workspace, &long_home);
    let record = record_for(&records, &format!("{} log -n 4", command.display()));
    assert_eq!(record["decision"], "bypassed", "{record}");
    assert_eq!(record["reason"], "non-shrinking", "{record}");

    // A larger removal in the same long home still compacts, and the long
    // locator it prints points at the byte-exact original.
    let larger = command_output(&command, &["log", "-n", "8"], &workspace);
    let applied = invoke(
        &binary,
        &["compact", command.to_str().unwrap(), "log", "-n", "8"],
        &workspace,
        &long_home,
        None,
    );
    assert_eq!(applied.status.code(), Some(0));
    let text = String::from_utf8(applied.stdout).unwrap();
    let locator = text
        .lines()
        .find(|line| line.starts_with("[rtk raw: "))
        .expect("the applied run carries its locator");
    let raw_path = PathBuf::from(&locator["[rtk raw: ".len()..locator.len() - 1]);
    assert!(
        raw_path.to_string_lossy().len() > 260,
        "the long-path case really uses the long path: {raw_path:?}"
    );
    assert_eq!(fs::read(&raw_path).unwrap(), larger);
    let applied_handle = packed_handle(&text);
    let recalled = invoke(
        &binary,
        &["recall", &applied_handle],
        &workspace,
        &long_home,
        None,
    );
    assert_eq!(
        recalled.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&recalled.stderr)
    );

    // The short-home observation still recalls its own exact original.
    let recalled = invoke(&binary, &["recall", &handle], &workspace, &short_home, None);
    assert_eq!(recalled.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&recalled.stdout).contains("digest: sha256 "));
}

/// A03: only lines whose status word ends exactly at Cargo's 12-column status
/// field are removed. Wrong-width imitations, bare status words, unknown
/// diagnostic blocks and binary input stay byte-preserved, the child's status
/// is preserved, the original stays recallable and Cargo runs exactly once.
#[test]
fn cargo_status_recognition_is_conservative_and_preserves_originals() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let home = root.path().join("codex");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&home).unwrap();
    let (binary, _) = staged(root.path());
    let cargo = cargo_double(&binary);
    let ledger = fixture_ledger(root.path());
    let user_lines = [
        "Compiling demo starts at column one\n",
        "  Compiling demo is one column short\n",
        "    Compiling demo is one column past\n",
        "    Doc-tests demo is not Cargo's layout\n",
        "error[E0308]: mismatched types\n",
        " --> broken/src/lib.rs:2:5\n",
    ];
    let mut stdout = String::new();
    for _ in 0..32 {
        stdout.push_str("    Checking synthetic-demo v0.1.0\n");
    }
    for line in user_lines {
        stdout.push_str(line);
    }
    stdout.push_str(&unknown_block());
    let mut stderr = String::new();
    stderr.push_str("  Compiling demo is one column short\n");
    for _ in 0..16 {
        stderr.push_str("     Running unittests src/lib.rs (target/debug/deps/demo-1)\n");
    }
    stderr.push_str("    Running unittests with one extra column\n");

    let output = invoke_bounded(
        &binary,
        &["compact", cargo.to_str().unwrap(), "check", "--locked"],
        &workspace,
        &home,
        &[
            ("HARNESS_RTK_FIXTURE_CARGO_STDOUT", stdout.as_str()),
            ("HARNESS_RTK_FIXTURE_CARGO_STDERR", stderr.as_str()),
            ("HARNESS_RTK_FIXTURE_CARGO_EXIT", "101"),
            ("HARNESS_RTK_FIXTURE_LEDGER", ledger.to_str().unwrap()),
        ],
        Duration::from_secs(30),
    );
    assert_eq!(
        output.status.code(),
        Some(101),
        "the child's own exit status survives: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    for line in user_lines {
        assert!(
            out.contains(line),
            "wrong-width user text stays byte-verbatim: {line:?} in {out}"
        );
    }
    assert!(out.contains(&unknown_block()), "{out}");
    assert!(
        !out.contains("Checking synthetic-demo"),
        "exactly aligned status lines are elided: {out}"
    );
    assert!(
        err.contains("  Compiling demo is one column short"),
        "a wrong-indentation stderr line survives: {err}"
    );
    assert!(
        err.contains("    Running unittests with one extra column"),
        "a wrong-width stderr line survives: {err}"
    );
    assert!(
        !err.contains("     Running unittests src/lib.rs"),
        "an aligned stderr status line is elided: {err}"
    );
    assert!(
        out.contains("[rtk cargo exit: 101]"),
        "the failure notice stays visible: {out}"
    );
    assert_eq!(ledger_runs(&ledger), 1, "Cargo runs exactly once");

    // The exact original, including every elided line, stays recallable.
    let handle = packed_handle(&out);
    let recalled = invoke(&binary, &["recall", &handle], &workspace, &home, None);
    assert_eq!(
        recalled.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&recalled.stderr)
    );
    let recalled = String::from_utf8(recalled.stdout).unwrap();
    assert!(
        recalled.contains("    Checking synthetic-demo v0.1.0"),
        "{recalled}"
    );
    for line in user_lines {
        assert!(recalled.contains(line), "{recalled}");
    }

    // The shared binary guard bypasses recognition entirely.
    let binary_input: Vec<u8> = [0xff, 0xfe, 0x00, 0x01, b'\n'].repeat(120);
    let output = invoke(
        &binary,
        &["filter", "git-log"],
        &workspace,
        &home,
        Some(&binary_input),
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, binary_input, "binary input passes byte-raw");
}
