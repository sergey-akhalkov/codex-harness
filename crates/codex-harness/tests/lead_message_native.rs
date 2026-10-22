#![cfg(windows)]

//! Model-free native checks around managed and ordinary lead input.
//!
//! `installed_queue_endpoint_reaches_its_owning_thread` is the positive
//! control for the installed upstream `codex queue` command against its own
//! idle thread. `installed_queue_boundary_against_a_running_ordinary_tui`
//! documents the upstream queue boundary that made the former queue fallback
//! invalid for operational messages. Neither is acceptance for `lead message`:
//! that command no longer starts the queue, and without a verified live managed
//! endpoint it refuses before any effect with the same-thread managed resume
//! remedy. The non-ignored `executor_message` regression owns that refusal;
//! Root owns verification against the real installed route.
//!
//! The executable comes from `HARNESS_CONTROL_CODEX_EXE` at runtime. These
//! checks start that fixture's existing app-server; they do not add a listener,
//! daemon, launcher change, or task_control.
//!
//! `installed_two_leads_keep_questions_waiting_and_replies_isolated` is the
//! installed two-lead isolation acceptance: two independent lead roots, each
//! with its own native lead session, its own owned canned Responses provider
//! and its own fresh `codex-harness executor spawn` of the installed entry
//! point. That dispatch itself records the originating lead, inherits the
//! registered lead endpoint, synchronizes the checkout's pool slot, hosts the
//! native executor session and writes the receipt, lease and control endpoint;
//! nothing about the executor is handwritten by this check. The questions are
//! issued by the executor's own native session through the installed entry point
//! and the answer is issued by the lead's own native session; every delivery is
//! established by the receiving conversation's own provider input, never by a
//! local record alone.

#[path = "fixtures/control_responses.rs"]
mod control_responses;

#[path = "fixtures/skill_catalog_responses.rs"]
mod skill_catalog_responses;

use harness_core::{
    broker_state::BrokerRoot,
    build_identity,
    console::{ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, ProcessIdentity, StopReason},
    process_service::{self, ServiceProcess},
    task_control::ControlConnection,
    task_request::RequestIdentity,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Ordinary-lead queue boundary markers: one per installed `codex queue`
/// invocation, so a delivery can only be attributed to the invocation that
/// carried its mark, plus the control turn that establishes the running
/// conversation's own thread before any queue attempt.
const ORDINARY_QUEUE_MARK: &str = "ORDINARY-LEAD-QUEUE-NO-ENDPOINT-8me13";
const ENDPOINT_QUEUE_MARK: &str = "ORDINARY-LEAD-QUEUE-ENDPOINT-8me13";
const MISSING_QUEUE_MARK: &str = "ORDINARY-LEAD-QUEUE-MISSING-THREAD-8me13";
const ORDINARY_TUI_CONTROL_PROMPT: &str = "ORDINARY-LEAD-TUI-CONTROL-PROMPT-8me13";
const ORDINARY_TUI_CONTROL_REPLY: &str = "ORDINARY-LEAD-TUI-CONTROL-REPLY-8me13";
/// A well-formed thread id the running receiver does not own.
const MISSING_THREAD_ID: &str = "00000000-0000-4000-8000-000000000000";

const WAIT: Duration = Duration::from_secs(20);
/// One fresh dispatch runs the executor shell preflight before its session
/// exists, so the first record takes longer than a watch step.
const DISPATCH_WAIT: Duration = Duration::from_secs(300);
/// An answered executor continues on its own thread and completes; that turn
/// plus the run's own closure is bounded well below a live model call.
const ANSWER_WAIT: Duration = Duration::from_secs(120);
const PAYLOAD: &str = "\u{43f}\u{440}\u{438}\u{432}\u{435}\u{442} from the executor\nsecond line: $(throw), %PATH%, `whoami`";
const ACTIVE_TURN_PROMPT: &str = "ACTIVE-NATIVE-LEAD-TURN-HOLD-20261017";
const ACTIVE_TOOL_CALL: &str = "control-active-tool-1";
const ENVELOPE_MARK: &str = "\n---payload---\n";
/// One native lead root: how long a request from a conversation the root has
/// not identified yet waits for the dispatch's own session record. The record
/// is written before the assignment is submitted, so a real run never reaches
/// this bound; a foreign conversation does.
const EXECUTOR_IDENTITY_WAIT: Duration = Duration::from_secs(60);

struct Fixture {
    exe: PathBuf,
    root: BrokerRoot,
    workspace: PathBuf,
    port: u16,
    token: String,
    job: Job,
    _responses: control_responses::Responses,
    server: ProcessIdentity,
}

fn codex_exe() -> PathBuf {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    assert!(
        exe.is_absolute() && exe.is_file(),
        "HARNESS_CONTROL_CODEX_EXE must be an absolute file"
    );
    exe
}

fn command(exe: &Path, home: &Path, workspace: &Path) -> CommandSpec {
    let mut command = CommandSpec::new(exe);
    command.current_dir = Some(workspace.into());
    command.env.insert("CODEX_HOME".into(), Some(home.into()));
    for key in [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "CODEX_ACCESS_TOKEN",
        "CODEX_SESSION_ID",
        "CODEX_THREAD_ID",
    ] {
        command.env.insert(key.into(), None);
    }
    command.env.insert(
        "HARNESS_CONTROL_FIXTURE_KEY".into(),
        Some("synthetic-owned-fixture".into()),
    );
    command
}

fn version_text(exe: &Path) -> String {
    let output = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("could not run {} --version: {error}", exe.display()));
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn contains_payload(value: &Value, payload: &str) -> bool {
    match value {
        Value::String(text) => text.contains(payload),
        Value::Array(items) => items.iter().any(|item| contains_payload(item, payload)),
        Value::Object(map) => map.values().any(|item| contains_payload(item, payload)),
        _ => false,
    }
}

/// Copies the installed launcher registration into an owned home: it names the
/// native upstream an owned frontend attaches to, and it is what lets an owned
/// home host the installed native CLI. Copying it adds no listener or launcher.
fn seed_launcher_registration(home: &Path) {
    let source = harness_core::native_launcher::codex_home()
        .expect("installed launcher home")
        .join("harness/native-launch.json");
    assert!(
        source.is_file(),
        "installed launcher registration harness/native-launch.json is missing; the installed native CLI cannot host this fixture"
    );
    let destination = home.join("harness");
    fs::create_dir_all(&destination).unwrap();
    fs::copy(&source, destination.join("native-launch.json")).unwrap_or_else(|error| {
        panic!("could not seed harness/native-launch.json into the fixture home: {error}")
    });
}

/// The native CLI versions the retained lead-message protocol has been run
/// against: the historical pin, plus the installed upstream that was qualified
/// from its own local `app-server --help` (WebSocket transport flags) and its
/// generated protocol schema (initialize, thread/start, thread/read,
/// turn/start, turn/steer, thread/resume). A further upstream version must be
/// qualified the same way before this check claims it.
const QUALIFIED_UPSTREAM_VERSIONS: &[&str] = &["0.157.0", "0.163.0"];

/// Requires the fixture executable to be the installation-record upstream
/// binary at one of the qualified versions and returns the recorded version
/// text: the fixture hosts the installed native CLI, and a different binary or
/// an unqualified upstream version is not this check's contract.
fn qualified_installed_upstream(exe: &Path) -> String {
    let upstream = installed_upstream();
    assert!(
        same_executable(exe, &upstream),
        "HARNESS_CONTROL_CODEX_EXE {} must be the installation-record upstream executable {}; this check runs the installed native CLI, not another binary",
        exe.display(),
        upstream.display()
    );
    let version = version_text(exe);
    let qualified = QUALIFIED_UPSTREAM_VERSIONS
        .iter()
        .any(|candidate| version.contains(&format!("codex-cli {candidate}")));
    assert!(
        qualified,
        "the installation-record upstream must be a qualified Codex CLI version (one of {QUALIFIED_UPSTREAM_VERSIONS:?}): {version}"
    );
    version
}

fn fixture() -> Fixture {
    let owned_root = BrokerRoot::prepare().unwrap().keep();
    let responses = control_responses::Responses::start(owned_root.path().into(), true);
    fixture_with_responses(owned_root, responses)
}

fn fixture_with_responses(
    owned_root: BrokerRoot,
    responses: control_responses::Responses,
) -> Fixture {
    let exe = codex_exe();
    qualified_installed_upstream(&exe);
    let root = owned_root.path();
    let home = root.join("home");
    let workspace = root.join("workspace");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&workspace).unwrap();
    seed_launcher_registration(&home);
    let trusted = serde_json::to_string(&workspace.to_string_lossy()).unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            r#"
model = "gpt-6-astra"
model_provider = "control_fixture"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
cli_auth_credentials_store = "file"
[model_providers.control_fixture]
name = "Owned native controller fixture"
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
multi_agent = true
multi_agent_v2 = false
[analytics]
enabled = false
[tui]
screen_reader_detection_done = true
[projects.{trusted}]
trust_level = "trusted"
"#,
            responses.port
        ),
    )
    .unwrap();
    let token = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
    fs::write(root.join("ws-token"), &token).unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut spec = command(&exe, &home, &workspace);
    spec.args = vec![
        "app-server".into(),
        "--listen".into(),
        format!("ws://127.0.0.1:{port}").into(),
        "--ws-auth".into(),
        "capability-token".into(),
        "--ws-token-file".into(),
        root.join("ws-token").into_os_string(),
    ];
    spec.stdout = Some(fs::File::create(root.join("server-stdout.txt")).unwrap());
    spec.stderr = Some(fs::File::create(root.join("server-stderr.txt")).unwrap());
    let job = Job::new(Limits {
        memory_bytes: Some(1024 * 1024 * 1024),
        cpu_percent: Some(50.0),
    })
    .unwrap();
    let server = job.spawn(&spec).unwrap();
    let until = Instant::now() + WAIT;
    while ControlConnection::connect(port, &token, Duration::from_millis(100)).is_err() {
        assert!(
            Instant::now() < until,
            "existing native app-server fixture did not accept its own listener; {}",
            fs::read_to_string(root.join("server-stderr.txt")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let server = server.identity();
    Fixture {
        exe,
        root: owned_root,
        workspace,
        port,
        token,
        job,
        _responses: responses,
        server,
    }
}

struct Client {
    connection: ControlConnection,
    id: u64,
}

impl Client {
    fn connect(port: u16, token: &str) -> Self {
        let mut client = Self {
            connection: ControlConnection::connect(port, token, WAIT).unwrap(),
            id: 0,
        };
        client.request(
            "initialize",
            json!({"clientInfo":{"name":"harness-control-contract","version":"1"},"capabilities":{"experimentalApi":true}}),
        );
        client
            .connection
            .send(&json!({"method": "initialized"}), WAIT)
            .unwrap();
        client
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.connection
            .send(&json!({"id": id, "method": method, "params": params}), WAIT)
            .unwrap();
        let until = Instant::now() + WAIT;
        loop {
            assert!(Instant::now() < until, "{method} was not answered");
            let Some(value) = self.connection.receive(Duration::from_millis(200)).unwrap() else {
                continue;
            };
            if value["id"] == id {
                assert!(value.get("error").is_none(), "{method}: {value}");
                return value["result"].clone();
            }
        }
    }
}

fn record_native_lead(fixture: &Fixture, thread: &str, generation: &str) -> PathBuf {
    let root = fixture.root.path();
    let user = process_service::current_user().unwrap();
    let server = ServiceProcess::inspect(fixture.server, &fixture.exe, &user)
        .unwrap()
        .expect("the fixture app-server process is not the installed executable");
    let host_program = std::env::current_exe().unwrap();
    let host = ServiceProcess::observe(std::process::id(), &host_program, 0, &user)
        .unwrap()
        .identity();
    let state = root.join("run-home/harness/executor-pool/native-lead");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        state.join("spawn-1.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "originatingLead": {
                "schema": 1,
                "threadId": thread,
                "runGeneration": generation,
                "dispatcher": {
                    "pid": host.pid,
                    "creationTime": host.creation_time,
                    "program": host_program,
                }
            },
            "slot": {
                "index": 1,
                "path": fixture.workspace,
                "source": fixture.workspace,
                "owner": "exec-native-lead",
            },
            "observation": {
                "schema": 1,
                "state": "running",
                "session": "executor-session-synthetic",
                "host": {
                    "pid": host.pid,
                    "created": host.creation_time,
                    "program": host_program,
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        state.join("lead-endpoint-1.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "port": fixture.port,
            "token": fixture.token,
            "threadId": thread,
            "process": {
                "pid": server.identity().pid,
                "creationTime": server.identity().creation_time,
                "program": fixture.exe,
            }
        }))
        .unwrap(),
    )
    .unwrap();
    state
}

fn send_lead_message(
    harness: &Path,
    home: &Path,
    generation: &str,
    payload: &str,
) -> std::process::Output {
    Command::new(harness)
        .args(["lead", "message", "--text", payload])
        .env("CODEX_HOME", home)
        .env("HARNESS_EXECUTOR_RUN", generation)
        .env("HARNESS_EXECUTOR_SESSION", "1")
        .env_remove("HARNESS_ORIGINATING_LEAD")
        .env_remove("HARNESS_LEAD_THREAD")
        .env_remove("HARNESS_LEAD_RECIPIENT")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn record_harness_selection(root: &Path, harness: &Path, arm: &str) {
    assert!(
        harness.is_absolute() && harness.is_file(),
        "the {arm} harness executable must be an explicit absolute file: {}",
        harness.display()
    );
    let selected = native_path(&harness.canonicalize().unwrap());
    let sha256 = build_identity::hash_file(&selected).unwrap();
    fs::write(
        root.join("harness-selection.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "arm": arm,
            "path": selected,
            "sha256": sha256,
        }))
        .unwrap(),
    )
    .unwrap();
}

struct ActiveToolRelease(PathBuf);

impl Drop for ActiveToolRelease {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, b"release");
    }
}

fn active_tool_continuation(request: &Value) -> bool {
    request["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["type"] == "function_call_output"
                && item["call_id"] == ACTIVE_TOOL_CALL
                && item["output"].as_str().is_some_and(|output| {
                    output.contains("Process exited with code 0") && output.contains("released")
                })
        })
    })
}

fn delivered_envelope(value: &Value) -> Option<(Value, String)> {
    match value {
        Value::String(text) => text
            .split_once(ENVELOPE_MARK)
            .and_then(|(header, payload)| {
                serde_json::from_str(header)
                    .ok()
                    .map(|header| (header, payload.to_owned()))
            }),
        Value::Array(items) => items.iter().find_map(delivered_envelope),
        Value::Object(map) => map.values().find_map(delivered_envelope),
        _ => None,
    }
}

