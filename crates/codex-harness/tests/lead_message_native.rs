#![cfg(windows)]

//! One model-free proof that `lead message` reaches the owning thread of the
//! native app-server fixture used by
//! `native_lead_queue_and_app_server_input_on_owning_thread`.
//!
//! The executable comes from `HARNESS_CONTROL_CODEX_EXE` at runtime. This test
//! starts that fixture's existing app-server; it does not add a listener,
//! daemon, launcher change, or task_control.
//!
//! `installed_two_leads_keep_questions_waiting_and_replies_isolated` is the
//! installed two-lead isolation acceptance: two independent lead roots, each
//! with its own native lead session, its own spawned executor relationship with
//! its own worktree and session records, and its own owned canned Responses
//! provider. The questions are issued by the executor's own native session
//! through the installed entry point and the answer is issued by the lead's own
//! native session; every delivery is established by the receiving
//! conversation's own provider input, never by a local record alone.

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
    sync::{Arc, Mutex},
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

/// The installed `codex.exe` is the harness launcher. It reads
/// `harness/native-launch.json` from `CODEX_HOME` before it starts the
/// registered app-server. The fixture home is empty, so copy that existing
/// registration in; this does not add a listener or launcher.
fn seed_launcher_registration(home: &Path) {
    let source = harness_core::native_launcher::codex_home()
        .expect("installed launcher home")
        .join("harness/native-launch.json");
    assert!(
        source.is_file(),
        "installed launcher registration harness/native-launch.json is missing; the 0.156.1 launcher cannot host this fixture"
    );
    let destination = home.join("harness");
    fs::create_dir_all(&destination).unwrap();
    fs::copy(&source, destination.join("native-launch.json")).unwrap_or_else(|error| {
        panic!("could not seed harness/native-launch.json into the fixture home: {error}")
    });
}

