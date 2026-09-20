//! Live hooks-off matching-task apply. Gated: spends a real model.
#![cfg(windows)]
use harness_core::{
    build_identity,
    console::{ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline},
};
use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const EXPECTED_SHA: &str = "e4c11374bd9de8ad5c3b7617fd4654bb7839901edb0863f9930666863c7a021b";
const EXPECTED_LEN: u64 = 307_150_128;
const MARKER_V1: &str = "SKILL_NATURAL_APPLY_MARKER_e7a1";
const MARKER_V2: &str = "SKILL_NATURAL_APPLY_MARKER_v2";
const MATCHING_PROMPT: &str =
    "Record the natural-apply probe token for this workspace. Do not mention a skill name.";
const UNRELATED_PROMPT: &str =
    "Summarize README.md in one short sentence. Do not create applied.txt.";

fn live_codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex"))
}

fn native_launcher() -> PathBuf {
    let requested = PathBuf::from(std::env::var_os("HARNESS_NATIVE_CODEX").unwrap());
    let registration = live_codex_home().join("harness/native-launch.json");
    if let Ok(bytes) = fs::read(&registration)
        && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
        && let Some(path) = value["upstream"]["executable"].as_str()
    {
        let upstream = PathBuf::from(path);
        if upstream.is_file() {
            return upstream;
        }
    }
    requested
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

fn json_escape(path: &Path) -> String {
    serde_json::to_string(&path.to_str().unwrap().to_lowercase()).unwrap()
}

fn filtered_path() -> std::ffi::OsString {
    let child_path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .filter(|entry| {
            !entry.components().any(|part| {
                part.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("WindowsApps")
            })
        })
        .collect::<Vec<_>>();
    std::env::join_paths(child_path).unwrap()
}

fn send_line(session: &ConsoleSession, text: &str) -> io::Result<()> {
    session.send(text)?;
    std::thread::sleep(Duration::from_millis(250));
    session.send("\r")
}

fn wait_applied(path: &Path, marker: &str, session: &ConsoleSession, evidence: &Path) -> bool {
    let until = Instant::now() + Duration::from_secs(180);
    while Instant::now() < until {
        let _ = fs::write(evidence.join("terminal.txt"), session.transcript());
        if path.is_file() && fs::read_to_string(path).unwrap().trim() == marker {
            return true;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    let _ = fs::write(evidence.join("terminal.txt"), session.transcript());
    false
}

fn write_skill(path: &Path, marker: &str) {
    fs::create_dir_all(path).unwrap();
    fs::write(
        path.join("SKILL.md"),
        format!(
            "---\nname: natural-apply-probe\ndescription: Record the natural-apply probe token into applied.txt when the user asks to record that probe token. Do not use for unrelated coding or documentation.\n---\nEach use: read this skill's SKILL.md from disk and use only the marker in that file. If that file is missing, do not write applied.txt. When the user asks to record the natural-apply probe token, write applied.txt containing exactly {marker} and no other text. Do not mention this skill name. Do not ask questions.\n"
        ),
    )
    .unwrap();
}

fn wait_idle(session: &ConsoleSession, evidence: &Path) {
    let until = Instant::now() + Duration::from_secs(45);
    let mut quiet = 0u32;
    while Instant::now() < until {
        let _ = fs::write(evidence.join("terminal.txt"), session.transcript());
        let text = session.transcript();
        let tail = if text.len() > 4000 {
            let mut start = text.len() - 4000;
            while start < text.len() && !text.is_char_boundary(start) {
                start += 1;
            }
            &text[start..]
        } else {
            text.as_str()
        };
        if tail.contains("esc to interrupt") {
            quiet = 0;
        } else {
            quiet += 1;
            if quiet >= 8 {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn exec_prompt(exe: &Path, home: &Path, workspace: &Path, prompt: &str) -> i32 {
    let mut child = Command::new(exe)
        .args([
            "exec",
            "--skip-git-repo-check",
            "--sandbox",
            "danger-full-access",
            "--disable",
            "hooks",
            "--",
            prompt,
        ])
        .env("CODEX_HOME", home)
        .current_dir(workspace)
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(180);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code().unwrap_or(1);
        }
        if Instant::now() > until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("codex exec timed out");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn matching_prompt_does_not_embed_the_body_marker() {
    assert!(!MATCHING_PROMPT.contains(MARKER_V1));
    assert!(!MATCHING_PROMPT.contains(MARKER_V2));
    assert!(!UNRELATED_PROMPT.contains(MARKER_V1));
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LIVE_APPLY=1; live grok-4.6 via xAI shim"]
fn live_exec_matching_task_applies_and_updates_without_dollar_skill() {
    assert_eq!(
        std::env::var("HARNESS_SKILL_LIVE_APPLY").as_deref(),
        Ok("1"),
        "refusing to spend a live model without HARNESS_SKILL_LIVE_APPLY=1"
    );
    std::env::var_os("HARNESS_NATIVE_CODEX").expect("HARNESS_NATIVE_CODEX required");
    let exe = native_launcher();
    assert!(exe.is_file());
    assert_eq!(build_identity::hash_file(&exe).unwrap(), EXPECTED_SHA);
    assert_eq!(fs::metadata(&exe).unwrap().len(), EXPECTED_LEN);

    let local =
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("codex-harness-probes");
    fs::create_dir_all(&local).unwrap();
    let evidence = tempfile::Builder::new()
        .prefix("skill-natural-apply-")
        .tempdir_in(&local)
        .unwrap();
    let home = evidence.path().join("home");
    let workspace = evidence.path().join("workspace");
    let skill = home.join("skills/natural-apply-probe");
    fs::create_dir_all(&workspace).unwrap();
    write_skill(&skill, MARKER_V1);
    let manager = live_codex_home().join("harness/bin/codex-harness.exe");
    let live = live_codex_home();
    let trusted = json_escape(&workspace);
    fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "grok-4.6"
model_provider = "xai"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"

[model_providers.xai]
name = "xAI"
base_url = "http://127.0.0.1:56122/v1"
wire_api = "responses"

[model_providers.xai.auth]
command = "{}"
args = ["xai-token", "--codex-home", "{}"]
timeout_ms = 15000

[windows]
sandbox = "unelevated"

[features]
hooks = false
apps = false
multi_agent = false
memories = false
goals = false
plugins = false
skill_search = true
code_mode = false

[projects.{trusted}]
trust_level = "trusted"
"#,
            toml_path(&manager),
            toml_path(&live)
        ),
    )
    .unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&workspace)
            .status()
            .unwrap()
            .success()
    );
    fs::write(workspace.join("README.md"), "natural apply workspace\n").unwrap();

    let applied = workspace.join("applied.txt");
    assert_eq!(exec_prompt(&exe, &home, &workspace, MATCHING_PROMPT), 0);
    assert_eq!(fs::read_to_string(&applied).unwrap().trim(), MARKER_V1);

    fs::remove_file(&applied).ok();
    assert_eq!(exec_prompt(&exe, &home, &workspace, UNRELATED_PROMPT), 0);
    assert!(
        !applied.exists(),
        "unrelated prompt must not apply the skill body"
    );

    write_skill(&skill, MARKER_V2);
    fs::remove_file(&applied).ok();
    assert_eq!(exec_prompt(&exe, &home, &workspace, MATCHING_PROMPT), 0);
    assert_eq!(fs::read_to_string(&applied).unwrap().trim(), MARKER_V2);
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LIVE_APPLY=1; live grok-4.6 TUI via xAI shim"]
fn live_tui_matching_task_applies_without_dollar_skill() {
    assert_eq!(
        std::env::var("HARNESS_SKILL_LIVE_APPLY").as_deref(),
        Ok("1"),
        "refusing to spend a live model without HARNESS_SKILL_LIVE_APPLY=1"
    );
    std::env::var_os("HARNESS_NATIVE_CODEX").expect("HARNESS_NATIVE_CODEX required");
    let exe = native_launcher();
    assert!(exe.is_file());
    assert_eq!(build_identity::hash_file(&exe).unwrap(), EXPECTED_SHA);
    assert_eq!(fs::metadata(&exe).unwrap().len(), EXPECTED_LEN);

    let local =
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("codex-harness-probes");
    fs::create_dir_all(&local).unwrap();
    let evidence = tempfile::Builder::new()
        .prefix("skill-natural-tui-")
        .tempdir_in(&local)
        .unwrap()
        .keep();
    println!("skill natural tui evidence: {}", evidence.display());
    let home = evidence.join("home");
    let workspace = evidence.join("workspace");
    let skill = home.join("skills/natural-apply-probe");
    fs::create_dir_all(&workspace).unwrap();
    write_skill(&skill, MARKER_V1);
    let manager = live_codex_home().join("harness/bin/codex-harness.exe");
    let live = live_codex_home();
    let trusted = json_escape(&workspace);
    fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "grok-4.6"
model_provider = "xai"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"

[model_providers.xai]
name = "xAI"
base_url = "http://127.0.0.1:56122/v1"
wire_api = "responses"

[model_providers.xai.auth]
command = "{}"
args = ["xai-token", "--codex-home", "{}"]
timeout_ms = 15000

[windows]
sandbox = "unelevated"

[features]
hooks = false
apps = false
multi_agent = false
memories = false
goals = false
plugins = false
skill_search = true
code_mode = false

[projects.{trusted}]
trust_level = "trusted"
"#,
            toml_path(&manager),
            toml_path(&live)
        ),
    )
    .unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&workspace)
            .status()
            .unwrap()
            .success()
    );
    fs::write(workspace.join("README.md"), "natural apply workspace\n").unwrap();

    let mut command = CommandSpec::new(&exe);
    command.args = vec!["--no-alt-screen".into()];
    command.current_dir = Some(workspace.clone());
    command
        .env
        .insert("CODEX_HOME".into(), Some(home.clone().into_os_string()));
    command.env.insert("PATH".into(), Some(filtered_path()));
    command.env.insert("OPENAI_API_KEY".into(), None);
    command.env.insert("CODEX_API_KEY".into(), None);
    let mut spec = ConsoleSpec::new(command);
    spec.limits.memory_bytes = Some(512 * 1024 * 1024);
    spec.limits.cpu_percent = Some(50.0);
    let session = ConsoleSession::spawn(spec).unwrap();
    let ready = {
        let until = Instant::now() + Duration::from_secs(60);
        let mut ok = false;
        while Instant::now() < until {
            let _ = fs::write(evidence.join("terminal.txt"), session.transcript());
            let text = session.transcript();
            if text.contains("Ask Codex to do anything") && !text.contains("Do you trust") {
                ok = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        ok
    };
    assert!(
        ready,
        "live TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(3));
    send_line(&session, MATCHING_PROMPT).unwrap();
    let applied = workspace.join("applied.txt");
    let written = wait_applied(&applied, MARKER_V1, &session, &evidence);
    assert!(
        written,
        "matching TUI task did not apply live skill body; evidence {}",
        evidence.display()
    );
    wait_idle(&session, &evidence);
    write_skill(&skill, MARKER_V2);
    fs::remove_file(&applied).ok();
    send_line(&session, MATCHING_PROMPT).unwrap();
    let updated = wait_applied(&applied, MARKER_V2, &session, &evidence);
    assert!(
        updated,
        "updated TUI skill body was not applied; evidence {}",
        evidence.display()
    );
    wait_idle(&session, &evidence);
    fs::remove_file(&applied).ok();
    send_line(&session, UNRELATED_PROMPT).unwrap();
    let unrelated_until = Instant::now() + Duration::from_secs(90);
    while Instant::now() < unrelated_until {
        let _ = fs::write(evidence.join("terminal.txt"), session.transcript());
        assert!(
            !applied.exists(),
            "unrelated TUI prompt applied the skill body; evidence {}",
            evidence.display()
        );
        std::thread::sleep(Duration::from_millis(400));
    }
    let _ = send_line(&session, "/quit");
    let _ = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
}

fn enable_auto_compact(home: &Path) {
    let path = home.join("config.toml");
    let mut cfg = fs::read_to_string(&path).unwrap();
    cfg.push_str("\nmodel_auto_compact_token_limit = 800\nmodel_context_window = 4000\n");
    fs::write(path, cfg).unwrap();
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LIVE_APPLY=1; live grok-4.6 TUI via xAI shim"]
fn live_tui_auto_compact_then_matching_applies() {
    let exe = require_live_apply();
    let (evidence, home, workspace, skill) = prepare_tui_home("skill-natural-auto-ctx-", MARKER_V1);
    fs::remove_dir_all(&skill).unwrap();
    enable_auto_compact(&home);
    let session = spawn_live_tui(&exe, &home, &workspace);
    assert!(
        wait_tui_ready(&session, &evidence),
        "live TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(3));
    send_line(
        &session,
        &format!(
            "Reply with exactly one word. Do not create applied.txt.\n{}",
            "alpha ".repeat(3000)
        ),
    )
    .unwrap();
    let until = Instant::now() + Duration::from_secs(90);
    let mut compacted = false;
    while Instant::now() < until {
        let _ = fs::write(evidence.join("terminal.txt"), session.transcript());
        let text = session.transcript().to_lowercase();
        if text.contains("compacting") || text.contains("compacted context") {
            compacted = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    wait_idle(&session, &evidence);
    write_skill(&skill, MARKER_V1);
    send_line(&session, MATCHING_PROMPT).unwrap();
    let applied = workspace.join("applied.txt");
    let written = wait_applied(&applied, MARKER_V1, &session, &evidence);
    let _ = send_line(&session, "/quit");
    let _ = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
    fs::write(
        evidence.join("acceptance.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "compact_observed": compacted,
            "applied": written,
        }))
        .unwrap(),
    )
    .unwrap();
    println!("compact_observed={compacted} applied={written}");
    assert!(
        compacted,
        "automatic compact UI was not observed; evidence {}",
        evidence.display()
    );
    assert!(
        written,
        "matching task after auto-compact setup did not apply; evidence {}",
        evidence.display()
    );
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LIVE_APPLY=1; live grok-4.6 TUI via xAI shim"]
fn live_tui_rollback_applies_restored_revision() {
    let exe = require_live_apply();
    let (evidence, home, workspace, skill) = prepare_tui_home("skill-natural-rollback-", MARKER_V2);
    let session = spawn_live_tui(&exe, &home, &workspace);
    assert!(
        wait_tui_ready(&session, &evidence),
        "live TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(3));
    send_line(&session, MATCHING_PROMPT).unwrap();
    let applied = workspace.join("applied.txt");
    assert!(
        wait_applied(&applied, MARKER_V2, &session, &evidence),
        "candidate revision was not applied before rollback; evidence {}",
        evidence.display()
    );
    wait_idle(&session, &evidence);
    write_skill(&skill, MARKER_V1);
    fs::remove_file(&applied).ok();
    send_line(&session, MATCHING_PROMPT).unwrap();
    let restored = wait_applied(&applied, MARKER_V1, &session, &evidence);
    let _ = send_line(&session, "/quit");
    let _ = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
    assert!(
        restored,
        "rolled-back skill body was not applied; evidence {}",
        evidence.display()
    );
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LIVE_APPLY=1; live grok-4.6 TUI via xAI shim"]
fn live_tui_same_turn_applies_skill_added_during_turn() {
    let exe = require_live_apply();
    let (evidence, home, workspace, skill) = prepare_tui_home("skill-natural-sameturn-", MARKER_V1);
    fs::remove_dir_all(&skill).unwrap();
    let session = spawn_live_tui(&exe, &home, &workspace);
    assert!(
        wait_tui_ready(&session, &evidence),
        "live TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(3));
    send_line(&session, MATCHING_PROMPT).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    write_skill(&skill, MARKER_V1);
    let applied = workspace.join("applied.txt");
    let written = wait_applied(&applied, MARKER_V1, &session, &evidence);
    let _ = send_line(&session, "/quit");
    let _ = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
    assert!(
        written,
        "same-turn add was not applied before a later user prompt; evidence {}",
        evidence.display()
    );
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LIVE_APPLY=1; live grok-4.6 TUI via xAI shim"]
fn live_tui_same_turn_delete_identity_is_incomplete() {
    let exe = require_live_apply();
    let (evidence, home, workspace, skill) =
        prepare_tui_home("skill-natural-sameturn-retire-", MARKER_V1);
    let session = spawn_live_tui(&exe, &home, &workspace);
    assert!(
        wait_tui_ready(&session, &evidence),
        "live TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(3));
    send_line(&session, MATCHING_PROMPT).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    fs::remove_dir_all(&skill).unwrap();
    let applied = workspace.join("applied.txt");
    let until = Instant::now() + Duration::from_secs(90);
    let mut stale_apply = false;
    while Instant::now() < until {
        let _ = fs::write(evidence.join("terminal.txt"), session.transcript());
        if applied.is_file() {
            let body = fs::read_to_string(&applied).unwrap();
            if body.trim() == MARKER_V1 || body.trim() == MARKER_V2 {
                stale_apply = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    let identity = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&skill)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&identity.stdout);
    fs::write(
        evidence.join("acceptance.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "stale_same_turn_apply": stale_apply,
            "identity_status": identity.status.code(),
            "identity_delivery_complete": report.contains("\"delivery_complete\": true"),
        }))
        .unwrap(),
    )
    .unwrap();
    println!("stale_same_turn_apply={stale_apply}");
    assert!(
        !identity.status.success() || !report.contains("\"delivery_complete\": true"),
        "identity must not report delivery_complete after same-turn delete: {report}"
    );
    assert!(
        !stale_apply,
        "same-turn delete still applied a cached marker; evidence {}",
        evidence.display()
    );
    let _ = send_line(&session, "/quit");
    let _ = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
}

fn require_live_apply() -> PathBuf {
    assert_eq!(
        std::env::var("HARNESS_SKILL_LIVE_APPLY").as_deref(),
        Ok("1"),
        "refusing to spend a live model without HARNESS_SKILL_LIVE_APPLY=1"
    );
    std::env::var_os("HARNESS_NATIVE_CODEX").expect("HARNESS_NATIVE_CODEX required");
    let exe = native_launcher();
    assert!(exe.is_file());
    assert_eq!(build_identity::hash_file(&exe).unwrap(), EXPECTED_SHA);
    assert_eq!(fs::metadata(&exe).unwrap().len(), EXPECTED_LEN);
    exe
}

fn prepare_tui_home(prefix: &str, marker: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let local =
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("codex-harness-probes");
    fs::create_dir_all(&local).unwrap();
    let evidence = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(&local)
        .unwrap()
        .keep();
    println!("skill natural tui evidence: {}", evidence.display());
    let home = evidence.join("home");
    let workspace = evidence.join("workspace");
    let skill = home.join("skills/natural-apply-probe");
    fs::create_dir_all(&workspace).unwrap();
    write_skill(&skill, marker);
    let manager = live_codex_home().join("harness/bin/codex-harness.exe");
    let live = live_codex_home();
    let trusted = json_escape(&workspace);
    fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "grok-4.6"
model_provider = "xai"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"

[model_providers.xai]
name = "xAI"
base_url = "http://127.0.0.1:56122/v1"
wire_api = "responses"

[model_providers.xai.auth]
command = "{}"
args = ["xai-token", "--codex-home", "{}"]
timeout_ms = 15000

[windows]
sandbox = "unelevated"

[features]
hooks = false
apps = false
multi_agent = false
memories = false
goals = false
plugins = false
skill_search = true
code_mode = false

[projects.{trusted}]
trust_level = "trusted"
"#,
            toml_path(&manager),
            toml_path(&live)
        ),
    )
    .unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&workspace)
            .status()
            .unwrap()
            .success()
    );
    fs::write(workspace.join("README.md"), "natural apply workspace\n").unwrap();
    (evidence, home, workspace, skill)
}

fn spawn_live_tui(exe: &Path, home: &Path, workspace: &Path) -> ConsoleSession {
    let mut command = CommandSpec::new(exe);
    command.args = vec!["--no-alt-screen".into()];
    command.current_dir = Some(workspace.to_path_buf());
    command.env.insert(
        "CODEX_HOME".into(),
        Some(home.to_path_buf().into_os_string()),
    );
    command.env.insert("PATH".into(), Some(filtered_path()));
    command.env.insert("OPENAI_API_KEY".into(), None);
    command.env.insert("CODEX_API_KEY".into(), None);
    let mut spec = ConsoleSpec::new(command);
    spec.limits.memory_bytes = Some(512 * 1024 * 1024);
    spec.limits.cpu_percent = Some(50.0);
    ConsoleSession::spawn(spec).unwrap()
}

fn wait_tui_ready(session: &ConsoleSession, evidence: &Path) -> bool {
    let until = Instant::now() + Duration::from_secs(60);
    while Instant::now() < until {
        let _ = fs::write(evidence.join("terminal.txt"), session.transcript());
        let text = session.transcript();
        if text.contains("Ask Codex to do anything") && !text.contains("Do you trust") {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LIVE_APPLY=1; live grok-4.6 TUI via xAI shim"]
fn live_tui_retired_skill_is_not_applied() {
    let exe = require_live_apply();
    let (evidence, home, workspace, skill) = prepare_tui_home("skill-natural-retire-", MARKER_V1);
    let session = spawn_live_tui(&exe, &home, &workspace);
    assert!(
        wait_tui_ready(&session, &evidence),
        "live TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(3));
    send_line(&session, MATCHING_PROMPT).unwrap();
    let applied = workspace.join("applied.txt");
    assert!(
        wait_applied(&applied, MARKER_V1, &session, &evidence),
        "matching TUI task did not apply before retirement; evidence {}",
        evidence.display()
    );
    wait_idle(&session, &evidence);
    fs::remove_dir_all(&skill).unwrap();
    fs::remove_file(&applied).ok();
    send_line(&session, MATCHING_PROMPT).unwrap();
    let until = Instant::now() + Duration::from_secs(90);
    while Instant::now() < until {
        let _ = fs::write(evidence.join("terminal.txt"), session.transcript());
        if applied.is_file() {
            let body = fs::read_to_string(&applied).unwrap();
            assert!(
                body.trim() != MARKER_V1 && body.trim() != MARKER_V2,
                "retired skill body was applied; evidence {}",
                evidence.display()
            );
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    let identity = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&skill)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&identity.stdout);
    assert!(
        !identity.status.success() || !report.contains("\"delivery_complete\": true"),
        "identity must not report delivery_complete after delete: {report}"
    );
    let _ = send_line(&session, "/quit");
    let _ = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LIVE_APPLY=1; live grok-4.6 TUI via xAI shim"]
fn live_tui_manual_compact_then_matching_applies() {
    let exe = require_live_apply();
    let (evidence, home, workspace, skill) = prepare_tui_home("skill-natural-compact-", MARKER_V1);
    fs::remove_dir_all(&skill).unwrap();
    let session = spawn_live_tui(&exe, &home, &workspace);
    assert!(
        wait_tui_ready(&session, &evidence),
        "live TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(3));
    send_line(&session, UNRELATED_PROMPT).unwrap();
    wait_idle(&session, &evidence);
    write_skill(&skill, MARKER_V1);
    send_line(&session, "/compact").unwrap();
    wait_idle(&session, &evidence);
    send_line(&session, MATCHING_PROMPT).unwrap();
    let applied = workspace.join("applied.txt");
    let written = wait_applied(&applied, MARKER_V1, &session, &evidence);
    let _ = send_line(&session, "/quit");
    let _ = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
    assert!(
        written,
        "matching task after idle /compact did not apply the late skill; evidence {}",
        evidence.display()
    );
}

fn enable_multi_agent(home: &Path) {
    let path = home.join("config.toml");
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("multi_agent = false", "multi_agent = true");
    fs::write(path, text).unwrap();
}

fn rollout_count(home: &Path) -> usize {
    fn walk(path: &Path, n: &mut usize) {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, n);
            } else if path.extension().is_some_and(|ext| ext == "jsonl") {
                *n += 1;
            }
        }
    }
    let mut n = 0;
    walk(&home.join("sessions"), &mut n);
    n
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LIVE_APPLY=1; live grok-4.6 TUI via xAI shim"]
fn live_tui_new_child_applies_without_fork() {
    let exe = require_live_apply();
    let (evidence, home, workspace, _skill) = prepare_tui_home("skill-natural-child-", MARKER_V1);
    enable_multi_agent(&home);
    let session = spawn_live_tui(&exe, &home, &workspace);
    assert!(
        wait_tui_ready(&session, &evidence),
        "live TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(3));
    send_line(
        &session,
        "Do not write applied.txt yourself. Spawn one child agent without inheriting this conversation. The child must record the natural-apply probe token for this workspace. Do not mention a skill name.",
    )
    .unwrap();
    let applied = workspace.join("applied.txt");
    let written = wait_applied(&applied, MARKER_V1, &session, &evidence);
    let spawned = rollout_count(&home) >= 2;
    let _ = send_line(&session, "/quit");
    let _ = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
    assert!(
        written,
        "child matching task did not apply the skill; evidence {}",
        evidence.display()
    );
    assert!(
        spawned,
        "no child spawn observed; evidence {}",
        evidence.display()
    );
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LIVE_APPLY=1; live grok-4.6 TUI via xAI shim"]
fn live_tui_running_child_applies_skill_added_while_active() {
    let exe = require_live_apply();
    let (evidence, home, workspace, skill) =
        prepare_tui_home("skill-natural-running-child-", MARKER_V1);
    fs::remove_dir_all(&skill).unwrap();
    enable_multi_agent(&home);
    let session = spawn_live_tui(&exe, &home, &workspace);
    assert!(
        wait_tui_ready(&session, &evidence),
        "live TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(3));
    send_line(
        &session,
        "Do not write applied.txt yourself. Spawn one child agent without inheriting this conversation. The child must wait 15 seconds, then record the natural-apply probe token for this workspace. Do not mention a skill name.",
    )
    .unwrap();
    let until_child = Instant::now() + Duration::from_secs(45);
    while Instant::now() < until_child && rollout_count(&home) < 2 {
        let _ = fs::write(evidence.join("terminal.txt"), session.transcript());
        std::thread::sleep(Duration::from_millis(400));
    }
    write_skill(&skill, MARKER_V1);
    let applied = workspace.join("applied.txt");
    let written = wait_applied(&applied, MARKER_V1, &session, &evidence);
    let spawned = rollout_count(&home) >= 2;
    let _ = send_line(&session, "/quit");
    let _ = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
    assert!(
        spawned,
        "running-child probe had no second rollout; evidence {}",
        evidence.display()
    );
    assert!(
        written,
        "running child did not apply a skill added while it was active; evidence {}",
        evidence.display()
    );
}

fn disable_skill_config(home: &Path, skill_md: &Path) {
    let mut cfg = fs::read_to_string(home.join("config.toml")).unwrap();
    cfg.push_str(&format!(
        "\n[[skills.config]]\npath = {}\nenabled = false\n",
        json_escape(skill_md)
    ));
    fs::write(home.join("config.toml"), cfg).unwrap();
}

fn spawn_live_tui_args(
    exe: &Path,
    home: &Path,
    workspace: &Path,
    args: Vec<std::ffi::OsString>,
) -> ConsoleSession {
    let mut command = CommandSpec::new(exe);
    command.args = args;
    command.current_dir = Some(workspace.to_path_buf());
    command.env.insert(
        "CODEX_HOME".into(),
        Some(home.to_path_buf().into_os_string()),
    );
    command.env.insert("PATH".into(), Some(filtered_path()));
    command.env.insert("OPENAI_API_KEY".into(), None);
    command.env.insert("CODEX_API_KEY".into(), None);
    let mut spec = ConsoleSpec::new(command);
    spec.limits.memory_bytes = Some(512 * 1024 * 1024);
    spec.limits.cpu_percent = Some(50.0);
    ConsoleSession::spawn(spec).unwrap()
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LIVE_APPLY=1; live grok-4.6 TUI via xAI shim"]
fn live_tui_resume_disablement_identity_is_incomplete() {
    let exe = require_live_apply();
    let (evidence, home, workspace, skill) =
        prepare_tui_home("skill-natural-resume-disable-", MARKER_V1);
    let session = spawn_live_tui(&exe, &home, &workspace);
    assert!(
        wait_tui_ready(&session, &evidence),
        "live TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(3));
    send_line(&session, MATCHING_PROMPT).unwrap();
    let applied = workspace.join("applied.txt");
    assert!(
        wait_applied(&applied, MARKER_V1, &session, &evidence),
        "matching TUI task did not apply before resume; evidence {}",
        evidence.display()
    );
    wait_idle(&session, &evidence);
    let _ = send_line(&session, "/quit");
    let _ = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
    disable_skill_config(&home, &skill.join("SKILL.md"));
    fs::remove_file(&applied).ok();
    let resumed = spawn_live_tui_args(
        &exe,
        &home,
        &workspace,
        vec!["resume".into(), "--last".into(), "--no-alt-screen".into()],
    );
    assert!(
        wait_tui_ready(&resumed, &evidence),
        "resume TUI did not become ready; evidence {}",
        evidence.display()
    );
    std::thread::sleep(Duration::from_secs(2));
    send_line(&resumed, MATCHING_PROMPT).unwrap();
    let until = Instant::now() + Duration::from_secs(90);
    let mut stale_apply = false;
    while Instant::now() < until {
        let _ = fs::write(evidence.join("terminal.txt"), resumed.transcript());
        if applied.is_file() {
            let body = fs::read_to_string(&applied).unwrap();
            if body.trim() == MARKER_V1 || body.trim() == MARKER_V2 {
                stale_apply = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    let identity = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&skill)
        .args(["--codex-home"])
        .arg(&home)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&identity.stdout);
    fs::write(
        evidence.join("acceptance.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "stale_resume_apply": stale_apply,
            "identity_status": identity.status.code(),
            "identity_enabled_false": report.contains("\"enabled\": false"),
            "identity_delivery_complete": report.contains("\"delivery_complete\": true"),
        }))
        .unwrap(),
    )
    .unwrap();
    println!("stale_resume_apply={stale_apply}");
    println!("{report}");
    assert!(
        report.contains("\"enabled\": false") && !report.contains("\"delivery_complete\": true"),
        "resume disablement must be visible to identity: {report}"
    );
    let _ = send_line(&resumed, "/quit");
    let _ = resumed.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
}
