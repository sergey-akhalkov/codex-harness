//! Installed ordinary-session acceptance for automatic goal recovery.
//! Owned isolated homes, a synthetic provider and the real native CLI; no model.
#![cfg(windows)]
#[path = "fixtures/control_responses.rs"]
mod control_responses;

use harness_core::{
    build_identity::{self, BuildRecord},
    console::{ConsoleOutcome, ConsoleSession, ConsoleSpec},
    core_install,
    installation_state::PathScope,
    process::{Cancellation, CommandSpec, Deadline, StopReason},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(90);
const EXHAUSTION_WAIT: Duration = Duration::from_secs(190);
const ATTEMPT_GRACE: Duration = Duration::from_secs(9);
const SETTLE: Duration = Duration::from_secs(5);
const OBJECTIVE: &str = "Perform the owned proof command and return its consumed result.";
const OBJECTIVE_TWO: &str = "Complete the second owned proof in the same thread.";

#[derive(Clone, Copy)]
enum Scenario {
    Recover,
    Exhaust,
    Repeat,
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
                    while i < bytes.len()
                        && bytes[i] != 0x07
                        && !(bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\'))
                    {
                        i += 1;
                    }
                    i += if bytes.get(i) == Some(&0x1b) { 2 } else { 1 };
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

fn wait_visible(session: &ConsoleSession, evidence: &Path, patterns: &[&str], until: Instant) {
    loop {
        let transcript = session.transcript();
        let text = visible(&transcript);
        let _ = fs::write(evidence, &transcript);
        if patterns.iter().any(|pattern| text.contains(pattern)) {
            return;
        }
        assert!(
            Instant::now() < until,
            "missing {patterns:?}; inspect {}",
            evidence.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn disk_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text.as_ref()))
}

fn prepare_user_profile(user: &Path) {
    for dir in [
        user.join("AppData/Roaming"),
        user.join("AppData/Local/Temp"),
        user.join(".agents/skills"),
    ] {
        fs::create_dir_all(dir).unwrap();
    }
}

fn provider_requests(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("provider-") && name.ends_with(".json"))
        })
        .collect();
    files.sort();
    files
}

fn private_root(codex_home: &Path) -> PathBuf {
    codex_home.join("harness/lead-endpoints/host")
}

struct Installed {
    root: PathBuf,
    codex_home: PathBuf,
    user_home: PathBuf,
    workspace: PathBuf,
    #[allow(dead_code)] // Owns the fixture for the whole test and stops it on drop.
    provider: control_responses::Responses,
}

