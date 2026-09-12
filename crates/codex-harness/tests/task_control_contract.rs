//! Explicit native CLI input; owned model-free Responses, homes, tools and jobs.
#![cfg(windows)]
#[path = "fixtures/control_responses.rs"]
mod control_responses;

use harness_core::{
    broker_state::BrokerRoot,
    console::{ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason},
    task_control::ControlConnection,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(15);

struct Client {
    connection: ControlConnection,
    log: fs::File,
    pending: VecDeque<Value>,
    id: u64,
}

impl Client {
    fn connect(port: u16, token: &str, root: &Path, label: &str) -> Self {
        let mut client = Self {
            connection: ControlConnection::connect(port, token, WAIT).unwrap(),
            log: OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(root.join(format!("{label}.jsonl")))
                .unwrap(),
            pending: VecDeque::new(),
            id: 0,
        };
        client.request("initialize", json!({"clientInfo":{"name":"harness-control-contract","version":"1"},"capabilities":{"experimentalApi":true}}));
        client
            .connection
            .send(&json!({"method":"initialized"}), WAIT)
            .unwrap();
        client
    }

    fn receive(&mut self, until: Instant) -> Value {
        loop {
            assert!(
                Instant::now() < until,
                "observation deadline; task state is unknown"
            );
            if let Some(value) = self.connection.receive(Duration::from_millis(200)).unwrap() {
                writeln!(self.log, "{value}").unwrap();
                return value;
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        self.connection
            .send(&json!({"id":self.id,"method":method,"params":params}), WAIT)
            .unwrap();
        let until = Instant::now() + WAIT;
        loop {
            let value = self.receive(until);
            if value["id"] == self.id {
                assert!(value.get("error").is_none(), "{method}: {value}");
                return value["result"].clone();
            }
            assert!(self.pending.len() < 512, "contract pending-event bound");
            self.pending.push_back(value);
        }
    }

    fn event(&mut self, predicate: impl Fn(&Value) -> bool) -> Value {
        if let Some(index) = self.pending.iter().position(&predicate) {
            return self.pending.remove(index).unwrap();
        }
        let until = Instant::now() + WAIT;
        loop {
            let value = self.receive(until);
            if predicate(&value) {
                return value;
            }
            assert!(self.pending.len() < 512);
            self.pending.push_back(value);
        }
    }
}

fn command(exe: &Path, home: &Path, workspace: &Path) -> CommandSpec {
    let mut command = CommandSpec::new(exe);
    command.current_dir = Some(workspace.into());
    command.env.insert("CODEX_HOME".into(), Some(home.into()));
    for key in ["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_ACCESS_TOKEN"] {
        command.env.insert(key.into(), None);
    }
    command.env.insert(
        "HARNESS_CONTROL_FIXTURE_KEY".into(),
        Some("synthetic-owned-fixture".into()),
    );
    let path = std::env::join_paths(
        std::env::split_paths(&std::env::var_os("PATH").unwrap()).filter(|path| {
            !path
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("windowsapps")
        }),
    )
    .unwrap();
    command.env.insert("PATH".into(), Some(path));
    command
}

fn capture(exe: &Path, home: &Path, workspace: &Path, root: &Path, args: &[&str], label: &str) {
    let mut spec = command(exe, home, workspace);
    spec.args = args.iter().map(Into::into).collect();
    spec.stdout = Some(fs::File::create(root.join(format!("{label}-stdout.txt"))).unwrap());
    spec.stderr = Some(fs::File::create(root.join(format!("{label}-stderr.txt"))).unwrap());
    let job = Job::new(Limits::default()).unwrap();
    let child = job.spawn(&spec).unwrap();
    let outcome = job
        .wait(
            &child,
            Deadline::after(WAIT).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(outcome.reason, StopReason::Exited);
    assert_eq!(outcome.exit_code, 0);
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; runs only owned native model-free state"]
fn native_two_clients_reconnect_tool_result_and_tui() {
    let NativeFixture {
        exe,
        root: owned_root,
        home,
        workspace,
        port,
        token,
        job,
        _responses,
        server,
    } = native_fixture(false);
    let root = owned_root.path();
    let mut first = Client::connect(port, &token, root, "first");
    let mut observer = Client::connect(port, &token, root, "observer");
    let started = first.request("thread/start", json!({"cwd":workspace,"model":"gpt-6-astra","modelProvider":"control_fixture","allowProviderModelFallback":false,"approvalPolicy":"never","sandbox":"danger-full-access"}));
    assert_eq!(started["model"], "gpt-6-astra");
    assert_eq!(started["modelProvider"], "control_fixture");
    let parent = started["thread"]["id"].as_str().unwrap();
    observer.event(|value| {
        value["method"] == "thread/started" && value["params"]["thread"]["id"] == parent
    });
    first.request("turn/start", json!({"threadId":parent,"input":[{"type":"text","text":"Delegate the owned proof to one native child."}]}));
    observer.event(|value| {
        value["method"] == "thread/status/changed"
            && value["params"]["threadId"] == parent
            && value["params"]["status"]["type"] == "active"
    });
    observer.request("thread/resume", json!({"threadId":parent}));
    let spawned = first.event(|value| {
        (value["method"] == "item/completed"
            && value["params"]["item"]["type"] == "subAgentActivity"
            && value["params"]["item"]["kind"] == "started")
            || (value["method"] == "turn/completed" && value["params"]["threadId"] == parent)
    });
    assert_eq!(spawned["params"]["threadId"], parent, "{spawned}");
    let thread = spawned["params"]["item"]["agentThreadId"].as_str().unwrap();
    assert_ne!(thread, parent);
    let mut second = Client::connect(port, &token, root, "second");
    let resumed = second.request("thread/resume", json!({"threadId":thread}));
    assert_eq!(resumed["thread"]["id"], thread);
    assert_eq!(resumed["thread"]["parentThreadId"], parent);
    assert_eq!(resumed["thread"]["sessionId"], parent);
    assert_eq!(resumed["model"], "gpt-6-astra");
    assert_eq!(resumed["modelProvider"], "control_fixture");
    // Spawn is visible before the child's first turn; observe actual tool work
    // before testing disconnect survival.
    if !resumed.to_string().contains("commandExecution") {
        second.event(|value| {
            value["method"] == "item/started"
                && value["params"]["threadId"] == thread
                && value["params"]["item"]["type"] == "commandExecution"
        });
    }
    let working = second.request(
        "thread/read",
        json!({"threadId":thread,"includeTurns":true}),
    );
    assert_eq!(working["thread"]["status"]["type"], "active");
    let turn_id = working["thread"]["turns"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["id"]
        .clone();
    // This tests transport, not the native goal scheduler: a canned final does
    // not call update_goal and an active goal would request further turns.
    second.request("thread/resume", json!({"threadId":parent}));
    first.request("thread/goal/set", json!({"threadId":parent,"objective":"Preserve the owned proof and consume the tool result","status":"paused"}));
    second.event(|value| {
        value["method"] == "thread/goal/updated" && value["params"]["threadId"] == parent
    });
    drop(first);
    let mut third = Client::connect(port, &token, root, "reconnected");
    let reconnected = third.request("thread/resume", json!({"threadId":thread}));
    assert_eq!(reconnected["thread"]["status"]["type"], "active");
    for client in [&mut second, &mut third] {
        let completed = client.event(|value| {
            value["method"] == "turn/completed"
                && value["params"]["threadId"] == thread
                && value["params"]["turn"]["id"] == turn_id
        });
        assert_eq!(
            completed["params"]["turn"]["status"], "completed",
            "{completed}"
        );
    }
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt")).unwrap(),
        "one",
        "tool mutation must happen exactly once"
    );
    let saved = third.request(
        "thread/read",
        json!({"threadId":thread,"includeTurns":true}),
    );
    assert!(saved.to_string().contains(control_responses::FINAL));
    assert_eq!(
        third.request("thread/goal/get", json!({"threadId":parent}))["goal"]["status"],
        "paused"
    );
    fs::write(root.join("protocol-result.json"), serde_json::to_vec_pretty(&json!({"two_clients":true,"reconnect":true,"native_child_survived":true,"mutation_count":1,"final_result":true,"parentThreadId":parent,"threadId":thread,"server_pid":server.pid,"cli":exe,"cli_version":fs::read_to_string(root.join("version-stdout.txt")).unwrap().trim(),"sha256":format!("{:x}",Sha256::digest(fs::read(&exe).unwrap())),"schema_sha256":format!("{:x}",Sha256::digest(fs::read(root.join("schema/codex_app_server_protocol.schemas.json")).unwrap()))})).unwrap()).unwrap();

    let mut tui = command(&exe, &home, &workspace);
    tui.args = vec![
        "--remote".into(),
        format!("ws://127.0.0.1:{port}").into(),
        "--remote-auth-token-env".into(),
        "HARNESS_CONTROL_TOKEN".into(),
        "--no-alt-screen".into(),
        "resume".into(),
        thread.into(),
    ];
    tui.env
        .insert("HARNESS_CONTROL_TOKEN".into(), Some(token.into()));
    let session = ConsoleSession::spawn(ConsoleSpec::new(tui)).unwrap();
    let until = Instant::now() + WAIT;
    while !session.transcript().contains(control_responses::FINAL) && Instant::now() < until {
        fs::write(root.join("terminal.txt"), session.transcript()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
    }
    fs::write(root.join("terminal.txt"), session.transcript()).unwrap();
    assert!(
        session.transcript().contains(control_responses::FINAL),
        "native TUI final visibility; inspect terminal.txt"
    );
    fs::write(root.join("tui-result.json"), "{\"final_visible\":true}").unwrap();
    // Owned console/server Jobs terminate their descendants even on an assertion.
    drop(session);
    drop(third);
    drop(second);
    drop(job);
}
struct NativeFixture {
    exe: PathBuf,
    root: BrokerRoot,
    home: PathBuf,
    workspace: PathBuf,
    port: u16,
    token: String,
    job: Job,
    _responses: control_responses::Responses,
    server: harness_core::process::ProcessIdentity,
}

#[test]
#[ignore = "requires explicit native CLI; owned protocol and synthetic responses"]
fn native_named_empty_thread_can_be_attached() {
    let fixture = native_fixture(true);
    let root = fixture.root.path();
    let mut owner = Client::connect(fixture.port, &fixture.token, root, "empty-owner");
    let started = owner.request("thread/start", json!({"cwd":fixture.workspace,"model":"gpt-6-astra","modelProvider":"control_fixture","allowProviderModelFallback":false,"approvalPolicy":"never","sandbox":"danger-full-access"}));
    let id = started["thread"]["id"].as_str().unwrap();
    assert_eq!(started["approvalPolicy"], "never");
    assert_eq!(started["sandbox"]["type"], "dangerFullAccess");
    owner.request(
        "thread/name/set",
        json!({"threadId":id,"name":"Owned empty conversation"}),
    );
    let attached = owner.request("thread/resume", json!({"threadId":id}));
    assert_eq!(attached["thread"]["id"], id);
    assert!(!root.join("provider-1.json").exists());
    fs::write(
        root.join("named-empty-result.json"),
        serde_json::to_vec_pretty(&json!({"threadId":id,"attachedBeforeModel":true})).unwrap(),
    )
    .unwrap();
    let arguments: Vec<_> = [
        "-c",
        "approval_policy=\"never\"",
        "-c",
        "sandbox_mode=\"danger-full-access\"",
        "--no-alt-screen",
        "Perform the owned proof command and return its consumed result.",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    let planned = harness_core::task_arguments::plan(&arguments, &fixture.workspace)
        .unwrap()
        .unwrap();
    let mut tui = command(&fixture.exe, &fixture.home, &fixture.workspace);
    tui.args = vec![
        "--remote".into(),
        format!("ws://127.0.0.1:{}", fixture.port).into(),
        "--remote-auth-token-env".into(),
        "HARNESS_CONTROL_TOKEN".into(),
        "resume".into(),
        id.into(),
    ];
    tui.args.extend(planned.attachment);
    tui.env.insert(
        "HARNESS_CONTROL_TOKEN".into(),
        Some(fixture.token.clone().into()),
    );
    let session = ConsoleSession::spawn(ConsoleSpec::new(tui)).unwrap();
    let until = Instant::now() + WAIT;
    while !session.transcript().contains(control_responses::FINAL) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    fs::write(root.join("named-empty-terminal.txt"), session.transcript()).unwrap();
    assert!(
        session.transcript().contains(control_responses::FINAL),
        "native resume of named empty thread; inspect named-empty-terminal.txt"
    );
    assert_eq!(
        fs::read_to_string(fixture.workspace.join("proof.txt")).unwrap(),
        "one"
    );
}

#[test]
#[ignore = "opens owned visible native windows; requires HARNESS_CONTROL_VISUAL_ACCEPTANCE=1 and scoped desktop receipts"]
fn native_visible_chats_before_model_dispatch() {
    assert_eq!(
        std::env::var("HARNESS_CONTROL_VISUAL_ACCEPTANCE").as_deref(),
        Ok("1")
    );
    let fixture = native_fixture(true);
    let root = fixture.root.path();
    let mut control = Client::connect(fixture.port, &fixture.token, root, "views-control");
    let mut native_views = Vec::new();
    let placements = harness_core::task_view::three_windows().unwrap();
    let mut views = Vec::new();
    for label in ["lead", "executor-a", "executor-b"] {
        let mut spec = command(&fixture.exe, &fixture.home, &fixture.workspace);
        spec.new_console = Some(format!("Harness visibility {label}").into());
        spec.args = vec![
            "--remote".into(),
            format!("ws://127.0.0.1:{}", fixture.port).into(),
            "--remote-auth-token-env".into(),
            "HARNESS_CONTROL_TOKEN".into(),
            "--no-alt-screen".into(),
        ];
        spec.env.insert(
            "HARNESS_CONTROL_TOKEN".into(),
            Some(fixture.token.clone().into()),
        );
        let child =
            harness_core::task_view::View::spawn(&spec, placements[native_views.len()], WAIT)
                .unwrap();
        eprintln!(
            "visible native client: {label}, pid {}",
            child.identity().pid
        );
        let started = control.event(|event| {
            event["method"] == "thread/started" && event["params"]["thread"]["ephemeral"] != true
        });
        let id = started["params"]["thread"]["id"].as_str().unwrap();
        control.request(
            "thread/name/set",
            json!({"threadId":id,"name":format!("Harness visibility {label}")}),
        );
        views.push(json!({"label":label,"threadId":id,"process":{
            "pid":child.identity().pid,"creationTime":child.identity().creation_time}}));
        fs::write(
            root.join("visible-clients.json"),
            serde_json::to_vec_pretty(&views).unwrap(),
        )
        .unwrap();
        native_views.push(child);
    }
    assert!(
        !root.join("provider-1.json").exists(),
        "opening conversations must not call a model"
    );
    fs::write(
        root.join("views-ready.json"),
        serde_json::to_vec_pretty(&json!({"views":views,"providerRequests":0})).unwrap(),
    )
    .unwrap();
    eprintln!(
        "awaiting scoped desktop observation of all three windows: {}",
        root.display()
    );
    let observed = root.join("views-observed.json");
    let until = Instant::now() + Duration::from_secs(90);
    while !observed.exists() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
    let receipt: Value =
        serde_json::from_slice(&fs::read(observed).expect("owned desktop observation receipt"))
            .unwrap();
    assert_eq!(receipt["simultaneouslyVisible"], true);
    assert_eq!(
        receipt["threads"],
        json!(
            views
                .iter()
                .map(|v| v["threadId"].clone())
                .collect::<Vec<_>>()
        )
    );
    assert!(
        !root.join("provider-1.json").exists(),
        "no model request before visible views"
    );
    let lead = views[0]["threadId"].as_str().unwrap();
    let snapshots = native_views
        .iter()
        .map(|view| view.snapshot().unwrap())
        .collect::<Vec<_>>();
    fs::write(
        root.join("window-readiness.json"),
        serde_json::to_vec_pretty(&snapshots).unwrap(),
    )
    .unwrap();
    control.request("turn/start", json!({"threadId":lead,"input":[{"type":"text","text":"Run the owned proof and return its result."}]}));
    control.event(|event| {
        event["method"] == "thread/status/changed"
            && event["params"]["threadId"] == lead
            && event["params"]["status"]["type"] == "active"
    });
    control.request("thread/resume", json!({"threadId":lead}));
    control
        .event(|event| event["method"] == "turn/completed" && event["params"]["threadId"] == lead);
    assert_eq!(
        fs::read_to_string(fixture.workspace.join("proof.txt")).unwrap(),
        "one"
    );
    fs::write(root.join("visible-result-ready.json"), b"{}").unwrap();
    let until = Instant::now() + Duration::from_secs(90);
    while !root.join("visible-result-observed.json").exists() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
    let receipt: Value = serde_json::from_slice(
        &fs::read(root.join("visible-result-observed.json"))
            .expect("native final desktop observation"),
    )
    .unwrap();
    assert_eq!(receipt["finalVisible"], true);
    fs::write(root.join("visible-result.json"), serde_json::to_vec_pretty(&json!({"windows":3,"requestsBeforeViews":0,"toolEffects":1,"finalVisible":true,"automaticViewGuard":true,"runtimeIntegration":false})).unwrap()).unwrap();
    drop(native_views);
}

fn native_fixture(direct: bool) -> NativeFixture {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    assert!(exe.is_absolute() && exe.is_file());
    let owned_root = BrokerRoot::prepare().unwrap().keep();
    let root = owned_root.path();
    eprintln!("control contract evidence: {}", root.display());
    let home = root.join("home");
    let workspace = root.join("workspace");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&workspace).unwrap();
    capture(&exe, &home, &workspace, root, &["--version"], "version");
    let schema = root.join("schema");
    capture(
        &exe,
        &home,
        &workspace,
        root,
        &[
            "app-server",
            "generate-json-schema",
            "--experimental",
            "--out",
            schema.to_str().unwrap(),
        ],
        "schema",
    );
    let responses = control_responses::Responses::start(root.into(), direct);
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
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
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
            "server startup; inspect retained stderr"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    NativeFixture {
        exe,
        root: owned_root,
        home,
        workspace,
        port,
        token,
        job,
        _responses: responses,
        server: server.identity(),
    }
}
