//! Process regressions for the installed script launcher fallback.
//! Owned isolated homes only; no global PATH mutation or process kill-by-name.
#![cfg(windows)]
use harness_core::{
    console::{ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline, StopReason},
};
use serde_json::{Value, json};
use std::{
    env, fs, io,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

const DESKTOP_PWSH: &str = r"C:\Program Files\PowerShell\7\pwsh.exe";
const UNAVAILABLE: &str = "codex-harness: Harness unavailable; starting ordinary Codex CLI with your original arguments and local settings.";
const NO_ORIGINAL: &str = "codex-harness: Harness unavailable and the original Codex CLI could not be found. Restore its installation or PATH; no command was launched.";
const RECURSIVE: &str =
    "codex-harness: Recursive launcher registration; repair the original Codex CLI path.";
const SHARED_DEFAULTS: &str = "codex-harness: Shared defaults unavailable; starting ordinary Codex CLI with your original arguments and local settings.";
const TRICKY_ARGS: [&str; 6] = [
    "exec",
    "",
    "проверка \"кавычки\"",
    "trailing\\",
    "$() `literal` ; &",
    "--",
];
const STDIN: &str = "первая строка\nsecond line\n";

struct Fixture {
    root: tempfile::TempDir,
    home: PathBuf,
    source: PathBuf,
    workspace: PathBuf,
    launcher: PathBuf,
    launcher_source: PathBuf,
    bin: PathBuf,
    path_fixture: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("script-fallback проверка-")
            .tempdir()
            .unwrap();
        let home = root.path().join("home");
        let source = root.path().join("source");
        let workspace = root.path().join("workspace");
        let bin = root.path().join("bin");
        let launchers = home.join("harness/launchers/owned/codex.ps1");
        let tools = source.join("tools");
        fs::create_dir_all(launchers.parent().unwrap()).unwrap();
        fs::create_dir_all(&tools).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&bin).unwrap();
        let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let bootstrap = fs::read(checkout.join("tools/codex.ps1")).unwrap();
        let module = fs::read(checkout.join("tools/launcher.psm1")).unwrap();
        fs::write(&launchers, &bootstrap).unwrap();
        fs::write(tools.join("codex.ps1"), &bootstrap).unwrap();
        fs::write(tools.join("launcher.psm1"), module).unwrap();
        let launcher = bin.join("codex.ps1");
        std::os::windows::fs::symlink_file(&launchers, &launcher).unwrap();
        let upstream = root.path().join("upstream.exe");
        fs::copy(env!("CARGO_BIN_EXE_harness-launch-fixture"), &upstream).unwrap();
        let path_fixture = bin.join("codex.exe");
        fs::copy(&upstream, &path_fixture).unwrap();
        let f = Self {
            root,
            home,
            source,
            workspace,
            launcher,
            launcher_source: launchers,
            bin,
            path_fixture,
        };
        f.write_registration(f.healthy_registration());
        f
    }

    fn healthy_registration(&self) -> Value {
        json!({
            "schemaVersion": 1,
            "sourceRoot": self.source,
            "codexHome": self.home,
            "userHome": self.root.path().join("user"),
            "codexCommand": self.root.path().join("upstream.exe"),
            "profileName": "harness",
            "pathScope": "Process",
            "pathAdded": false,
            "versions": {},
            "links": [],
            "launcherSource": self.launcher_source,
            "configBridge": env!("CARGO_BIN_EXE_codex-harness"),
        })
    }

    fn write_registration(&self, value: Value) {
        fs::write(
            self.home.join("harness/installation.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }

    fn mutate_registration(&self, edit: impl FnOnce(&mut serde_json::Map<String, Value>)) {
        let mut value = self.healthy_registration();
        edit(value.as_object_mut().unwrap());
        self.write_registration(value);
    }

    fn isolated_path(&self, include_bin: bool) -> std::ffi::OsString {
        let mut entries = Vec::new();
        if include_bin {
            entries.push(self.bin.clone());
        }
        entries.extend(
            env::split_paths(&env::var_os("PATH").unwrap()).filter(|entry| {
                let text = entry.to_string_lossy().to_ascii_lowercase();
                !text.contains("windowsapps")
                    && !same_path(entry, &self.bin)
                    && !looks_like_codex_dir(entry)
            }),
        );
        env::join_paths(entries).unwrap()
    }

    fn command(&self) -> Command {
        let mut command = Command::new(desktop_pwsh());
        command
            .args(["-NoLogo", "-NoProfile", "-File"])
            .arg(&self.launcher)
            .current_dir(&self.workspace)
            .env("CODEX_HOME", &self.home)
            .env("PATH", self.isolated_path(true))
            .env_remove("HARNESS_LAUNCH_FIXTURE_MODE")
            .env("PYTHONUTF8", "1")
            .env("POWERSHELL_TELEMETRY_OPTOUT", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    fn run_with_stdin(&self, args: &[&str], stdin: &str, mode: Option<&str>) -> Output {
        let runner = self.root.path().join("stdin-runner.ps1");
        fs::write(
            &runner,
            concat!(
                "[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)\n",
                "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)\n",
                "$OutputEncoding = [Console]::OutputEncoding\n",
                "[string[]]$forwarded = @(ConvertFrom-Json $env:HARNESS_SCRIPT_ARGUMENTS)\n",
                "$input | & $env:HARNESS_SCRIPT_LAUNCHER @forwarded\n",
                "exit $LASTEXITCODE\n"
            ),
        )
        .unwrap();
        let mut command = Command::new(desktop_pwsh());
        command
            .args(["-NoLogo", "-NoProfile", "-File"])
            .arg(&runner)
            .current_dir(&self.workspace)
            .env("CODEX_HOME", &self.home)
            .env("PATH", self.isolated_path(true))
            .env("HARNESS_SCRIPT_LAUNCHER", &self.launcher)
            .env(
                "HARNESS_SCRIPT_ARGUMENTS",
                serde_json::to_string(args).unwrap(),
            )
            .env_remove("HARNESS_LAUNCH_FIXTURE_MODE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(mode) = mode {
            command.env("HARNESS_LAUNCH_FIXTURE_MODE", mode);
        }
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

fn desktop_pwsh() -> PathBuf {
    if let Some(path) = env::var_os("HARNESS_ACCEPTANCE_POWERSHELL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return path;
        }
    }
    let path = PathBuf::from(DESKTOP_PWSH);
    assert!(
        path.is_file(),
        "desktop PowerShell is required at {DESKTOP_PWSH}"
    );
    path
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn looks_like_codex_dir(entry: &Path) -> bool {
    ["codex.exe", "codex.cmd", "codex.ps1", "codex.bat"]
        .iter()
        .any(|name| entry.join(name).is_file())
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_fallback_stderr(output: &Output) {
    let stderr = stderr_text(output);
    assert!(
        stderr.contains(UNAVAILABLE),
        "missing fallback notice: {stderr}"
    );
}

fn report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not fixture JSON ({error}): stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            stderr_text(output)
        )
    })
}

fn assert_original_launch(output: &Output, expected_args: &[&str]) {
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(output));
    let payload = report(output);
    assert_eq!(payload["args"], json!(expected_args));
    assert_eq!(payload["stdin"], "");
}

fn corrupt_module(path: &Path) {
    fs::write(path, "this is not a PowerShell module {").unwrap();
}

#[test]
fn missing_bridge_keeps_original_native_arguments() {
    let f = Fixture::new();
    f.mutate_registration(|metadata| {
        metadata.remove("configBridge");
    });
    let output = f.run(&["exec", "hello"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains(SHARED_DEFAULTS),
        "{}",
        stderr_text(&output)
    );
    assert_eq!(report(&output)["args"], json!(["exec", "hello"]));
}

#[test]
fn failing_bridge_keeps_original_native_arguments() {
    let f = Fixture::new();
    f.mutate_registration(|metadata| {
        metadata.insert(
            "configBridge".into(),
            json!(f.root.path().join("missing-bridge.exe")),
        );
    });
    let output = f.run(&["debug", "prompt-input"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains(SHARED_DEFAULTS),
        "{}",
        stderr_text(&output)
    );
    assert_eq!(report(&output)["args"], json!(["debug", "prompt-input"]));
}

#[test]
fn hung_bridge_falls_back_within_deadline() {
    let f = Fixture::new();
    let hung = env!("CARGO_BIN_EXE_harness-launch-fixture");
    let bridge_started = f.root.path().join("bridge-started.pid");
    f.mutate_registration(|metadata| {
        metadata.insert("configBridge".into(), json!(&hung));
    });
    let started = Instant::now();
    let output = f
        .command()
        .env("HARNESS_LAUNCH_FIXTURE_BRIDGE_MODE", "hang")
        .env("HARNESS_LAUNCH_FIXTURE_BRIDGE_STARTED", &bridge_started)
        .args(["exec", "after-timeout"])
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    assert!(
        bridge_started.is_file(),
        "native bridge did not reach its 20s hang"
    );
    assert!(
        elapsed >= Duration::from_secs(5),
        "bridge deadline was not waited: {elapsed:?} {}",
        stderr_text(&output)
    );
    assert!(
        elapsed < Duration::from_secs(12),
        "hung bridge exceeded the 5s+2s cleanup envelope: {elapsed:?} {}",
        stderr_text(&output)
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains(SHARED_DEFAULTS),
        "{}",
        stderr_text(&output)
    );
    assert_eq!(report(&output)["args"], json!(["exec", "after-timeout"]));
}

#[test]
fn missing_module_falls_back_to_stored_codex_command() {
    let f = Fixture::new();
    fs::remove_file(f.source.join("tools/launcher.psm1")).unwrap();
    let output = f.run(&["exec", "hello"]);
    assert_fallback_stderr(&output);
    assert_original_launch(&output, &["exec", "hello"]);
}

#[test]
fn corrupt_module_falls_back_to_stored_codex_command() {
    let f = Fixture::new();
    corrupt_module(&f.source.join("tools/launcher.psm1"));
    let output = f.run(&["exec", "hello"]);
    assert_fallback_stderr(&output);
    assert_original_launch(&output, &["exec", "hello"]);
}

#[test]
fn renamed_source_falls_back_to_stored_codex_command() {
    let f = Fixture::new();
    let moved = f.root.path().join("moved-source");
    fs::rename(&f.source, &moved).unwrap();
    let output = f.run(&["exec", "hello"]);
    assert_fallback_stderr(&output);
    assert_original_launch(&output, &["exec", "hello"]);
}

#[test]
fn missing_registration_uses_path_fixture() {
    let f = Fixture::new();
    fs::remove_file(f.home.join("harness/installation.json")).unwrap();
    let output = f.run(&["exec", "from-path"]);
    assert_fallback_stderr(&output);
    assert_original_launch(&output, &["exec", "from-path"]);
}

#[test]
fn malformed_registration_uses_path_fixture() {
    let f = Fixture::new();
    fs::write(f.home.join("harness/installation.json"), "{not json").unwrap();
    let output = f.run(&["exec", "from-path"]);
    assert_fallback_stderr(&output);
    assert_original_launch(&output, &["exec", "from-path"]);
}

#[test]
fn stored_self_command_is_skipped_for_path_fixture() {
    let f = Fixture::new();
    f.mutate_registration(|metadata| {
        metadata.insert("codexCommand".into(), json!(&f.launcher));
        metadata.remove("configBridge");
        metadata.insert("sourceRoot".into(), json!(f.root.path().join("gone")));
    });
    let output = f.run(&["exec", "skip-self"]);
    assert_fallback_stderr(&output);
    assert_original_launch(&output, &["exec", "skip-self"]);
}

#[test]
fn wrapper_scripts_are_skipped_when_searching_path() {
    let f = Fixture::new();
    fs::remove_file(f.home.join("harness/installation.json")).unwrap();
    fs::remove_file(&f.path_fixture).unwrap();
    let output = f.run(&["exec", "no-original"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains(NO_ORIGINAL),
        "{}",
        stderr_text(&output)
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn recursive_same_process_guard_refuses_before_launch() {
    let f = Fixture::new();
    let nested = f.root.path().join("nested.ps1");
    fs::write(
        &nested,
        concat!(
            "$global:CodexHarnessLauncherActive = $true\n",
            "$entry = $args[0]\n",
            "[string[]]$forwarded = @()\n",
            "if ($args.Count -gt 1) { $forwarded = [string[]]$args[1..($args.Count-1)] }\n",
            "& $entry @forwarded\n",
            "exit $LASTEXITCODE\n"
        ),
    )
    .unwrap();
    let output = Command::new(desktop_pwsh())
        .args(["-NoLogo", "-NoProfile", "-File"])
        .arg(&nested)
        .arg(&f.launcher)
        .arg("exec")
        .arg("nested")
        .current_dir(&f.workspace)
        .env("CODEX_HOME", &f.home)
        .env("PATH", f.isolated_path(true))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains(RECURSIVE),
        "{}",
        stderr_text(&output)
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn fallback_preserves_unicode_empty_quotes_and_end_of_options() {
    let f = Fixture::new();
    fs::remove_file(f.source.join("tools/launcher.psm1")).unwrap();
    let output = f.run(&TRICKY_ARGS);
    assert_fallback_stderr(&output);
    assert_original_launch(&output, &TRICKY_ARGS);
}

#[test]
fn fallback_preserves_stdin_and_nonzero_exit() {
    let f = Fixture::new();
    fs::remove_file(f.source.join("tools/launcher.psm1")).unwrap();
    let output = f.run_with_stdin(&TRICKY_ARGS, STDIN, Some("nonzero"));
    assert_eq!(output.status.code(), Some(19), "{}", stderr_text(&output));
    assert_fallback_stderr(&output);
    assert!(
        stderr_text(&output).contains("upstream stderr"),
        "{}",
        stderr_text(&output)
    );
    let payload = report(&output);
    assert_eq!(payload["args"], json!(TRICKY_ARGS));
    let stdin = payload["stdin"].as_str().unwrap();
    assert_eq!(stdin.replace("\r\n", "\n"), STDIN);
    assert!(
        stdin.contains("первая строка"),
        "unicode stdin was lost: {stdin:?}"
    );
}

#[test]
fn fallback_does_not_retry_after_original_process_starts() {
    let f = Fixture::new();
    fs::remove_file(f.source.join("tools/launcher.psm1")).unwrap();
    let started = f.root.path().join("started.pid");
    let output = f
        .command()
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &started)
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
        .args(["exec", "once"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(19), "{}", stderr_text(&output));
    assert_fallback_stderr(&output);
    let pid: u32 = fs::read_to_string(&started)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(pid > 0);
    assert_eq!(report(&output)["args"], json!(["exec", "once"]));
}

#[test]
#[ignore = "explicit CODEX_P0_LAUNCHER, CODEX_P0_HOME and CODEX_P0_CWD; no prompt or model call"]
fn actual_global_cli_tui_smoke_ready_then_quit() {
    let launcher = env_path("CODEX_P0_LAUNCHER");
    let home = env_path("CODEX_P0_HOME");
    let cwd = env_path("CODEX_P0_CWD");
    assert!(launcher.is_file(), "CODEX_P0_LAUNCHER is not a file");
    assert!(home.is_dir(), "CODEX_P0_HOME is not a directory");
    assert!(cwd.is_dir(), "CODEX_P0_CWD is not a directory");
    let evidence = env::temp_dir().join(format!(
        "codex-p0-tui-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::create_dir_all(&evidence).unwrap();
    let mut command = CommandSpec::new(desktop_pwsh());
    command.args = vec![
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-File".into(),
        launcher.into_os_string(),
        "--no-alt-screen".into(),
    ];
    command.current_dir = Some(cwd);
    command
        .env
        .insert("CODEX_HOME".into(), Some(home.into_os_string()));
    let child_path = env::split_paths(&env::var_os("PATH").unwrap())
        .filter(|entry| {
            !entry
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("windowsapps")
        })
        .collect::<Vec<_>>();
    command
        .env
        .insert("PATH".into(), Some(env::join_paths(child_path).unwrap()));
    let mut spec = ConsoleSpec::new(command);
    // The activated global home serves four managed MCP frontends alongside
    // the TUI itself. The prior 512 MiB cap rejected that ordinary set while
    // shutting down; keep a bounded runaway check without failing delivery.
    spec.limits.memory_bytes = Some(1024 * 1024 * 1024);
    let session = ConsoleSession::spawn(spec).unwrap();
    wait_ready(&session, &evidence, 40);
    send_line(&session, "/quit").unwrap();
    let result = session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    fs::write(evidence.join("terminal.txt"), &result.transcript).unwrap();
    assert_eq!(result.outcome.reason, StopReason::Exited);
    assert_eq!(result.outcome.exit_code, 0);
}

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(env::var_os(name).unwrap_or_else(|| panic!("{name} is required")))
}

fn send_line(session: &ConsoleSession, text: &str) -> io::Result<()> {
    session.send(text)?;
    std::thread::sleep(Duration::from_millis(250));
    session.send("\r")
}

fn wait_ready(session: &ConsoleSession, evidence: &Path, seconds: u64) {
    let until = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < until {
        let transcript = session.transcript();
        let _ = fs::write(evidence.join("terminal.txt"), &transcript);
        if ["OpenAI Codex", "Welcome to Codex", "gpt-6-astra", "Sign in"]
            .iter()
            .any(|needle| transcript.contains(needle))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!(
        "global CLI TUI was not ready; private transcript {}",
        evidence.join("terminal.txt").display()
    );
}