impl Installed {
    fn new(scenario: Scenario) -> Self {
        let root = tempfile::Builder::new()
            .prefix("goal-recovery-installed-")
            .tempdir()
            .unwrap()
            .keep();
        let root_path = disk_path(&root);
        let source = disk_path(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .unwrap(),
        );
        let build = root_path.join("build");
        let codex_home = root_path.join("codex");
        let user_home = root_path.join("user");
        let workspace = root_path.join("consumer");
        fs::create_dir_all(&build).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        for name in build_identity::BINARIES {
            let from = Path::new(env!("CARGO_BIN_EXE_codex")).with_file_name(name);
            fs::copy(&from, build.join(name))
                .unwrap_or_else(|error| panic!("copy {name} from {}: {error}", from.display()));
        }
        let record = BuildRecord {
            schema: build_identity::SCHEMA,
            source_root: source.clone(),
            source: build_identity::source_identity(&source).unwrap(),
            rustc: "installed goal recovery acceptance".into(),
            cargo: "installed goal recovery acceptance".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: "release".into(),
            binaries: build_identity::BINARIES
                .iter()
                .map(|name| {
                    (
                        name.to_string(),
                        build_identity::hash_file(&build.join(name)).unwrap(),
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        };
        fs::write(
            build.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        prepare_user_profile(&user_home);
        fs::create_dir_all(root_path.join("provider")).unwrap();
        let provider = match scenario {
            Scenario::Recover => {
                control_responses::Responses::with_server_overload(root_path.join("provider"), 2)
            }
            Scenario::Exhaust => control_responses::Responses::with_server_overload_exhaustion(
                root_path.join("provider"),
            ),
            Scenario::Repeat => control_responses::Responses::with_server_overload_repeat(
                root_path.join("provider"),
            ),
        };
        let upstream = PathBuf::from(
            std::env::var_os("HARNESS_CONTROL_CODEX_EXE")
                .expect("explicit native Codex executable"),
        );
        let request = core_install::Request {
            source,
            build,
            codex_home: codex_home.clone(),
            user_home: user_home.clone(),
            dependency_user_home: user_home.clone(),
            upstream: Some(upstream),
            timeout: Duration::from_secs(60),
            path_scope: Some(PathScope::Process),
            goal_recovery: Some(true),
        };
        let _path = PathRestore::capture();
        let report = core_install::connect(&request, false).unwrap();
        assert_eq!(report.status, "connected");
        assert!(report.runtime.unwrap().passed);
        let launch: Value = serde_json::from_slice(
            &fs::read(codex_home.join("harness/native-launch.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(launch["schema"], 3);
        assert_eq!(launch["goal_recovery"], true);
        let trusted = serde_json::to_string(&workspace.to_string_lossy()).unwrap();
        fs::write(
            codex_home.join("config.toml"),
            format!(
                r#"
model = "gpt-6-astra"
model_provider = "control_fixture"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
cli_auth_credentials_store = "file"
[model_providers.control_fixture]
name = "Owned installed goal fixture"
base_url = "http://127.0.0.1:{}/v1"
wire_api = "responses"
env_key = "HARNESS_CONTROL_FIXTURE_KEY"
requires_openai_auth = false
request_max_retries = 0
stream_max_retries = 0
supports_websockets = false
[features]
code_mode = false
shell_snapshot = false
hooks = false
goals = true
[analytics]
enabled = false
[projects.{trusted}]
trust_level = "trusted"
"#,
                provider.port
            ),
        )
        .unwrap();
        fs::write(workspace.join("AGENTS.md"), "Owned consumer guidance\n").unwrap();
        Self {
            root,
            codex_home,
            user_home,
            workspace,
            provider,
        }
    }

    fn session(&self) -> ConsoleSession {
        let mut spec = CommandSpec::new(self.codex_home.join("harness/bin/codex.exe"));
        spec.current_dir = Some(self.workspace.clone());
        spec.args = vec![
            "-c".into(),
            "features.goals=true".into(),
            "-c".into(),
            "model=\"gpt-6-astra\"".into(),
            "-c".into(),
            "model_provider=\"control_fixture\"".into(),
            "-c".into(),
            "model_reasoning_effort=\"low\"".into(),
            "--no-alt-screen".into(),
        ];
        spec.env.insert(
            "CODEX_HOME".into(),
            Some(self.codex_home.clone().into_os_string()),
        );
        spec.env.insert(
            "USERPROFILE".into(),
            Some(self.user_home.clone().into_os_string()),
        );
        spec.env
            .insert("HOME".into(), Some(self.user_home.clone().into_os_string()));
        spec.env.insert(
            "APPDATA".into(),
            Some(self.user_home.join("AppData/Roaming").into_os_string()),
        );
        spec.env.insert(
            "LOCALAPPDATA".into(),
            Some(self.user_home.join("AppData/Local").into_os_string()),
        );
        spec.env.insert(
            "TEMP".into(),
            Some(self.user_home.join("AppData/Local/Temp").into_os_string()),
        );
        spec.env.insert(
            "TMP".into(),
            Some(self.user_home.join("AppData/Local/Temp").into_os_string()),
        );
        for name in [
            "HARNESS_EXECUTOR_SESSION",
            "HARNESS_REFLECTION_SESSION",
            "HARNESS_BOUNDED_LEAD_SESSION",
            "HARNESS_EXECUTOR_FIXTURE_MODE",
            "HARNESS_EXECUTOR_CHILD_FIXTURE_MODE",
        ] {
            spec.env.insert(name.into(), None);
        }
        spec.env.insert(
            "HARNESS_CONTROL_FIXTURE_KEY".into(),
            Some("synthetic-owned-fixture".into()),
        );
        ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap()
    }
}

struct PathRestore(Option<std::ffi::OsString>);
impl PathRestore {
    fn capture() -> Self {
        Self(std::env::var_os("PATH"))
    }
}
impl Drop for PathRestore {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            unsafe { std::env::set_var("PATH", path) };
        }
    }
}

struct TuiSession {
    session: Option<ConsoleSession>,
    terminal: PathBuf,
}

impl TuiSession {
    fn new(installed: &Installed) -> Self {
        Self {
            session: Some(installed.session()),
            terminal: installed.root.join("installed-terminal.txt"),
        }
    }

    fn save(&self) {
        if let Some(session) = &self.session {
            let transcript = session.transcript();
            let _ = fs::write(&self.terminal, &transcript);
            let _ = fs::write(
                self.terminal
                    .with_file_name("installed-terminal-before-quit.txt"),
                transcript,
            );
        }
    }

    fn close(&mut self) -> Option<ConsoleOutcome> {
        self.save();
        let session = self.session.take()?;
        let _ = session.send("/quit");
        std::thread::sleep(Duration::from_millis(300));
        let _ = session.send("\r");
        let outcome = session.wait(
            Deadline::after(WAIT).ok()?,
            &Cancellation::default(),
            Duration::from_secs(5),
        );
        if let Ok(finished) = &outcome {
            let _ = fs::write(&self.terminal, &finished.transcript);
        }
        outcome.ok()
    }

    fn quit(mut self) -> ConsoleOutcome {
        self.close().expect("the installed TUI did not exit")
    }
}

impl Deref for TuiSession {
    type Target = ConsoleSession;

    fn deref(&self) -> &Self::Target {
        self.session
            .as_ref()
            .expect("the installed TUI is still open")
    }
}

impl Drop for TuiSession {
    fn drop(&mut self) {
        self.close();
    }
}

fn send_enter(session: &TuiSession, text: &str) {
    session.send(text).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    session.send("\r").unwrap();
}

fn recovery_checkpoint(installed: &Installed) -> Option<Value> {
    let path = fs::read_dir(private_root(&installed.codex_home))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".recovery.json"))
        })?;
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn wait_checkpoint(
    installed: &Installed,
    terminal: &Path,
    what: &str,
    timeout: Duration,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    let until = Instant::now() + timeout;
    loop {
        if let Some(checkpoint) = recovery_checkpoint(installed)
            && predicate(&checkpoint)
        {
            return checkpoint;
        }
        assert!(
            Instant::now() < until,
            "{what} was not recorded within {timeout:?}; checkpoint {:?}; inspect {}",
            recovery_checkpoint(installed),
            terminal.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn request_bodies(installed: &Installed) -> Vec<Value> {
    provider_requests(&installed.root.join("provider"))
        .into_iter()
        .map(|path| serde_json::from_slice(&fs::read(path).unwrap()).unwrap())
        .collect()
}

fn request_times(installed: &Installed) -> Vec<Value> {
    fs::read_to_string(installed.root.join("provider/provider-requests.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn request_thread(installed: &Installed, requests: &[Value]) -> String {
    let thread = requests
        .first()
        .and_then(|request| request["client_metadata"]["thread_id"].as_str())
        .expect("the provider request names its thread")
        .to_owned();
    for request in requests {
        assert_eq!(
            request["client_metadata"]["thread_id"], thread,
            "recovery changed the native thread"
        );
    }
    let timestamps = request_times(installed);
    assert_eq!(
        timestamps.len(),
        requests.len(),
        "every provider request must have one retained timestamp"
    );
    thread
}

fn completed_goals(installed: &Installed) -> Vec<Value> {
    fn files(directory: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                files(&path, found);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                found.push(path);
            }
        }
    }

    let mut paths = Vec::new();
    files(&installed.codex_home.join("sessions"), &mut paths);
    paths.sort();
    paths
        .into_iter()
        .flat_map(|path| {
            fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .filter(|entry| {
                    entry["type"] == "response_item"
                        && entry["payload"]["type"] == "function_call_output"
                        && entry["payload"]["call_id"]
                            .as_str()
                            .is_some_and(|id| id.starts_with("goal-complete-"))
                })
                .filter_map(|entry| {
                    serde_json::from_str::<Value>(entry["payload"]["output"].as_str()?).ok()
                })
                .filter_map(|output| output.get("goal").cloned())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn endpoint_records(installed: &Installed) -> Vec<PathBuf> {
    let mut records = Vec::new();
    let Ok(entries) = fs::read_dir(installed.codex_home.join("harness/lead-endpoints")) else {
        return records;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            if let Ok(files) = fs::read_dir(path) {
                records.extend(
                    files
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .filter(|path| {
                            path.extension()
                                .is_some_and(|extension| extension == "json")
                        }),
                );
            }
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            records.push(path);
        }
    }
    records
}

fn write_result(installed: &Installed, name: &str, value: Value) {
    fs::write(
        installed.root.join(name),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
}

fn wait_provider_count(
    installed: &Installed,
    terminal: &Path,
    expected: usize,
    timeout: Duration,
) -> Vec<Value> {
    let until = Instant::now() + timeout;
    loop {
        let requests = request_bodies(installed);
        if requests.len() >= expected {
            assert_eq!(
                requests.len(),
                expected,
                "unexpected provider request count; inspect {}",
                terminal.display()
            );
            return requests;
        }
        assert!(
            Instant::now() < until,
            "provider request {expected} was not observed within {timeout:?}; got {}; inspect {}",
            requests.len(),
            terminal.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned installed homes and synthetic Responses"]
fn installed_ordinary_session_recovers_a_blocked_goal_after_overload() {
    let installed = Installed::new(Scenario::Recover);
    let root = &installed.root;
    let terminal = root.join("installed-terminal.txt");
    eprintln!("installed goal recovery evidence: {}", root.display());
    let session = installed.session();
    let until = Instant::now() + WAIT;
    wait_visible(&session, &terminal, &["GPT-6-Astra"], until);
    std::thread::sleep(Duration::from_secs(1));
    session.send(&format!("/goal {OBJECTIVE}")).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    session.send("\r").unwrap();
    wait_visible(
        &session,
        &terminal,
        &["Goal stalled (/goal resume)", "Pursuing goal", "Goal set"],
        until,
    );
    wait_visible(
        &session,
        &terminal,
        &[control_responses::FINAL, "Goal achieved"],
        until,
    );
    assert_eq!(
        fs::read_to_string(installed.workspace.join("proof.txt")).unwrap(),
        "one",
        "the owned tool effect must happen exactly once"
    );
    let requests = provider_requests(&root.join("provider"));
    assert_eq!(requests.len(), 4, "{requests:?}");
    let thread = {
        let body: Value = serde_json::from_slice(&fs::read(&requests[2]).unwrap()).unwrap();
        body["client_metadata"]["thread_id"]
            .as_str()
            .expect("recovered request names its thread")
            .to_owned()
    };
    for request in &requests {
        let body: Value = serde_json::from_slice(&fs::read(request).unwrap()).unwrap();
        assert_eq!(body["client_metadata"]["thread_id"], thread);
    }
    let transcript = visible(&session.transcript());
    assert!(
        transcript.contains("temporarily overloaded")
            || transcript.contains("warnings")
            || transcript.contains("warning"),
        "the installed TUI must show the recovery notification: {transcript}"
    );
    assert!(
        transcript.contains("Goal achieved"),
        "the installed TUI must show the completed goal: {transcript}"
    );
    let checkpoint_until = Instant::now() + Duration::from_secs(5);
    let checkpoint = loop {
        let path = fs::read_dir(private_root(&installed.codex_home))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.ends_with(".recovery.json"))
            })
            .expect("the host records its recovery checkpoint");
        let checkpoint: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        if checkpoint["status"] == "complete" || Instant::now() >= checkpoint_until {
            break checkpoint;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(checkpoint["status"], "complete");
    session.send("/quit").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    session.send("\r").unwrap();
    let outcome = session
        .wait(
            Deadline::after(WAIT).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert!(outcome.outcome.reason == StopReason::Exited, "{outcome:?}");
    fs::write(
        root.join("installed-result.json"),
        serde_json::to_vec_pretty(&json!({
            "threadId": thread,
            "providerRequests": requests.len(),
            "checkpoint": checkpoint,
            "toolEffect": "one",
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned installed homes and synthetic Responses"]
fn installed_pause_before_dispatch_cancels_recovery() {
    let installed = Installed::new(Scenario::Recover);
    eprintln!("installed pause evidence: {}", installed.root.display());
    let tui = TuiSession::new(&installed);
    let terminal = tui.terminal.clone();
    let until = Instant::now() + WAIT;
    wait_visible(&tui, &terminal, &["GPT-6-Astra"], until);
    std::thread::sleep(Duration::from_secs(1));
    send_enter(&tui, &format!("/goal {OBJECTIVE}"));
    wait_visible(
        &tui,
        &terminal,
        &["Goal stalled (/goal resume)", "Goal set", "Pursuing goal"],
        until,
    );

    send_enter(&tui, "/goal pause");
    wait_visible(&tui, &terminal, &["Goal paused (/goal resume)"], until);
    let checkpoint = wait_checkpoint(
        &installed,
        &terminal,
        "the cancelled pause",
        WAIT,
        |value| value["status"] == "cancelled",
    );
    assert_eq!(checkpoint["attempt"], 0, "{checkpoint}");

    std::thread::sleep(ATTEMPT_GRACE);
    let requests = request_bodies(&installed);
    assert_eq!(
        requests.len(),
        2,
        "a cancelled recovery sent another provider request"
    );
    let thread = request_thread(&installed, &requests);
    assert_eq!(checkpoint["threadId"], thread);
    assert_eq!(
        fs::read_to_string(installed.workspace.join("proof.txt")).unwrap(),
        "one",
        "the owned tool effect changed during cancellation"
    );
    let transcript = visible(&tui.transcript());
    assert!(
        transcript.contains("Goal paused (/goal resume)"),
        "the TUI did not retain the paused state: {transcript}"
    );
    assert!(
        transcript.contains("warning"),
        "the TUI did not expose the cancellation badge: {transcript}"
    );

    let outcome = tui.quit();
    assert_eq!(outcome.outcome.reason, StopReason::Exited, "{outcome:?}");
    std::thread::sleep(SETTLE);
    assert_eq!(request_bodies(&installed).len(), 2);
    assert!(
        endpoint_records(&installed).is_empty(),
        "the paused host left endpoint records"
    );
    write_result(
        &installed,
        "pause-result.json",
        json!({
            "threadId": thread,
            "providerRequests": 2,
            "automaticSets": 0,
            "goalState": "paused",
            "checkpoint": checkpoint,
            "requestTimestamps": request_times(&installed),
            "toolEffect": "one",
        }),
    );
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned installed homes and synthetic Responses"]
fn installed_five_attempt_exhaustion_stops_without_a_sixth() {
    let installed = Installed::new(Scenario::Exhaust);
    eprintln!(
        "installed exhaustion evidence: {}",
        installed.root.display()
    );
    let tui = TuiSession::new(&installed);
    let terminal = tui.terminal.clone();
    let until = Instant::now() + WAIT;
    wait_visible(&tui, &terminal, &["GPT-6-Astra"], until);
    std::thread::sleep(Duration::from_secs(1));
    send_enter(&tui, &format!("/goal {OBJECTIVE}"));
    wait_visible(
        &tui,
        &terminal,
        &["Goal stalled (/goal resume)", "Goal set", "Pursuing goal"],
        until,
    );

    let checkpoint = wait_checkpoint(
        &installed,
        &terminal,
        "the fifth exhausted attempt",
        EXHAUSTION_WAIT,
        |value| value["status"] == "exhausted",
    );
    assert_eq!(checkpoint["attempt"], 5, "{checkpoint}");
    let requests = wait_provider_count(&installed, &terminal, 7, WAIT);
    let thread = request_thread(&installed, &requests);
    assert_eq!(checkpoint["threadId"], thread);
    assert_eq!(
        fs::read_to_string(installed.workspace.join("proof.txt")).unwrap(),
        "one",
        "the owned effect repeated during exhaustion"
    );
    std::thread::sleep(SETTLE);
    assert_eq!(
        request_bodies(&installed).len(),
        7,
        "a sixth automatic recovery reached the provider"
    );
    let transcript = visible(&tui.transcript());
    assert!(
        transcript.contains("Goal stalled (/goal resume)"),
        "the TUI lost the blocked goal state: {transcript}"
    );
    assert!(
        transcript.contains("warning"),
        "the TUI did not expose the exhaustion badge: {transcript}"
    );

    let outcome = tui.quit();
    assert_eq!(outcome.outcome.reason, StopReason::Exited, "{outcome:?}");
    std::thread::sleep(SETTLE);
    assert_eq!(request_bodies(&installed).len(), 7);
    assert!(
        endpoint_records(&installed).is_empty(),
        "the exhausted host left endpoint records"
    );
    write_result(
        &installed,
        "exhaustion-result.json",
        json!({
            "threadId": thread,
            "providerRequests": 7,
            "automaticSets": 5,
            "goalState": "blocked",
            "checkpoint": checkpoint,
            "requestTimestamps": request_times(&installed),
            "toolEffect": "one",
        }),
    );
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned installed homes and synthetic Responses"]
fn installed_second_goal_recovers_after_the_first() {
    let installed = Installed::new(Scenario::Repeat);
    eprintln!("installed repeat evidence: {}", installed.root.display());
    let tui = TuiSession::new(&installed);
    let terminal = tui.terminal.clone();
    let until = Instant::now() + WAIT;
    wait_visible(&tui, &terminal, &["GPT-6-Astra"], until);
    std::thread::sleep(Duration::from_secs(1));
    send_enter(&tui, &format!("/goal {OBJECTIVE}"));
    wait_visible(
        &tui,
        &terminal,
        &["Goal stalled (/goal resume)", "Goal set", "Pursuing goal"],
        until,
    );
    wait_visible(&tui, &terminal, &["Goal achieved"], until);
    wait_visible(&tui, &terminal, &[control_responses::FINAL], until);
    wait_visible(
        &tui,
        &terminal,
        &["Ask Codex to do anything", "Type a message"],
        until,
    );
    std::thread::sleep(Duration::from_secs(1));
    let first = wait_checkpoint(
        &installed,
        &terminal,
        "the first completed recovery",
        WAIT,
        |value| value["status"] == "complete",
    );
    let first_goals = completed_goals(&installed);
    assert_eq!(first_goals.len(), 1, "{first_goals:?}");
    assert_eq!(first_goals[0]["objective"], OBJECTIVE);

    send_enter(&tui, "/goal clear");
    std::thread::sleep(Duration::from_secs(1));
    send_enter(&tui, "/goal");
    wait_visible(&tui, &terminal, &["No goal is currently set"], until);
    send_enter(&tui, &format!("/goal {OBJECTIVE_TWO}"));
    let requests = wait_provider_count(&installed, &terminal, 8, WAIT);
    let thread = request_thread(&installed, &requests);
    assert_eq!(first["threadId"], thread);
    assert!(
        requests[4]["input"].to_string().contains(OBJECTIVE_TWO),
        "the second goal turn did not carry its objective"
    );
    let goals = completed_goals(&installed);
    assert_eq!(goals.len(), 2, "{goals:?}");
    assert_eq!(goals[0]["objective"], OBJECTIVE);
    assert_eq!(goals[1]["objective"], OBJECTIVE_TWO);
    assert_eq!(goals[0]["threadId"], thread);
    assert_eq!(goals[1]["threadId"], thread);
    assert_ne!(
        goals[0]["createdAt"], goals[1]["createdAt"],
        "the second goal reused the first goal's creation identity"
    );
    for goal in &goals {
        assert_eq!(goal["status"], "complete", "{goal}");
    }
    let checkpoint = wait_checkpoint(
        &installed,
        &terminal,
        "the second completed recovery",
        WAIT,
        // Completion clears the episode, so attempt is zero in both completed checkpoints.
        |value| {
            value["status"] == "complete"
                && value["savedAtUnixMs"].as_u64() > first["savedAtUnixMs"].as_u64()
        },
    );
    assert_eq!(checkpoint["threadId"], thread);
    assert_eq!(
        fs::read_to_string(installed.workspace.join("proof-first.txt")).unwrap(),
        "one"
    );
    assert_eq!(
        fs::read_to_string(installed.workspace.join("proof-second.txt")).unwrap(),
        "two"
    );
    let transcript = visible(&tui.transcript());
    assert!(
        transcript.contains("Goal achieved"),
        "the TUI did not report the second completion: {transcript}"
    );

    let outcome = tui.quit();
    assert_eq!(outcome.outcome.reason, StopReason::Exited, "{outcome:?}");
    std::thread::sleep(SETTLE);
    assert_eq!(request_bodies(&installed).len(), 8);
    assert!(
        endpoint_records(&installed).is_empty(),
        "the repeated host left endpoint records"
    );
    write_result(
        &installed,
        "repeat-result.json",
        json!({
            "threadId": thread,
            "providerRequests": 8,
            "automaticSets": 2,
            "firstGoal": goals[0],
            "secondGoal": goals[1],
            "firstCheckpoint": first,
            "secondCheckpoint": checkpoint,
            "requestTimestamps": request_times(&installed),
            "toolEffects": ["one", "two"],
        }),
    );
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned installed homes and synthetic Responses"]
fn installed_closure_before_dispatch_leaves_no_retry() {
    let installed = Installed::new(Scenario::Recover);
    eprintln!("installed closure evidence: {}", installed.root.display());
    let tui = TuiSession::new(&installed);
    let terminal = tui.terminal.clone();
    let until = Instant::now() + WAIT;
    wait_visible(&tui, &terminal, &["GPT-6-Astra"], until);
    std::thread::sleep(Duration::from_secs(1));
    send_enter(&tui, &format!("/goal {OBJECTIVE}"));
    wait_visible(
        &tui,
        &terminal,
        &["Goal stalled (/goal resume)", "Goal set", "Pursuing goal"],
        until,
    );
    let waiting = wait_checkpoint(
        &installed,
        &terminal,
        "the scheduled recovery before closure",
        WAIT,
        |value| value["status"] == "waiting",
    );
    assert_eq!(waiting["attempt"], 0, "{waiting}");
    let before = request_bodies(&installed);
    assert_eq!(before.len(), 2, "{before:?}");
    let thread = request_thread(&installed, &before);

    let outcome = tui.quit();
    assert_eq!(outcome.outcome.reason, StopReason::Exited, "{outcome:?}");
    std::thread::sleep(ATTEMPT_GRACE);
    let after = request_bodies(&installed);
    assert_eq!(after.len(), 2, "a pending attempt ran after the TUI closed");
    assert!(
        endpoint_records(&installed).is_empty(),
        "the closed host left endpoint or checkpoint records"
    );
    assert_eq!(
        fs::read_to_string(installed.workspace.join("proof.txt")).unwrap(),
        "one"
    );
    write_result(
        &installed,
        "closure-result.json",
        json!({
            "threadId": thread,
            "providerRequests": 2,
            "automaticSets": 0,
            "goalState": "blocked at closure",
            "checkpointBeforeClosure": waiting,
            "requestTimestamps": request_times(&installed),
            "toolEffect": "one",
        }),
    );
}
