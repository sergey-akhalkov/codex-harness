//! Native preparation of the configured reflection profile.
//!
//! The app-server has no `--profile` flag. Ordinary dispatch layers the selected
//! profile onto its process with `-c` values; this check proves that the same
//! preparation makes a profile-defined provider available to `thread/fork`.
//! The check starts no model turn and uses only owned synthetic configuration.

use super::*;
use harness_core::orchestration_config::profile_config_overrides;
use std::io::{BufRead, BufReader, Write};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;

#[path = "../fixtures/control_responses.rs"]
mod control_responses;

const BASE_MODEL: &str = "gpt-6-astra";
const BASE_PROVIDER: &str = "base_only";
const BASE_EFFORT: &str = "low";
const PROVIDER_KEY_ENV: &str = "REFLECTION_PROVIDER_PROBE_KEY";
const PROVIDER_KEY: &str = "synthetic-reflection-provider-key";
const SOURCE_REPLY: &str = "owned source turn complete";
const TITLE_PROMPT: &str = "Generate a concise, single-line task title";

/// True for the native title-generation request, which must be answered but is
/// not a source or reflection turn.
fn title_generation_request(request: &Value) -> bool {
    request["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["role"] == "user"
                && item["content"].as_array().is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part["text"]
                            .as_str()
                            .is_some_and(|text| text.starts_with(TITLE_PROMPT))
                    })
                })
        })
    })
}

fn message(text: &str) -> Value {
    json!({
        "type": "message",
        "id": format!("profile-provider-{}", text.len()),
        "role": "assistant",
        "content": [{"type": "output_text", "text": text}],
    })
}

/// The opt-in native executable. The check deliberately does not copy or
/// alter an installed launcher; it only uses the executable named by the
/// caller for this owned run.
fn native_exe() -> PathBuf {
    let value = std::env::var_os("HARNESS_CONTROL_CODEX_EXE").unwrap_or_else(|| {
        panic!("set HARNESS_CONTROL_CODEX_EXE to a Codex executable to run this native check")
    });
    let path = PathBuf::from(value);
    assert!(
        path.is_file(),
        "HARNESS_CONTROL_CODEX_EXE does not name a file: {}",
        path.display()
    );
    path
}

fn write_home(home: &Path, port: u16) {
    fs::create_dir_all(home).unwrap();
    let trusted = serde_json::to_string(&home.to_string_lossy().to_lowercase()).unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "{BASE_MODEL}"
model_provider = "{BASE_PROVIDER}"
model_reasoning_effort = "{BASE_EFFORT}"
approval_policy = "never"
sandbox_mode = "read-only"
model_context_window = 654321
cli_auth_credentials_store = "file"

[features]
hooks = true

[model_providers.base_only]
name = "owned base route"
base_url = "http://127.0.0.1:{port}/v1"
env_key = "{PROVIDER_KEY_ENV}"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0

[windows]
sandbox = "unelevated"

[projects.{trusted}]
trust_level = "trusted"
"#
        ),
    )
    .unwrap();
    fs::write(
        home.join(format!("{PROFILE}.config.toml")),
        format!(
            r#"model = "{MODEL}"
model_provider = "{PROVIDER}"
model_reasoning_effort = "{EFFORT}"

[features]
hooks = false

[model_providers.deepseek]
name = "owned deepseek route"
base_url = "http://127.0.0.1:{port}/v1"
env_key = "{PROVIDER_KEY_ENV}"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
"#
        ),
    )
    .unwrap();
    fs::write(
        home.join("auth.json"),
        serde_json::to_vec_pretty(&json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "synthetic-unused-openai-key"
        }))
        .unwrap(),
    )
    .unwrap();
}

/// One real stdio app-server connection. Notifications are retained so the
/// check can prove that neither thread/start nor thread/fork started a turn.
struct Native {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    id: u64,
    notifications: Vec<Value>,
}

