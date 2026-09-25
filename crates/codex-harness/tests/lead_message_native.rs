#![cfg(windows)]

//! One model-free proof that `lead message` reaches the owning thread of the
//! native app-server fixture used by
//! `native_lead_queue_and_app_server_input_on_owning_thread`.
//!
//! The executable comes from `HARNESS_CONTROL_CODEX_EXE` at runtime. This test
//! starts that fixture's existing app-server; it does not add a listener,
//! daemon, launcher change, or task_control.

#[path = "fixtures/control_responses.rs"]
mod control_responses;

use harness_core::{
    broker_state::BrokerRoot,
    process::{CommandSpec, Job, Limits, ProcessIdentity},
    process_service::{self, ServiceProcess},
    task_control::ControlConnection,
    task_request::RequestIdentity,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(20);
const PAYLOAD: &str = "\u{43f}\u{440}\u{438}\u{432}\u{435}\u{442} from the executor\nsecond line: $(throw), %PATH%, `whoami`";

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

fn fixture() -> Fixture {
    let exe = codex_exe();
    let owned_root = BrokerRoot::prepare().unwrap().keep();
    let root = owned_root.path();
    let home = root.join("home");
    let workspace = root.join("workspace");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&workspace).unwrap();
    let version = version_text(&exe);
    assert!(
        version.contains("codex-cli 0.156.1"),
        "HARNESS_CONTROL_CODEX_EXE must be the installed Codex 0.156.1 binary: {version}"
    );
    let responses = control_responses::Responses::start(root.into(), true);
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
    let generation = "generation-native-lead";
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
    let harness = PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"));
    let sent = Command::new(harness)
        .args(["lead", "message", "--text", PAYLOAD])
        .env("CODEX_HOME", root.join("run-home"))
        .env("HARNESS_EXECUTOR_RUN", generation)
        .env("HARNESS_EXECUTOR_SESSION", "1")
        .env_remove("HARNESS_ORIGINATING_LEAD")
        .env_remove("HARNESS_LEAD_THREAD")
        .env_remove("HARNESS_LEAD_RECIPIENT")
        .stdin(Stdio::null())
        .output()
        .unwrap();
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