fn fixture() -> Fixture {
    let exe = codex_exe();
    let owned_root = BrokerRoot::prepare().unwrap().keep();
    let root = owned_root.path();
    let home = root.join("home");
    let workspace = root.join("workspace");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&workspace).unwrap();
    seed_launcher_registration(&home);
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
/// can name it.
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
        let input = input_text(&request["input"]);
        let (answer, hold) = {
            let mut route = state.lock().unwrap();
            let thread = identity.thread.clone();
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
                    "shell": "powershell",
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

/// One isolated lead root: its own kit home, checkout, worktree, owned provider
/// and both native sessions (the lead and its spawned executor) served by the
/// installed upstream CLI.
struct LeadRoot {
    // Processes first: the jobs reap both owned sessions before the evidence
    // directory is removed or kept.
    _lead_job: Job,
    _exec_job: Job,
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
    generation: String,
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
}

impl LeadRoot {
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
        let checkout = root.join("checkout");
        // The pool position of slot 1 of this checkout, exactly as the
        // configured pool names it.
        let worktree = root.join("checkout-wt1");
        for dir in [&home, &workspace, &checkout] {
            fs::create_dir_all(dir).unwrap();
        }
        // The installed launcher registration is what lets an owned home host
        // the installed native CLI; a synthetic home is not a launcher.
        seed_launcher_registration(&home);
        // A real checkout with one committed base and the pool slot as its own
        // worktree, so the recorded mapping is the one a dispatch records.
        git(&checkout, &["init", "-q", "--initial-branch=main"]);
        git(
            &checkout,
            &["config", "user.email", "executor@example.test"],
        );
        git(&checkout, &["config", "user.name", "Executor"]);
        fs::create_dir_all(checkout.join("global")).unwrap();
        fs::write(checkout.join("global/orchestration.toml"), orchestration()).unwrap();
        fs::write(checkout.join("README.md"), "seed\n").unwrap();
        git(&checkout, &["add", "."]);
        git(&checkout, &["commit", "-qm", "seed"]);
        let base = git_text(&checkout, &["rev-parse", "HEAD"]);
        git(
            &checkout,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                worktree.to_str().unwrap(),
            ],
        );
        let home = home.canonicalize().unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let checkout = checkout.canonicalize().unwrap();
        let worktree = worktree.canonicalize().unwrap();
        let owner = format!("exec-two-lead-{role}");
        let generation = format!("generation-two-lead-{role}-{}", now_ms());
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
        write_root_config(&home, provider.port, &[&workspace, &worktree, &checkout]);
        let (lead_job, lead, lead_client, lead_thread) =
            launch_native_session(exe, &home, &workspace, &root, "lead", &[]);
        let (exec_job, executor, exec_client, exec_thread) = launch_native_session(
            exe,
            &home,
            &worktree,
            &root,
            "executor",
            &[
                ("HARNESS_EXECUTOR_RUN", generation.as_str()),
                ("HARNESS_EXECUTOR_SESSION", "1"),
            ],
        );
        {
            let mut route = route.lock().unwrap();
            route.lead_thread = lead_thread.clone();
            route.exec_thread = exec_thread.clone();
        }
        let state = harness_core::task_worktree::pool_state_dir(&home, &checkout).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("slot-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "source": checkout,
                "index": 1,
                "path": worktree,
                "state": "occupied",
                "owner": owner,
                "base": base,
                "disposition": null,
                "reason": null,
            }))
            .unwrap(),
        )
        .unwrap();
        // The live identity of this run: its own host process (the app-server
        // that serves the conversation) and the lead session that dispatched
        // it. Both are real, live and distinct per root.
        let lease = json!({
            "schema": 1,
            "owner": owner,
            "index": 1,
            "path": worktree,
            "pid": executor.identity.pid,
            "created": executor.identity.creation_time,
            "program": exe,
        });
        fs::write(
            state.join("lease-1.json"),
            serde_json::to_vec_pretty(&lease).unwrap(),
        )
        .unwrap();
        fs::write(
            state.join("endpoint-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "port": executor.port,
                "token": read_token(&root.join("executor-ws-token")),
                "threadId": exec_thread,
                "process": {
                    "pid": executor.identity.pid,
                    "creationTime": executor.identity.creation_time,
                    "program": exe,
                }
            }))
            .unwrap(),
        )
        .unwrap();
        // The lead's verified existing endpoint: the real native session this
        // root's executor reports to. Ordinary installed leads expose none, so
        // the acceptance owns this one exactly as the single-lead check does.
        fs::write(
            state.join("lead-endpoint-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "port": lead.port,
                "token": read_token(&root.join("lead-ws-token")),
                "threadId": lead_thread,
                "process": {
                    "pid": lead.identity.pid,
                    "creationTime": lead.identity.creation_time,
                    "program": exe,
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            state.join("spawn-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "launcher": exe,
                "profile": "default",
                "mode": "exec",
                "args": [],
                "visible": true,
                "host": "owned-console",
                "control": {
                    "schema": 1,
                    "assignment": "Execute the recorded proof command and report its consumed result.",
                    "identity": {
                        "profile": "default",
                        "model": "gpt-6-astra",
                        "modelProvider": "control_fixture",
                        "reasoningEffort": "low"
                    }
                },
                "terminal": null,
                "isolation": false,
                "slot": {
                    "index": 1,
                    "path": worktree,
                    "source": checkout,
                    "owner": owner,
                    "base": base,
                    "remote": "origin",
                    "branch": "main"
                },
                "model": "gpt-6-astra",
                "modelProvider": "control_fixture",
                "reasoningEffort": "low",
                "window": null,
                "shell": null,
                "observation": {
                    "schema": 1,
                    "coverage": "native",
                    "state": "running",
                    "session": exec_thread,
                    "exitCode": null,
                    "events": 4,
                    "messages": 1,
                    "toolCalls": 1,
                    "malformed": 0,
                    "cause": null,
                    "host": {
                        "pid": executor.identity.pid,
                        "created": executor.identity.creation_time,
                        "program": exe,
                    },
                    "result": state.join("message-1.txt"),
                    "detail": state.join("stream-1.jsonl"),
                    "updatedMs": now_ms(),
                },
                "originatingLead": {
                    "schema": 1,
                    "threadId": lead_thread,
                    "runGeneration": generation,
                    "dispatcher": {
                        "pid": lead.identity.pid,
                        "creationTime": lead.identity.creation_time,
                        "program": exe,
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            _lead_job: lead_job,
            _exec_job: exec_job,
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
            generation,
            question,
            answer,
            route,
            _provider: provider,
            lead,
            executor,
            lead_client,
            exec_client,
            lead_thread,
            exec_thread,
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

    /// The executor's own native session submits its assignment. This is what
    /// the installed host does; the canned provider then issues the recorded
    /// `lead message` command through the executor's own shell.
    fn ask(&mut self) {
        self.exec_client.request(
            "turn/start",
            json!({
                "threadId": self.exec_thread,
                "input": [{
                    "type": "text",
                    "text": "Execute the recorded proof command and report its consumed result."
                }]
            }),
        );
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// The configured pool of one fixture checkout: the single default profile the
/// two-lead acceptance binds, and one pool slot.
fn orchestration() -> String {
    "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"default\"\nexecutor_profiles = [\"default\"]\nmax_concurrent_executors = 1\n".to_owned()
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
            .to_ascii_lowercase()
    };
    normalize(reported) == normalize(&expected.to_string_lossy())
}

/// Two actual installed native lead sessions, each with its own spawned
/// executor relationship, worktree, session records and owned provider root.
/// Each executor's own native session asks its own lead through the installed
/// entry point; the blocked executor stays waiting while the lead answers
/// through the recorded reply reference. Every step is established inside the
/// receiving conversation's own provider input, and the two roots never see
/// each other's exchange.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE as the installation-record upstream and the deployed kit; two native lead roots with owned canned Responses; no live model"]
fn installed_two_leads_keep_questions_waiting_and_replies_isolated() {
    let exe = codex_exe();
    let upstream = installed_upstream();
    assert!(
        same_executable(&exe, &upstream),
        "HARNESS_CONTROL_CODEX_EXE must be the installation-record upstream executable {} so this check runs the installed native CLI, not another binary",
        upstream.display()
    );
    let version = version_text(&exe);
    assert!(
        version.contains("codex-cli 0.156.1"),
        "the installation-record upstream must be Codex 0.156.1: {version}"
    );
    let entry = installed_entry();
    eprintln!(
        "installed two-lead isolation: entry={} upstream={} ({})",
        entry.display(),
        exe.display(),
        version.trim()
    );
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
    assert_ne!(lead_a.worktree, lead_b.worktree);
    assert_ne!(lead_a.generation, lead_b.generation);

    // Each executor's own native session submits its assignment and asks its
    // own lead through the installed entry point; its turn then ends blocked.
    lead_a.ask();
    lead_b.ask();
    lead_a.wait_kind("lead-question");
    lead_b.wait_kind("lead-question");
    let id_a = lead_a.wait_question_recorded();
    let id_b = lead_b.wait_question_recorded();
    lead_a.wait_kind("exec-waiting");
    lead_b.wait_kind("exec-waiting");
    lead_a.wait_executor_idle();
    lead_b.wait_executor_idle();
    assert_ne!(
        id_a, id_b,
        "the two requests must have their own identities"
    );

    // Each root's owned provider served exactly its own exchange, in the order
    // the two native sessions produced it: the assignment asked, the question
    // was delivered into the lead's own conversation, and the recorded tool
    // result ended the executor's turn waiting for the answer.
    for root in [&lead_a, &lead_b] {
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

    // One command from the correct lead, inside its own native session,
    // continues only that lead's executor on the same thread.
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

    // The answer reached nothing else: the neighbor root produced no further
    // traffic, never saw the answer or the answered identity, and its own
    // executor stays waiting on its own request.
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
    assert_eq!(
        lead_a.kinds().len(),
        6,
        "a refused attempt reached a conversation"
    );
    assert_eq!(
        lead_b.kinds().len(),
        3,
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
    let (thread, cwd) = (lead_a.exec_thread.clone(), lead_a.worktree.clone());
    assert_bound_thread(&mut lead_a.exec_client, &thread, &cwd, "executor a");
    let (thread, cwd) = (lead_b.lead_thread.clone(), lead_b.workspace.clone());
    assert_bound_thread(&mut lead_b.lead_client, &thread, &cwd, "lead b");
    let (thread, cwd) = (lead_b.exec_thread.clone(), lead_b.worktree.clone());
    assert_bound_thread(&mut lead_b.exec_client, &thread, &cwd, "executor b");

    // The runs keep their own host, session, lease, slot and worktree: waiting
    // and answering changed no lifecycle record, started no resume and left no
    // terminal surface to rearrange.
    let user = process_service::current_user().unwrap();
    for root in [&lead_a, &lead_b] {
        let receipt = root.receipt();
        assert_eq!(
            receipt["observation"]["state"], "running",
            "{}: {receipt}",
            root.role
        );
        assert_eq!(
            receipt["observation"]["session"],
            root.exec_thread.as_str(),
            "{}: {receipt}",
            root.role
        );
        assert_eq!(
            receipt["slot"]["owner"],
            root.owner.as_str(),
            "{}: {receipt}",
            root.role
        );
        assert_eq!(
            receipt["slot"]["path"],
            root.worktree.to_string_lossy().as_ref(),
            "{}: {receipt}",
            root.role
        );
        assert!(receipt["terminal"].is_null(), "{}: {receipt}", root.role);
        assert!(receipt["window"].is_null(), "{}: {receipt}", root.role);
        for address in [&root.lead, &root.executor] {
            let observed = ServiceProcess::inspect(address.identity, &root.exe, &user)
                .unwrap()
                .unwrap_or_else(|| panic!("{}: a native session is no longer live", root.role));
            assert_eq!(observed.identity(), address.identity, "{}", root.role);
        }
        let lease: Value =
            serde_json::from_slice(&fs::read(root.state.join("lease-1.json")).unwrap()).unwrap();
        let lease_identity = ProcessIdentity {
            pid: lease["pid"].as_u64().unwrap() as u32,
            creation_time: lease["created"].as_u64().unwrap(),
        };
        assert!(
            ServiceProcess::inspect(lease_identity, &root.exe, &user)
                .unwrap()
                .is_some(),
            "{}: the run's lease host was stopped",
            root.role
        );
        let endpoint: Value =
            serde_json::from_slice(&fs::read(root.state.join("endpoint-1.json")).unwrap()).unwrap();
        assert_eq!(
            endpoint["threadId"],
            root.exec_thread.as_str(),
            "{}",
            root.role
        );
        assert_eq!(
            endpoint["port"].as_u64().unwrap() as u16,
            root.executor.port,
            "{}: the executor's conversation was replaced",
            root.role
        );
        let lead_endpoint: Value =
            serde_json::from_slice(&fs::read(root.state.join("lead-endpoint-1.json")).unwrap())
                .unwrap();
        assert_eq!(
            lead_endpoint["threadId"],
            root.lead_thread.as_str(),
            "{}",
            root.role
        );
        for unexpected in [
            "spawn-2.json",
            "endpoint-2.json",
            "frontend-1.json",
            "message-2.txt",
        ] {
            assert!(
                !root.state.join(unexpected).exists(),
                "{}: an unexpected record appeared: {unexpected}",
                root.role
            );
        }
    }

    eprintln!(
        "two-lead isolation: requests a={} b={}; lead a answered {}; lead b request {} stayed waiting; no cross-root conversation",
        lead_a.kinds().len(),
        lead_b.kinds().len(),
        id_a,
        id_b
    );
}