impl Native {
    fn spawn(exe: &Path, home: &Path, label: &str, extra: &[String]) -> Self {
        let log = fs::File::create(home.join(format!("app-server-{label}.log"))).unwrap();
        let mut child = Command::new(exe)
            .args(["app-server", "--stdio"])
            .args(extra)
            .env("CODEX_HOME", home)
            .env(PROVIDER_KEY_ENV, PROVIDER_KEY)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            id: 0,
            notifications: Vec::new(),
        }
    }

    fn initialize(&mut self) {
        let response = self.request(
            "initialize",
            json!({
                "clientInfo": {"name": "reflection-profile-check", "version": "1"},
                "capabilities": {"experimentalApi": true},
            }),
        );
        assert!(response.get("error").is_none(), "{response}");
        self.notify("initialized");
    }

    fn notify(&mut self, method: &str) {
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "method": method})
        )
        .unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .unwrap();
        self.stdin.flush().unwrap();
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).unwrap();
            assert!(read > 0, "app-server closed before answering {method}");
            let value: Value = serde_json::from_str(&line).unwrap();
            if value.get("method").is_some() {
                self.notifications.push(value);
                continue;
            }
            if value["id"].as_u64() == Some(id) {
                return value;
            }
        }
    }

    fn result(&mut self, method: &str, params: Value) -> Value {
        let response = self.request(method, params);
        assert!(response.get("error").is_none(), "{method}: {response}");
        response["result"].clone()
    }

    fn start_source(&mut self, home: &Path, model: &str, provider: &str, effort: &str) -> Value {
        let started = self.result(
            "thread/start",
            json!({
                "cwd": home,
                "model": model,
                "modelProvider": provider,
                "allowProviderModelFallback": false,
                "approvalPolicy": "never",
                "sandbox": "read-only",
                "config": {"model_reasoning_effort": effort},
            }),
        );
        assert_route(&started, model, provider, effort);
        assert!(
            started["thread"]["id"]
                .as_str()
                .is_some_and(|id| !id.is_empty()),
            "thread/start answered without a thread identity: {started}"
        );
        started
    }

    fn start_turn(&mut self, thread: &str, text: &str) -> String {
        self.result(
            "turn/start",
            json!({
                "threadId": thread,
                "input": [{"type": "text", "text": text}],
            }),
        )["turn"]["id"]
            .as_str()
            .expect("turn/start names its turn")
            .to_owned()
    }

    fn pump(&mut self, thread: &str) {
        let _ = self.request("thread/read", json!({"threadId": thread}));
    }

    fn wait_for_completed_turn(&mut self, thread: &str, turn: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.pump(thread);
            if self.notifications.iter().any(|note| {
                note["method"] == "turn/completed"
                    && note["params"]["threadId"].as_str() == Some(thread)
                    && note["params"]["turn"]["id"].as_str() == Some(turn)
            }) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for turn {turn} to complete: {:?}",
                self.notifications
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn turn_started_for(&self, thread: &str) -> usize {
        self.notifications
            .iter()
            .filter(|note| {
                note["method"] == "turn/started"
                    && note["params"]["threadId"].as_str() == Some(thread)
            })
            .count()
    }

    fn fork(
        &mut self,
        home: &Path,
        source: &str,
        model: &str,
        provider: &str,
        effort: &str,
    ) -> Value {
        // Provider resolution is the same native preparation used by the
        // production fork request; no turn is submitted by this request.
        self.request(
            "thread/fork",
            json!({
                "threadId": source,
                "cwd": home,
                "model": model,
                "modelProvider": provider,
                "approvalPolicy": "never",
                "sandbox": "read-only",
                "deferGoalContinuation": true,
                "developerInstructions": "synthetic read-only reflection role",
                "config": {"model_reasoning_effort": effort},
            }),
        )
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn assert_route(value: &Value, model: &str, provider: &str, effort: &str) {
    assert_eq!(value["model"].as_str(), Some(model), "{value}");
    assert_eq!(value["modelProvider"].as_str(), Some(provider), "{value}");
    assert_eq!(value["reasoningEffort"].as_str(), Some(effort), "{value}");
}

fn effective_config(read: &Value) -> &Value {
    read.get("config").unwrap_or(read)
}

struct Evidence {
    dir: Option<tempfile::TempDir>,
}

impl Evidence {
    fn new(prefix: &str) -> Self {
        Self {
            dir: Some(
                tempfile::Builder::new()
                    .prefix(prefix)
                    .tempdir()
                    .expect("owned profile evidence"),
            ),
        }
    }

    fn path(&self) -> &Path {
        self.dir.as_ref().expect("evidence directory").path()
    }
}

impl Drop for Evidence {
    fn drop(&mut self) {
        if std::thread::panicking() || std::env::var_os("HARNESS_KEEP_WORKSPACE_EVIDENCE").is_some()
        {
            if let Some(dir) = self.dir.take() {
                eprintln!("retained profile evidence: {}", dir.keep().display());
            }
        }
    }
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; runs a real model-free app-server fork"]
fn configured_profile_provider_resolves_native_reflection_fork() {
    let evidence = Evidence::new("reflection-profile-");
    let upstream = native_exe();
    let version = Command::new(&upstream)
        .arg("--version")
        .output()
        .expect("native version");
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    assert!(version.starts_with("codex-cli "), "{version}");
    fs::write(evidence.path().join("native-version.txt"), &version).unwrap();

    let provider_turns = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&provider_turns);
    let responses =
        control_responses::Responses::routed(evidence.path().join("provider"), move |request| {
            if title_generation_request(request) {
                return message("Owned profile source");
            }
            counted.fetch_add(1, Ordering::SeqCst);
            message(SOURCE_REPLY)
        });
    let home = evidence.path().join("home");
    write_home(&home, responses.port);

    // The failure observed in the live path: the base home has no deepseek
    // provider, so a fork declaration naming it is refused before any turn.
    // The source itself is a real completed native turn over the owned local
    // Responses route, so the refusal cannot be an empty-thread artifact.
    let mut unconfigured = Native::spawn(&upstream, &home, "without-profile", &[]);
    unconfigured.initialize();
    let base_source = unconfigured.start_source(&home, BASE_MODEL, BASE_PROVIDER, BASE_EFFORT);
    let source_id = base_source["thread"]["id"]
        .as_str()
        .expect("the base source names its thread")
        .to_owned();
    let source_turn = unconfigured.start_turn(&source_id, "complete the owned source turn");
    unconfigured.wait_for_completed_turn(&source_id, &source_turn);
    assert_eq!(
        provider_turns.load(Ordering::SeqCst),
        1,
        "the base source turn must reach the owned provider exactly once"
    );
    let unconfigured_source_turns = unconfigured.turn_started_for(&source_id);
    let refused = unconfigured.fork(&home, &source_id, MODEL, PROVIDER, EFFORT);
    let refusal = refused["error"]["message"].as_str().unwrap_or_default();
    assert!(
        refusal.to_ascii_lowercase().contains("deepseek")
            && refusal.to_ascii_lowercase().contains("not found"),
        "the unconfigured process did not demonstrate the missing provider: {refused}"
    );
    unconfigured.pump(&source_id);
    assert_eq!(
        unconfigured.turn_started_for(&source_id),
        unconfigured_source_turns,
        "the refused fork started another source turn: {:?}",
        unconfigured.notifications
    );
    assert_eq!(
        provider_turns.load(Ordering::SeqCst),
        1,
        "the refused fork reached the provider"
    );
    drop(unconfigured);

    // The ordinary preparation: the exact selected profile is layered onto
    // the app-server with `-c` values. The base config remains loaded, so the
    // unrelated base route still starts a source thread.
    let overrides = profile_config_overrides(&home, PROFILE).unwrap();
    assert!(
        !overrides.is_empty(),
        "the synthetic profile produced no overrides"
    );
    let mut args = Vec::with_capacity(overrides.len() * 2);
    for config in &overrides {
        args.push("-c".to_owned());
        args.push(config.clone());
    }
    let mut configured = Native::spawn(&upstream, &home, "with-profile", &args);
    configured.initialize();
    let read = configured.result("config/read", json!({"cwd": home, "includeLayers": true}));
    fs::write(
        evidence.path().join("config-read.json"),
        serde_json::to_vec_pretty(&read).unwrap(),
    )
    .unwrap();
    let effective = effective_config(&read);
    assert_eq!(effective["model"].as_str(), Some(MODEL), "{read}");
    assert_eq!(
        effective["model_provider"].as_str(),
        Some(PROVIDER),
        "{read}"
    );
    assert_eq!(
        effective["model_reasoning_effort"].as_str(),
        Some(EFFORT),
        "{read}"
    );
    assert_eq!(effective["features"]["hooks"], false, "{read}");
    assert_eq!(effective["approval_policy"], "never", "{read}");
    assert_eq!(effective["sandbox_mode"], "read-only", "{read}");
    assert_eq!(effective["model_context_window"], 654321, "{read}");
    let expected_base = format!("http://127.0.0.1:{}/v1", responses.port);
    let deepseek = effective["model_providers"]
        .get("deepseek")
        .unwrap_or_else(|| panic!("the profile provider is not in config/read: {read}"));
    assert_eq!(deepseek["base_url"].as_str(), Some(expected_base.as_str()));
    assert_eq!(deepseek["env_key"].as_str(), Some(PROVIDER_KEY_ENV));
    assert_eq!(deepseek["wire_api"].as_str(), Some("responses"));
    assert_eq!(deepseek["requires_openai_auth"], false);
    assert_eq!(deepseek["supports_websockets"], false);
    assert!(
        effective["model_providers"].get("base_only").is_some(),
        "the unrelated base provider must remain loaded: {read}"
    );

    let configured_base_source =
        configured.start_source(&home, BASE_MODEL, BASE_PROVIDER, BASE_EFFORT);
    let configured_source = configured_base_source["thread"]["id"]
        .as_str()
        .expect("the configured base source names its thread")
        .to_owned();
    let configured_turn = configured.start_turn(
        &configured_source,
        "complete the configured base source turn",
    );
    configured.wait_for_completed_turn(&configured_source, &configured_turn);
    assert_eq!(
        provider_turns.load(Ordering::SeqCst),
        2,
        "the configured base source turn must reach the owned provider exactly once"
    );
    let configured_source_turns = configured.turn_started_for(&configured_source);
    let configured_fork = configured.fork(&home, &configured_source, MODEL, PROVIDER, EFFORT);
    assert!(
        configured_fork.get("error").is_none(),
        "the configured profile still refused native fork: {configured_fork}"
    );
    let forked = &configured_fork["result"];
    assert_route(forked, MODEL, PROVIDER, EFFORT);
    let fork_thread = forked["thread"]["id"]
        .as_str()
        .expect("configured fork names its thread");
    assert_ne!(fork_thread, configured_source.as_str());
    configured.pump(fork_thread);
    assert_eq!(
        configured.turn_started_for(fork_thread),
        0,
        "the configured fork started a turn: {:?}",
        configured.notifications
    );
    assert_eq!(
        configured.turn_started_for(&configured_source),
        configured_source_turns,
        "the configured fork started another source turn: {:?}",
        configured.notifications
    );
    assert_eq!(
        provider_turns.load(Ordering::SeqCst),
        2,
        "the configured fork reached the provider"
    );

    fs::write(
        evidence.path().join("profile-fork-evidence.json"),
        serde_json::to_vec_pretty(&json!({
            "nativeVersion": version,
            "profile": PROFILE,
            "profileOverrides": overrides,
            "unconfigured": {
                "sourceThread": source_id,
                "sourceTurn": source_turn,
                "sourceProviderTurns": 1,
                "missingProviderFork": refused,
                "sourceTurnStartsAfterFork": unconfigured_source_turns,
            },
            "configured": {
                "configRead": read,
                "baseSourceThread": configured_source,
                "sourceTurn": configured_turn,
                "forkThread": fork_thread,
                "forkRoute": {
                    "model": forked["model"],
                    "modelProvider": forked["modelProvider"],
                    "reasoningEffort": forked["reasoningEffort"],
                },
                "sourceTurnStarts": configured_source_turns,
                "forkTurnStarts": configured.turn_started_for(fork_thread),
                "providerTurns": provider_turns.load(Ordering::SeqCst),
            },
        }))
        .unwrap(),
    )
    .unwrap();
}