fn wait_for_active_continuation(root: &Path, thread: &str, turn: &str) -> Value {
    let until = Instant::now() + WAIT;
    loop {
        if let Some((_, request, hit_thread, hit_turn)) =
            provider_entries(root)
                .into_iter()
                .find(|(_, request, hit_thread, hit_turn)| {
                    hit_thread == thread
                        && hit_turn == turn
                        && active_tool_continuation(request)
                        && contains_payload(request, PAYLOAD)
                })
        {
            assert_eq!(hit_thread, thread);
            assert_eq!(hit_turn, turn);
            return request;
        }
        assert!(
            Instant::now() < until,
            "the active tool did not continue on the original turn {turn}; provider evidence {:?}",
            provider_entries(root)
                .into_iter()
                .map(|(name, _, thread, turn)| (name, thread, turn))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Reads every native turn of one thread through the paginated RPC the
/// installed app-server actually implements. `thread/read includeTurns`
/// reaches its unimplemented `list_turns` path (-32601, "list_turns is not
/// supported yet"), so completion and source-prefix checks must not depend on
/// it.
fn thread_turns(owner: &mut Client, thread: &str) -> Vec<Value> {
    let mut turns = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..64 {
        let mut params = json!({
            "threadId": thread,
            "itemsView": "full",
            "sortDirection": "asc",
            "limit": 100,
        });
        if let Some(cursor) = &cursor {
            params["cursor"] = json!(cursor);
        }
        let page = owner.request("thread/turns/list", params);
        turns.extend(page["data"].as_array().cloned().unwrap_or_default());
        cursor = page["nextCursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            return turns;
        }
    }
    panic!("thread/turns/list did not terminate for {thread}");
}

fn wait_for_turn_completion(owner: &mut Client, thread: &str, turn: &str) -> Value {
    let until = Instant::now() + WAIT;
    loop {
        let state = owner.request("thread/read", json!({"threadId": thread}));
        let turns = thread_turns(owner, thread);
        let completed = turns
            .iter()
            .any(|candidate| candidate["id"] == turn && candidate["status"] == "completed");
        let returned = json!({"thread": {
            "status": state["thread"]["status"].clone(),
            "turns": turns,
        }});
        if completed && state["thread"]["status"]["type"] == "idle" {
            return returned;
        }
        assert!(
            Instant::now() < until,
            "the original active turn {turn} did not complete after its tool was released: {returned}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned native app-server fixture and canned Responses; no live model"]
fn lead_message_reaches_the_native_owning_thread() {
    let fixture = fixture();
    let root = fixture.root.path();
    let mut owner = Client::connect(fixture.port, &fixture.token);
    let started = owner.request(
        "thread/start",
        json!({
            "cwd": fixture.workspace,
            "model": "gpt-6-astra",
            "modelProvider": "control_fixture",
            "allowProviderModelFallback": false,
            "approvalPolicy": "never",
            "sandbox": "danger-full-access"
        }),
    );
    let thread = started["thread"]["id"].as_str().unwrap().to_owned();
    let status = owner.request("thread/read", json!({"threadId": thread}));
    assert_eq!(status["thread"]["id"], thread);
    assert_eq!(status["thread"]["status"]["type"], "idle", "{status}");
    let generation = "generation-native-lead";
    record_native_lead(&fixture, &thread, generation);
    let harness = PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"));
    let sent = send_lead_message(&harness, &root.join("run-home"), generation, PAYLOAD);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&sent.stdout),
        String::from_utf8_lossy(&sent.stderr)
    );
    assert!(
        sent.status.success(),
        "lead message failed against the existing fixture listener; {text}; evidence {}",
        root.display()
    );
    assert!(
        text.contains("method: turn/start") || text.contains("method: turn/steer"),
        "{text}"
    );
    assert!(text.contains(&format!("leadThreadId: {thread}")), "{text}");
    assert!(!text.contains(&fixture.token), "{text}");
    let until = Instant::now() + Duration::from_secs(25);
    let mut provider_hit = false;
    while Instant::now() < until {
        for entry in fs::read_dir(root).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("provider-") && name.ends_with(".json")) {
                continue;
            }
            let body = fs::read(entry.path()).unwrap_or_default();
            let request = serde_json::from_slice::<Value>(&body).unwrap();
            if !contains_payload(&request, PAYLOAD) {
                continue;
            }
            let identity = RequestIdentity::from_request(&request).unwrap();
            assert_eq!(identity.thread, thread, "{name}");
            provider_hit = true;
            break;
        }
        if provider_hit {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        provider_hit,
        "owning thread {thread} provider input did not keep the unchanged payload; evidence {}",
        root.display()
    );
    drop(fixture.job);
}

/// Canned controls use the same shell and patch tools as the read-only fork.
/// The writable source proves the operations are valid; the fork must retain
/// its permitted read while both mutations are refused. No real model runs.
fn reflection_probe_response(request: &Value, proof: &Path) -> Value {
    let reflection = contains_payload(request, "Bounded read-only executor self-reflection.");
    let probing = reflection || contains_payload(request, "accepted executor work");
    let prefix = if reflection { "reflection" } else { "source" };
    let shell_call = format!("{prefix}-read-only-proof");
    let write_call = format!("{prefix}-write-proof");
    let patch_call = format!("{prefix}-patch-proof");
    let completed = |call: &str| {
        request["input"].as_array().is_some_and(|items| {
            items.iter().any(|item| {
                matches!(
                    item["type"].as_str(),
                    Some("function_call_output" | "custom_tool_call_output")
                ) && item["call_id"] == call
            })
        })
    };
    if probing && (!completed(&shell_call) || !completed(&write_call)) {
        let reading = !completed(&shell_call);
        let target = if reflection {
            proof.to_path_buf()
        } else {
            proof.with_file_name("writable-control.txt")
        };
        let path = native_path(&target).to_string_lossy().replace('\'', "''");
        let command = if reading {
            format!("Get-Content -LiteralPath '{path}' -Raw")
        } else {
            format!(
                "Set-Content -LiteralPath '{path}' -Value 'forbidden-write' -NoNewline -ErrorAction Stop"
            )
        };
        let arguments = serde_json::to_string(&json!({
            "cmd":command,
            "shell":control_responses::owned_pwsh(),
            "login":false,
            "yield_time_ms":10000,
            "max_output_tokens":1000,
        }))
        .unwrap();
        return json!({
            "type":"custom_tool_call", "call_id":if reading { shell_call } else { write_call },
            "namespace":"functions", "name":"exec",
            "input":format!("const result = await tools.exec_command({arguments}); text(result);"),
        });
    }
    if probing && !completed(&patch_call) {
        let target = proof.with_file_name(format!("{prefix}-patch-proof.txt"));
        let path = native_path(&target).to_string_lossy().replace('\\', "/");
        let patch = format!("*** Begin Patch\n*** Add File: {path}\n+patch-write\n*** End Patch");
        let input = serde_json::to_string(&patch).unwrap();
        return json!({
            "type":"custom_tool_call", "call_id":patch_call,
            "namespace":"functions", "name":"exec",
            "input":format!("const result = await tools.apply_patch({input}); text(result);"),
        });
    }
    json!({"type":"message","id":"msg-native-reflection","role":"assistant",
        "content":[{"type":"output_text","text":"NO FINDING"}]})
}

/// The checkout's real self-reflection entry point against the installed native
/// app-server and frontend. Only Responses are canned; source threads, forks,
/// tool catalogues, retained turns and delivery use native components.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE and native frontend; owned canned Responses and MCP; no live model"]
fn self_reflection_forks_use_native_read_only_catalogues_and_return_to_the_lead() {
    let owned_root = BrokerRoot::prepare().unwrap().keep();
    let proof = owned_root.path().join("workspace/readonly-proof.txt");
    let provider_proof = proof.clone();
    let responses =
        control_responses::Responses::routed(owned_root.path().into(), move |request| {
            reflection_probe_response(request, &provider_proof)
        });
    let fixture = fixture_with_responses(owned_root, responses);
    let root = fixture.root.path();
    let home = root.join("home");
    let checkout = native_path(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap(),
    );
    println!("native reflection evidence: {}", root.display());
    let proof_contents = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
    fs::write(&proof, &proof_contents).unwrap();
    fs::write(
        proof.with_file_name("writable-control.txt"),
        &proof_contents,
    )
    .unwrap();
    let config_path = home.join("config.toml");
    let unrelated = skill_catalog_responses::unrelated_probe_mcp(&home).replace(
        "mcp_servers.unrelated_probe",
        "mcp_servers.\"unrelated.probe\"",
    );
    let original_config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        // This isolated home uses the native restricted-token backend without
        // provisioning machine-wide users. Installed elevated-backend acceptance
        // remains a separate check; no owner sandbox configuration is changed.
        format!("{original_config}\n{unrelated}\n[windows]\nsandbox = \"unelevated\"\n[profiles.reflection_fixture]\nmodel = \"gpt-6-astra\"\nmodel_provider = \"control_fixture\"\nmodel_reasoning_effort = \"low\"\n"),
    ).unwrap();
    let config_before = fs::read(&config_path).unwrap();
    let mcp_config: toml::Table = unrelated.parse().unwrap();
    let mut owner = Client::connect(fixture.port, &fixture.token);
    let mut sources = Vec::new();
    for label in ["accepted executor work", "lead acceptance"] {
        let source = owner.request(
            "thread/start",
            json!({
                "cwd": fixture.workspace,
                "model":"gpt-6-astra", "modelProvider":"control_fixture",
                "allowProviderModelFallback":false, "approvalPolicy":"never",
                "sandbox":"danger-full-access", "config":mcp_config,
            }),
        );
        let thread = source["thread"]["id"].as_str().unwrap().to_owned();
        let turn = owner.request(
            "turn/start",
            json!({
                "threadId":thread,
                "input":[{"type":"text","text":label}],
            }),
        )["turn"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let completed = wait_for_turn_completion(&mut owner, &thread, &turn);
        let catalogue = owner.request(
            "mcpServerStatus/list",
            json!({
                "threadId":thread, "detail":"toolsAndAuthOnly",
            }),
        );
        assert!(
            catalogue["data"].as_array().unwrap().iter().any(|server| {
                server["name"] == "unrelated.probe"
                    && server["tools"]
                        .as_object()
                        .is_some_and(|tools| !tools.is_empty())
            }),
            "source must expose the actual MCP before suppression: {catalogue}"
        );
        // A paused goal is ordinary live source state. The reflection fork must
        // leave the source goal and its turns untouched, and its own goal
        // exposure must stay suppressed so no automatic continuation starts.
        let goal = owner.request(
            "thread/goal/set",
            json!({
                "threadId":thread,
                "objective":format!("preserve the accepted {label} source work"),
                "status":"paused",
            }),
        )["goal"]
            .clone();
        assert_eq!(goal["status"], "paused", "{goal}");
        let after_goal = thread_turns(&mut owner, &thread);
        assert_eq!(
            Value::Array(after_goal),
            completed["thread"]["turns"],
            "setting a paused goal started an automatic continuation"
        );
        sources.push((thread, turn, completed["thread"]["turns"].clone(), goal));
    }
    assert_eq!(
        fs::read_to_string(proof.with_file_name("writable-control.txt")).unwrap(),
        "forbidden-write",
        "the ordinary native source must execute the writable shell control"
    );
    assert_eq!(
        fs::read_to_string(proof.with_file_name("source-patch-proof.txt"))
            .unwrap()
            .trim(),
        "patch-write",
        "the ordinary native source must execute the writable patch control"
    );
    let endpoint_dir = record_native_lead(&fixture, &sources[1].0, "native-reflection-return");
    let state_dir = root.join("reflection-state");
    let stage = |role: harness_core::role_context::Role| {
        let resolved = harness_core::role_context::resolve(&checkout, role).unwrap();
        json!({
            "role":role.label(), "revision":resolved.revision,
            "profile":"reflection_fixture", "model":"gpt-6-astra",
            "provider":"control_fixture", "effort":"low",
            "checkout":fixture.workspace, "title":format!("native model-free {}", role.label()),
        })
    };
    let request = json!({
        "schema":1, "task":"owned accepted task", "run":"native-reflection-chain",
        "acceptance":"owned native completed checkpoints", "source":checkout,
        "codexHome":home, "launcher":fixture.exe, "stateDir":state_dir,
        "executorSource":{"thread":sources[0].0,"lastTurn":sources[0].1},
        "leadSource":{"thread":sources[1].0,"lastTurn":sources[1].1},
        "executorStage":stage(harness_core::role_context::Role::ExecutorSelfReflection),
        "leadStage":stage(harness_core::role_context::Role::LeadSelfReflection),
        "leadReturn":{"endpoint":endpoint_dir.join("lead-endpoint-1.json"),"thread":sources[1].0},
        "boundSeconds":45,
    });
    let request_path = root.join("reflection-request.json");
    fs::write(&request_path, serde_json::to_vec_pretty(&request).unwrap()).unwrap();
    let mut spec = command(
        Path::new(env!("CARGO_BIN_EXE_codex-harness")),
        &home,
        &fixture.workspace,
    );
    spec.args = vec![
        "self-reflection".into(),
        "--request".into(),
        request_path.into_os_string(),
    ];
    for key in [
        "HARNESS_REFLECTION_SESSION",
        "HARNESS_EXECUTOR_SESSION",
        "HARNESS_EXECUTOR_RUN",
        "HARNESS_CONTEXT_ROLE",
        "HARNESS_CONTEXT_REVISION",
        "WT_SESSION",
    ] {
        spec.env.insert(key.into(), None);
    }
    let session = ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap();
    let outcome = session
        .wait(
            Deadline::after(Duration::from_secs(150)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    fs::write(root.join("reflection-terminal.txt"), &outcome.transcript).unwrap();
    assert_eq!(
        outcome.outcome.exit_code,
        0,
        "native reflection failed; evidence {}: {}",
        root.display(),
        outcome.transcript
    );
    assert_eq!(fs::read(&config_path).unwrap(), config_before);
    for (index, label) in ["executor", "lead"].into_iter().enumerate() {
        let retained: Value =
            read_json(&state_dir.join(format!("{label}-stage.json"))).expect("retained stage");
        assert_eq!(retained["result"], "NO FINDING", "{retained}");
        let fork = &retained["fork"];
        let fork_id = fork["forkThread"].as_str().unwrap();
        assert_ne!(fork_id, sources[index].0);
        assert_eq!(fork["sourcePreserved"], true, "{fork}");
        let seeded = &sources[index].3;
        for boundary in ["sourceGoalBefore", "sourceGoalAfter"] {
            assert_eq!(fork[boundary]["threadId"], sources[index].0, "{fork}");
            assert_eq!(fork[boundary]["objective"], seeded["objective"], "{fork}");
            assert_eq!(fork[boundary]["status"], "paused", "{fork}");
        }
        assert_eq!(
            fork["sourceGoalAfter"], fork["sourceGoalBefore"],
            "the fork changed the live source goal: {fork}"
        );
        // The production fork sets `features.goals=false` in its own thread
        // config. Native `thread/fork` therefore copies the source turns but
        // exposes no goal on the fork at all, so the fork has no inherited goal
        // whose continuation could start. A goals-enabled fork of the same
        // source does inherit the paused goal, which is why the fork-local
        // switch is the enforced boundary rather than `deferGoalContinuation`
        // (that forward-compatible field is not in this upstream's schema).
        assert_eq!(fork["forkGoalBeforeClear"], Value::Null, "{fork}");
        assert_eq!(fork["forkGoalAfterClear"], Value::Null, "{fork}");
        assert_eq!(fork["goalCleared"], false, "{fork}");
        assert_eq!(fork["toolSurface"]["verified"], true, "{fork}");
        assert_eq!(
            fork["suppressedMcpServers"],
            json!(["unrelated.probe"]),
            "{fork}"
        );
        assert_eq!(fork["sourceThread"], sources[index].0, "{fork}");
        let requests: Vec<Value> = fs::read_dir(root)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("provider-"))
            .map(|entry| read_json(&entry.path()).expect("retained provider request"))
            .filter(|body| {
                RequestIdentity::from_request(body).is_ok_and(|identity| identity.thread == fork_id)
            })
            .collect();
        assert!(
            !requests.is_empty(),
            "no native provider input for {fork_id}"
        );
        if index == 0 {
            assert!(
                requests.iter().any(|body| {
                    body["input"].as_array().is_some_and(|items| {
                        items.iter().any(|item| {
                            matches!(
                                item["type"].as_str(),
                                Some("function_call_output" | "custom_tool_call_output")
                            ) && item["call_id"] == "reflection-read-only-proof"
                                && contains_payload(&item["output"], &proof_contents)
                        })
                    })
                }),
                "the native read-only tool must perform a real permitted read; evidence {}",
                root.display()
            );
            assert_eq!(
                fs::read_to_string(&proof).unwrap(),
                proof_contents,
                "the read-only reflection changed its owned proof file"
            );
            assert!(
                !proof.with_file_name("reflection-patch-proof.txt").exists(),
                "the read-only reflection wrote through apply_patch"
            );
            for call in ["reflection-write-proof", "reflection-patch-proof"] {
                assert!(
                    requests
                        .iter()
                        .any(|body| body["input"].as_array().is_some_and(|items| items
                            .iter()
                            .any(|item| matches!(
                                item["type"].as_str(),
                                Some("function_call_output" | "custom_tool_call_output")
                            ) && item["call_id"] == call))),
                    "the native mutation attempt {call} must return its own result"
                );
            }
        }
        for body in requests {
            let surface = control_responses::tool_catalogue::tool_surface(&body.to_string());
            assert!(
                !surface.1.is_empty(),
                "missing native tool surface for {fork_id}"
            );
            assert!(
                !surface.1.contains("unrelated.probe") && !surface.1.contains("unrelated_probe"),
                "{surface:?}"
            );
            assert_eq!(
                RequestIdentity::from_request(&body).unwrap().turn,
                retained["turnId"].as_str().unwrap(),
                "the reflection fork ran a turn other than its recorded stage turn: {body}"
            );
        }
        let fork_turns = thread_turns(&mut owner, fork_id);
        let copied = sources[index].2.as_array().unwrap();
        assert_eq!(
            fork_turns.len(),
            copied.len() + 1,
            "the reflection fork ran an automatic continuation: {fork_turns:?}"
        );
        assert_eq!(
            &fork_turns[..copied.len()],
            copied,
            "the fork did not preserve the exact source checkpoint: {fork_turns:?}"
        );
        assert_eq!(
            fork_turns.last().unwrap()["id"],
            retained["turnId"],
            "{fork_turns:?}"
        );
        let turns = thread_turns(&mut owner, &sources[index].0);
        let original = sources[index].2.as_array().unwrap();
        assert_eq!(
            &turns[..original.len()],
            original,
            "the source checkpoint prefix moved: {turns:?}"
        );
        if index == 0 {
            assert_eq!(
                turns.len(),
                original.len(),
                "the executor source gained an automatic goal continuation: {turns:?}"
            );
        }
        let catalogue = owner.request(
            "mcpServerStatus/list",
            json!({
                "threadId":sources[index].0, "detail":"toolsAndAuthOnly",
            }),
        );
        assert!(
            catalogue["data"].as_array().unwrap().iter().any(|server| {
                server["name"] == "unrelated.probe"
                    && server["tools"]
                        .as_object()
                        .is_some_and(|tools| !tools.is_empty())
            }),
            "source MCP must survive the fork: {catalogue}"
        );
    }
    let delivery: Value =
        read_json(&state_dir.join("lead-delivery.json")).expect("retained delivery");
    assert_eq!(delivery["state"], "delivered", "{delivery}");
    assert!(
        fs::read_dir(root)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("provider-"))
            .map(|entry| read_json(&entry.path()).expect("retained provider request"))
            .any(|body| RequestIdentity::from_request(&body)
                .is_ok_and(|identity| identity.thread == sources[1].0)
                && contains_payload(&body, "native-reflection-chain")),
        "the actual lead provider input must consume the synthesis"
    );
}

fn lead_message_steers_the_active_native_owning_thread_with(harness: &Path, arm: &str) {
    let owned_root = BrokerRoot::prepare().unwrap().keep();
    let evidence = owned_root.path().to_path_buf();
    let responses = control_responses::Responses::routed(evidence, move |request| {
        if title_generation_request(request) {
            return json!({"type":"message","id":"msg-active-title","role":"assistant","content":[{"type":"output_text","text":"Active native turn"}]});
        }
        if !contains_payload(request, ACTIVE_TURN_PROMPT) {
            return json!({"type":"message","id":"msg-active-unexpected","role":"assistant","content":[{"type":"output_text","text":"UNEXPECTED_ACTIVE_PROVIDER_REQUEST"}]});
        }
        let tool_completed = request["input"].as_array().is_some_and(|items| {
            items.iter().any(|item| {
                item["type"] == "function_call_output" && item["call_id"] == ACTIVE_TOOL_CALL
            })
        });
        if tool_completed {
            return json!({"type":"message","id":"msg-active-final","role":"assistant","content":[{"type":"output_text","text":"ACTIVE_NATIVE_TURN_COMPLETE"}]});
        }
        let command = "[IO.File]::WriteAllText('active-tool-ready', 'ready'); for ($n = 0; $n -lt 600 -and !(Test-Path 'active-tool-release'); $n++) { Start-Sleep -Milliseconds 100 }; if (!(Test-Path 'active-tool-release')) { throw 'active tool release deadline' }; [IO.File]::WriteAllText('active-tool-done', 'done'); 'released'";
        let arguments = serde_json::to_string(&json!({
            "cmd": command,
            "shell": control_responses::owned_pwsh(),
            "yield_time_ms": 30000
        }))
        .unwrap();
        json!({"type":"function_call","call_id":ACTIVE_TOOL_CALL,"name":"exec_command","arguments":arguments})
    });
    let fixture = fixture_with_responses(owned_root, responses);
    let root = fixture.root.path();
    let workspace = fixture.workspace.clone();
    record_harness_selection(root, harness, arm);
    let release_marker = workspace.join("active-tool-release");
    let _release_on_drop = ActiveToolRelease(release_marker.clone());

    let mut owner = Client::connect(fixture.port, &fixture.token);
    let started = owner.request(
        "thread/start",
        json!({
            "cwd": fixture.workspace,
            "model": "gpt-6-astra",
            "modelProvider": "control_fixture",
            "allowProviderModelFallback": false,
            "approvalPolicy": "never",
            "sandbox": "danger-full-access"
        }),
    );
    let thread = started["thread"]["id"].as_str().unwrap().to_owned();
    let turn_started = owner.request(
        "turn/start",
        json!({
            "threadId": thread,
            "input": [{"type": "text", "text": ACTIVE_TURN_PROMPT}]
        }),
    );
    let turn = turn_started["turn"]["id"]
        .as_str()
        .or_else(|| turn_started["id"].as_str())
        .unwrap_or_else(|| panic!("turn/start answered without a turn identity: {turn_started}"))
        .to_owned();
    assert!(
        wait_until(WAIT.as_secs(), || workspace
            .join("active-tool-ready")
            .is_file()),
        "the owned native tool did not reach its readiness marker; provider {:?}; server {}",
        provider_entries(root)
            .into_iter()
            .map(|(name, _, thread, turn)| (name, thread, turn))
            .collect::<Vec<_>>(),
        fs::read_to_string(root.join("server-stderr.txt")).unwrap_or_default()
    );
    let active_turns = thread_turns(&mut owner, &thread);
    let active = active_turns
        .iter()
        .rev()
        .find(|candidate| candidate["status"] == "inProgress")
        .and_then(|candidate| candidate["id"].as_str());
    assert_eq!(active, Some(turn.as_str()), "{active_turns:?}");

    let generation = "generation-native-active-lead";
    record_native_lead(&fixture, &thread, generation);
    let sent = send_lead_message(harness, &root.join("run-home"), generation, PAYLOAD);
    let text = output_text(&sent);
    assert!(
        sent.status.success(),
        "lead message failed against the active fixture turn; {text}; evidence {}",
        root.display()
    );
    assert!(
        text.contains("lead message: delivered") || text.contains("lead message: queued"),
        "{text}"
    );
    let transport_status = if text.contains("lead message: delivered") {
        "delivered"
    } else {
        "queued"
    };
    assert!(text.contains("method: turn/steer"), "{text}");
    assert!(text.contains(&format!("leadThreadId: {thread}")), "{text}");
    assert!(!text.contains(&fixture.token), "{text}");
    let id = text
        .lines()
        .find_map(|line| line.strip_prefix("id: "))
        .unwrap_or_else(|| panic!("the delivery report named no message id: {text}"));
    fs::write(root.join("lead-message-send.txt"), &text).unwrap();
    fs::write(
        root.join("lead-message-transport.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "transportStatus": transport_status,
            "method": "turn/steer",
            "threadId": thread,
            "turnId": turn,
            "messageId": id,
        }))
        .unwrap(),
    )
    .unwrap();

    fs::write(&release_marker, b"release").unwrap();
    assert!(
        wait_until(WAIT.as_secs(), || workspace
            .join("active-tool-done")
            .is_file()),
        "the released native tool did not report completion; provider {:?}; server {}",
        provider_entries(root)
            .into_iter()
            .map(|(name, _, thread, turn)| (name, thread, turn))
            .collect::<Vec<_>>(),
        fs::read_to_string(root.join("server-stderr.txt")).unwrap_or_default()
    );
    let continuation = wait_for_active_continuation(root, &thread, &turn);
    let identity = RequestIdentity::from_request(&continuation).unwrap();
    assert_eq!(identity.thread, thread);
    assert_eq!(identity.turn, turn, "the steer opened another turn");
    let (header, payload) = delivered_envelope(&continuation)
        .unwrap_or_else(|| panic!("the continuation carried no lead envelope: {continuation}"));
    assert_eq!(payload, PAYLOAD, "the payload was not delivered unchanged");
    assert_eq!(header["id"], id, "{header}");
    assert_eq!(header["kind"], "reply-request", "{header}");
    assert_eq!(header["runGeneration"], generation, "{header}");
    assert_eq!(header["leadThreadId"], thread, "{header}");

    let completed = wait_for_turn_completion(&mut owner, &thread, &turn);
    let turns = completed["thread"]["turns"].as_array().unwrap();
    assert_eq!(
        turns.len(),
        1,
        "the steer created a second turn: {completed}"
    );
    assert_eq!(turns[0]["id"], turn, "{completed}");
    assert_eq!(turns[0]["status"], "completed", "{completed}");
    assert!(
        foreign_provider_requests(root, &thread).is_empty(),
        "a provider request reached another conversation: {:?}",
        foreign_provider_requests(root, &thread)
    );
    eprintln!(
        "active native lead: harness {arm} {}; thread {thread}; turn {turn}; transport {transport_status}; tool completed in the steered turn; evidence {}",
        harness.display(),
        root.display()
    );
    drop(fixture.job);
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; checkout-built harness; owned native app-server and canned Responses; no live model"]
fn lead_message_steers_the_active_native_owning_thread() {
    let harness = PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"));
    lead_message_steers_the_active_native_owning_thread_with(&harness, "source");
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE and the deployed harness; owned native app-server and canned Responses; no live model"]
fn lead_message_steers_the_active_native_owning_thread_installed() {
    let harness = installed_entry();
    lead_message_steers_the_active_native_owning_thread_with(&harness, "installed");
}

// ---------------------------------------------------------------------------
// Installed two-lead isolation acceptance
// ---------------------------------------------------------------------------

/// One request of an owned provider, kept for the exact per-root traffic
/// assertion: which native conversation asked, and which recorded exchange the
/// request belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RouteHit {
    thread: String,
    kind: &'static str,
}

/// The recorded exchange of one lead root, shared with its owned provider. Only
/// the root's own lead thread and its own executor session may be answered;
/// every other conversation is recorded as itself so the isolation assertion
/// can name it. The lead session exists before the dispatch, so its thread is
/// known first; the executor's thread is the one the real dispatch records
/// beside the receipt, before it submits the assignment.
#[derive(Default)]
struct LeadRoute {
    lead_thread: String,
    exec_thread: String,
    harness: PathBuf,
    question: String,
    answer: String,
    reply_id: Option<String>,
    /// Both delivering turns are held open for this long so the delivered input
    /// is in the conversation's own items while the command reads its delivery
    /// evidence: acceptance on the wire is never treated as delivery.
    question_hold: Duration,
    reply_hold: Duration,
    hits: Vec<RouteHit>,
}

impl LeadRoute {
    /// Whether this root's own dispatch has already recorded the conversation:
    /// the lead session before the dispatch exists, the executor once the
    /// dispatch wrote its endpoint beside its receipt.
    fn identified(&self, thread: &str) -> bool {
        thread == self.lead_thread || thread == self.exec_thread
    }
}

enum RouteAnswer {
    Final(&'static str),
    Tool {
        call_id: &'static str,
        command: String,
    },
}

/// The owned provider of one lead root. It answers only that root's two
/// recorded conversations: the executor's assignment asks its lead through the
/// installed entry point, the lead's own turn answers through the recorded
/// reply reference, and any third conversation is recorded as `foreign-thread`
/// instead of being answered as one of them.
fn two_lead_router(state: Arc<Mutex<LeadRoute>>) -> impl Fn(&Value) -> Value + Send + 'static {
    move |request: &Value| {
        let identity = RequestIdentity::from_request(request)
            .expect("owned provider request carries one conversation identity");
        let thread = identity.thread.clone();
        // A conversation this root has not identified yet waits for the
        // dispatch's own record instead of being answered as the executor or
        // recorded as a foreign conversation.
        let until = Instant::now() + EXECUTOR_IDENTITY_WAIT;
        while !state.lock().unwrap().identified(&thread) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        let input = input_text(&request["input"]);
        let (answer, hold) = {
            let mut route = state.lock().unwrap();
            let lead = thread == route.lead_thread;
            let exec = thread == route.exec_thread;
            let (kind, answer, hold) = if lead && input.contains("ANSWER-THE-EXECUTOR") {
                if input.contains("lead-reply-1") {
                    (
                        "lead-answer-done",
                        RouteAnswer::Final("LEAD_ANSWERED"),
                        Duration::ZERO,
                    )
                } else {
                    let id = route.reply_id.clone().unwrap_or_default();
                    let command = format!(
                        "& '{}' executor message --reply-to {id} --text '{}'",
                        route.harness.display(),
                        route.answer
                    );
                    (
                        "lead-answer-command",
                        RouteAnswer::Tool {
                            call_id: "lead-reply-1",
                            command,
                        },
                        Duration::ZERO,
                    )
                }
            } else if lead {
                (
                    "lead-question",
                    RouteAnswer::Final("LEAD_ACK"),
                    route.question_hold,
                )
            } else if exec && input.contains(&route.answer) {
                (
                    "exec-reply-continued",
                    RouteAnswer::Final("EXECUTOR_CONTINUED"),
                    route.reply_hold,
                )
            } else if exec && input.contains("lead-message-1") {
                (
                    "exec-waiting",
                    RouteAnswer::Final("EXECUTOR_WAITING"),
                    Duration::ZERO,
                )
            } else if exec {
                let command = format!(
                    "& '{}' lead message --text '{}'",
                    route.harness.display(),
                    route.question
                );
                (
                    "exec-question",
                    RouteAnswer::Tool {
                        call_id: "lead-message-1",
                        command,
                    },
                    Duration::ZERO,
                )
            } else {
                (
                    "foreign-thread",
                    RouteAnswer::Final("FOREIGN_THREAD"),
                    Duration::ZERO,
                )
            };
            route.hits.push(RouteHit { thread, kind });
            (answer, hold)
        };
        if !hold.is_zero() {
            std::thread::sleep(hold);
        }
        match answer {
            RouteAnswer::Final(text) => json!({
                "type": "message",
                "id": "msg-fixture",
                "role": "assistant",
                "content": [{"type": "output_text", "text": text}]
            }),
            RouteAnswer::Tool { call_id, command } => json!({
                "type": "function_call",
                "call_id": call_id,
                "name": "exec_command",
                "arguments": serde_json::to_string(&json!({
                    "cmd": command,
                    "shell": control_responses::owned_pwsh(),
                    "yield_time_ms": 60000
                }))
                .unwrap()
            }),
        }
    }
}

/// The private directory of one root: kept, with its path printed, when the
/// check panics so the failure can be inspected, and removed otherwise.
struct RootEvidence {
    path: PathBuf,
}

impl Drop for RootEvidence {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("two-lead isolation evidence kept: {}", self.path.display());
        } else {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// The recorded address of one native session: the process the run's records
/// name, plus the loopback endpoint of its conversation.
struct SessionAddress {
    identity: ProcessIdentity,
    port: u16,
}

/// One isolated lead root: its own kit home, checkout, owned provider and its
/// own native lead session, plus the fresh executor the installed dispatch
/// started for that lead. Every executor record this check reads - the dispatch
/// receipt, the slot record, the lease, the control endpoint, the inherited lead
/// endpoint and the observation - was written by that dispatch or by the host it
/// owns; none of them is written here.
struct LeadRoot {
    // Processes first: the dispatch is stopped, then the job reaps the owned
    // lead session, before the evidence directory is removed or kept.
    _lead_job: Job,
    evidence: RootEvidence,
    role: &'static str,
    exe: PathBuf,
    entry: PathBuf,
    home: PathBuf,
    workspace: PathBuf,
    checkout: PathBuf,
    worktree: PathBuf,
    state: PathBuf,
    owner: String,
    assignment: String,
    question: String,
    answer: String,
    route: Arc<Mutex<LeadRoute>>,
    _provider: control_responses::Responses,
    lead: SessionAddress,
    executor: SessionAddress,
    lead_client: Client,
    exec_client: Client,
    lead_thread: String,
    exec_thread: String,
    /// The installed `executor spawn` that hosts this root's executor: it owns
    /// the console surface, the lease and the run's lifetime.
    dispatch: Child,
    /// Recorded by that dispatch, never chosen here.
    generation: String,
}

impl LeadRoot {
    /// Builds one isolated lead root and dispatches its own fresh executor
    /// through the installed entry point. Everything about the executor - the
    /// slot, the lease, the receipt with its originating lead, the control
    /// endpoint and the observation - is written by that dispatch, never here.
    fn start(
        role: &'static str,
        exe: &Path,
        entry: &Path,
        question: String,
        answer: String,
    ) -> Self {
        let root = std::env::temp_dir().join(format!(
            "codex-harness-two-lead-{role}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let evidence = RootEvidence { path: root.clone() };
        let home = root.join("home");
        let workspace = root.join("workspace");
        for dir in [&home, &workspace] {
            fs::create_dir_all(dir).unwrap();
        }
        // The installed launcher registration lets an owned home host the
        // installed native CLI and names the native upstream the owned frontend
        // attaches; a synthetic home is neither.
        seed_launcher_registration(&home);
        let launcher = home.join("harness/bin/codex.exe");
        fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        if fs::hard_link(exe, &launcher).is_err() {
            fs::copy(exe, &launcher).unwrap_or_else(|error| {
                panic!("{role}: the fixture home could not install the native CLI: {error}")
            });
        }
        // A real checkout of a real upstream: the dispatch creates and
        // synchronizes the pool slot '<checkout>-wt1' from it itself.
        let upstream = root.join("upstream.git");
        git(
            &root,
            &[
                "init",
                "--bare",
                "-q",
                "--initial-branch=main",
                upstream.to_str().unwrap(),
            ],
        );
        let seed = root.join("seed");
        git(
            &root,
            &[
                "init",
                "-q",
                "--initial-branch=main",
                seed.to_str().unwrap(),
            ],
        );
        git(&seed, &["config", "user.email", "executor@example.test"]);
        git(&seed, &["config", "user.name", "Executor"]);
        fs::create_dir_all(seed.join("global")).unwrap();
        fs::write(seed.join("README.md"), "seed\n").unwrap();
        fs::write(seed.join("global/orchestration.toml"), orchestration()).unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-qm", "seed"]);
        git(&seed, &["remote", "add", "origin", &file_url(&upstream)]);
        git(&seed, &["push", "-q", "origin", "main"]);
        let checkout = root.join("checkout");
        git(
            &root,
            &[
                "clone",
                "-q",
                &file_url(&upstream),
                checkout.to_str().unwrap(),
            ],
        );
        git(
            &checkout,
            &["config", "user.email", "executor@example.test"],
        );
        git(&checkout, &["config", "user.name", "Executor"]);
        let home = home.canonicalize().unwrap();
        let workspace = workspace.canonicalize().unwrap();
        // The resolved spelling the pool itself records for this checkout:
        // canonical, with the Windows verbatim prefix removed.
        let checkout = native_path(&checkout.canonicalize().unwrap());
        // The pool position of slot 1 of this checkout, exactly as the
        // configured pool names it. The dispatch creates it.
        let worktree = checkout.parent().unwrap().join(format!(
            "{}-wt1",
            checkout.file_name().unwrap().to_str().unwrap()
        ));
        let owner = format!("exec-two-lead-{role}");
        let assignment = format!(
            "Apply the recorded contract of this worktree and report its consumed result ({role})."
        );
        let route = Arc::new(Mutex::new(LeadRoute {
            harness: entry.to_path_buf(),
            question: question.clone(),
            answer: answer.clone(),
            question_hold: Duration::from_millis(1500),
            reply_hold: Duration::from_millis(1200),
            ..LeadRoute::default()
        }));
        let provider = control_responses::Responses::routed(
            root.join("provider"),
            two_lead_router(route.clone()),
        );
        write_root_config(&home, provider.port, &[&workspace, &checkout]);
        // This root's own installed native lead session: its own home, its own
        // thread and its own owned provider root.
        let (lead_job, lead, lead_client, lead_thread) =
            launch_native_session(exe, &home, &workspace, &root, "lead", &[]);
        // Its verified endpoint, exactly as a managed `lead start` publishes it.
        // An ordinary lead exposes none, so this check owns the lead session's
        // own address; the dispatch under test is what inherits it.
        write_lead_endpoint(
            &home,
            &lead_thread,
            lead.port,
            &read_token(&root.join("lead-ws-token")),
            &lead.identity,
            exe,
        );
        route.lock().unwrap().lead_thread = lead_thread.clone();
        // The fresh dispatch: the installed entry point, run with the lead's own
        // thread id and kit home and with no executor marker.
        let mut dispatch = lead_shell(entry, &home, &checkout)
            .args([
                "executor",
                "spawn",
                "--source",
                checkout.to_str().unwrap(),
                "--codex-home",
                home.to_str().unwrap(),
                "--workspace",
                worktree.to_str().unwrap(),
                "--owner",
                &owner,
                "--mode",
                "tui",
                "--exec",
                &assignment,
            ])
            .env("CODEX_THREAD_ID", &lead_thread)
            .stdout(fs::File::create(root.join("dispatch-stdout.txt")).unwrap())
            .stderr(fs::File::create(root.join("dispatch-stderr.txt")).unwrap())
            .spawn()
            .unwrap_or_else(|error| {
                panic!("{role}: the installed dispatch could not start: {error}")
            });
        let state = harness_core::task_worktree::pool_state_dir(&home, &checkout).unwrap();
        let records = wait_for_dispatch(role, &mut dispatch, &root, &state, &route);
        let exec_client = Client::connect(records.address.port, &records.token);
        Self {
            _lead_job: lead_job,
            evidence,
            role,
            exe: exe.to_path_buf(),
            entry: entry.to_path_buf(),
            home,
            workspace,
            checkout,
            worktree,
            state,
            owner,
            assignment,
            question,
            answer,
            route,
            _provider: provider,
            lead,
            executor: records.address,
            lead_client,
            exec_client,
            lead_thread,
            exec_thread: records.thread,
            dispatch,
            generation: records.generation,
        }
    }

    fn hits(&self) -> Vec<RouteHit> {
        self.route.lock().unwrap().hits.clone()
    }

    fn kinds(&self) -> Vec<&'static str> {
        self.hits().into_iter().map(|hit| hit.kind).collect()
    }

    fn wait_kind(&self, kind: &str) {
        let until = Instant::now() + WAIT;
        while Instant::now() < until {
            if self.hits().iter().any(|hit| hit.kind == kind) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!(
            "{}: the recorded exchange never reached {kind}: {:?}",
            self.role,
            self.kinds()
        );
    }

    fn receipt(&self) -> Value {
        serde_json::from_slice(&fs::read(self.state.join("spawn-1.json")).unwrap()).unwrap()
    }

    /// Waits until the real `lead message` recorded its request on this run and
    /// returns the recorded message identity.
    fn wait_question_recorded(&self) -> String {
        let until = Instant::now() + WAIT;
        loop {
            let receipt = self.receipt();
            let entry = &receipt["leadMessages"][0];
            if entry["status"] == "delivered"
                && entry["kind"] == "reply-request"
                && entry["session"] == self.exec_thread.as_str()
                && entry["leadThreadId"] == self.lead_thread.as_str()
                && entry["id"].is_string()
            {
                return entry["id"].as_str().unwrap().to_owned();
            }
            assert!(
                Instant::now() < until,
                "{}: the question was never recorded as delivered: {receipt}",
                self.role
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// The blocked executor ends its own turn and stays idle with the request
    /// unresolved; the run keeps its live session.
    fn wait_executor_idle(&mut self) {
        let until = Instant::now() + WAIT;
        loop {
            let status = self
                .exec_client
                .request("thread/read", json!({"threadId": self.exec_thread}));
            if status["thread"]["status"]["type"] == "idle" {
                return;
            }
            assert!(
                Instant::now() < until,
                "{}: the executor never finished its blocked turn: {status}",
                self.role
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// One dispatch-written record of this root's run, read from the kit-local
    /// state directory the dispatch itself names.
    fn record(&self, name: &str) -> Value {
        serde_json::from_slice(
            &fs::read(self.state.join(name)).unwrap_or_else(|error| {
                panic!("{}: the dispatch wrote no {name}: {error}", self.role)
            }),
        )
        .unwrap_or_else(|error| panic!("{}: {name} is not JSON: {error}", self.role))
    }

    /// The control endpoint the dispatch recorded for its executor session: the
    /// address `executor message` and this check both use.
    fn endpoint(&self) -> Value {
        self.record("endpoint-1.json")
    }

    /// The lead endpoint this run inherited from the dispatching lead's
    /// registered session.
    fn lead_endpoint(&self) -> Value {
        self.record("lead-endpoint-1.json")
    }

    /// The run's own native frontend record, as the dispatch appended it: the
    /// visible conversation this run owns, named by its exact thread.
    fn frontend_record(&self) -> Value {
        let text = fs::read_to_string(self.state.join("frontend-1.json")).unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .next_back()
            .unwrap_or_else(|| panic!("{}: the dispatch wrote no frontend record", self.role))
    }

    /// Whether that record still names a live surface: the spawn-written
    /// evidence that this run's own visible conversation is open.
    fn frontend_alive(&self) -> bool {
        let record = self.frontend_record();
        record["alive"] == true
    }

    /// The answered executor's own run reaches its recorded completion: the
    /// dispatch persists the final message and ends the run's own surface.
    /// Returns the persisted final message.
    fn wait_completed(&mut self) -> String {
        let ended = self.wait_dispatch_ended(ANSWER_WAIT);
        assert!(
            ended.success(),
            "{}: the answered dispatch exited {ended}: {}{}",
            self.role,
            self.receipt(),
            self.dispatch_output()
        );
        let receipt = self.receipt();
        assert_eq!(
            receipt["observation"]["state"], "completed",
            "{}: the answered run recorded another state: {receipt}",
            self.role
        );
        fs::read_to_string(self.state.join("message-1.txt")).unwrap_or_else(|error| {
            panic!(
                "{}: the completed run recorded no final message: {error}",
                self.role
            )
        })
    }

    /// Ends this root's fresh executor through the installed stop command: a
    /// run waiting for its reply keeps its session, slot and worktree until the
    /// lead stops it, and the dispatch that hosted that run ends with its own
    /// surface.
    fn stop(&mut self) -> String {
        let output = self
            .installed_command()
            .args([
                "executor",
                "stop",
                "--source",
                self.checkout.to_str().unwrap(),
                "--codex-home",
                self.home.to_str().unwrap(),
                "--slot",
                "1",
                "--owner",
                &self.owner,
                "--session",
                &self.exec_thread,
                "--timeout",
                "30",
            ])
            .output()
            .unwrap();
        let text = output_text(&output);
        assert!(output.status.success(), "{}: {text}", self.role);
        self.wait_dispatch_ended(WAIT);
        text
    }

    /// The dispatch that hosted this root's run ends when that run's own
    /// surface closes.
    fn wait_dispatch_ended(&mut self, bound: Duration) -> std::process::ExitStatus {
        let until = Instant::now() + bound;
        loop {
            if let Some(status) = self.dispatch.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < until,
                "{}: the dispatch that hosted this run is still live: {}",
                self.role,
                self.dispatch_output()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// What the installed dispatch itself reported while it ran this root.
    fn dispatch_output(&self) -> String {
        dispatch_text(&self.evidence.path)
    }

    /// The lead's own native session answers with the one recorded reply
    /// command, inside its own shell, exactly as the delivered envelope asks.
    fn reply(&mut self, id: &str) {
        self.route.lock().unwrap().reply_id = Some(id.to_owned());
        self.lead_client.request(
            "turn/start",
            json!({
                "threadId": self.lead_thread,
                "input": [{
                    "type": "text",
                    "text": format!("ANSWER-THE-EXECUTOR {id}")
                }]
            }),
        );
    }

    /// The installed entry point as an ordinary lead or executor shell calls
    /// it, without this check's own kit session identity.
    fn installed_command(&self) -> Command {
        let mut command = Command::new(&self.entry);
        for key in [
            "HARNESS_EXECUTOR_RUN",
            "HARNESS_EXECUTOR_SESSION",
            "HARNESS_ORIGINATING_LEAD",
            "HARNESS_LEAD_THREAD",
            "HARNESS_LEAD_RECIPIENT",
            "CODEX_THREAD_ID",
            "CODEX_SESSION_ID",
        ] {
            command.env_remove(key);
        }
        command.env("CODEX_HOME", &self.home);
        command.stdin(Stdio::null());
        command
    }

    /// The bounded watch result for this run, taken through the installed
    /// entry point.
    fn watch(&self, timeout: &str) -> std::process::Output {
        self.installed_command()
            .args([
                "executor",
                "watch",
                "--source",
                self.checkout.to_str().unwrap(),
                "--codex-home",
                self.home.to_str().unwrap(),
                "--slot",
                "1",
                "--timeout",
                timeout,
            ])
            .output()
            .unwrap()
    }

    fn provider_bodies(&self) -> Vec<Value> {
        let mut bodies = Vec::new();
        for index in 1..=64u32 {
            let path = self
                .evidence
                .path
                .join("provider")
                .join(format!("provider-{index}.json"));
            if !path.is_file() {
                break;
            }
            bodies.push(serde_json::from_slice(&fs::read(&path).unwrap()).unwrap());
        }
        bodies
    }

    /// The text of the provider input the lead's own conversation sent for the
    /// delivered question: the envelope plus the literal payload.
    fn envelope(&self) -> String {
        let body = self
            .provider_bodies()
            .into_iter()
            .find(|body| {
                input_text(&body["input"]).contains(&self.question)
                    && RequestIdentity::from_request(body).unwrap().thread == self.lead_thread
            })
            .unwrap_or_else(|| {
                panic!(
                    "{}: the lead conversation never received the question",
                    self.role
                )
            });
        input_text(&body["input"])
    }

    /// The provider request of this root's executor conversation that carries
    /// the given text, or a panic naming the missing exchange.
    fn executor_request(&self, needle: &str) -> Value {
        self.provider_bodies()
            .into_iter()
            .find(|body| {
                input_text(&body["input"]).contains(needle)
                    && RequestIdentity::from_request(body).unwrap().thread == self.exec_thread
            })
            .unwrap_or_else(|| panic!("{}: no executor request carried {needle:?}", self.role))
    }

    /// The provider request of this root's lead conversation that carries the
    /// given text.
    fn lead_request(&self, needle: &str) -> Value {
        self.provider_bodies()
            .into_iter()
            .find(|body| {
                input_text(&body["input"]).contains(needle)
                    && RequestIdentity::from_request(body).unwrap().thread == self.lead_thread
            })
            .unwrap_or_else(|| panic!("{}: no lead request carried {needle:?}", self.role))
    }
}

/// The configured pool of one fixture checkout: the single default profile the
/// two-lead acceptance binds, and one pool slot.
fn orchestration() -> String {
    "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"default\"\nexecutor_profiles = [\"default\"]\nmax_concurrent_executors = 1\n".to_owned()
}

/// One Git remote URL for an owned local upstream.
fn file_url(path: &Path) -> String {
    format!("file:///{}", path.to_str().unwrap().replace('\\', "/"))
}

/// A check that fails before its own cleanup must not leave a fresh executor
/// running: this ends exactly the run the root's own dispatch recorded, through
/// the installed stop path, and waits for the dispatch that hosted it to end.
/// Any failure here is reported by the failing check's own output, never by a
/// second panic.
impl Drop for LeadRoot {
    fn drop(&mut self) {
        if self.dispatch.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = self
            .installed_command()
            .args([
                "executor",
                "stop",
                "--source",
                self.checkout.to_str().unwrap_or_default(),
                "--codex-home",
                self.home.to_str().unwrap_or_default(),
                "--slot",
                "1",
                "--owner",
                &self.owner,
                "--session",
                &self.exec_thread,
                "--timeout",
                "15",
            ])
            .output();
        let until = Instant::now() + WAIT;
        while Instant::now() < until {
            if self.dispatch.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.dispatch.kill();
        let _ = self.dispatch.wait();
    }
}

/// A path in the spelling the harness records: the canonical path without the
/// Windows verbatim prefix, exactly as the worktree pool resolves one.
fn native_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    PathBuf::from(text.replace('/', "\\"))
}

/// The lead's own shell as the installed entry point sees it: the dispatching
/// session's kit home, and neither an executor marker nor a terminal-tab
/// address (this check opens owned consoles, never the owner's terminal).
fn lead_shell(entry: &Path, home: &Path, cwd: &Path) -> Command {
    let mut command = Command::new(entry);
    for key in [
        "HARNESS_EXECUTOR_RUN",
        "HARNESS_EXECUTOR_SESSION",
        "HARNESS_ORIGINATING_LEAD",
        "HARNESS_LEAD_THREAD",
        "HARNESS_LEAD_RECIPIENT",
        "CODEX_SESSION_ID",
        "WT_SESSION",
    ] {
        command.env_remove(key);
    }
    command.env("CODEX_HOME", home);
    command.env("HARNESS_CONTROL_FIXTURE_KEY", "synthetic-owned-fixture");
    command.current_dir(cwd);
    command.stdin(Stdio::null());
    command
}

/// The live endpoint of one managed lead session, in the registry layout the
/// installed `lead start` publishes: the address of the exact native thread and
/// the app-server process that serves it. An ordinary unmanaged lead exposes
/// none, so this check owns the lead session's own address; the dispatch under
/// test is what inherits it.
fn write_lead_endpoint(
    home: &Path,
    thread: &str,
    port: u16,
    token: &str,
    process: &ProcessIdentity,
    exe: &Path,
) {
    let path = home.join(format!("harness/lead-endpoints/{thread}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "port": port,
            "token": token,
            "threadId": thread,
            "process": {
                "pid": process.pid,
                "creationTime": process.creation_time,
                "program": exe,
            },
        }))
        .unwrap(),
    )
    .unwrap();
}

/// One dispatch record while it is being replaced is not read as fact.
fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// The exact process identity a record names, in either spelling the harness
/// records for a host or an app-server child.
fn recorded_identity(value: &Value) -> ProcessIdentity {
    ProcessIdentity {
        pid: value["pid"].as_u64().unwrap_or(0) as u32,
        creation_time: value["creationTime"]
            .as_u64()
            .or_else(|| value["created"].as_u64())
            .unwrap_or(0),
    }
}

/// Nothing of a run survives it: the exact recorded identity no longer runs.
fn assert_process_gone(identity: ProcessIdentity, program: &Path, user: &str, label: &str) {
    let live = ServiceProcess::inspect(identity, program, user)
        .map(|process| process.is_some())
        .unwrap_or(true);
    assert!(!live, "{label} (pid {})", identity.pid);
}

/// The surface the dispatch recorded is a real, non-minimized window of the
/// exact process it names - not a claim inside a record. The deciding rule is
/// `harness_core::task_view`'s retained-view rule, so it also accepts a
/// console the terminal host presents for that exact recorded process.
fn assert_visible_surface(root: &LeadRoot) {
    let receipt = root.receipt();
    let surface = &receipt["window"];
    let snapshot: harness_core::task_view::Snapshot = serde_json::from_value(surface.clone())
        .unwrap_or_else(|error| {
            panic!(
                "{}: the dispatch recorded no usable visible surface ({error}): {receipt}",
                root.role
            )
        });
    let host = &receipt["observation"]["host"];
    let program = PathBuf::from(host["program"].as_str().unwrap_or_default());
    let user = process_service::current_user().unwrap();
    // The harness's own retained-view rule: the exact process identity plus a
    // live, non-minimized window, whether the console owns it or the terminal
    // host presents it. A surface that is still settling gets a bounded moment.
    let until = Instant::now() + WAIT;
    loop {
        match snapshot.is_visible(&program, &user) {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => panic!(
                "{}: the recorded surface could not be checked: {error}; {surface}",
                root.role
            ),
        }
        assert!(
            Instant::now() < until,
            "{}: the recorded surface is not this run's own live window: {surface} {}",
            root.role,
            window_state(snapshot.window)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The observed state of one window handle, for the surface failure message:
/// existence, owning process, visibility, minimize state and class. The
/// presentation rule itself stays owned by `harness_core::task_view`.
fn window_state(window: usize) -> Value {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
    };
    let handle = window as HWND;
    let exists = unsafe { IsWindow(handle) } != 0;
    let mut owner = 0u32;
    let mut class = [0u16; 128];
    let copied = if exists {
        unsafe {
            GetWindowThreadProcessId(handle, &mut owner);
            GetClassNameW(handle, class.as_mut_ptr(), class.len() as i32)
        }
    } else {
        0
    };
    json!({
        "window": window,
        "exists": exists,
        "ownerPid": owner,
        "class": String::from_utf16_lossy(&class[..copied.max(0) as usize]),
        "visible": exists && unsafe { IsWindowVisible(handle) } != 0,
        "minimized": exists && unsafe { IsIconic(handle) } != 0,
    })
}

/// What the real dispatch recorded for one root's fresh executor: the run
/// generation it chose itself, the exact native session it started, and the
/// control endpoint this check and `executor message` both address.
struct DispatchRecords {
    generation: String,
    thread: String,
    address: SessionAddress,
    token: String,
}

/// Waits for the dispatch's own records - the receipt and slot it writes before
/// the session starts, the control endpoint of that session, the lead endpoint
/// it inherits and the attached native frontend - instead of writing any of
/// them. A dispatch that exits, or records no live run, fails with its own
/// output as evidence.
fn wait_for_dispatch(
    role: &str,
    dispatch: &mut Child,
    root: &Path,
    state: &Path,
    route: &Arc<Mutex<LeadRoute>>,
) -> DispatchRecords {
    let until = Instant::now() + DISPATCH_WAIT;
    loop {
        if let Some(status) = dispatch.try_wait().unwrap() {
            panic!(
                "{role}: the installed dispatch exited {status} before its run was up: {}",
                dispatch_text(root)
            );
        }
        if let (Some(receipt), Some(endpoint)) = (
            read_json(&state.join("spawn-1.json")),
            read_json(&state.join("endpoint-1.json")),
        ) {
            let thread = receipt["observation"]["session"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let port = endpoint["port"].as_u64().unwrap_or(0) as u16;
            let token = endpoint["token"].as_str().unwrap_or_default().to_owned();
            let ready = !thread.is_empty()
                && port != 0
                && !token.is_empty()
                && endpoint["threadId"].as_str() == Some(thread.as_str())
                && state.join("slot-1.json").is_file()
                && state.join("lease-1.json").is_file()
                && state.join("lead-endpoint-1.json").is_file()
                && state.join("frontend-1.json").is_file();
            if ready {
                // The root's own provider keys this conversation as its
                // executor from the dispatch's record, never from a guess.
                route.lock().unwrap().exec_thread = thread.clone();
                return DispatchRecords {
                    generation: receipt["originatingLead"]["runGeneration"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    thread,
                    address: SessionAddress {
                        identity: ProcessIdentity {
                            pid: endpoint["process"]["pid"].as_u64().unwrap_or(0) as u32,
                            creation_time: endpoint["process"]["creationTime"]
                                .as_u64()
                                .unwrap_or(0),
                        },
                        port,
                    },
                    token,
                };
            }
        }
        assert!(
            Instant::now() < until,
            "{role}: the installed dispatch recorded no live run within {}s: {}",
            DISPATCH_WAIT.as_secs(),
            dispatch_text(root)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The bounded tail of what an installed dispatch itself reported.
fn dispatch_text(root: &Path) -> String {
    let tail = |name: &str| {
        let text = fs::read_to_string(root.join(name)).unwrap_or_default();
        let start = text.len().saturating_sub(1500);
        text[start..].to_owned()
    };
    format!(
        "\ndispatch stdout: {}\ndispatch stderr: {}",
        tail("dispatch-stdout.txt"),
        tail("dispatch-stderr.txt")
    )
}

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("git {args:?}: {error}"));
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_text(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn read_token(path: &Path) -> String {
    fs::read_to_string(path).unwrap().trim().to_owned()
}

fn output_text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// One path as it appears inside the envelope's JSON metadata.
fn json_path(path: &Path) -> String {
    serde_json::to_string(path.to_string_lossy().as_ref())
        .unwrap()
        .trim_matches('"')
        .to_owned()
}

/// Every string one conversation's provider request carried, as the strings
/// themselves: the delivered literal payloads, tool arguments and tool outputs
/// are compared unescaped instead of inside their JSON encoding.
fn input_text(value: &Value) -> String {
    match value {
        Value::String(text) => format!("{text}\n"),
        Value::Array(items) => items.iter().map(input_text).collect(),
        Value::Object(map) => map.values().map(input_text).collect(),
        _ => String::new(),
    }
}

/// A bounded tail of one conversation text for a failure message: the assertion
/// names the missing part without dumping a whole turn into the log.
fn excerpt(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let start = text
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| text.len() - index <= limit)
        .unwrap_or(0);
    format!("...{}", &text[start..])
}

/// The installed kit home and its recorded upstream executable: the acceptance
/// runs the deployed immutable build, and its native sessions must be exactly
/// the binary the installation records.
fn installed_kit_home() -> PathBuf {
    harness_core::native_launcher::codex_home().expect("installed kit home")
}

fn installed_entry() -> PathBuf {
    let entry = installed_kit_home().join("harness/bin/codex-harness.exe");
    assert!(
        entry.is_file(),
        "the installed harness entry point {} is missing; deploy the kit before running this check",
        entry.display()
    );
    entry
}

fn installed_upstream() -> PathBuf {
    let registration = installed_kit_home().join("harness/native-launch.json");
    let value: Value = serde_json::from_slice(&fs::read(&registration).unwrap_or_else(|error| {
        panic!(
            "installed launcher registration {}: {error}",
            registration.display()
        )
    }))
    .expect("launcher registration JSON");
    PathBuf::from(
        value["upstream"]["executable"]
            .as_str()
            .expect("registered upstream executable"),
    )
}

fn same_executable(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy()),
        _ => false,
    }
}

/// The fixture home's configuration: the owned provider of this root, the same
/// bound model, provider and reasoning effort for both sessions, and trusted
/// directories so no interactive prompt can appear.
fn write_root_config(home: &Path, provider_port: u16, trusted: &[&Path]) {
    let mut config = format!(
        r#"
model = "gpt-6-astra"
model_provider = "control_fixture"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
cli_auth_credentials_store = "file"
[model_providers.control_fixture]
name = "Owned two-lead isolation fixture"
base_url = "http://127.0.0.1:{provider_port}/v1"
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
multi_agent = true
multi_agent_v2 = false
[analytics]
enabled = false
"#
    );
    for path in trusted {
        config.push_str(&format!(
            "[projects.{}]\ntrust_level = \"trusted\"\n",
            serde_json::to_string(&path.to_string_lossy().to_lowercase()).unwrap()
        ));
    }
    fs::write(home.join("config.toml"), config).unwrap();
}

/// Starts one installed native app-server session, connects a client to it and
/// starts the bound thread in the passed checkout under the bound identity.
fn launch_native_session(
    exe: &Path,
    home: &Path,
    cwd: &Path,
    root: &Path,
    label: &str,
    extra_env: &[(&str, &str)],
) -> (Job, SessionAddress, Client, String) {
    let token = format!(
        "{:x}",
        Sha256::digest(format!("{label}-{}", root.display()).as_bytes())
    );
    let token_file = root.join(format!("{label}-ws-token"));
    fs::write(&token_file, &token).unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut spec = command(exe, home, cwd);
    // The kit session identity of whatever runs this check must not become the
    // fixture session's identity: each native session here is its own lead or
    // its own executor, exactly as the installed launcher clears those markers
    // before a host starts.
    for key in [
        "HARNESS_EXECUTOR_RUN",
        "HARNESS_EXECUTOR_SESSION",
        "HARNESS_ORIGINATING_LEAD",
        "HARNESS_LEAD_THREAD",
        "HARNESS_LEAD_RECIPIENT",
    ] {
        spec.env.insert(key.into(), None);
    }
    spec.args = vec![
        "app-server".into(),
        "--listen".into(),
        format!("ws://127.0.0.1:{port}").into(),
        "--ws-auth".into(),
        "capability-token".into(),
        "--ws-token-file".into(),
        token_file.into_os_string(),
    ];
    for (key, value) in extra_env {
        spec.env
            .insert((*key).into(), Some(PathBuf::from(value).into_os_string()));
    }
    spec.stdout = Some(fs::File::create(root.join(format!("{label}-stdout.txt"))).unwrap());
    spec.stderr = Some(fs::File::create(root.join(format!("{label}-stderr.txt"))).unwrap());
    let job = Job::new(Limits {
        memory_bytes: Some(1024 * 1024 * 1024),
        cpu_percent: Some(50.0),
    })
    .unwrap();
    let server = job.spawn(&spec).unwrap();
    let until = Instant::now() + WAIT;
    while ControlConnection::connect(port, &token, Duration::from_millis(100)).is_err() {
        assert!(
            Instant::now() < until,
            "{label}: the installed native session did not accept its own listener; {}",
            fs::read_to_string(root.join(format!("{label}-stderr.txt"))).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let identity = server.identity();
    let mut client = Client::connect(port, &token);
    let started = client.request(
        "thread/start",
        json!({
            "cwd": cwd,
            "model": "gpt-6-astra",
            "modelProvider": "control_fixture",
            "allowProviderModelFallback": false,
            "approvalPolicy": "never",
            "sandbox": "danger-full-access",
            "config": {"model_reasoning_effort": "low"}
        }),
    );
    let thread = started["thread"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{label}: thread/start answered {started}"))
        .to_owned();
    let status = client.request("thread/read", json!({"threadId": thread}));
    assert_eq!(
        status["thread"]["status"]["type"], "idle",
        "{label}: {status}"
    );
    (job, SessionAddress { identity, port }, client, thread)
}

/// The bound identity every thread of one root reports, and the directory each
/// conversation runs in.
fn assert_bound_thread(client: &mut Client, thread: &str, cwd: &Path, label: &str) {
    let status = client.request("thread/read", json!({"threadId": thread}));
    assert_eq!(status["thread"]["id"], thread, "{label}: {status}");
    assert_eq!(
        status["thread"]["status"]["type"], "idle",
        "{label}: the exchange left the conversation running: {status}"
    );
    assert_eq!(
        status["thread"]["model"], "gpt-6-astra",
        "{label}: the exchange changed the routed model: {status}"
    );
    assert_eq!(
        status["thread"]["modelProvider"], "control_fixture",
        "{label}: the exchange changed the routed provider: {status}"
    );
    assert_eq!(
        status["thread"]["reasoningEffort"], "low",
        "{label}: the exchange changed the routed effort: {status}"
    );
    let reported = status["thread"]["cwd"]
        .as_str()
        .unwrap_or_else(|| panic!("{label}: the conversation reported no directory: {status}"));
    assert!(
        same_directory(reported, cwd),
        "{label}: the conversation left its checkout: {status}"
    );
}

/// One directory reported in either of the spellings Windows uses for it: the
/// verbatim form a canonicalized fixture path takes, and the plain form the
/// native conversation reports.
fn same_directory(reported: &str, expected: &Path) -> bool {
    let normalize = |text: &str| {
        text.trim_start_matches(r"\\?\")
            .trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_ascii_lowercase()
    };
    normalize(reported) == normalize(&expected.to_string_lossy())
}

/// Two actual installed native lead sessions, each with its own owned canned
/// Responses provider and its own real installed `executor spawn`. Each
/// dispatch records that lead as the originating lead, inherits the lead's
/// registered endpoint, synchronizes its pool slot, hosts the native executor
/// session and writes every executor record read here; nothing about an
/// executor is handwritten by this check. Each executor's own native session
/// asks its own lead through the installed entry point, the blocked executor
/// stays waiting while its lead answers through the recorded reply reference,
/// every step is established inside the receiving conversation's own provider
/// input, and the two roots never see each other's exchange.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE as the installation-record upstream and the deployed kit; two native lead roots with owned canned Responses; no live model"]
fn installed_two_leads_keep_questions_waiting_and_replies_isolated() {
    let exe = codex_exe();
    let version = qualified_installed_upstream(&exe);
    let entry = installed_entry();
    eprintln!(
        "installed two-lead isolation: entry={} upstream={} ({})",
        entry.display(),
        exe.display(),
        version.trim()
    );
    // Each root starts its own installed native lead session and its own owned
    // provider, then dispatches its own fresh executor through the installed
    // entry point. The dispatch - not this check - writes the executor's slot,
    // lease, receipt, control endpoint and observation.
    let mut lead_a = LeadRoot::start(
        "a",
        &exe,
        &entry,
        "Executor A asks which input contract applies to its own worktree".to_owned(),
        "Lead A answers with the versioned contract of that worktree".to_owned(),
    );
    let mut lead_b = LeadRoot::start(
        "b",
        &exe,
        &entry,
        "Executor B asks which validation rule applies to its own worktree".to_owned(),
        "Lead B answers with the recorded rule of that worktree".to_owned(),
    );
    assert_ne!(
        lead_a.lead_thread, lead_b.lead_thread,
        "the two leads must be distinct native sessions"
    );
    assert_ne!(
        lead_a.exec_thread, lead_b.exec_thread,
        "the two dispatches must have started distinct native sessions"
    );
    assert_ne!(lead_a.worktree, lead_b.worktree);
    assert_ne!(lead_a.generation, lead_b.generation);
    for root in [&lead_a, &lead_b] {
        assert!(
            !root.generation.is_empty(),
            "{}: the dispatch recorded no run generation",
            root.role
        );
    }

    // Each root's own dispatch submitted the assignment to its own executor's
    // native session; the question was delivered into its own lead's
    // conversation; and the blocked executor's own turn ended with the request
    // unresolved.
    for root in [&lead_a, &lead_b] {
        root.wait_kind("exec-question");
        root.wait_kind("lead-question");
        root.wait_kind("exec-waiting");
        assert_eq!(
            root.kinds(),
            ["exec-question", "lead-question", "exec-waiting"],
            "{}: unexpected provider traffic",
            root.role
        );
        assert!(
            !root.hits().iter().any(|hit| hit.kind == "foreign-thread"),
            "{}: a conversation outside this root reached its provider",
            root.role
        );
    }
    let id_a = lead_a.wait_question_recorded();
    let id_b = lead_b.wait_question_recorded();
    assert_ne!(
        id_a, id_b,
        "the two requests must have their own identities"
    );
    lead_a.wait_executor_idle();
    lead_b.wait_executor_idle();

    // The delivered envelope identifies the sender run exactly, and no word of
    // the other root's exchange appears in this lead's own conversation.
    for (root, other, id) in [(&lead_a, &lead_b, &id_a), (&lead_b, &lead_a, &id_b)] {
        let envelope = root.envelope();
        for expected in [
            format!("\"owner\":\"{}\"", root.owner),
            format!("\"session\":\"{}\"", root.exec_thread),
            format!("\"worktree\":\"{}\"", json_path(&root.worktree)),
            format!("\"checkout\":\"{}\"", json_path(&root.checkout)),
            format!("\"runGeneration\":\"{}\"", root.generation),
            format!("\"leadThreadId\":\"{}\"", root.lead_thread),
            "\"slot\":1".to_owned(),
            "\"kind\":\"reply-request\"".to_owned(),
            root.question.clone(),
            format!("\"id\":\"{id}\""),
            format!("--reply-to {id}"),
        ] {
            assert!(
                envelope.contains(&expected),
                "{}: the delivered envelope is missing {expected}: {}",
                root.role,
                excerpt(&envelope, 3000)
            );
        }
        for foreign in [
            other.owner.clone(),
            other.exec_thread.clone(),
            other.lead_thread.clone(),
            other.generation.clone(),
            json_path(&other.worktree),
            json_path(&other.checkout),
            other.question.clone(),
        ] {
            assert!(
                !envelope.contains(&foreign),
                "{}: the neighbor's {foreign} reached this lead's conversation: {}",
                root.role,
                excerpt(&envelope, 3000)
            );
        }
    }

    // The executor's own turn kept the delivery receipt, including the one
    // command that answers it.
    for (root, id) in [(&lead_a, &id_a), (&lead_b, &id_b)] {
        let delivered = root.executor_request("lead-message-1");
        let text = input_text(&delivered["input"]);
        assert!(
            text.contains("lead message: delivered"),
            "{}: the executor's own turn kept no delivered receipt: {}",
            root.role,
            excerpt(&text, 3000)
        );
        for expected in [
            format!("--reply-to {id}"),
            format!("session: {}", root.exec_thread),
            format!("worktree: {}", root.worktree.display()),
            format!("leadThreadId: {}", root.lead_thread),
        ] {
            assert!(
                text.contains(&expected),
                "{}: the executor's receipt is missing {expected}: {}",
                root.role,
                excerpt(&text, 3000)
            );
        }
    }

    // The blocked executors remain waiting: watch returns the action-required
    // result, naming the exact run, its request and its reply reference, and it
    // claims no terminal outcome, no timeout and no resume.
    for (root, id) in [(&lead_a, &id_a), (&lead_b, &id_b)] {
        let watched = root.watch("30");
        let text = output_text(&watched);
        assert_eq!(watched.status.code(), Some(3), "{}: {text}", root.role);
        for expected in [
            "action required: waiting for reply".to_owned(),
            format!("id={id}"),
            format!("--reply-to {id}"),
            format!("session: {}", root.exec_thread),
            format!("lead={}", root.lead_thread),
        ] {
            assert!(
                text.contains(&expected),
                "{}: the waiting result is missing {expected}: {text}",
                root.role
            );
        }
        for forbidden in [
            "state=completed",
            "state=defect",
            "timed out",
            "resume",
            "exit: ",
        ] {
            assert!(
                !text.contains(forbidden),
                "{}: a waiting run claimed {forbidden}: {text}",
                root.role
            );
        }
    }

    // Waiting is not model activity: a bounded quiet period produces no
    // provider request for either blocked executor.
    let quiet_a = lead_a.hits();
    let quiet_b = lead_b.hits();
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(lead_a.hits(), quiet_a, "waiting produced model traffic");
    assert_eq!(lead_b.hits(), quiet_b, "waiting produced model traffic");

    // Everything about each executor was written by its own real dispatch and
    // is verified against the live processes those records name: the pool slot
    // it selected, created and synchronized, the originating lead it captured
    // before the session existed, the lease, the control endpoint, the lead
    // endpoint it inherited and the native session it started.
    let user = process_service::current_user().unwrap();
    for root in [&mut lead_a, &mut lead_b] {
        // The slot: this checkout's slot 1, synchronized to the committed base
        // and registered as a worktree of this checkout, occupied by this run.
        let slot = root.record("slot-1.json");
        assert_eq!(slot["index"], 1, "{}: {slot}", root.role);
        assert_eq!(slot["state"], "occupied", "{}: {slot}", root.role);
        assert_eq!(slot["owner"], root.owner.as_str(), "{}: {slot}", root.role);
        assert!(
            same_directory(slot["path"].as_str().unwrap_or_default(), &root.worktree),
            "{}: the dispatch synchronized another tree: {slot}",
            root.role
        );
        assert!(
            same_directory(slot["source"].as_str().unwrap_or_default(), &root.checkout),
            "{}: the dispatch recorded another checkout: {slot}",
            root.role
        );
        assert_eq!(
            slot["base"],
            git_text(&root.checkout, &["rev-parse", "HEAD"]).as_str(),
            "{}: the dispatch synchronized another base: {slot}",
            root.role
        );
        let registered = git_text(&root.checkout, &["worktree", "list", "--porcelain"]);
        assert!(
            registered
                .lines()
                .filter_map(|line| line.strip_prefix("worktree "))
                .any(|path| same_directory(path, &root.worktree)),
            "{}: slot 1 is not a worktree this checkout registered: {registered}",
            root.role
        );
        // The originating lead the dispatch captured: this root's own lead
        // thread, a generation the dispatch chose itself, and the dispatching
        // process's own verified identity.
        let receipt = root.receipt();
        let lead = &receipt["originatingLead"];
        assert_eq!(
            lead["threadId"],
            root.lead_thread.as_str(),
            "{}: {receipt}",
            root.role
        );
        assert_eq!(
            lead["runGeneration"],
            root.generation.as_str(),
            "{}: {receipt}",
            root.role
        );
        assert_eq!(
            lead["dispatcher"]["pid"].as_u64(),
            Some(root.dispatch.id() as u64),
            "{}: the receipt names another dispatcher: {receipt}",
            root.role
        );
        assert!(
            same_executable(
                Path::new(lead["dispatcher"]["program"].as_str().unwrap_or_default()),
                &root.entry
            ),
            "{}: the recorded dispatcher is not the installed entry point: {receipt}",
            root.role
        );
        assert_ne!(
            lead["dispatcher"]["creationTime"].as_u64().unwrap_or(0),
            0,
            "{}: {receipt}",
            root.role
        );
        assert_eq!(receipt["slot"]["index"], 1, "{}: {receipt}", root.role);
        assert_eq!(
            receipt["slot"]["owner"],
            root.owner.as_str(),
            "{}: {receipt}",
            root.role
        );
        assert_eq!(
            receipt["slot"]["path"], slot["path"],
            "{}: the receipt and the slot record disagree: {receipt}",
            root.role
        );
        assert_eq!(
            receipt["slot"]["source"], slot["source"],
            "{}: the receipt and the slot record disagree: {receipt}",
            root.role
        );
        // The run's own surface: an owned console this dispatch opened, never a
        // tab or rearrangement of the lead's terminal.
        assert_eq!(receipt["host"], "owned-console", "{}: {receipt}", root.role);
        assert!(
            receipt["terminal"].is_null(),
            "{}: the dispatch rearranged a terminal: {receipt}",
            root.role
        );
        assert!(
            same_executable(
                Path::new(receipt["launcher"].as_str().unwrap_or_default()),
                &root.home.join("harness/bin/codex.exe")
            ),
            "{}: the receipt names another launcher: {receipt}",
            root.role
        );
        assert!(
            receipt["control"]["assignment"]
                .as_str()
                .unwrap_or_default()
                .starts_with(&root.assignment),
            "{}: the dispatch submitted another assignment: {receipt}",
            root.role
        );
        assert_eq!(
            receipt["mode"], "tui",
            "{}: the dispatch selected another presentation: {receipt}",
            root.role
        );
        assert_visible_surface(root);
        // The lease is this run's live host, and the observation names the exact
        // native session and app-server child the dispatch started.
        let lease = root.record("lease-1.json");
        assert_eq!(
            lease["owner"],
            root.owner.as_str(),
            "{}: {lease}",
            root.role
        );
        assert_eq!(lease["index"], 1, "{}: {lease}", root.role);
        assert_eq!(lease["path"], slot["path"], "{}: {lease}", root.role);
        let lease_program = PathBuf::from(lease["program"].as_str().unwrap_or_default());
        assert!(
            ServiceProcess::inspect(recorded_identity(&lease), &lease_program, &user)
                .unwrap()
                .is_some(),
            "{}: the run's lease host was stopped",
            root.role
        );
        assert_eq!(
            receipt["observation"]["session"],
            root.exec_thread.as_str(),
            "{}: {receipt}",
            root.role
        );
        assert_eq!(
            receipt["observation"]["state"], "running",
            "{}: the waiting run is not running: {receipt}",
            root.role
        );
        let host = receipt["observation"]["host"].clone();
        assert!(
            same_executable(
                Path::new(host["program"].as_str().unwrap_or_default()),
                &root.entry
            ),
            "{}: the observation names another host program: {receipt}",
            root.role
        );
        assert!(
            ServiceProcess::inspect(
                recorded_identity(&host),
                Path::new(host["program"].as_str().unwrap_or_default()),
                &user
            )
            .unwrap()
            .is_some(),
            "{}: the run's own host process was stopped",
            root.role
        );
        // The visible surface belongs to that same host, and the app-server
        // child the endpoint names is live under this run's own launcher.
        assert_eq!(
            receipt["window"]["process"]["pid"], host["pid"],
            "{}: the recorded surface belongs to another process: {receipt}",
            root.role
        );
        assert!(
            ServiceProcess::inspect(
                root.executor.identity,
                &root.home.join("harness/bin/codex.exe"),
                &user
            )
            .unwrap()
            .is_some(),
            "{}: the run's own app-server child is not live",
            root.role
        );
        // The control endpoint addresses exactly that child and conversation;
        // the inherited lead endpoint is this root's own registered lead.
        let endpoint = root.endpoint();
        assert_eq!(
            endpoint["threadId"],
            root.exec_thread.as_str(),
            "{}: {endpoint}",
            root.role
        );
        assert_eq!(
            endpoint["port"].as_u64().unwrap_or(0) as u16,
            root.executor.port,
            "{}: {endpoint}",
            root.role
        );
        assert_eq!(
            recorded_identity(&endpoint["process"]),
            root.executor.identity,
            "{}: {endpoint}",
            root.role
        );
        assert_ne!(
            endpoint["port"].as_u64().unwrap_or(0) as u16,
            root.lead.port,
            "{}: the run kept the lead's own port",
            root.role
        );
        let lead_endpoint = root.lead_endpoint();
        assert_eq!(
            lead_endpoint["threadId"],
            root.lead_thread.as_str(),
            "{}: {lead_endpoint}",
            root.role
        );
        assert_eq!(
            lead_endpoint["port"].as_u64().unwrap_or(0) as u16,
            root.lead.port,
            "{}: the inherited lead endpoint is not this root's lead: {lead_endpoint}",
            root.role
        );
        assert_eq!(
            recorded_identity(&lead_endpoint["process"]),
            root.lead.identity,
            "{}: {lead_endpoint}",
            root.role
        );
        // Both native sessions of this root are still live, and the run's own
        // visible conversation is the frontend the dispatch attached.
        let observed = ServiceProcess::inspect(root.lead.identity, &root.exe, &user)
            .unwrap()
            .unwrap_or_else(|| panic!("{}: the lead session is no longer live", root.role));
        assert_eq!(observed.identity(), root.lead.identity, "{}", root.role);
        let frontend = root.frontend_record();
        assert!(
            root.frontend_alive(),
            "{}: the run's own native frontend is not live: {frontend}",
            root.role
        );
        assert!(
            same_executable(
                Path::new(frontend["program"].as_str().unwrap_or_default()),
                &exe
            ),
            "{}: the run's frontend is not the registered native CLI: {frontend}",
            root.role
        );
        assert_eq!(
            frontend["threadId"],
            root.exec_thread.as_str(),
            "{}: {frontend}",
            root.role
        );
        // Routing is unchanged on the exact native session the dispatch
        // started, addressed through the endpoint it recorded.
        let (thread, cwd) = (root.exec_thread.clone(), root.worktree.clone());
        assert_bound_thread(&mut root.exec_client, &thread, &cwd, root.role);
    }

    // Isolation refusals through the installed entry point: another lead, a
    // reference recorded in another kit home, a copied marker outside the run's
    // lineage and a marker in a foreign root are all refused before any send.
    let attempts = [
        (
            "another lead",
            lead_a
                .installed_command()
                .args([
                    "executor",
                    "message",
                    "--reply-to",
                    &id_a,
                    "--text",
                    "cross-lead attempt",
                ])
                .env("CODEX_THREAD_ID", &lead_b.lead_thread)
                .output()
                .unwrap(),
            "another lead cannot use this reply reference",
        ),
        (
            "another kit home",
            lead_b
                .installed_command()
                .args([
                    "executor",
                    "message",
                    "--reply-to",
                    &id_a,
                    "--text",
                    "cross-home attempt",
                ])
                .env("CODEX_THREAD_ID", &lead_b.lead_thread)
                .output()
                .unwrap(),
            "unknown message id",
        ),
        (
            "copied marker",
            lead_a
                .installed_command()
                .env("HARNESS_EXECUTOR_RUN", &lead_a.generation)
                .env("HARNESS_EXECUTOR_SESSION", "1")
                .args(["lead", "message", "--text", "copied marker attempt"])
                .output()
                .unwrap(),
            "caller is not in the spawned run's process lineage",
        ),
        (
            "foreign root",
            lead_b
                .installed_command()
                .env("HARNESS_EXECUTOR_RUN", &lead_a.generation)
                .env("HARNESS_EXECUTOR_SESSION", "1")
                .args(["lead", "message", "--text", "foreign root attempt"])
                .output()
                .unwrap(),
            "no live spawned run records this run generation",
        ),
    ];
    for (label, out, expected) in &attempts {
        let text = output_text(out);
        assert!(
            !out.status.success() && text.contains(*expected),
            "{label}: {text}"
        );
    }
    // A refused attempt reached no conversation: both waiting runs still own
    // exactly the exchange their own dispatch and question produced.
    assert_eq!(
        lead_a.kinds(),
        ["exec-question", "lead-question", "exec-waiting"],
        "a refused attempt reached a conversation"
    );
    assert_eq!(
        lead_b.kinds(),
        ["exec-question", "lead-question", "exec-waiting"],
        "a refused attempt reached a conversation"
    );
    for root in [&lead_a, &lead_b] {
        for body in root.provider_bodies() {
            let text = input_text(&body["input"]);
            for attempt in [
                "cross-lead attempt",
                "cross-home attempt",
                "copied marker attempt",
                "foreign root attempt",
            ] {
                assert!(
                    !text.contains(attempt),
                    "a refused attempt reached a conversation: {text}"
                );
            }
        }
    }

    // One command from the correct lead, inside its own native session,
    // continues only that lead's executor on the same thread, and that run then
    // completes normally on the conversation the dispatch started.
    lead_a.reply(&id_a);
    lead_a.wait_kind("lead-answer-done");
    assert_eq!(
        lead_a.kinds(),
        [
            "exec-question",
            "lead-question",
            "exec-waiting",
            "lead-answer-command",
            "exec-reply-continued",
            "lead-answer-done"
        ],
        "root a: unexpected provider traffic"
    );
    let continued = lead_a.executor_request(&lead_a.answer);
    assert!(
        input_text(&continued["input"]).contains(&lead_a.answer),
        "the answer did not reach the executor's own conversation"
    );
    assert_eq!(
        lead_a.receipt()["observation"]["session"],
        lead_a.exec_thread.as_str(),
        "the answer replaced the answered conversation"
    );
    let answered = lead_a.lead_request("lead-reply-1");
    let answered_text = input_text(&answered["input"]);
    assert!(
        answered_text.contains(&format!(
            "slot 1 owner {} session {}",
            lead_a.owner, lead_a.exec_thread
        )) && answered_text.contains("delivered in"),
        "the lead's own turn kept no delivered receipt: {}",
        excerpt(&answered_text, 2000)
    );
    assert!(
        answered_text.contains(&format!("receipt: {}", lead_a.state.display())),
        "the reply did not name the addressed run's receipt: {}",
        excerpt(&answered_text, 2000)
    );
    let result_message = lead_a.wait_completed();
    assert!(
        result_message.contains("EXECUTOR_CONTINUED"),
        "the answered run persisted another result: {result_message}"
    );
    let settled = lead_a.watch("30");
    let settled_text = output_text(&settled);
    assert_eq!(settled.status.code(), Some(0), "{settled_text}");
    assert!(
        settled_text.contains("EXECUTOR_CONTINUED"),
        "the same watch command did not report the answered run's own result: {settled_text}"
    );
    // The answered run's own reference is retired with its recorded outcome: a
    // later reply to it is refused and names the state and the continuation
    // remedy instead of delivering into a conversation that has ended.
    let retired = lead_a
        .installed_command()
        .args([
            "executor",
            "message",
            "--reply-to",
            &id_a,
            "--text",
            "retired reference attempt",
        ])
        .env("CODEX_THREAD_ID", &lead_a.lead_thread)
        .output()
        .unwrap();
    let retired_text = output_text(&retired);
    assert!(
        !retired.status.success()
            && retired_text.contains("retired message id")
            && retired_text.contains("completed")
            && retired_text.contains("executor resume"),
        "a retired reference was not refused with its outcome and remedy: {retired_text}"
    );

    // The answer reached nothing else: the neighbor root produced no further
    // traffic, never saw the answer or the answered identity, and its own
    // executor stays waiting on its own request with its own surface live.
    assert_eq!(
        lead_b.kinds(),
        ["exec-question", "lead-question", "exec-waiting"],
        "root b: the neighbor's reply changed its traffic"
    );
    for body in lead_b.provider_bodies() {
        let text = input_text(&body["input"]);
        assert!(
            !text.contains(&lead_a.answer) && !text.contains(&id_a),
            "root b saw root a's exchange: {text}"
        );
    }
    let still_waiting = lead_b.watch("30");
    let still_waiting_text = output_text(&still_waiting);
    assert_eq!(still_waiting.status.code(), Some(3), "{still_waiting_text}");
    assert!(
        still_waiting_text.contains(&format!("id={id_b}")),
        "{still_waiting_text}"
    );
    assert_eq!(
        lead_b.receipt()["observation"]["state"],
        "running",
        "the neighbor's waiting run stopped when its neighbor was answered"
    );
    assert!(
        lead_b.frontend_alive(),
        "root b: the waiting run's own visible conversation was closed"
    );

    // Unchanged effective routing for every request either conversation made,
    // and both conversations still report the bound model, provider, effort and
    // their own checkout.
    for root in [&lead_a, &lead_b] {
        for body in root.provider_bodies() {
            assert_eq!(body["model"], "gpt-6-astra", "{}", root.role);
            match body["reasoning"]["effort"].as_str() {
                Some(effort) => assert_eq!(effort, "low", "{}", root.role),
                None => panic!(
                    "{}: a routed request carried no reasoning effort",
                    root.role
                ),
            }
        }
    }
    let (thread, cwd) = (lead_a.lead_thread.clone(), lead_a.workspace.clone());
    assert_bound_thread(&mut lead_a.lead_client, &thread, &cwd, "lead a");
    let (thread, cwd) = (lead_b.lead_thread.clone(), lead_b.workspace.clone());
    assert_bound_thread(&mut lead_b.lead_client, &thread, &cwd, "lead b");

    // The answered run ended by itself and left nothing of its own behind; the
    // still-waiting run keeps its session, slot and worktree until its lead
    // stops it through the installed command.
    assert!(
        lead_a.receipt()["observation"]["state"] == "completed",
        "a: the answered run is not completed"
    );
    assert!(
        !lead_a.state.join("lease-1.json").exists(),
        "a: the ended run kept its slot lease"
    );
    assert_process_gone(
        lead_a.executor.identity,
        &lead_a.home.join("harness/bin/codex.exe"),
        &user,
        "a: the answered run left its app-server child running",
    );
    let stopped = lead_b.stop();
    eprintln!("root b's waiting executor was stopped by its lead:\n{stopped}");
    assert_ne!(
        lead_b.receipt()["observation"]["state"],
        "running",
        "b: the stopped run still reports running"
    );
    assert_process_gone(
        lead_b.executor.identity,
        &lead_b.home.join("harness/bin/codex.exe"),
        &user,
        "b: the stopped run left its app-server child running",
    );
    let stopped_frontend = lead_b.frontend_record();
    assert_process_gone(
        recorded_identity(&stopped_frontend),
        Path::new(stopped_frontend["program"].as_str().unwrap_or_default()),
        &user,
        "b: the stopped run left its native frontend running",
    );

    eprintln!(
        "two-lead isolation: requests a={} b={}; lead a answered {} ({}); lead b request {} stayed waiting and was stopped; no cross-root conversation",
        lead_a.kinds().len(),
        lead_b.kinds().len(),
        id_a,
        lead_a.state.display(),
        id_b
    );
}

/// Runs one installed native command in the passed home and workspace, bounded
/// by its own Job; the command's own report is recorded, never accepted as
/// delivery.
fn run_native_command(
    exe: &Path,
    home: &Path,
    workspace: &Path,
    root: &Path,
    args: Vec<OsString>,
    envs: Vec<(String, String)>,
    label: &str,
) -> Value {
    let mut spec = command(exe, home, workspace);
    spec.args = args;
    for (key, value) in envs {
        spec.env.insert(key.into(), Some(value.into()));
    }
    let stdout_path = root.join(format!("{label}-stdout.txt"));
    let stderr_path = root.join(format!("{label}-stderr.txt"));
    spec.stdout = Some(fs::File::create(&stdout_path).unwrap());
    spec.stderr = Some(fs::File::create(&stderr_path).unwrap());
    let job = Job::new(Limits::default()).unwrap();
    let child = job.spawn(&spec).unwrap();
    let outcome = job
        .wait(
            &child,
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
    json!({
        "exitCode": outcome.exit_code,
        "exited": outcome.reason == StopReason::Exited,
        "reason": format!("{:?}", outcome.reason),
        "stdout": excerpt(&stdout, 1500),
        "stderr": excerpt(&stderr, 1500),
    })
}

/// One `codex queue` invocation: the target thread and the message are always
/// explicit; only `Some(port)` names an owning listener and passes the
/// capability token through the environment of the command itself.
fn queue_arguments(thread: &str, message: &str, endpoint: Option<u16>) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "queue".into(),
        "--thread".into(),
        thread.into(),
        "--message".into(),
        message.into(),
    ];
    if let Some(port) = endpoint {
        args.push("--remote".into());
        args.push(format!("ws://127.0.0.1:{port}").into());
        args.push("--remote-auth-token-env".into());
        args.push("HARNESS_ORDINARY_LEAD_TOKEN".into());
    }
    args
}

/// Sends one line to the owned console session the way a user would: the text,
/// a short beat, then carriage return.
fn send_line(session: &ConsoleSession, text: &str) -> io::Result<()> {
    session.send(text)?;
    std::thread::sleep(Duration::from_millis(250));
    session.send("\r")
}

/// Polls a predicate for at most the given seconds.
fn wait_until(seconds: u64, predicate: impl Fn() -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < until {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    predicate()
}

/// The child path without the WindowsApps shims, exactly as the ordinary TUI
/// fixture selects it.
fn filtered_path() -> OsString {
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

fn dump_terminal(root: &Path, session: &ConsoleSession) {
    let _ = fs::write(root.join("terminal.txt"), session.transcript());
}

/// Every conversation rollout line under the running session's own `sessions`
/// root, one JSON value per line; the receiver's own events are read here and
/// nowhere claimed on behalf of another conversation.
fn session_lines(home: &Path) -> Vec<Value> {
    fn walk(path: &Path, lines: &mut Vec<Value>) {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                walk(&child, lines);
            } else if child
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                for line in fs::read_to_string(&child).unwrap_or_default().lines() {
                    if let Ok(value) = serde_json::from_str::<Value>(line) {
                        lines.push(value);
                    }
                }
            }
        }
    }
    let mut lines = Vec::new();
    walk(&home.join("sessions"), &mut lines);
    lines
}

/// Rollout rows of the running conversation that carry one marker.
fn session_mark_count(home: &Path, marker: &str) -> usize {
    session_lines(home)
        .iter()
        .filter(|row| row.to_string().contains(marker))
        .count()
}

/// The delivered-message identities the running conversation's own rollout
/// records for one marker: a delivered user message is one `response_item`
/// message row, while the completion event beside it only reports that item.
fn session_mark_ids(home: &Path, marker: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for row in session_lines(home) {
        if row["type"] != "response_item"
            || row["payload"]["type"] != "message"
            || row["payload"]["role"] != "user"
            || !row.to_string().contains(marker)
        {
            continue;
        }
        let id = row["payload"]["id"].as_str().unwrap_or_default();
        let id = id.strip_prefix("msg_").unwrap_or(id);
        ids.push(id.to_owned());
    }
    ids
}

/// The conversation identities the running session's own rollout meta events
/// record: another conversation's id appearing here is a clone or resume.
fn session_meta_ids(home: &Path) -> Vec<String> {
    let mut ids = Vec::new();
    for row in session_lines(home) {
        if row["type"] != "session_meta" {
            continue;
        }
        for key in ["id", "session_id"] {
            if let Some(value) = row["payload"][key].as_str() {
                ids.push(value.to_owned());
            }
        }
    }
    ids
}

/// Every provider request the owned canned provider recorded, with the
/// conversation identity the native request carried.
fn provider_entries(root: &Path) -> Vec<(String, Value, String, String)> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(root).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("provider-") && name.ends_with(".json")) {
            continue;
        }
        let body = fs::read(entry.path()).unwrap_or_default();
        let request = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
        let identity = RequestIdentity::from_request(&request)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        entries.push((name, request, identity.thread, identity.turn));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

/// Provider requests containing the marker, with the conversation identity
/// each native request carried.
fn provider_hits(root: &Path, marker: &str) -> Vec<(String, String, String)> {
    provider_entries(root)
        .into_iter()
        .filter(|(_, request, _, _)| contains_payload(request, marker))
        .map(|(name, _, thread, turn)| (name, thread, turn))
        .collect()
}

/// One recorded provider request body by its evidence file name.
fn provider_request(root: &Path, file: &str) -> Value {
    let body = fs::read(root.join(file)).unwrap_or_default();
    serde_json::from_slice(&body).unwrap_or(Value::Null)
}

/// The title-generation request installed 0.162.1 issues beside a user turn:
/// an auxiliary user item carrying the installed instruction, recognized
/// exactly as the shared fixture recognizes it.
fn title_generation_request(request: &Value) -> bool {
    request["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["role"] == "user"
                && item["content"].as_array().is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part["text"].as_str().is_some_and(|text| {
                            text.starts_with("Generate a concise, single-line task title")
                        })
                    })
                })
        })
    })
}

/// The tool continuation of the managed control exchange: the native request
/// that carries the completed `control-tool-1` proof command's own output.
fn proof_continuation(request: &Value) -> bool {
    request["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["type"] == "function_call_output"
                && item["call_id"] == "control-tool-1"
                && item["output"].as_str().is_some_and(|output| {
                    output.contains("Process exited with code 0") && output.contains("\none")
                })
        })
    })
}

fn provider_request_count(root: &Path) -> usize {
    provider_entries(root).len()
}

/// Requests that reached a conversation other than the receiver's own: a
/// clone, resume or second conversation appears here.
fn foreign_provider_requests(root: &Path, thread: &str) -> Vec<(String, String, String)> {
    provider_entries(root)
        .into_iter()
        .filter(|(_, _, hit_thread, _)| hit_thread != thread)
        .map(|(name, _, hit_thread, turn)| (name, hit_thread, turn))
        .collect()
}

fn hit_json(hits: &[(String, String, String)]) -> Vec<Value> {
    hits.iter()
        .map(|(file, thread, turn)| json!({"file": file, "thread": thread, "turn": turn}))
        .collect()
}

fn count_files(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_files(&path);
            } else {
                count += 1;
            }
        }
    }
    count
}

/// Historical upstream queue-boundary evidence, not production acceptance:
/// the installed native queue for an already running ordinary CLI
/// conversation. One owned ordinary native TUI hosts its own embedded runtime
/// in its own CODEX_HOME against the canned local Responses provider, one
/// control turn establishes that conversation's exact thread, and the
/// installed `codex queue --thread <id> --message <text>` then addresses that
/// running conversation without any endpoint, daemon or token. Delivery is the
/// running conversation's own turn request carrying the mark on its exact
/// thread, together with its own stored and rendered copy; exit status, report
/// text and process state are recorded separately and are not delivery.
/// Beside each turn the running TUI issues its own native title-generation
/// request (recognized by the installed instruction, exactly as the shared
/// fixture recognizes it) on an ephemeral thread; those auxiliaries are
/// classified and recorded, never counted as delivery or as a second
/// conversation. The whole observed record is written before any expectation,
/// and the expectations themselves name both outcomes.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE as the installation-record upstream; starts an owned ordinary native TUI with the canned local Responses provider; no live model, no daemon, no desktop"]
fn installed_queue_boundary_against_a_running_ordinary_tui() {
    let exe = codex_exe();
    let upstream = installed_upstream();
    assert!(
        same_executable(&exe, &upstream),
        "HARNESS_CONTROL_CODEX_EXE {} must be the installation-record upstream executable {}; the ordinary arm runs the installed native CLI, not another binary",
        exe.display(),
        upstream.display()
    );
    let version = version_text(&exe);
    let sha = build_identity::hash_file(&exe).unwrap();
    let owned_root = BrokerRoot::prepare().unwrap().keep();
    let root = owned_root.path().to_path_buf();
    let home = root.join("ordinary-home");
    let workspace = root.join("ordinary-workspace");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    eprintln!("ordinary-lead queue boundary evidence: {}", root.display());
    let user = process_service::current_user().unwrap();

    // The receiver is a real ordinary native TUI: the installed executable
    // hosts its own embedded runtime, and every request is answered with one
    // model-free assistant message so the control turn completes.
    let responses = control_responses::Responses::routed(root.clone(), |_request| {
        json!({
            "type": "message",
            "id": "ordinary-control-reply",
            "role": "assistant",
            "content": [{"type": "output_text", "text": ORDINARY_TUI_CONTROL_REPLY}]
        })
    });
    let trusted = serde_json::to_string(&workspace.to_string_lossy().to_lowercase()).unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            r#"
model = "gpt-6-astra"
model_provider = "control_fixture"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"
cli_auth_credentials_store = "file"
[model_providers.control_fixture]
name = "Owned ordinary TUI fixture"
base_url = "http://127.0.0.1:{}/v1"
wire_api = "responses"
env_key = "HARNESS_CONTROL_FIXTURE_KEY"
requires_openai_auth = false
request_max_retries = 0
stream_max_retries = 0
supports_websockets = false
[windows]
sandbox = "unelevated"
[features]
hooks = false
apps = false
multi_agent = false
multi_agent_v2 = false
memories = false
goals = false
plugins = false
skill_search = false
skip_host_skill_discovery = true
[analytics]
enabled = false
[tui]
screen_reader_detection_done = true
[projects.{trusted}]
trust_level = "trusted"
"#,
            responses.port
        ),
    )
    .unwrap();
    fs::write(
        home.join("auth.json"),
        serde_json::to_vec_pretty(&json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "harness-control-ordinary-tui"
        }))
        .unwrap(),
    )
    .unwrap();
    let mut spec = command(&exe, &home, &workspace);
    spec.args = vec!["--no-alt-screen".into()];
    spec.env.insert("PATH".into(), Some(filtered_path()));
    for key in [
        "HARNESS_EXECUTOR_RUN",
        "HARNESS_EXECUTOR_SESSION",
        "HARNESS_ORIGINATING_LEAD",
        "HARNESS_LEAD_THREAD",
        "HARNESS_LEAD_RECIPIENT",
    ] {
        spec.env.insert(key.into(), None);
    }
    let mut console = ConsoleSpec::new(spec);
    console.limits.memory_bytes = Some(512 * 1024 * 1024);
    console.limits.cpu_percent = Some(50.0);
    let session = ConsoleSession::spawn(console).unwrap();
    let identity = session.identity();
    let ready = wait_until(60, || {
        let text = session.transcript();
        text.contains("gpt-6-astra") || text.contains("Codex")
    });
    if !ready {
        dump_terminal(&root, &session);
        panic!(
            "the owned ordinary TUI did not become ready; evidence {}",
            root.display()
        );
    }
    std::thread::sleep(Duration::from_secs(2));

    // The control turn is the running conversation's own first model input:
    // its request identity establishes the exact receiver thread, cross
    // recorded with the conversation's own session meta. No slash command is
    // typed: the first composer submission is the prompt itself. If the
    // frontend has not yet accepted Enter, one further plain Enter commits the
    // same text the way a user would; the submission is observed as the
    // conversation's own recorded request, never as echoed text.
    send_line(&session, ORDINARY_TUI_CONTROL_PROMPT).unwrap();
    let mut submit_retries = 0;
    let mut control_requested = wait_until(20, || root.join("provider-1.json").is_file());
    while !control_requested && submit_retries < 2 {
        let _ = session.send("\r");
        submit_retries += 1;
        control_requested = wait_until(20, || root.join("provider-1.json").is_file());
    }
    if !control_requested {
        dump_terminal(&root, &session);
        panic!(
            "the owned provider recorded no request for the control turn, so either the running conversation issued none or the fixture rejected its request identity; evidence {}",
            root.display()
        );
    }
    let control_answered = wait_until(60, || {
        session_mark_count(&home, ORDINARY_TUI_CONTROL_REPLY) >= 1
            || session.transcript().contains(ORDINARY_TUI_CONTROL_REPLY)
    });
    // The running TUI's own title-generation request for the control turn
    // lands beside it; settle on the owned provider so the baseline covers the
    // whole turn instead of racing that native auxiliary request.
    let mut baseline_requests = provider_request_count(&root);
    for _ in 0..8 {
        std::thread::sleep(Duration::from_secs(1));
        let settled = provider_request_count(&root);
        if settled == baseline_requests {
            break;
        }
        baseline_requests = settled;
    }
    let entries = provider_entries(&root);
    let (_, _, thread, control_turn) = entries
        .first()
        .unwrap_or_else(|| panic!("no recorded provider request; evidence {}", root.display()))
        .clone();
    let baseline_auxiliary = entries
        .iter()
        .filter(|(_, request, _, _)| title_generation_request(request))
        .count();
    let meta_before = session_meta_ids(&home);
    let sessions_before = count_files(&home.join("sessions"));
    let daemon_socket = home
        .join("app-server-control")
        .join("app-server-control.sock");
    let daemon_socket_before = daemon_socket.exists();
    eprintln!(
        "ordinary lead queue boundary: receiver pid {} creation {} thread {thread} controlTurn {control_turn} controlAnswered {control_answered} submitRetries {submit_retries}; evidence {}",
        identity.pid,
        identity.creation_time,
        root.display()
    );

    // The queue attempt against that running conversation, with this
    // CODEX_HOME as the only locator: no endpoint, no capability token and no
    // daemon ahead of it; the command's own report is recorded, and the mark
    // can only be attributed to this exact invocation.
    let unmanaged = run_native_command(
        &exe,
        &home,
        &workspace,
        &root,
        queue_arguments(&thread, ORDINARY_QUEUE_MARK, None),
        Vec::new(),
        "queue-no-endpoint",
    );
    let first_hit = wait_until(WAIT.as_secs(), || {
        !provider_hits(&root, ORDINARY_QUEUE_MARK).is_empty()
    });
    std::thread::sleep(Duration::from_secs(6));
    let unmanaged_hits = provider_hits(&root, ORDINARY_QUEUE_MARK);
    let delivered_without_endpoint = first_hit || !unmanaged_hits.is_empty();
    let total_after_unmanaged = provider_request_count(&root);
    let auxiliary_after = provider_entries(&root)
        .iter()
        .filter(|(_, request, _, _)| title_generation_request(request))
        .count();
    let added_auxiliary = auxiliary_after.saturating_sub(baseline_auxiliary);
    let primary_hits: Vec<(String, String, String)> = unmanaged_hits
        .iter()
        .filter(|(_, hit_thread, _)| hit_thread == &thread)
        .cloned()
        .collect();
    let auxiliary_hits: Vec<(String, String, String)> = unmanaged_hits
        .iter()
        .filter(|(_, hit_thread, _)| hit_thread != &thread)
        .cloned()
        .collect();
    let transcript_copies = session.transcript().matches(ORDINARY_QUEUE_MARK).count();
    let rollout_copies = session_mark_count(&home, ORDINARY_QUEUE_MARK);
    let rollout_message_ids = session_mark_ids(&home, ORDINARY_QUEUE_MARK);
    let sessions_after_unmanaged = count_files(&home.join("sessions"));
    let meta_after = session_meta_ids(&home);
    let daemon_socket_after = daemon_socket.exists();
    let mut daemon_cleanup = Value::Null;
    if daemon_socket_after {
        daemon_cleanup = run_native_command(
            &exe,
            &home,
            &workspace,
            &root,
            vec!["app-server".into(), "daemon".into(), "stop".into()],
            Vec::new(),
            "daemon-stop",
        );
    }

    // A live delivery is the only case where a closed target still needs its
    // own refusal: the same route, a well-formed thread id the receiver does
    // not own.
    let mut closed_target = Value::Null;
    let mut closed_hits: Vec<(String, String, String)> = Vec::new();
    if delivered_without_endpoint {
        let closed = run_native_command(
            &exe,
            &home,
            &workspace,
            &root,
            queue_arguments(MISSING_THREAD_ID, MISSING_QUEUE_MARK, None),
            Vec::new(),
            "queue-missing-thread",
        );
        std::thread::sleep(Duration::from_secs(6));
        closed_hits = provider_hits(&root, MISSING_QUEUE_MARK);
        closed_target = json!({
            "command": closed,
            "providerHits": hit_json(&closed_hits),
            "providerRequestsTotal": provider_request_count(&root),
            "sessionFilesDelta": count_files(&home.join("sessions")) as i64
                - sessions_after_unmanaged as i64,
        });
    }
    let total_after_all = provider_request_count(&root);
    let foreign = foreign_provider_requests(&root, &thread);
    let receiver_alive = ServiceProcess::inspect(identity, &exe, &user)
        .map(|process| process.is_some())
        .unwrap_or(false);

    // Restoration: the owned receiver quits through its own console and the
    // installed executable no longer runs afterwards.
    dump_terminal(&root, &session);
    let _ = send_line(&session, "/quit");
    let console_result = session.wait(
        Deadline::after(Duration::from_secs(25)).unwrap(),
        &Cancellation::default(),
        Duration::from_secs(5),
    );
    let console_outcome = match &console_result {
        Ok(value) => {
            fs::write(root.join("terminal-final.txt"), &value.transcript).unwrap();
            json!({
                "exited": value.outcome.reason == StopReason::Exited,
                "outcome": value.outcome,
                "truncated": value.output_truncated,
            })
        }
        Err(error) => json!({"error": error.to_string()}),
    };
    // Recorded before the receipt and asserted only after it, so a failure
    // still leaves the observed liveness beside the raw provider requests.
    let receiver_gone_after_close = ServiceProcess::inspect(identity, &exe, &user)
        .map(|process| process.is_none())
        .unwrap_or(false);

    // The complete observed record is written before any expectation: a failed
    // assertion still leaves the receipt beside the raw provider requests.
    let receipt = json!({
        "schema": 1,
        "subject": "installed native queue against an already running ordinary embedded CLI conversation",
        "nativeVersion": version.trim(),
        "sha256": sha,
        "exe": exe.to_string_lossy(),
        "home": home.to_string_lossy(),
        "workspace": workspace.to_string_lossy(),
        "threadId": &thread,
        "receiver": {
            "pid": identity.pid,
            "creationTime": identity.creation_time,
            "aliveAfterAttempts": receiver_alive,
            "goneAfterClose": receiver_gone_after_close,
        },
        "marks": {
            "controlPrompt": ORDINARY_TUI_CONTROL_PROMPT,
            "controlReply": ORDINARY_TUI_CONTROL_REPLY,
            "queue": ORDINARY_QUEUE_MARK,
            "closed": MISSING_QUEUE_MARK,
        },
        "control": {
            "answered": control_answered,
            "turn": control_turn,
            "submitRetries": submit_retries,
        },
        "baseline": {
            "providerRequests": baseline_requests,
            "titleRequests": baseline_auxiliary,
            "sessionFiles": sessions_before,
            "sessionMetaIds": &meta_before,
            "daemonSocket": daemon_socket_before,
        },
        "queueWithoutEndpoint": {
            "command": &unmanaged,
            "delivered": delivered_without_endpoint,
            "providerHits": hit_json(&unmanaged_hits),
            "primaryHits": hit_json(&primary_hits),
            "auxiliaryTitleRequests": hit_json(&auxiliary_hits),
            "addedTitleRequests": added_auxiliary,
            "transcriptCopies": transcript_copies,
            "rolloutCopies": rollout_copies,
            "rolloutMessageIds": &rollout_message_ids,
            "providerRequestsTotal": total_after_unmanaged,
            "sessionFilesAfter": sessions_after_unmanaged,
            "sessionMetaIdsAfter": &meta_after,
            "daemonSocketAfter": daemon_socket_after,
            "daemonCleanup": &daemon_cleanup,
        },
        "closedTarget": &closed_target,
        "providerRequestsAfterAll": total_after_all,
        "foreignProviderRequests": hit_json(&foreign),
        "consoleOutcome": &console_outcome,
        "deliveryEvidence": "the running conversation's own model request carrying the mark on its exact thread, with its own stored and rendered copy",
        "notDeliveryEvidence": "exit status, stdout/stderr text, process existence",
        "limitations": "one installed version, one owned home, one receiver; the queued message is observed through the receiver's own requests and rollout",
    });
    fs::write(
        root.join("ordinary-lead-queue-boundary.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();

    // Expectations, checked after the record exists.
    assert!(
        baseline_requests >= 1,
        "the control turn must be among the running conversation's own model requests before any queue attempt; evidence {}",
        root.display()
    );
    assert!(
        control_answered,
        "the control turn must complete against the owned provider before any queue attempt; evidence {}",
        root.display()
    );
    assert!(
        receiver_alive,
        "the queue attempts must leave the running ordinary conversation alive; pid {}; evidence {}",
        identity.pid,
        root.display()
    );
    for (file, hit_thread, _) in &auxiliary_hits {
        assert!(
            title_generation_request(&provider_request(&root, file)),
            "{file} carried the queued message on thread {hit_thread} without the native title-generation shape; evidence {}",
            root.display()
        );
    }
    assert!(
        auxiliary_hits.len() <= 1,
        "one delivered turn cannot add more than one native title-generation request: {auxiliary_hits:?}; evidence {}",
        root.display()
    );
    for (file, hit_thread, _) in &foreign {
        assert!(
            title_generation_request(&provider_request(&root, file)),
            "model work left the running conversation without the native title-generation shape: {file} on thread {hit_thread}; evidence {}",
            root.display()
        );
    }
    assert_eq!(
        meta_after,
        meta_before,
        "the running conversation's own recorded identity changed: {meta_after:?} against {meta_before:?}; evidence {}",
        root.display()
    );
    assert_eq!(
        sessions_after_unmanaged,
        sessions_before,
        "the queue attempt created conversation storage; evidence {}",
        root.display()
    );
    if delivered_without_endpoint {
        assert_eq!(
            primary_hits.len(),
            1,
            "a live delivery must be exactly one turn input on the running conversation's own thread: {primary_hits:?}; evidence {}",
            root.display()
        );
        assert!(
            transcript_copies >= 1 || rollout_copies >= 1,
            "the running conversation's own session must hold the delivered message: transcript {transcript_copies}, rollout {rollout_copies}; evidence {}",
            root.display()
        );
        if rollout_copies >= 1 {
            assert_eq!(
                rollout_message_ids.len(),
                1,
                "the running conversation must store the delivered message exactly once: {rollout_message_ids:?}; evidence {}",
                root.display()
            );
        }
        assert!(
            added_auxiliary <= 1,
            "one delivered turn cannot add more than one native title-generation request: added {added_auxiliary}; evidence {}",
            root.display()
        );
        assert_eq!(
            total_after_unmanaged,
            baseline_requests + 1 + added_auxiliary,
            "the queue attempt must add only the delivered turn and at most its own native title request: {unmanaged}; evidence {}",
            root.display()
        );
        assert!(
            closed_hits.is_empty(),
            "queue with an unowned thread id started another conversation: {closed_hits:?}; evidence {}",
            root.display()
        );
        assert_eq!(
            closed_target["providerRequestsTotal"].as_u64(),
            Some(total_after_unmanaged as u64),
            "the closed-target attempt added model work: {closed_target}; evidence {}",
            root.display()
        );
        assert_eq!(
            closed_target["sessionFilesDelta"].as_i64(),
            Some(0),
            "the closed-target attempt created conversation storage: {closed_target}; evidence {}",
            root.display()
        );
    } else {
        assert_eq!(
            total_after_unmanaged,
            baseline_requests,
            "the refused queue attempt added model work: {unmanaged}; evidence {}",
            root.display()
        );
        assert_eq!(
            transcript_copies,
            0,
            "the running conversation rendered a mark that was never delivered; evidence {}",
            root.display()
        );
        assert_eq!(
            rollout_copies,
            0,
            "the running conversation stored a mark that was never delivered; evidence {}",
            root.display()
        );
    }
    assert!(
        !daemon_socket_after || daemon_cleanup["exited"] == Value::Bool(true),
        "a daemon appeared for the owned home and its stop was not recorded as exited: {daemon_cleanup}; evidence {}",
        root.display()
    );
    assert!(
        receiver_gone_after_close,
        "the ordinary-lead queue boundary probe left the running receiver TUI alive; pid {}; evidence {}",
        identity.pid,
        root.display()
    );
    assert!(
        console_outcome["exited"] == Value::Bool(true),
        "the owned ordinary TUI did not quit cleanly: {console_outcome}; evidence {}",
        root.display()
    );
    eprintln!(
        "ordinary-lead queue boundary: deliveredWithoutEndpoint={delivered_without_endpoint} primaryDeliveries={} auxiliaryTitleRequests={} transcriptCopies={transcript_copies} rolloutCopies={rollout_copies} foreignRequests={} receiverAlive={receiver_alive} receiverGoneAfterClose={receiver_gone_after_close}; evidence {}",
        primary_hits.len(),
        auxiliary_hits.len(),
        foreign.len(),
        root.display()
    );
}

/// The separately labelled positive control for the same installed `codex
/// queue`: one owned native app-server hosts one running conversation, and the
/// queue command names that listener and its capability token explicitly. The
/// delivered message starts exactly one turn on the running thread; that
/// turn's own sampling rounds (the queued input, then its executed proof-tool
/// continuation) are one delivery, not duplicates. The closed-target arm
/// proves the same route fails closed for a thread the receiver does not own.
/// This receiver is managed by construction; it is not the ordinary arm.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE as the installation-record upstream; owned native app-server fixture and canned Responses; no live model"]
fn installed_queue_endpoint_reaches_its_owning_thread() {
    let fixture = fixture();
    let root = fixture.root.path().to_path_buf();
    let home = root.join("home");
    let version = version_text(&fixture.exe);
    let user = process_service::current_user().unwrap();
    eprintln!(
        "queue endpoint positive control evidence: {}",
        root.display()
    );
    let mut owner = Client::connect(fixture.port, &fixture.token);
    let started = owner.request(
        "thread/start",
        json!({
            "cwd": fixture.workspace,
            "model": "gpt-6-astra",
            "modelProvider": "control_fixture",
            "allowProviderModelFallback": false,
            "approvalPolicy": "never",
            "sandbox": "danger-full-access"
        }),
    );
    let thread = started["thread"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("thread/start answered {started}"))
        .to_owned();
    // Installed 0.162.1 answers this metadata read; `includeTurns` reaches its
    // unimplemented list_turns RPC (-32601, "list_turns is not supported yet"),
    // so delivery is observed in the receiver's own model requests instead.
    let baseline = owner.request("thread/read", json!({"threadId": thread}));
    assert_eq!(baseline["thread"]["id"], thread, "{baseline}");
    assert_eq!(baseline["thread"]["status"]["type"], "idle", "{baseline}");
    let baseline_requests = provider_request_count(&root);
    let sessions_before = count_files(&home.join("sessions"));

    // 1. The owning endpoint and capability token: exactly one delivered turn
    // on the exact running thread, its proof-tool continuation included, with
    // no other model work.
    let managed = run_native_command(
        &fixture.exe,
        &home,
        &fixture.workspace,
        &root,
        queue_arguments(&thread, ENDPOINT_QUEUE_MARK, Some(fixture.port)),
        vec![(
            "HARNESS_ORDINARY_LEAD_TOKEN".to_owned(),
            fixture.token.clone(),
        )],
        "queue-endpoint",
    );
    let first_hit = wait_until(WAIT.as_secs(), || {
        !provider_hits(&root, ENDPOINT_QUEUE_MARK).is_empty()
    });
    std::thread::sleep(Duration::from_secs(6));
    let endpoint_hits = provider_hits(&root, ENDPOINT_QUEUE_MARK);
    let total_after_managed = provider_request_count(&root);
    let after_managed = owner.request("thread/read", json!({"threadId": thread}));
    let same_thread = endpoint_hits
        .iter()
        .all(|(_, hit_thread, _)| hit_thread == &thread);
    let sessions_after_managed = count_files(&home.join("sessions"));
    let managed_text = format!(
        "{}{}",
        managed["stdout"].as_str().unwrap_or_default(),
        managed["stderr"].as_str().unwrap_or_default()
    );
    let endpoint_bodies: Vec<Value> = provider_entries(&root)
        .into_iter()
        .filter(|(_, request, _, _)| contains_payload(request, ENDPOINT_QUEUE_MARK))
        .map(|(_, request, _, _)| request)
        .collect();
    let endpoint_turns: Vec<String> = endpoint_hits.iter().map(|(_, _, turn)| turn.clone()).fold(
        Vec::new(),
        |mut turns, turn| {
            if !turns.contains(&turn) {
                turns.push(turn);
            }
            turns
        },
    );
    let initial_rounds = endpoint_bodies
        .iter()
        .filter(|request| !proof_continuation(request))
        .count();
    let proof_rounds = endpoint_bodies
        .iter()
        .filter(|request| proof_continuation(request))
        .count();

    // 2. A closed target fails closed: the same endpoint, a thread this
    // receiver does not own - no delivery, no second conversation, no model
    // work.
    let missing = run_native_command(
        &fixture.exe,
        &home,
        &fixture.workspace,
        &root,
        queue_arguments(MISSING_THREAD_ID, MISSING_QUEUE_MARK, Some(fixture.port)),
        vec![(
            "HARNESS_ORDINARY_LEAD_TOKEN".to_owned(),
            fixture.token.clone(),
        )],
        "queue-missing-thread",
    );
    std::thread::sleep(Duration::from_secs(6));
    let missing_hits = provider_hits(&root, MISSING_QUEUE_MARK);
    let total_after_missing = provider_request_count(&root);
    let status = owner.request("thread/read", json!({"threadId": thread}));
    let sessions_after_missing = count_files(&home.join("sessions"));
    let foreign = foreign_provider_requests(&root, &thread);

    let receipt = json!({
        "schema": 1,
        "subject": "installed queue endpoint positive control for its owning thread (managed app-server receiver, not the ordinary arm)",
        "nativeVersion": version.trim(),
        "exe": fixture.exe.to_string_lossy(),
        "threadId": &thread,
        "marks": {"endpoint": ENDPOINT_QUEUE_MARK, "missing": MISSING_QUEUE_MARK},
        "endpoint": {
            "command": &managed,
            "deliveredTurns": endpoint_turns.len(),
            "rounds": endpoint_hits.len(),
            "initialRounds": initial_rounds,
            "proofRounds": proof_rounds,
            "providerHits": hit_json(&endpoint_hits),
            "receiverAfterDelivery": {
                "id": after_managed["thread"]["id"].clone(),
                "status": after_managed["thread"]["status"].clone(),
            },
            "sameThread": same_thread,
            "providerRequestsTotal": total_after_managed,
        },
        "closedTarget": {
            "command": &missing,
            "providerHits": hit_json(&missing_hits),
            "providerRequestsTotal": total_after_missing,
            "sessionFilesDelta": sessions_after_missing as i64 - sessions_after_managed as i64,
        },
        "baselineProviderRequests": baseline_requests,
        "sessionFilesBaseline": sessions_before,
        "foreignProviderRequests": hit_json(&foreign),
        "deliveryEvidence": "the receiver's own model request carrying the queued message on the exact thread",
        "notDeliveryEvidence": "exit status, stdout/stderr text, process existence",
    });
    fs::write(
        root.join("queue-endpoint-positive-control.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();

    assert_eq!(
        baseline_requests, 0,
        "an idle running conversation must not have called a model before any queue attempt"
    );
    assert!(
        first_hit,
        "queue with the owning endpoint did not reach the running conversation: {managed}; evidence {}",
        root.display()
    );
    assert!(
        same_thread,
        "endpoint delivery reached another conversation: {endpoint_hits:?}; evidence {}",
        root.display()
    );
    assert_eq!(
        endpoint_turns.len(),
        1,
        "queue delivery must start exactly one turn on the running conversation: {endpoint_hits:?}; evidence {}",
        root.display()
    );
    assert_eq!(
        initial_rounds,
        1,
        "the queued message must enter the running turn exactly once: {endpoint_hits:?}; evidence {}",
        root.display()
    );
    assert!(
        proof_rounds >= 1,
        "the delivered turn must continue through its own proof-tool round: {endpoint_hits:?}; evidence {}",
        root.display()
    );
    assert_eq!(
        after_managed["thread"]["id"], thread,
        "the running conversation's thread identity changed after delivery: {after_managed}"
    );
    assert_eq!(
        total_after_managed,
        baseline_requests + endpoint_hits.len(),
        "the delivered turn's own rounds must be the only model work: {managed}"
    );
    assert_eq!(
        managed["exitCode"].as_i64(),
        Some(0),
        "queue with the owning endpoint failed: {managed}"
    );
    assert!(
        !managed_text.contains(fixture.token.as_str()),
        "queue must not echo the capability token: {managed}"
    );
    assert!(
        missing_hits.is_empty(),
        "queue with an unowned thread id started another conversation: {missing_hits:?}; command {missing}; evidence {}",
        root.display()
    );
    assert_eq!(
        total_after_missing, total_after_managed,
        "the closed-target attempt added model work: {missing}"
    );
    assert_eq!(
        status["thread"]["id"], thread,
        "the running conversation's thread identity changed: {status}"
    );
    assert!(
        foreign.is_empty(),
        "model work left the running conversation: {foreign:?}; evidence {}",
        root.display()
    );
    assert_eq!(
        sessions_after_missing, sessions_after_managed,
        "a closed-target queue attempt created conversation storage"
    );

    // Restoration: stop this fixture's own receiver through its own Job and
    // confirm the installed executable no longer runs.
    fixture.job.terminate(0, Duration::from_secs(2)).unwrap();
    // TerminateProcess is asynchronous: give the exact recorded identity a
    // bounded window to disappear before it counts as still running.
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until
        && ServiceProcess::inspect(fixture.server, &fixture.exe, &user)
            .map(|process| process.is_some())
            .unwrap_or(true)
    {
        std::thread::sleep(Duration::from_millis(250));
    }
    assert_process_gone(
        fixture.server,
        &fixture.exe,
        &user,
        "the queue endpoint positive control left its receiver app-server running",
    );
    eprintln!(
        "queue endpoint positive control: endpointTurns={} endpointRounds={} initialRounds={initial_rounds} proofRounds={proof_rounds} closedTargetDeliveries={} foreignRequests={}; evidence {}",
        endpoint_turns.len(),
        endpoint_hits.len(),
        missing_hits.len(),
        foreign.len(),
        root.display()
    );
}

// ---------------------------------------------------------------------------
// Managed lead start acceptance
// ---------------------------------------------------------------------------

/// Enable the lifecycle on a separate owned native board while a real managed
/// lead is already visible. No accepted task exists, so journal consumption
/// must start without launching either reflection conversation.
fn enable_reflection_for_native_lead(
    entry: &Path,
    home: &Path,
    checkout: &Path,
    evidence: &Path,
    thread: &str,
) -> (ProcessIdentity, PathBuf, PathBuf) {
    assert!(
        !home.join("harness/delivery-reflection/hosts").exists(),
        "a mode-off lead must not create a reflection controller"
    );
    let project = evidence.join("reflection-consumer");
    fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "--initial-branch=main"]);
    git(&project, &["config", "user.email", "fixture@example.test"]);
    git(&project, &["config", "user.name", "Fixture"]);
    fs::write(
        project.join("README.md"),
        "owned native reflection lifecycle check\n",
    )
    .unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "seed"]);
    let init = Command::new("bd.exe")
        .args([
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "rfl",
        ])
        .current_dir(&project)
        .env("BD_NON_INTERACTIVE", "1")
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    // Both routes are explicit fixture bindings. No job is eligible and no
    // provider request is permitted from this lifecycle check.
    fs::write(home.join("ds.config.toml"), "model = \"deepseek-flash\"\nmodel_provider = \"canned_skill_catalog\"\nmodel_reasoning_effort = \"max\"\n").unwrap();
    let enabled = Command::new(entry)
        .args(["delivery-improvement", "enable", "--project"])
        .arg(&project)
        .arg("--source")
        .arg(checkout)
        .arg("--codex-home")
        .arg(home)
        .env("CODEX_HOME", home)
        .env("CODEX_THREAD_ID", thread)
        .env_remove("HARNESS_EXECUTOR_SESSION")
        .env_remove("HARNESS_REFLECTION_SESSION")
        .output()
        .unwrap();
    assert!(
        enabled.status.success(),
        "{} {}",
        String::from_utf8_lossy(&enabled.stdout),
        String::from_utf8_lossy(&enabled.stderr)
    );
    let path = home.join("harness/delivery-reflection/hosts").join(format!(
        "{}.json",
        build_identity::hash_bytes(thread.as_bytes())
    ));
    assert!(
        wait_until(30, || fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|value| value["state"] == "running")),
        "the enabled lifecycle created no native controller; evidence {}",
        evidence.display()
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    fs::write(
        evidence.join("reflection-host-start.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    let process = ProcessIdentity {
        pid: receipt["process"]["pid"]
            .as_u64()
            .unwrap()
            .try_into()
            .unwrap(),
        creation_time: receipt["process"]["creationTime"].as_u64().unwrap(),
    };
    let program = PathBuf::from(receipt["process"]["program"].as_str().unwrap());
    assert!(
        ServiceProcess::inspect(process, &program, &process_service::current_user().unwrap())
            .unwrap()
            .is_some()
    );
    let status = Command::new(entry)
        .args(["delivery-improvement", "status", "--project"])
        .arg(&project)
        .arg("--source")
        .arg(checkout)
        .arg("--codex-home")
        .arg(home)
        .env("CODEX_HOME", home)
        .env_remove("HARNESS_EXECUTOR_SESSION")
        .env_remove("HARNESS_REFLECTION_SESSION")
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["host"]["processLive"], true);
    assert_eq!(status["pending"], 0);
    assert_eq!(status["retained"], 0);
    let lifecycle_file = PathBuf::from(status["stateFile"].as_str().unwrap());
    assert!(
        wait_until(20, || fs::read(&lifecycle_file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|value| value["last_poll"].is_object())),
        "the running controller never consumed the native journal; evidence {}",
        evidence.display()
    );
    assert!(
        ServiceProcess::inspect(process, &program, &process_service::current_user().unwrap())
            .unwrap()
            .is_some()
    );
    (process, program, path)
}

