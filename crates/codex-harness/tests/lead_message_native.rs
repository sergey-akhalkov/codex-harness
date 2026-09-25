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
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(20);
/// One fresh dispatch runs the executor shell preflight before its session
/// exists, so the first record takes longer than a watch step.
const DISPATCH_WAIT: Duration = Duration::from_secs(300);
/// An answered executor continues on its own thread and completes; that turn
/// plus the run's own closure is bounded well below a live model call.
const ANSWER_WAIT: Duration = Duration::from_secs(120);
const PAYLOAD: &str = "\u{43f}\u{440}\u{438}\u{432}\u{435}\u{442} from the executor\nsecond line: $(throw), %PATH%, `whoami`";
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
        version.contains("codex-cli 0.157.0"),
        "HARNESS_CONTROL_CODEX_EXE must be the installed Codex 0.157.0 binary: {version}"
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
/// exact process it names - not a claim inside a record.
fn assert_visible_surface(root: &LeadRoot) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
    };
    let receipt = root.receipt();
    let surface = &receipt["window"];
    let window = surface["window"].as_u64().unwrap_or_else(|| {
        panic!(
            "{}: the dispatch recorded no visible surface: {receipt}",
            root.role
        )
    });
    let pid = surface["process"]["pid"].as_u64().unwrap_or(0) as u32;
    let mut owner = 0u32;
    let bounds = &surface["bounds"];
    assert!(
        unsafe { IsWindow(window as HWND) } != 0
            && unsafe { GetWindowThreadProcessId(window as HWND, &mut owner) } != 0
            && owner == pid
            && unsafe { IsWindowVisible(window as HWND) } != 0
            && unsafe { IsIconic(window as HWND) } == 0
            && bounds["width"].as_i64().unwrap_or(0) > 0
            && bounds["height"].as_i64().unwrap_or(0) > 0,
        "{}: the recorded surface is not this run's own live window: {surface}",
        root.role
    );
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
    let upstream = installed_upstream();
    assert!(
        same_executable(&exe, &upstream),
        "HARNESS_CONTROL_CODEX_EXE must be the installation-record upstream executable {} so this check runs the installed native CLI, not another binary",
        upstream.display()
    );
    let version = version_text(&exe);
    assert!(
        version.contains("codex-cli 0.157.0"),
        "the installation-record upstream must be Codex 0.157.0: {version}"
    );
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
