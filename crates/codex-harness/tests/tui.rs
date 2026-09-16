//! Native Windows TUI smoke. Model-free; owned isolated homes only.
//!
//! Default tests prove ordinary `codex` resolution through an isolated PATH.
//! The actual Codex TUI /status /model /trust oracles stay opt-in:
//! `cargo test --locked -p codex-harness --test tui -- --ignored --test-threads=1 --nocapture`
#![cfg(windows)]

use harness_core::{
    console::{ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline, StopReason},
};
use serde_json::{Value, json};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const DESKTOP_PWSH: &str = r"C:\Program Files\PowerShell\7\pwsh.exe";

fn checkout() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
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

fn child_path(prefix: &Path) -> std::ffi::OsString {
    let path = env::var_os("PATH").unwrap();
    let rest = env::split_paths(&path).filter(|entry| {
        !entry
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("windowsapps")
    });
    let mut entries = vec![prefix.to_path_buf()];
    entries.extend(rest);
    env::join_paths(entries).unwrap()
}

fn visible(transcript: &str) -> String {
    let bytes = transcript.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'[' => {
                    i += 2;
                    while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    i += usize::from(i < bytes.len());
                    continue;
                }
                b']' => {
                    i += 2;
                    while i < bytes.len() && bytes[i] != 0x07 {
                        i += 1;
                    }
                    i += usize::from(i < bytes.len());
                    continue;
                }
                _ => {}
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn matches_any(text: &str, patterns: &[&str]) -> bool {
    let haystack = visible(text);
    patterns.iter().any(|pattern| haystack.contains(pattern))
}

fn send_line(session: &ConsoleSession, text: &str) -> io::Result<()> {
    session.send(text)?;
    std::thread::sleep(Duration::from_millis(300));
    session.send("\r")
}

fn wait_for(session: &ConsoleSession, evidence: &Path, seconds: u64, patterns: &[&str]) {
    let until = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < until {
        let transcript = session.transcript();
        let _ = fs::write(evidence.join("terminal.txt"), &transcript);
        if matches_any(&transcript, patterns) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "TUI timed out waiting for {patterns:?}; private transcript {}",
        evidence.join("terminal.txt").display()
    );
}

fn hash_file(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_default()
}

fn host_codex_home() -> PathBuf {
    env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap()
                .join(".codex")
        })
}

fn optional_host_codex_command() -> Option<PathBuf> {
    if let Some(path) = env::var_os("HARNESS_TUI_CODEX") {
        return Some(PathBuf::from(path));
    }
    let registration = host_codex_home().join("harness/installation.json");
    let bytes = fs::read(&registration).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value["codexCommand"].as_str().map(PathBuf::from)
}

struct IsolatedHome {
    root: PathBuf,
    home: PathBuf,
    bin: PathBuf,
    workspace: PathBuf,
    launcher: PathBuf,
    shared: PathBuf,
    shared_bytes: Vec<u8>,
}

impl IsolatedHome {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("codex-tui проверка-")
            .tempdir()
            .unwrap()
            .keep();
        let home = root.join("codex home");
        let bin = home.join("harness/bin");
        let workspace = root.join("neutral workspace");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        let source = checkout();
        let launcher = bin.join("codex.ps1");
        std::os::windows::fs::symlink_file(source.join("tools/codex.ps1"), &launcher).unwrap();
        std::os::windows::fs::symlink_file(
            source.join("global/principles-of-work.md"),
            home.join("AGENTS.md"),
        )
        .unwrap();
        let host = host_codex_home();
        if host.join("auth.json").is_file() {
            std::os::windows::fs::symlink_file(host.join("auth.json"), home.join("auth.json"))
                .unwrap();
        }
        let bridge = env::var_os("HARNESS_TUI_BRIDGE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_codex-harness")));
        fs::write(
            home.join("harness/installation.json"),
            serde_json::to_vec_pretty(&json!({
                "schemaVersion": 1,
                "sourceRoot": source,
                "codexCommand": optional_host_codex_command()
                    .unwrap_or_else(|| source.join("tools/codex.ps1")),
                "profileName": "harness",
                "configBridge": bridge,
            }))
            .unwrap(),
        )
        .unwrap();
        let workspace_key =
            serde_json::to_string(&workspace.to_string_lossy().into_owned()).unwrap();
        fs::write(
            home.join("config.toml"),
            format!(
                "model = \"gpt-6-astra\"\nmodel_reasoning_effort = \"low\"\n[projects.{workspace_key}]\ntrust_level = \"trusted\"\n"
            ),
        )
        .unwrap();
        let shared = source.join("global/harness.config.toml");
        Self {
            root,
            home,
            bin,
            workspace,
            launcher,
            shared: shared.clone(),
            shared_bytes: hash_file(&shared),
        }
    }

    fn assert_shared_unchanged(&self) {
        assert_eq!(
            hash_file(&self.shared),
            self.shared_bytes,
            "native TUI modified shared source"
        );
    }
}