/// One actual `codex-harness lead start` session: the checkout-built harness
/// hosts the lead profile in this terminal through one native app-server, its
/// attached native frontend, the canned local Responses provider and the
/// registered unrelated stdio MCP server. The entry point is the real
/// `lead start` command; an ordinary Normal session or a directly composed
/// app-server does not stand in for it.
///
/// The session's own runtime catalogue must lose the competing native
/// `codex_worktrees` tools while the unrelated server's tools survive, the
/// transcript must report the resolved role revision and the bound model,
/// provider and effort, and the published endpoint must appear only after the
/// frontend attached to the exact thread and must retire with the session.
///
/// The hosted app-server route cannot observe one permitted configuration
/// conflict - an explicit worktree leaf that re-exposes the competing tools is
/// owned by the registered-launcher check
/// `installed_managed_role_tui_catalogues_drop_native_worktree_tools` - so
/// this check claims only the observed suppression and the preserved
/// unrelated surface, never a successful managed-pool delivery.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE as the installation-record upstream and the checkout-built harness; owned canned Responses and unrelated MCP fixture; no live model"]
fn managed_lead_start_hosts_the_catalogue_without_native_worktree_tools() {
    let exe = codex_exe();
    let version = qualified_installed_upstream(&exe);
    let upstream = installed_upstream();
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let evidence = tempfile::Builder::new()
        .prefix("lead-start-native-")
        .tempdir()
        .unwrap()
        .keep();
    println!(
        "managed lead start evidence: {} (upstream {} {})",
        evidence.display(),
        upstream.display(),
        version.trim()
    );
    let home = evidence.join("home");
    fs::create_dir_all(home.join("harness")).unwrap();
    // `lead start` launches `<home>/harness/bin/codex.exe`, so the recorded
    // build is that exact directory: this checkout's real launcher beside its
    // integrity stubs, with the record's source root pointing at this
    // checkout. The registration names the installed upstream the launcher
    // forwards to; nothing global is read or changed.
    let build = home.join("harness/bin");
    skill_catalog_responses::managed_launcher_build_in(&build, &checkout);
    let registration_bytes = serde_json::to_vec(&harness_core::native_launcher::Registration {
        schema: 2,
        state: None,
        build: Some(build.clone()),
        task_control: false, goal_recovery: false,
        upstream: harness_core::native_launcher::Upstream {
            executable: upstream.clone(),
            sha256: build_identity::hash_file(&upstream).unwrap(),
            package: None,
        },
    })
    .unwrap();
    fs::write(home.join("harness/native-launch.json"), &registration_bytes).unwrap();
    fs::write(evidence.join("native-launch.json"), &registration_bytes).unwrap();
    let canned = skill_catalog_responses::CannedResponses::spawn_all_tools(&evidence).unwrap();
    let unrelated = skill_catalog_responses::unrelated_probe_mcp(&home);
    // The session runs in this checkout, so the checkout is the trusted
    // project; the canned provider is bound to the exact model, provider and
    // effort this run asserts, and the unrelated server is the only other
    // tool source.
    skill_catalog_responses::write_probe_config(&home, &checkout, &canned, &unrelated, false);
    skill_catalog_responses::clear_probe_receipt(&home);
    let expected_revision =
        harness_core::role_context::resolve(&checkout, harness_core::role_context::Role::Lead)
            .unwrap()
            .revision;

    let entry = PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"));
    let mut spec = CommandSpec::new(&entry);
    spec.args = vec![
        "lead".into(),
        "start".into(),
        "--source".into(),
        checkout.clone().into_os_string(),
        "--codex-home".into(),
        home.clone().into_os_string(),
        "--exec".into(),
        skill_catalog_responses::ALL_TOOLS_PROBE_PROMPT.into(),
    ];
    spec.current_dir = Some(checkout.clone());
    spec.env
        .insert("CODEX_HOME".into(), Some(home.clone().into_os_string()));
    spec.env.insert(
        skill_catalog_responses::API_KEY_ENV.into(),
        Some(skill_catalog_responses::API_KEY.into()),
    );
    spec.env.insert("PATH".into(), Some(filtered_path()));
    spec.env.insert(
        "CODEX_HARNESS_CPU_ACCOUNT".into(),
        Some(evidence.join("cpu-account").into_os_string()),
    );
    for key in [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "CODEX_SESSION_ID",
        "CODEX_THREAD_ID",
        "HARNESS_EXECUTOR_SESSION",
        "HARNESS_EXECUTOR_RUN",
        "HARNESS_CONTEXT_ROLE",
        "HARNESS_CONTEXT_REVISION",
        "HARNESS_ORIGINATING_LEAD",
        "HARNESS_LEAD_THREAD",
        "HARNESS_LEAD_RECIPIENT",
        "WT_SESSION",
    ] {
        spec.env.insert(key.into(), None);
    }
    let mut console = ConsoleSpec::new(spec);
    console.limits.memory_bytes = Some(1024 * 1024 * 1024);
    console.limits.cpu_percent = Some(50.0);
    let session = ConsoleSession::spawn(console).unwrap();
    let host = session.identity();

    // The host reports the resolved lead context and the bound routing before
    // it starts anything: these lines are this run's own source and runtime
    // identity, and the role revision is resolved from the same checkout the
    // registered build names.
    let expected_context =
        format!("lead role context: role=lead revision={expected_revision} previous=none");
    let expected_session = format!(
        "lead session: profile=default model=gpt-6-astra provider=canned_skill_catalog effort=low cwd={}",
        checkout.display()
    );
    let started = wait_until(60, || {
        let text = session.transcript();
        text.contains(&expected_context) && text.contains(&expected_session)
    });
    if !started {
        dump_terminal(&evidence, &session);
        panic!(
            "the managed lead host did not report its resolved role context and bound routing; evidence {}",
            evidence.display()
        );
    }

    // The registry record is published only after the native frontend attached
    // to this exact thread, so its appearance is the attachment evidence.
    let registry = home.join("harness/lead-endpoints");
    let published = wait_until(90, || {
        fs::read_dir(&registry)
            .map(|entries| {
                entries.flatten().any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "json")
                })
            })
            .unwrap_or(false)
    });
    if !published {
        dump_terminal(&evidence, &session);
        panic!(
            "the managed lead session published no endpoint; evidence {}",
            evidence.display()
        );
    }
    let mut records: Vec<PathBuf> = fs::read_dir(&registry)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    assert_eq!(
        records.len(),
        1,
        "the registry must publish exactly this session's record"
    );
    let endpoint_path = records.pop().unwrap();
    let endpoint: Value = serde_json::from_slice(&fs::read(&endpoint_path).unwrap()).unwrap();
    let port = u16::try_from(endpoint["port"].as_u64().unwrap()).unwrap();
    let token = endpoint["token"].as_str().unwrap().to_owned();
    let thread = endpoint["threadId"].as_str().unwrap().to_owned();
    let recorded = ProcessIdentity {
        pid: u32::try_from(endpoint["process"]["pid"].as_u64().unwrap()).unwrap(),
        creation_time: endpoint["process"]["creationTime"].as_u64().unwrap(),
    };
    let program = PathBuf::from(endpoint["process"]["program"].as_str().unwrap());
    assert!(
        same_executable(&program, &build.join("codex.exe")),
        "the published endpoint must record the registered launcher this home hosts, not another program: {}",
        program.display()
    );
    let user = process_service::current_user().unwrap();
    assert!(
        ServiceProcess::inspect(recorded, &program, &user)
            .unwrap()
            .is_some(),
        "the published endpoint must record the live app-server child of this session"
    );
    fs::write(
        evidence.join("endpoint.json"),
        serde_json::to_vec_pretty(&json!({
            "port": port,
            "threadId": thread,
            "process": {
                "pid": recorded.pid,
                "creationTime": recorded.creation_time,
                "program": program,
            }
        }))
        .unwrap(),
    )
    .unwrap();

    // Three probe rounds: the `--exec` first prompt and two typed turns, each
    // executing one fresh runtime-catalogue call inside this session. Every
    // reading must show the unrelated MCP surface and none of the competing
    // native worktree tools.
    let mut readings: Vec<(String, Vec<String>)> = Vec::new();
    for round in 1..=3 {
        if round > 1 {
            send_line(&session, skill_catalog_responses::ALL_TOOLS_PROBE_PROMPT).unwrap();
        }
        let seen = readings.last().map(|(call_id, _)| call_id.clone());
        let captured = canned.wait_for(Duration::from_secs(180), |requests| {
            skill_catalog_responses::probe_readings(requests)
                .last()
                .is_some_and(|(call_id, _)| Some(call_id) != seen.as_ref())
        });
        assert!(
            captured,
            "probe round {round} captured no runtime catalogue output; evidence {}",
            evidence.display()
        );
        let current = skill_catalog_responses::probe_readings(&canned.requests());
        let (call_id, names) = current
            .last()
            .cloned()
            .expect("a captured reading is present");
        assert!(
            !readings.iter().any(|(earlier, _)| *earlier == call_id),
            "probe round {round} re-read an earlier probe output; evidence {}",
            evidence.display()
        );
        assert!(
            !names.is_empty(),
            "probe round {round} reported an empty runtime catalogue; evidence {}",
            evidence.display()
        );
        let fragments = skill_catalog_responses::tool_catalogue::native_worktree_names(&names);
        assert!(
            fragments.is_empty(),
            "the managed lead runtime catalogue kept the competing native worktree tools: {fragments:?}; evidence {}",
            evidence.display()
        );
        let tools = skill_catalog_responses::tool_catalogue::unrelated_probe_surface(&names);
        assert!(
            !tools.is_empty(),
            "probe round {round} must keep the registered unrelated MCP server's tools: names {names:?}; evidence {}",
            evidence.display()
        );
        fs::write(
            evidence.join(format!("all-tools-round-{round}.json")),
            serde_json::to_vec_pretty(&json!({
                "call_id": call_id,
                "names": &names,
                "unrelated": &tools,
            }))
            .unwrap(),
        )
        .unwrap();
        readings.push((call_id, names));
    }

    // The session reached the registered unrelated server as a real stdio MCP
    // server: its own receipt records this session's full handshake, because
    // the previous receipt was cleared before the host started.
    let receipt = skill_catalog_responses::wait_for_probe_receipt(&home).unwrap_or_else(|| {
        panic!(
            "the managed lead session left no fresh unrelated MCP handshake receipt at {}",
            skill_catalog_responses::probe_receipt_path(&home).display()
        )
    });
    skill_catalog_responses::assert_probe_receipt(&receipt, "the managed lead-start session");
    fs::write(
        evidence.join("mcp-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();

    // The request that carried the first probe turn is the exact model and
    // effort this session bound, not the fixture's defaults.
    let probe = canned
        .requests()
        .into_iter()
        .find(|request| {
            request
                .body
                .contains(skill_catalog_responses::ALL_TOOLS_PROBE_PROMPT)
        })
        .expect("a captured request carries the probe prompt");
    fs::write(evidence.join("first-probe.body.json"), &probe.body).unwrap();
    let body: Value = serde_json::from_str(&probe.body).unwrap();
    assert_eq!(
        body["model"],
        "gpt-6-astra",
        "the lead turn must run the bound model; body keys {:?}",
        body.as_object().map(|map| map.keys().collect::<Vec<_>>())
    );
    assert_eq!(
        body["reasoning"]["effort"],
        "low",
        "the lead turn must run the bound reasoning effort; body keys {:?}",
        body.as_object().map(|map| map.keys().collect::<Vec<_>>())
    );

    let (reflection_process, reflection_program, reflection_receipt) =
        enable_reflection_for_native_lead(&entry, &home, &checkout, &evidence, &thread);

    // Ending the frontend ends the host: the exact thread and this host's
    // working files must retire, the app-server child must be gone, and the
    // retired port must refuse the recorded address.
    send_line(&session, "/quit").unwrap();
    let outcome = session
        .wait(
            Deadline::after(Duration::from_secs(90)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(10),
        )
        .unwrap();
    let transcript = outcome.transcript.clone();
    fs::write(evidence.join("terminal.txt"), &transcript).unwrap();
    assert_eq!(outcome.outcome.reason, StopReason::Exited, "{transcript}");
    assert_eq!(outcome.outcome.exit_code, 0, "{transcript}");
    assert!(
        transcript.contains(&format!(
            "lead session: ended thread {thread} profile default frontend-exit 0"
        )),
        "the host must report the exact ended thread and its frontend exit: {transcript}"
    );
    assert!(
        transcript.contains("lead endpoint:") && transcript.contains("was retired"),
        "the host must report the retired endpoint: {transcript}"
    );
    assert!(
        !transcript.contains("lead session note:"),
        "the host reported a cleanup problem: {transcript}"
    );
    assert!(
        !endpoint_path.exists(),
        "the retired session's registry record outlived it: {}",
        endpoint_path.display()
    );
    let host_dir = home.join("harness/lead-endpoints/host");
    for path in [
        host_dir.join(format!("{}.json", host.pid)),
        host_dir.join(format!("{}.token", host.pid)),
    ] {
        assert!(
            !path.exists(),
            "a retired lead host must leave no working address file: {}",
            path.display()
        );
    }
    if let Ok(log) = fs::read(host_dir.join(format!("{}.log", host.pid))) {
        fs::write(evidence.join("host.log"), log).unwrap();
    }
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until
        && ServiceProcess::inspect(recorded, &program, &user)
            .map(|process| process.is_some())
            .unwrap_or(true)
    {
        std::thread::sleep(Duration::from_millis(250));
    }
    assert_process_gone(
        recorded,
        &program,
        &user,
        "the managed lead session left its app-server child running",
    );
    assert_process_gone(
        reflection_process,
        &reflection_program,
        &user,
        "the managed lead left an orphaned reflection journal consumer",
    );
    let reflection_status: Value =
        serde_json::from_slice(&fs::read(&reflection_receipt).unwrap()).unwrap();
    assert_eq!(reflection_status["state"], "stopped");
    fs::write(
        evidence.join("reflection-host-stopped.json"),
        serde_json::to_vec_pretty(&reflection_status).unwrap(),
    )
    .unwrap();
    assert!(
        ControlConnection::connect(port, &token, Duration::from_millis(500)).is_err(),
        "the retired endpoint still accepted a connection on port {port}"
    );
    eprintln!(
        "managed lead start: thread {thread} rounds {} unrelatedTools {}; evidence {}",
        readings.len(),
        readings
            .last()
            .map(|(_, names)| {
                skill_catalog_responses::tool_catalogue::unrelated_probe_surface(names).len()
            })
            .unwrap_or(0),
        evidence.display()
    );
}
