//! Installed ordinary-session acceptance for automatic goal recovery.
//! Owned isolated homes, a synthetic provider and the real native CLI; no model.
#![cfg(windows)]
#[path = "fixtures/control_responses.rs"]
mod control_responses;

use harness_core::{
    build_identity::{self, BuildRecord},
    console::{ConsoleSession, ConsoleSpec},
    core_install,
    installation_state::PathScope,
    process::{Cancellation, CommandSpec, Deadline, StopReason},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(90);
const OBJECTIVE: &str = "Perform the owned proof command and return its consumed result.";

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
    codex_home.join("harness/lead")
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
    fn new() -> Self {
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
        let provider =
            control_responses::Responses::with_server_overload(root_path.join("provider"), 2);
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

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned installed homes and synthetic Responses"]
fn installed_ordinary_session_recovers_a_blocked_goal_after_overload() {
    let installed = Installed::new();
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
        transcript.contains("temporarily overloaded"),
        "the installed TUI must show the scheduled recovery: {transcript}"
    );
    let checkpoint = fs::read_dir(private_root(&installed.codex_home))
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
    let checkpoint: Value = serde_json::from_slice(&fs::read(checkpoint).unwrap()).unwrap();
    assert_eq!(checkpoint["status"], "complete");
    session.send("/quit\r").unwrap();
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