impl Drop for IsolatedHome {
    fn drop(&mut self) {
        if env::var_os("HARNESS_TUI_KEEP").is_some() {
            return;
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn pwsh_file(script: &Path, home: &Path, bin: &Path, cwd: &Path) -> CommandSpec {
    let mut command = CommandSpec::new(desktop_pwsh());
    command.args = vec![
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-File".into(),
        script.as_os_str().to_owned(),
    ];
    command.current_dir = Some(cwd.to_path_buf());
    command
        .env
        .insert("CODEX_HOME".into(), Some(home.as_os_str().to_owned()));
    command.env.insert("PATH".into(), Some(child_path(bin)));
    command
}

#[test]
fn isolated_path_resolves_the_linked_ordinary_codex_command() {
    let home = IsolatedHome::new();
    let entry = home.root.join("entry.ps1");
    fs::write(
        &entry,
        "Write-Output ('HARNESS_ENTRY=' + (Get-Command codex).Source)\nexit 0\n",
    )
    .unwrap();
    let output = Command::new(desktop_pwsh())
        .args(["-NoLogo", "-NoProfile", "-File"])
        .arg(&entry)
        .current_dir(&home.workspace)
        .env("CODEX_HOME", &home.home)
        .env("PATH", child_path(&home.bin))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}{}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
    let reported = stdout
        .lines()
        .find(|line| line.starts_with("HARNESS_ENTRY="))
        .map(|line| line.trim_start_matches("HARNESS_ENTRY="))
        .expect("HARNESS_ENTRY");
    assert_eq!(
        PathBuf::from(reported).canonicalize().unwrap(),
        home.launcher.canonicalize().unwrap(),
        "{stdout}"
    );
}

#[test]
#[ignore = "explicit owned Codex/auth and native ConPTY; no model prompt"]
fn actual_tui_status_model_and_trust_write_locally() {
    let fixture = IsolatedHome::new();
    assert!(
        fixture.home.join("auth.json").is_file(),
        "link host auth.json into the owned TUI home"
    );
    let command =
        optional_host_codex_command().expect("HARNESS_TUI_CODEX or host installation.json");
    let command_text = command.to_string_lossy().to_ascii_lowercase();
    let launcher_text = fixture.launcher.to_string_lossy().to_ascii_lowercase();
    assert_ne!(
        command_text, launcher_text,
        "actual TUI needs the original Codex command, not this launcher"
    );
    let evidence = fixture.root.clone();
    let entry = fixture.root.join("entry.ps1");
    fs::write(
        &entry,
        "Write-Output ('HARNESS_ENTRY=' + (Get-Command codex).Source)\ncodex --no-alt-screen\nexit $LASTEXITCODE\n",
    )
    .unwrap();
    let mut spec = ConsoleSpec::new(pwsh_file(
        &entry,
        &fixture.home,
        &fixture.bin,
        &fixture.workspace,
    ));
    spec.limits.memory_bytes = Some(1024 * 1024 * 1024);
    let session = ConsoleSession::spawn(spec).unwrap();
    wait_for(
        &session,
        &evidence,
        30,
        &["OpenAI Codex", "Welcome to Codex", "Sign in", "gpt-6-astra"],
    );
    let transcript = session.transcript();
    assert!(
        visible(&transcript).contains(&format!("HARNESS_ENTRY={}", fixture.launcher.display())),
        "{}",
        visible(&transcript)
    );
    wait_for(&session, &evidence, 20, &["gpt-6-astra low"]);
    send_line(&session, "/status").unwrap();
    wait_for(
        &session,
        &evidence,
        20,
        &[
            "Context window",
            "Approval",
            "Full Access",
            "Session ID",
            "Session:",
        ],
    );
    send_line(&session, "/quit").unwrap();
    let result = session
        .wait(
            Deadline::after(Duration::from_secs(15)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    fs::write(evidence.join("terminal.txt"), &result.transcript).unwrap();
    assert_eq!(result.outcome.reason, StopReason::Exited);
    assert_eq!(result.outcome.exit_code, 0);
    fixture.assert_shared_unchanged();

    let local = fixture.home.join("config.toml");
    fs::write(
        &local,
        fs::read_to_string(&local).unwrap().replace(
            "model_reasoning_effort = \"low\"",
            "model_reasoning_effort = \"xhigh\"",
        ),
    )
    .unwrap();
    let mut spec = ConsoleSpec::new(pwsh_file(
        &entry,
        &fixture.home,
        &fixture.bin,
        &fixture.workspace,
    ));
    spec.limits.memory_bytes = Some(1024 * 1024 * 1024);
    let session = ConsoleSession::spawn(spec).unwrap();
    wait_for(&session, &evidence, 30, &["gpt-6-astra xhigh"]);
    send_line(&session, "/model").unwrap();
    wait_for(
        &session,
        &evidence,
        15,
        &["Select model", "Select Model", "Choose model"],
    );
    send_line(&session, "").unwrap();
    wait_for(
        &session,
        &evidence,
        15,
        &["Select reasoning", "reasoning effort", "Reasoning Effort"],
    );
    session.send("\u{1b}[A").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    session.send("\r").unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until
        && !fs::read_to_string(&local)
            .unwrap()
            .contains("model_reasoning_effort = \"high\"")
    {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        fs::read_to_string(&local)
            .unwrap()
            .contains("model_reasoning_effort = \"high\""),
        "native /model did not persist the selected effort locally"
    );
    fixture.assert_shared_unchanged();
    send_line(&session, "/quit").unwrap();
    let result = session
        .wait(
            Deadline::after(Duration::from_secs(15)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(result.outcome.reason, StopReason::Exited);
    assert_eq!(result.outcome.exit_code, 0);

    let trust = fixture.root.join("untrusted git workspace");
    fs::create_dir_all(&trust).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&trust)
            .status()
            .unwrap()
            .success()
    );
    let before = fs::read_to_string(&local).unwrap();
    let trust_entry = fixture.root.join("trust-entry.ps1");
    fs::write(
        &trust_entry,
        "codex --no-alt-screen -c 'windows.sandbox=\"unelevated\"'\nexit $LASTEXITCODE\n",
    )
    .unwrap();
    let mut spec = ConsoleSpec::new(pwsh_file(&trust_entry, &fixture.home, &fixture.bin, &trust));
    spec.limits.memory_bytes = Some(1024 * 1024 * 1024);
    let session = ConsoleSession::spawn(spec).unwrap();
    wait_for(
        &session,
        &evidence,
        30,
        &["Do you trust the contents", "gpt-6-astra high"],
    );
    let shown = visible(&session.transcript());
    assert!(
        shown.contains("Do you trust the contents"),
        "trust fixture did not exercise the native trust writer"
    );
    assert!(
        !shown
            .to_ascii_lowercase()
            .contains("continue and create a sandbox"),
        "trust fixture would create a Windows sandbox"
    );
    send_line(&session, "").unwrap();
    wait_for(&session, &evidence, 30, &["gpt-6-astra high"]);
    let after = fs::read_to_string(&local).unwrap();
    assert_ne!(after, before);
    assert!(after.contains("untrusted git workspace"));
    assert!(after.contains("trust_level = \"trusted\"") || after.contains("trust_level='trusted'"));
    fixture.assert_shared_unchanged();
    send_line(&session, "/quit").unwrap();
    let result = session
        .wait(
            Deadline::after(Duration::from_secs(15)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(result.outcome.reason, StopReason::Exited);
    assert_eq!(result.outcome.exit_code, 0);
}
