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
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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
        let value = self.raw_request(method, params);
        assert!(value.get("error").is_none(), "{method}: {value}");
        value["result"].clone()
    }

    /// Returns the complete response record so contract probes can classify
    /// result versus error instead of assuming either one.
    fn raw_request(&mut self, method: &str, params: Value) -> Value {
        let id = self.send_request(method, params);
        self.await_response(id)
    }

    /// Sends a request and returns its id without waiting, so an independent
    /// action such as an interrupt is not delayed by the response.
    fn send_request(&mut self, method: &str, params: Value) -> u64 {
        self.id += 1;
        self.connection
            .send(&json!({"id":self.id,"method":method,"params":params}), WAIT)
            .unwrap();
        self.id
    }

    fn await_response(&mut self, id: u64) -> Value {
        let until = Instant::now() + WAIT;
        loop {
            let value = self.receive(until);
            if value["id"] == id {
                return value;
            }
            assert!(self.pending.len() < 512, "contract pending-event bound");
            self.pending.push_back(value);
        }
    }

    /// Bounded event search that returns None instead of panicking when the
    /// optional contract observation does not happen.
    fn find(&mut self, until: Instant, predicate: impl Fn(&Value) -> bool) -> Option<Value> {
        if let Some(index) = self.pending.iter().position(&predicate) {
            return Some(self.pending.remove(index).unwrap());
        }
        while Instant::now() < until {
            match self.connection.receive(Duration::from_millis(200)) {
                Ok(Some(value)) => {
                    writeln!(self.log, "{value}").unwrap();
                    if predicate(&value) {
                        return Some(value);
                    }
                    assert!(self.pending.len() < 512, "contract pending-event bound");
                    self.pending.push_back(value);
                }
                Ok(None) => {}
                Err(error) => panic!("control receive: {error}"),
            }
        }
        None
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

    /// Drains notifications for one short window without retaining them. A running
    /// tool can emit more events than the contract pending bound, and this
    /// comparison only needs to know whether the marker or turn completion was
    /// among them.
    fn drain_seen(
        &mut self,
        until: Instant,
        thread: &str,
        turn: &str,
        marker: &str,
    ) -> (bool, bool) {
        let mut marker_event = false;
        let mut turn_completed = false;
        let note = |value: &Value, marker_event: &mut bool, turn_completed: &mut bool| {
            if value["params"]["threadId"] == thread && value.to_string().contains(marker) {
                *marker_event = true;
            }
            if !turn.is_empty()
                && value["method"] == "turn/completed"
                && value["params"]["threadId"] == thread
                && value["params"]["turn"]["id"] == turn
            {
                *turn_completed = true;
            }
        };
        for value in self.pending.drain(..) {
            note(&value, &mut marker_event, &mut turn_completed);
        }
        while Instant::now() < until {
            match self.connection.receive(Duration::from_millis(40)) {
                Ok(Some(value)) => {
                    writeln!(self.log, "{value}").unwrap();
                    note(&value, &mut marker_event, &mut turn_completed);
                }
                Ok(None) => {}
                Err(error) => panic!("control receive: {error}"),
            }
        }
        (marker_event, turn_completed)
    }
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

/// Runs one native command to completion in its own Job and returns its exit
/// code with the bounded combined output. Unlike [`capture`], a nonzero exit is
/// a recorded outcome here: the shared-daemon comparison observes a refusal
/// instead of asserting success.
fn capture_outcome(
    exe: &Path,
    home: &Path,
    workspace: &Path,
    root: &Path,
    args: &[&str],
    label: &str,
) -> (u32, String) {
    let mut spec = command(exe, home, workspace);
    spec.args = args.iter().map(Into::into).collect();
    let stdout = root.join(format!("{label}-stdout.txt"));
    let stderr = root.join(format!("{label}-stderr.txt"));
    spec.stdout = Some(fs::File::create(&stdout).unwrap());
    spec.stderr = Some(fs::File::create(&stderr).unwrap());
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
    assert_eq!(outcome.reason, StopReason::Exited, "{label}");
    let text: String = format!(
        "{}{}",
        fs::read_to_string(&stdout).unwrap_or_default(),
        fs::read_to_string(&stderr).unwrap_or_default()
    )
    .chars()
    .take(512)
    .collect();
    (outcome.exit_code, text)
}

/// Provider request files in the owned root whose body carries the marker.
fn requests_containing(root: &Path, marker: &str) -> Vec<String> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("provider-")
            && fs::read_to_string(entry.path()).unwrap().contains(marker)
        {
            found.push(name);
        }
    }
    found.sort();
    found
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
    let routed = root.join("routed");
    fs::create_dir(&routed).unwrap();
    let routed_responses = control_responses::Responses::start(routed.clone(), false);
    let mut first = Client::connect(port, &token, root, "first");
    let mut observer = Client::connect(port, &token, root, "observer");
    let started = first.request("thread/start", json!({"cwd":workspace,"model":"gpt-6-astra","modelProvider":"control_fixture","allowProviderModelFallback":false,"approvalPolicy":"never","sandbox":"danger-full-access",
        "config":{"model_providers.control_fixture.base_url":format!("http://127.0.0.1:{}/v1", routed_responses.port)}}));
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
    let mut routed_threads = std::collections::BTreeSet::new();
    for entry in fs::read_dir(&routed).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().starts_with("provider-") {
            let request: Value = serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap();
            routed_threads.insert(
                harness_core::task_request::RequestIdentity::from_request(&request)
                    .unwrap()
                    .thread,
            );
        }
    }
    assert!(routed_threads.contains(parent));
    assert!(
        routed_threads.contains(thread),
        "native child must inherit its parent's route override"
    );
    assert!(
        !root.join("provider-1.json").exists(),
        "parent or child bypassed the selected route"
    );
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

#[test]
#[ignore = "requires explicit native CLI; synthetic Responses and owned background command"]
fn native_background_terminal_outlives_completed_turn() {
    let fixture = native_fixture_with_background(true, true);
    let root = fixture.root.path();
    let mut owner = Client::connect(fixture.port, &fixture.token, root, "background-owner");
    let started = owner.request("thread/start", json!({"cwd":fixture.workspace,"model":"gpt-6-astra","modelProvider":"control_fixture","allowProviderModelFallback":false,"approvalPolicy":"never","sandbox":"danger-full-access"}));
    let id = started["thread"]["id"].as_str().unwrap();
    owner.request(
        "thread/name/set",
        json!({"threadId":id,"name":"Owned background terminal contract"}),
    );
    owner.request("turn/start", json!({"threadId":id,"input":[{"type":"text","text":"Perform the owned proof command and return its consumed result."}]}));
    let completed = owner
        .event(|event| event["method"] == "turn/completed" && event["params"]["threadId"] == id);
    assert_eq!(completed["params"]["turn"]["status"], "completed");
    let running = owner.request(
        "thread/backgroundTerminals/list",
        json!({"threadId":id,"limit":5}),
    );
    fs::write(
        root.join("background-running.json"),
        serde_json::to_vec_pretty(&running).unwrap(),
    )
    .unwrap();
    assert_eq!(
        running["data"].as_array().unwrap().len(),
        1,
        "completed turn must not hide its running terminal: {running}"
    );
    assert!(running["nextCursor"].is_null());
    assert!(
        running["data"][0]["command"]
            .as_str()
            .unwrap()
            .contains("finish-tool")
    );
    assert!(running["data"][0]["processId"].is_string());
    assert_eq!(
        fs::read_to_string(fixture.workspace.join("proof.txt")).unwrap(),
        "one"
    );
    fs::write(
        fixture.workspace.join("finish-tool"),
        "release owned command",
    )
    .unwrap();
    let until = Instant::now() + WAIT;
    let finished = loop {
        let snapshot = owner.request(
            "thread/backgroundTerminals/list",
            json!({"threadId":id,"limit":5}),
        );
        if snapshot["data"].as_array().unwrap().is_empty() && snapshot["nextCursor"].is_null() {
            break snapshot;
        }
        assert!(
            Instant::now() < until,
            "owned terminal did not leave native inventory: {snapshot}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(
        !root.join("provider-3.json").exists(),
        "terminal inventory must not call a model"
    );
    fs::write(root.join("background-terminal-result.json"), serde_json::to_vec_pretty(&json!({"threadId":id,"completedTurnStillOwnedTerminal":true,"finished":finished,"providerRequests":2})).unwrap()).unwrap();
}

#[test]
#[ignore = "requires explicit native CLI; owned image and synthetic Responses"]
fn native_deferred_image_input_reaches_provider() {
    let fixture = native_fixture(true);
    let root = fixture.root.path();
    // Owned 2x2 RGBA fixture, generated from four solid colors. No image package
    // or external asset is needed to exercise the native localImage contract.
    let png: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 6,
        0, 0, 0, 114, 182, 13, 36, 0, 0, 0, 1, 115, 82, 71, 66, 0, 174, 206, 28, 233, 0, 0, 0, 4,
        103, 65, 77, 65, 0, 0, 177, 143, 11, 252, 97, 5, 0, 0, 0, 9, 112, 72, 89, 115, 0, 0, 14,
        195, 0, 0, 14, 195, 1, 199, 111, 168, 100, 0, 0, 0, 23, 73, 68, 65, 84, 24, 87, 99, 248,
        207, 192, 240, 159, 161, 129, 225, 63, 3, 3, 195, 127, 48, 0, 0, 67, 212, 9, 120, 78, 84,
        157, 149, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    let image_name = "owned image контроль.png";
    fs::write(fixture.workspace.join(image_name), png).unwrap();
    let text = "Inspect the attached owned image.\nPerform the owned proof command and return its consumed result.";
    let arguments = ["-i", image_name, "--", text].map(Into::into);
    let planned = harness_core::task_arguments::plan(&arguments, &fixture.workspace)
        .unwrap()
        .unwrap();
    assert!(planned.waiting_attachment.is_empty());
    let input = planned.initial_input.unwrap();
    let mut parts = vec![json!({"type":"text","text":input.text})];
    parts.extend(
        input
            .images
            .into_iter()
            .map(|path| json!({"type":"localImage","path":path})),
    );
    let mut owner = Client::connect(fixture.port, &fixture.token, root, "image-owner");
    let started = owner.request("thread/start", json!({"cwd":fixture.workspace,"model":"gpt-6-astra","modelProvider":"control_fixture","allowProviderModelFallback":false,"approvalPolicy":"never","sandbox":"danger-full-access"}));
    let id = started["thread"]["id"].as_str().unwrap();
    owner.request(
        "thread/name/set",
        json!({"threadId":id,"name":"Owned image input contract"}),
    );
    owner.request("turn/start", json!({"threadId":id,"input":parts}));
    let completed = owner
        .event(|event| event["method"] == "turn/completed" && event["params"]["threadId"] == id);
    assert_eq!(completed["params"]["turn"]["status"], "completed");
    let request: Value =
        serde_json::from_slice(&fs::read(root.join("provider-1.json")).unwrap()).unwrap();
    let content: Vec<_> = request["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["content"].as_array())
        .flatten()
        .collect();
    assert!(
        content
            .iter()
            .any(|part| part["type"] == "input_text" && part["text"] == text)
    );
    assert!(
        content.iter().any(|part| part["type"] == "input_image"
            && part["image_url"]
                .as_str()
                .is_some_and(|url| url.starts_with("data:image/png;base64,") && url.len() > 30)),
        "image bytes must reach the provider, not only a local path"
    );
    assert_eq!(
        fs::read_to_string(fixture.workspace.join("proof.txt")).unwrap(),
        "one"
    );
    assert!(!root.join("provider-3.json").exists());
    fs::write(root.join("image-input-result.json"), serde_json::to_vec_pretty(&json!({"threadId":id,"multilineTextPreserved":true,"localImageDelivered":true,"providerRequests":2,"toolEffect":"one"})).unwrap()).unwrap();
}

fn native_fixture(direct: bool) -> NativeFixture {
    native_fixture_with_background(direct, false)
}

fn native_fixture_with_background(direct: bool, background: bool) -> NativeFixture {
    native_fixture_with_arguments(direct, background, &[])
}

#[test]
#[ignore = "requires explicit native Codex executable; owned state only"]
fn native_provider_address_override_preserves_binding() {
    let fixture = native_fixture_with_arguments(
        true,
        false,
        &[
            "-c",
            "model_providers.control_fixture.base_url=\"http://127.0.0.1:9/gated/v1\"",
        ],
    );
    let root = fixture.root.path();
    let original = fs::read(fixture.home.join("config.toml")).unwrap();
    let mut client = Client::connect(fixture.port, &fixture.token, root, "route-config");
    let config = client.request("config/read", json!({"includeLayers":true}));
    fs::write(
        root.join("route-config.json"),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
    let provider = &config["config"]["model_providers"]["control_fixture"];
    assert_eq!(provider["base_url"], "http://127.0.0.1:9/gated/v1");
    assert_eq!(provider["env_key"], "HARNESS_CONTROL_FIXTURE_KEY");
    assert_eq!(provider["requires_openai_auth"], false);
    assert_eq!(provider["wire_api"], "responses");
    assert_eq!(config["config"]["model_provider"], "control_fixture");
    let thread = client.request(
        "thread/start",
        json!({"cwd":fixture.workspace,"allowProviderModelFallback":false}),
    );
    assert_eq!(thread["modelProvider"], "control_fixture");
    assert_eq!(thread["model"], "gpt-6-astra");
    assert_eq!(
        fs::read(fixture.home.join("config.toml")).unwrap(),
        original
    );
    assert!(!root.join("provider-1.json").exists());
    fixture.job.terminate(0, Duration::from_secs(2)).unwrap();
}

#[test]
#[ignore = "requires explicit native Codex executable; synthetic Responses only"]
fn native_thread_route_override_reaches_only_selected_upstream() {
    let fixture = native_fixture(true);
    let root = fixture.root.path();
    let routed = root.join("routed");
    fs::create_dir(&routed).unwrap();
    let upstream = control_responses::Responses::start(routed.clone(), true);
    let original = fs::read(fixture.home.join("config.toml")).unwrap();
    let mut client = Client::connect(fixture.port, &fixture.token, root, "thread-route");
    let started = client.request("thread/start", json!({
        "cwd":fixture.workspace,
        "allowProviderModelFallback":false,
        "config":{"model_providers.control_fixture.base_url":format!("http://127.0.0.1:{}/v1", upstream.port)}
    }));
    assert_eq!(started["modelProvider"], "control_fixture");
    assert_eq!(started["model"], "gpt-6-astra");
    let id = started["thread"]["id"].as_str().unwrap();
    client.request(
        "thread/name/set",
        json!({"threadId":id,"name":"Owned route attachment contract"}),
    );
    let attached = client.request("thread/resume", json!({"threadId":id}));
    assert_eq!(attached["thread"]["id"], id);
    assert_eq!(attached["modelProvider"], started["modelProvider"]);
    client.request("turn/start", json!({"threadId":id,"input":[{"type":"text","text":"Perform the owned proof command and return its consumed result."}]}));
    let completed = client
        .event(|event| event["method"] == "turn/completed" && event["params"]["threadId"] == id);
    assert_eq!(completed["params"]["turn"]["status"], "completed");
    assert!(routed.join("provider-1.json").exists());
    assert!(routed.join("provider-2.json").exists());
    assert!(!routed.join("provider-3.json").exists());
    assert!(!root.join("provider-1.json").exists());
    assert_eq!(
        fs::read_to_string(fixture.workspace.join("proof.txt")).unwrap(),
        "one"
    );
    assert_eq!(
        fs::read(fixture.home.join("config.toml")).unwrap(),
        original
    );
}

/// Executor-control probe: how a second client's `turn/start` behaves while the
/// thread is already executing a tool call, and what `turn/interrupt` does to
/// that call and its child process. The accepted/queued/error classification is
/// recorded from the raw response instead of being assumed.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned native model-free executor-control probe"]
fn native_executor_control_active_turn_and_tool_interrupt() {
    let fixture = native_fixture(true);
    let root = fixture.root.path();
    let mut owner = Client::connect(fixture.port, &fixture.token, root, "active-owner");
    let started = owner.request(
        "thread/start",
        json!({"cwd":fixture.workspace,"model":"gpt-6-astra","modelProvider":"control_fixture","allowProviderModelFallback":false,"approvalPolicy":"never","sandbox":"danger-full-access",
            "config":{"model_reasoning_effort":"high"}}),
    );
    assert_eq!(started["model"], "gpt-6-astra");
    assert_eq!(started["modelProvider"], "control_fixture");
    assert_eq!(
        started["reasoningEffort"], "high",
        "thread/start config override must pin the reasoning effort: {started}"
    );
    let thread = started["thread"]["id"].as_str().unwrap().to_owned();
    let baseline = fixture.job.snapshot().unwrap().active_processes;
    let interrupted = owner.request(
        "turn/start",
        json!({"threadId":thread,"input":[{"type":"text","text":"Perform the owned proof command and return its consumed result."}]}),
    )["turn"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    owner.event(|value| {
        value["method"] == "item/started"
            && value["params"]["threadId"] == thread
            && value["params"]["item"]["type"] == "commandExecution"
    });
    let during_tool = fixture.job.snapshot().unwrap().active_processes;
    let first_request: Value =
        serde_json::from_slice(&fs::read(root.join("provider-1.json")).unwrap()).unwrap();
    assert_eq!(
        first_request["reasoning"]["effort"], "high",
        "thread effort override must reach the provider: {first_request}"
    );
    // A second client's `turn/start` while this turn runs the tool call. Its
    // response is awaited after the interrupt so it delays nothing.
    let mut second = Client::connect(fixture.port, &fixture.token, root, "active-second");
    let busy_id = second.send_request(
        "turn/start",
        json!({"threadId":thread,"input":[{"type":"text","text":"CONTROL_EXECUTOR_CORRECTION: preserve partial work and report the current tool result."}]}),
    );
    let prompted = Instant::now();
    let interrupt = owner.raw_request(
        "turn/interrupt",
        json!({"threadId":thread,"turnId":interrupted}),
    );
    fs::write(
        root.join("tool-call-interrupt.json"),
        serde_json::to_vec_pretty(&interrupt).unwrap(),
    )
    .unwrap();
    assert!(
        interrupt.get("error").is_none(),
        "tool-call interrupt: {interrupt}"
    );
    let completed = owner.event(|value| {
        value["method"] == "turn/completed"
            && value["params"]["threadId"] == thread
            && value["params"]["turn"]["id"] == interrupted
    });
    let interrupt_latency_ms = prompted.elapsed().as_millis() as u64;
    let status = completed["params"]["turn"]["status"].clone();
    let after_interrupt = fixture.job.snapshot().unwrap().active_processes;
    let terminals = owner.request(
        "thread/backgroundTerminals/list",
        json!({"threadId":thread,"limit":5}),
    );
    // The interrupted command's process fate is observed through the item's own
    // completion and the terminal inventory instead of being assumed.
    let tool_item = owner.find(Instant::now() + Duration::from_secs(10), |value| {
        value["method"] == "item/completed"
            && value["params"]["threadId"] == thread
            && value["params"]["item"]["id"] == "control-tool-1"
    });
    let after_tool = fixture.job.snapshot().unwrap().active_processes;
    let mut terminals_settled = owner.request(
        "thread/backgroundTerminals/list",
        json!({"threadId":thread,"limit":5}),
    );
    let until = Instant::now() + Duration::from_secs(10);
    while !terminals_settled["data"].as_array().unwrap().is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
        terminals_settled = owner.request(
            "thread/backgroundTerminals/list",
            json!({"threadId":thread,"limit":5}),
        );
    }
    let read = owner.request("thread/read", json!({"threadId":thread}));
    let busy = second.await_response(busy_id);
    fs::write(
        root.join("busy-turn-start.json"),
        serde_json::to_vec_pretty(&busy).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("tool-interrupt-result.json"),
        serde_json::to_vec_pretty(
            &json!({"threadId":thread,"turnId":interrupted,"turnStatus":status,
            "interruptLatencyMs":interrupt_latency_ms,"jobProcessesBaseline":baseline,
            "jobProcessesDuringTool":during_tool,"jobProcessesAfterInterrupt":after_interrupt,
            "jobProcessesAfterToolItem":after_tool,"backgroundTerminalsAfterInterrupt":terminals,
            "backgroundTerminalsSettled":terminals_settled,"toolItemCompletion":tool_item,
            "threadState":read}),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(status, "interrupted", "{completed}");

    // A queued follow-up turn (if the second client's input was accepted while
    // the thread was busy) runs after the interrupted turn; record its fate and
    // whether the correction reached the provider.
    let mut follow_up = Value::Null;
    if let Some(queued) = busy["result"]["turn"]["id"].as_str()
        && queued != interrupted
    {
        let until = Instant::now() + WAIT;
        if let Some(value) = owner.find(until, |value| {
            value["method"] == "turn/completed"
                && value["params"]["threadId"] == thread
                && value["params"]["turn"]["id"] == queued
        }) {
            follow_up = value["params"]["turn"].clone();
        }
    }
    let correction_requests = requests_containing(root, "CONTROL_EXECUTOR_CORRECTION");
    // Whether an accepted active-turn input reaches the model is observed on a
    // second thread whose turn is allowed to finish.
    let fresh = owner.request(
        "thread/start",
        json!({"cwd":fixture.workspace,"model":"gpt-6-astra","modelProvider":"control_fixture","allowProviderModelFallback":false,"approvalPolicy":"never","sandbox":"danger-full-access"}),
    );
    let fresh_thread = fresh["thread"]["id"].as_str().unwrap().to_owned();
    let fresh_turn = owner.request(
        "turn/start",
        json!({"threadId":fresh_thread,"input":[{"type":"text","text":"Perform the owned proof command and return its consumed result."}]}),
    )["turn"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    owner.event(|value| {
        value["method"] == "item/started"
            && value["params"]["threadId"] == fresh_thread
            && value["params"]["item"]["type"] == "commandExecution"
    });
    let correction = second.raw_request(
        "turn/start",
        json!({"threadId":fresh_thread,"input":[{"type":"text","text":"CONTROL_EXECUTOR_FOLLOWUP: preserve the partial proof and report it."}]}),
    );
    fs::write(
        root.join("active-correction-delivery.json"),
        serde_json::to_vec_pretty(&correction).unwrap(),
    )
    .unwrap();
    let fresh_completion = owner.event(|value| {
        value["method"] == "turn/completed"
            && value["params"]["threadId"] == fresh_thread
            && value["params"]["turn"]["id"] == fresh_turn
    });
    assert_eq!(
        fresh_completion["params"]["turn"]["status"], "completed",
        "{fresh_completion}"
    );
    let delivered_requests = requests_containing(root, "CONTROL_EXECUTOR_FOLLOWUP");
    let until_idle = Instant::now() + WAIT;
    while owner.request("thread/read", json!({"threadId":thread}))["thread"]["status"]["type"]
        != "idle"
    {
        assert!(
            Instant::now() < until_idle,
            "thread must settle after the interrupted turn"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let final_turn = owner.request(
        "turn/start",
        json!({"threadId":thread,"input":[{"type":"text","text":"Report the owned proof result."}]}),
    )["turn"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let completion = owner.event(|value| {
        value["method"] == "turn/completed"
            && value["params"]["threadId"] == thread
            && value["params"]["turn"]["id"] == final_turn
    });
    let saved = owner.request(
        "thread/read",
        json!({"threadId":thread,"includeTurns":true}),
    );
    let final_in_items = saved.to_string().contains(control_responses::FINAL);
    fs::write(
        root.join("active-turn-result.json"),
        serde_json::to_vec_pretty(&json!({"threadId":thread,"interruptedTurn":interrupted,
            "busyTurnStart":busy,"busyFollowUpTurn":follow_up,
            "activeTurnId":fresh_turn,
            "correctionReachedProviderRequests":correction_requests,
            "correctionAcceptedIntoActiveTurn":correction,
            "activeTurnCompletionStatus":fresh_completion["params"]["turn"]["status"],
            "activeTurnCorrectionRequests":delivered_requests,
            "finalTurnStatus":completion["params"]["turn"]["status"],
            "finalMessageInThreadItems":final_in_items}))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        completion["params"]["turn"]["status"], "completed",
        "{completion}"
    );
    assert!(
        final_in_items,
        "the final message must be available from thread items"
    );
}

/// Executor-control probe: `turn/interrupt` while the turn waits on an
/// in-flight provider request, with a hung owned provider so no tool call or
/// model output exists yet.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned hung-provider executor-control probe"]
fn native_executor_control_generation_interrupt() {
    let hang = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    hang.set_nonblocking(true).unwrap();
    let hang_port = hang.local_addr().unwrap().port();
    let base_url =
        format!("model_providers.control_fixture.base_url=\"http://127.0.0.1:{hang_port}/v1\"");
    let fixture = native_fixture_with_arguments(true, false, &["-c", base_url.as_str()]);
    let root = fixture.root.path();
    let release = Arc::new(AtomicBool::new(false));
    let holding = release.clone();
    let evidence_root = root.to_path_buf();
    let recorder = std::thread::spawn(move || {
        while !holding.load(Ordering::Relaxed) {
            match hang.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(20)))
                        .unwrap();
                    let mut buffer = [0u8; 8192];
                    let captured = match stream.read(&mut buffer) {
                        Ok(count) => buffer[..count].to_vec(),
                        Err(_) => Vec::new(),
                    };
                    fs::write(evidence_root.join("hung-provider-request.bin"), &captured).unwrap();
                    while !holding.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });
    let mut owner = Client::connect(fixture.port, &fixture.token, root, "generation-owner");
    let started = owner.request(
        "thread/start",
        json!({"cwd":fixture.workspace,"model":"gpt-6-astra","modelProvider":"control_fixture","allowProviderModelFallback":false,"approvalPolicy":"never","sandbox":"danger-full-access"}),
    );
    assert_eq!(started["model"], "gpt-6-astra");
    let thread = started["thread"]["id"].as_str().unwrap().to_owned();
    let turn = owner.request(
        "turn/start",
        json!({"threadId":thread,"input":[{"type":"text","text":"Perform the owned proof command and return its consumed result."}]}),
    )["turn"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let until = Instant::now() + WAIT;
    while !root.join("hung-provider-request.bin").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        root.join("hung-provider-request.bin").is_file(),
        "the active turn must hold an in-flight provider request"
    );
    let state = owner.request("thread/read", json!({"threadId":thread}));
    let prompted = Instant::now();
    let interrupt = owner.raw_request("turn/interrupt", json!({"threadId":thread,"turnId":turn}));
    fs::write(
        root.join("generation-interrupt.json"),
        serde_json::to_vec_pretty(
            &json!({"turnId":turn,"threadState":state,"interrupt":interrupt}),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        interrupt.get("error").is_none(),
        "generation interrupt: {interrupt}"
    );
    let completed = owner.event(|value| {
        value["method"] == "turn/completed"
            && value["params"]["threadId"] == thread
            && value["params"]["turn"]["id"] == turn
    });
    let interrupt_latency_ms = prompted.elapsed().as_millis() as u64;
    let status = completed["params"]["turn"]["status"].clone();
    let saved = owner.request(
        "thread/read",
        json!({"threadId":thread,"includeTurns":true}),
    );
    fs::write(
        root.join("generation-interrupt-result.json"),
        serde_json::to_vec_pretty(
            &json!({"threadId":thread,"turnStatus":status,"interruptLatencyMs":interrupt_latency_ms,"threadRead":saved}),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(status, "interrupted", "{completed}");
    let captured = fs::read(root.join("hung-provider-request.bin")).unwrap();
    assert!(
        !captured.is_empty(),
        "the hung provider must have received the in-flight request"
    );
    release.store(true, Ordering::Relaxed);
    recorder.join().unwrap();
    fixture.job.terminate(0, Duration::from_secs(2)).unwrap();
}

/// Shared-daemon comparison for the executor backend: why the native
/// `codex app-server daemon` route cannot carry one run's ownership, and which
/// run-owned facts are retained instead.
///
/// - The daemon interface exposes no per-run attach controls, so a run cannot
///   pin its own listen address, capability token or process identity through
///   it; a pre-existing daemon is shared per `CODEX_HOME` and started outside
///   the run.
/// - On Windows the daemon refuses to start from the elevated sessions this
///   kit's sessions run in ("shared clients must not inherit administrator
///   privileges"), so its pre-request visible attachment, profile binding,
///   active/idle delivery, reconnect and neighbor-survival behavior cannot be
///   exercised in the everyday environment at all.
/// - The run-owned child starts inside the caller's Job, and that kernel
///   membership is what the stop path and the account CPU allowance rely on.
///
/// This check records the comparison receipt and verifies the run-owned
/// membership; it does not adopt the daemon. If a future executable serves an
/// elevated session, the check stops that daemon and fails so the comparison
/// is redone before any adoption.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; records the shared-daemon ownership gap; no model call"]
fn native_shared_daemon_gap_keeps_the_run_owned_backend() {
    let fixture = native_fixture(false);
    let root = fixture.root.path();
    let version = fs::read_to_string(root.join("version-stdout.txt")).unwrap();
    let version = version.trim().to_owned();
    // The run-owned backend is a member of the run's Job: the same kernel
    // object the host ends the run with, and the membership the executor
    // session's account CPU allowance covers.
    assert!(
        fixture.job.owns(fixture.server).unwrap(),
        "the run-owned app-server child must be a member of the run's Job"
    );
    // The daemon's start surface offers no per-run endpoint, token or Job
    // membership, so a run cannot own its backend through it.
    let (help_exit, help_text) = capture_outcome(
        &fixture.exe,
        &fixture.home,
        &fixture.workspace,
        root,
        &["app-server", "daemon", "start", "--help"],
        "daemon-start-help",
    );
    assert_eq!(help_exit, 0, "daemon start help: {help_text}");
    for option in ["--listen", "--ws-auth", "--ws-token-file"] {
        assert!(
            !help_text.contains(option),
            "the daemon interface exposes {option}; redo the ownership comparison: {help_text}"
        );
    }
    // Start the candidate in this owned home and query it afterwards. A start
    // command that exits zero, or a version query that answers, means the
    // route became available here and the comparison must be redone.
    let (start_exit, start_text) = capture_outcome(
        &fixture.exe,
        &fixture.home,
        &fixture.workspace,
        root,
        &["app-server", "daemon", "start"],
        "daemon-start",
    );
    let (version_exit, daemon_version_text) = capture_outcome(
        &fixture.exe,
        &fixture.home,
        &fixture.workspace,
        root,
        &["app-server", "daemon", "version"],
        "daemon-version",
    );
    let socket = fixture
        .home
        .join("app-server-control")
        .join("app-server-control.sock");
    let receipt = json!({
        "schema": 1,
        "route": "run-owned isolated app-server (retained)",
        "candidate": "native shared daemon (codex app-server daemon)",
        "nativeVersion": version,
        "runOwned": {
            "jobOwnsServer": true,
            "serverPid": fixture.server.pid,
            "providerRequests": 0,
        },
        "daemon": {
            "startExit": start_exit,
            "startOutput": start_text,
            "versionExit": version_exit,
            "versionOutput": daemon_version_text,
            "socket": socket.to_string_lossy(),
            "socketExists": socket.exists(),
            "perRunEndpointOptions": false,
        },
        "dimensions": {
            "preRequestVisibleAttachment": {
                "runOwned": "native_named_empty_thread_can_be_attached, native_visible_chats_before_model_dispatch",
                "sharedDaemon": "not exercisable: the daemon did not start",
            },
            "exactProfiles": {
                "runOwned": "native_provider_address_override_preserves_binding, native_thread_route_override_reaches_only_selected_upstream, native_app_server_binds_the_resolved_profile_and_records_an_addressable_endpoint",
                "sharedDaemon": "not exercisable: the daemon did not start",
            },
            "activeIdleDelivery": {
                "runOwned": "native_lead_queue_and_app_server_input_on_owning_thread",
                "sharedDaemon": "not exercisable: the daemon did not start",
            },
            "reconnect": {
                "runOwned": "native_two_clients_reconnect_tool_result_and_tui",
                "sharedDaemon": "not exercisable: the daemon did not start",
            },
            "cpuMembership": {
                "runOwned": "this check: the run's Job owns the app-server child by kernel membership",
                "sharedDaemon": "impossible: a pre-existing daemon is started outside the run and is shared by every session of the same CODEX_HOME",
            },
            "neighborSurvivalOnStopViewLoss": {
                "runOwned": "message_steers_an_active_tool_without_touching_the_neighbor_or_partial_files, native_executor_control_active_turn_and_tool_interrupt",
                "sharedDaemon": "not exercisable: the daemon did not start",
            },
        },
        "gap": "app-server daemon start refuses this kit's elevated Windows sessions (shared clients must not inherit administrator privileges) and the daemon interface has no per-run listen address, capability token or Job membership, so the run cannot own its endpoint, exact process identity, stop authority or CPU admission",
        "decision": "retain the run-owned isolated backend",
    });
    fs::write(
        root.join("daemon-comparison.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    eprintln!("shared-daemon comparison evidence: {}", root.display());
    assert!(
        !root.join("provider-1.json").exists(),
        "the comparison must not call a model"
    );
    if start_exit == 0 || version_exit == 0 {
        // The candidate served this session; stop it and fail, because
        // adoption requires the full property comparison this route does not
        // have and this check must not leave a daemon behind.
        let (stop_exit, stop_text) = capture_outcome(
            &fixture.exe,
            &fixture.home,
            &fixture.workspace,
            root,
            &["app-server", "daemon", "stop"],
            "daemon-stop",
        );
        panic!(
            "the native shared daemon served this session (start exit {start_exit}: {start_text}; version exit {version_exit}: {daemon_version_text}); stop exit {stop_exit} ({stop_text}); the ownership comparison must be redone before adoption"
        );
    }
    assert_ne!(
        start_exit, 0,
        "daemon start neither served nor refused: {start_text}"
    );
    fixture.job.terminate(0, Duration::from_secs(2)).unwrap();
}

const PROOF_PROMPT: &str = "Perform the owned proof command and return its consumed result.";

struct ProviderHit {
    file: String,
    thread: String,
    turn: String,
}

struct DeliverySeen {
    native_item: bool,
    marker_event: bool,
    provider_on_thread: bool,
    provider_same_turn: bool,
    provider_before_completion: bool,
    other_thread: bool,
    turn_completed: bool,
    hits: Vec<String>,
}

struct LeadProbe {
    state: &'static str,
    route: &'static str,
    method: &'static str,
    accepted: bool,
    native_item: bool,
    marker_event: bool,
    provider_on_thread: bool,
    timely: bool,
    delivered: bool,
    other_thread: bool,
    evidence: PathBuf,
}

fn provider_hits(root: &Path, marker: &str) -> Vec<ProviderHit> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        if name.starts_with("provider-") && name.ends_with(".json") {
            names.push(name);
        }
    }
    names.sort();
    let mut hits = Vec::new();
    for name in names {
        let Ok(body) = fs::read(root.join(&name)) else {
            continue;
        };
        if !String::from_utf8_lossy(&body).contains(marker) {
            continue;
        }
        let Ok(request) = serde_json::from_slice::<Value>(&body) else {
            continue;
        };
        let (thread, turn) =
            match harness_core::task_request::RequestIdentity::from_request(&request) {
                Ok(identity) => (identity.thread, identity.turn),
                Err(_) => (String::new(), String::new()),
            };
        hits.push(ProviderHit {
            file: name,
            thread,
            turn,
        });
    }
    hits
}

fn text_bound(text: &str) -> &str {
    let mut end = text.len().min(1200);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn read_http_message(stream: &mut std::net::TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut bytes = Vec::new();
    let (start, length) = loop {
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&bytes[..end]).into_owned();
            let length = header.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            });
            let length = length
                .unwrap_or_else(|| panic!("provider exchange has no Content-Length:\n{header}"));
            assert!(length <= 2 * 1024 * 1024, "provider body limit");
            break (end + 4, length);
        }
        assert!(bytes.len() < 64 * 1024, "provider header limit");
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count != 0, "incomplete provider header");
        bytes.extend_from_slice(&chunk[..count]);
    };
    while bytes.len() < start + length {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).unwrap();
        assert!(count != 0, "incomplete provider body");
        bytes.extend_from_slice(&chunk[..count]);
    }
    bytes.truncate(start + length);
    bytes
}

/// Holds the first canned Responses body so the owning turn stays in active
/// generation. Later requests are forwarded immediately; a replacement request
/// is itself subsequent provider input.
struct GenerationGate {
    port: u16,
    held: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl GenerationGate {
    fn start(upstream: u16, evidence: &Path) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let held = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let held_flag = held.clone();
        let release_flag = release.clone();
        let stop_flag = stop.clone();
        let hold_first = Arc::new(AtomicBool::new(true));
        let evidence = evidence.to_path_buf();
        std::thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let held_flag = held_flag.clone();
                        let release_flag = release_flag.clone();
                        let hold_first = hold_first.clone();
                        let evidence = evidence.clone();
                        std::thread::spawn(move || {
                            forward_held(
                                stream,
                                upstream,
                                evidence,
                                held_flag,
                                release_flag,
                                hold_first,
                            );
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("generation gate accept: {error}"),
                }
            }
        });
        Self {
            port,
            held,
            release,
            stop,
        }
    }
}

impl Drop for GenerationGate {
    fn drop(&mut self) {
        self.release.store(true, Ordering::SeqCst);
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn forward_held(
    mut client: std::net::TcpStream,
    upstream: u16,
    evidence: PathBuf,
    held: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    hold_first: Arc<AtomicBool>,
) {
    client.set_nonblocking(false).unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let request = read_http_message(&mut client);
    let _ = fs::write(
        evidence.join("generation-gate-request.txt"),
        &request[..request.len().min(800)],
    );
    let mut server = std::net::TcpStream::connect(("127.0.0.1", upstream)).unwrap();
    server.set_nonblocking(false).unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    server
        .set_write_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    server.write_all(&request).unwrap();
    let response = read_http_message(&mut server);
    if hold_first.swap(false, Ordering::SeqCst) {
        held.store(true, Ordering::SeqCst);
        let until = Instant::now() + Duration::from_secs(30);
        while !release.load(Ordering::SeqCst) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let _ = client.write_all(&response);
}

fn run_queue(fixture: &NativeFixture, thread: &str, message: &str, label: &str) -> Value {
    let root = fixture.root.path();
    let mut spec = command(&fixture.exe, &fixture.home, &fixture.workspace);
    spec.args = vec![
        "queue".into(),
        "--thread".into(),
        thread.into(),
        "--message".into(),
        message.into(),
        "--remote".into(),
        format!("ws://127.0.0.1:{}", fixture.port).into(),
        "--remote-auth-token-env".into(),
        "HARNESS_CONTROL_TOKEN".into(),
    ];
    spec.env.insert(
        "HARNESS_CONTROL_TOKEN".into(),
        Some(fixture.token.clone().into()),
    );
    let stdout_path = root.join(format!("{label}-stdout.txt"));
    let stderr_path = root.join(format!("{label}-stderr.txt"));
    spec.stdout = Some(fs::File::create(&stdout_path).unwrap());
    spec.stderr = Some(fs::File::create(&stderr_path).unwrap());
    let job = Job::new(Limits::default()).unwrap();
    let child = job.spawn(&spec).unwrap();
    let outcome = job
        .wait(
            &child,
            Deadline::after(Duration::from_secs(12)).unwrap(),
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
        "stdout": text_bound(&stdout),
        "stderr": text_bound(&stderr),
    })
}

fn start_owned_thread(client: &mut Client, workspace: &Path, proxy_port: Option<u16>) -> String {
    let mut params = json!({
        "cwd": workspace,
        "model": "gpt-6-astra",
        "modelProvider": "control_fixture",
        "allowProviderModelFallback": false,
        "approvalPolicy": "never",
        "sandbox": "danger-full-access"
    });
    if let Some(port) = proxy_port {
        params["config"] = json!({
            "model_providers.control_fixture.base_url": format!("http://127.0.0.1:{port}/v1")
        });
    }
    let started = client.request("thread/start", params);
    assert_eq!(started["modelProvider"], "control_fixture", "{started}");
    started["thread"]["id"].as_str().unwrap().to_owned()
}

fn thread_status(client: &mut Client, thread: &str) -> String {
    client.request("thread/read", json!({"threadId": thread}))["thread"]["status"]["type"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn thread_has_marker(client: &mut Client, thread: &str, marker: &str) -> bool {
    client
        .request(
            "thread/read",
            json!({"threadId": thread, "includeTurns": true}),
        )
        .to_string()
        .contains(marker)
}

fn watch_delivery(
    owner: &mut Client,
    root: &Path,
    thread: &str,
    turn: &str,
    marker: &str,
    until: Instant,
) -> DeliverySeen {
    let mut seen = DeliverySeen {
        native_item: false,
        marker_event: false,
        provider_on_thread: false,
        provider_same_turn: false,
        provider_before_completion: false,
        other_thread: false,
        turn_completed: false,
        hits: Vec::new(),
    };
    while Instant::now() < until {
        let hits = provider_hits(root, marker);
        if hits.iter().any(|hit| hit.thread != thread) {
            seen.other_thread = true;
        }
        if let Some(hit) = hits.iter().find(|hit| hit.thread == thread) {
            if !seen.provider_on_thread && !seen.turn_completed {
                seen.provider_before_completion = true;
            }
            seen.provider_on_thread = true;
            seen.provider_same_turn |= !turn.is_empty() && hit.turn == turn;
            seen.hits = hits
                .iter()
                .map(|hit| format!("{}:{}:{}", hit.file, hit.thread, hit.turn))
                .collect();
        }
        let (marker_event, completed) = owner.drain_seen(
            Instant::now() + Duration::from_millis(80),
            thread,
            turn,
            marker,
        );
        seen.marker_event |= marker_event;
        seen.turn_completed |= completed;
        if !seen.native_item {
            seen.native_item = thread_has_marker(owner, thread, marker);
        }
        if seen.native_item
            && seen.provider_on_thread
            && (seen.turn_completed || seen.provider_before_completion)
        {
            break;
        }
    }
    seen
}

fn accepted_invocation(acceptance: &Value) -> bool {
    if acceptance.get("exitCode").is_some() {
        acceptance["exited"] == true && acceptance["exitCode"] == 0
    } else {
        acceptance.get("error").is_none()
    }
}

/// What one delivery probe exercises: the conversation state it starts from,
/// the input route it uses and the native method that route names.
struct ProbeCase {
    state: &'static str,
    route: &'static str,
    method: &'static str,
}

fn finish_probe(
    case: ProbeCase,
    thread: &str,
    turn: &str,
    acceptance: &Value,
    seen: &DeliverySeen,
    evidence: PathBuf,
) -> LeadProbe {
    let accepted = accepted_invocation(acceptance);
    let delivered = seen.native_item && seen.provider_on_thread;
    let timely = delivered
        && (case.state == "idle" || seen.provider_same_turn || seen.provider_before_completion);
    let probe = LeadProbe {
        state: case.state,
        route: case.route,
        method: case.method,
        accepted,
        native_item: seen.native_item,
        marker_event: seen.marker_event,
        provider_on_thread: seen.provider_on_thread,
        timely,
        delivered,
        other_thread: seen.other_thread,
        evidence: evidence.clone(),
    };
    fs::write(
        evidence.join("lead-input-probe.json"),
        serde_json::to_vec_pretty(&json!({
            "state": case.state,
            "route": case.route,
            "method": case.method,
            "threadId": thread,
            "turnId": turn,
            "accepted": accepted,
            "delivered": delivered,
            "timely": timely,
            "nativeItem": seen.native_item,
            "markerEvent": seen.marker_event,
            "providerOnOwningThread": seen.provider_on_thread,
            "providerSameTurn": seen.provider_same_turn,
            "providerBeforeCompletion": seen.provider_before_completion,
            "otherThread": seen.other_thread,
            "turnCompleted": seen.turn_completed,
            "providerHits": seen.hits,
            "acceptance": acceptance,
            "deliveryDefinition": "owning-thread item plus subsequent provider input; help, exit 0 and RPC success are not delivery"
        }))
        .unwrap(),
    )
    .unwrap();
    probe
}

fn inject_input(
    fixture: &NativeFixture,
    owner: &mut Client,
    route: &str,
    thread: &str,
    turn: &str,
    marker: &str,
    label: &str,
) -> Value {
    if route == "queue" {
        run_queue(fixture, thread, marker, label)
    } else if turn.is_empty() {
        owner.raw_request(
            "turn/start",
            json!({"threadId": thread, "input": [{"type": "text", "text": marker}]}),
        )
    } else {
        owner.raw_request(
            "turn/steer",
            json!({
                "threadId": thread,
                "expectedTurnId": turn,
                "input": [{"type": "text", "text": marker}]
            }),
        )
    }
}

fn probe_generation(route: &'static str) -> LeadProbe {
    let fixture = native_fixture(true);
    let root = fixture.root.path().to_path_buf();
    let gate = GenerationGate::start(fixture._responses.port, &root);
    let mut owner = Client::connect(fixture.port, &fixture.token, &root, "generation-owner");
    let thread = start_owned_thread(&mut owner, &fixture.workspace, Some(gate.port));
    let turn = owner.request(
        "turn/start",
        json!({"threadId": thread, "input": [{"type": "text", "text": PROOF_PROMPT}]}),
    )["turn"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let until = Instant::now() + WAIT;
    while !gate.held.load(Ordering::SeqCst) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        gate.held.load(Ordering::SeqCst),
        "active generation did not reach the canned fixture; {}",
        root.display()
    );
    assert_eq!(
        thread_status(&mut owner, &thread),
        "active",
        "generation input must target the active owning turn"
    );
    let marker = if route == "queue" {
        "LEAD_QUEUE_GENERATION_INPUT"
    } else {
        "LEAD_STEER_GENERATION_INPUT"
    };
    let method = if route == "queue" {
        "codex queue --thread <owning-id> --remote <owning-endpoint>"
    } else {
        "turn/steer"
    };
    // Release even if the native RPC waits on the in-flight provider call.
    // Three seconds is long enough to send input and short of an unbounded hold.
    let release = gate.release.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        release.store(true, Ordering::SeqCst);
    });
    let acceptance = inject_input(
        &fixture,
        &mut owner,
        route,
        &thread,
        &turn,
        marker,
        "generation",
    );
    let during_hold = provider_hits(&root, marker)
        .iter()
        .any(|hit| hit.thread == thread);
    gate.release.store(true, Ordering::SeqCst);
    let mut seen = watch_delivery(
        &mut owner,
        &root,
        &thread,
        &turn,
        marker,
        Instant::now() + Duration::from_secs(25),
    );
    seen.provider_before_completion |= during_hold;
    finish_probe(
        ProbeCase {
            state: "generation",
            route,
            method,
        },
        &thread,
        &turn,
        &acceptance,
        &seen,
        root,
    )
}

fn probe_tool_observation_wait(route: &'static str) -> LeadProbe {
    let fixture = native_fixture(true);
    let root = fixture.root.path().to_path_buf();
    // The canned fixture's close-view gate keeps this tool blocked until the
    // observation release file appears. It does not emit `executor watch`, and
    // this comparison does not spawn an executor.
    fs::write(
        root.join("close-view"),
        "hold the owned tool for observation\n",
    )
    .unwrap();
    let mut owner = Client::connect(fixture.port, &fixture.token, &root, "tool-owner");
    let thread = start_owned_thread(&mut owner, &fixture.workspace, None);
    let turn = owner.request(
        "turn/start",
        json!({"threadId": thread, "input": [{"type": "text", "text": PROOF_PROMPT}]}),
    )["turn"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    owner.event(|value| {
        value["method"] == "item/started"
            && value["params"]["threadId"] == thread
            && value["params"]["item"]["type"] == "commandExecution"
    });
    let until = Instant::now() + WAIT;
    while !fixture.workspace.join("proof.txt").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        fixture.workspace.join("proof.txt").is_file(),
        "the owned tool did not enter its observation wait; {}",
        root.display()
    );
    assert!(
        !fixture.workspace.join("finish-tool").exists(),
        "observation release must not already be present"
    );
    assert_eq!(thread_status(&mut owner, &thread), "active");
    let marker = if route == "queue" {
        "LEAD_QUEUE_TOOL_WAIT_INPUT"
    } else {
        "LEAD_STEER_TOOL_WAIT_INPUT"
    };
    let method = if route == "queue" {
        "codex queue --thread <owning-id> --remote <owning-endpoint>"
    } else {
        "turn/steer"
    };
    let acceptance = inject_input(
        &fixture,
        &mut owner,
        route,
        &thread,
        &turn,
        marker,
        "tool-wait",
    );
    assert_eq!(
        thread_status(&mut owner, &thread),
        "active",
        "input during the observation wait must not require the tool to finish first"
    );
    fs::write(
        fixture.workspace.join("finish-tool"),
        "release owned observation\n",
    )
    .unwrap();
    let seen = watch_delivery(
        &mut owner,
        &root,
        &thread,
        &turn,
        marker,
        Instant::now() + Duration::from_secs(25),
    );
    finish_probe(
        ProbeCase {
            state: "tool_observation_wait",
            route,
            method,
        },
        &thread,
        &turn,
        &acceptance,
        &seen,
        root,
    )
}

fn probe_idle(route: &'static str) -> LeadProbe {
    let fixture = native_fixture(true);
    let root = fixture.root.path().to_path_buf();
    let mut owner = Client::connect(fixture.port, &fixture.token, &root, "idle-owner");
    let thread = start_owned_thread(&mut owner, &fixture.workspace, None);
    assert_eq!(thread_status(&mut owner, &thread), "idle");
    assert!(
        !root.join("provider-1.json").exists(),
        "an idle loaded thread must not call a model before input"
    );
    let marker = if route == "queue" {
        "LEAD_QUEUE_IDLE_INPUT"
    } else {
        "LEAD_START_IDLE_INPUT"
    };
    let method = if route == "queue" {
        "codex queue --thread <owning-id> --remote <owning-endpoint>"
    } else {
        "turn/start"
    };
    let acceptance = inject_input(&fixture, &mut owner, route, &thread, "", marker, "idle");
    let turn = acceptance["result"]["turn"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let seen = watch_delivery(
        &mut owner,
        &root,
        &thread,
        &turn,
        marker,
        Instant::now() + Duration::from_secs(20),
    );
    finish_probe(
        ProbeCase {
            state: "idle",
            route,
            method,
        },
        &thread,
        &turn,
        &acceptance,
        &seen,
        root,
    )
}

fn probe_json(probe: &LeadProbe) -> Value {
    json!({
        "state": probe.state,
        "route": probe.route,
        "method": probe.method,
        "accepted": probe.accepted,
        "delivered": probe.delivered,
        "timely": probe.timely,
        "nativeItem": probe.native_item,
        "markerEvent": probe.marker_event,
        "providerOnOwningThread": probe.provider_on_thread,
        "otherThread": probe.other_thread,
        "evidence": probe.evidence,
    })
}

fn route_probe<'a>(probes: &'a [LeadProbe], state: &str, route: &str) -> &'a LeadProbe {
    probes
        .iter()
        .find(|probe| probe.state == state && probe.route == route)
        .unwrap()
}

fn chosen_invocation<'a>(probes: &'a [LeadProbe], state: &str) -> &'a LeadProbe {
    let native = route_probe(probes, state, "app-server");
    let queue = route_probe(probes, state, "queue");
    // One transport owner. Queue is selected only when the app-server
    // operation is not timely for that state.
    if native.timely { native } else { queue }
}

/// Compares installed `codex queue` with native app-server input on the exact
/// owning thread. Delivery is an owning-thread item plus a later provider
/// request that carries the marker. Help text, process exit 0 and an RPC
/// without `error` are recorded separately and are not that evidence.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned canned Responses; help and RPC acceptance are not delivery"]
fn native_lead_queue_and_app_server_input_on_owning_thread() {
    let probes = vec![
        probe_generation("queue"),
        probe_generation("app-server"),
        probe_tool_observation_wait("queue"),
        probe_tool_observation_wait("app-server"),
        probe_idle("queue"),
        probe_idle("app-server"),
    ];
    let summary_root = probes[0].evidence.clone();
    let version = fs::read_to_string(summary_root.join("version-stdout.txt"))
        .unwrap()
        .trim()
        .to_owned();
    let states = ["generation", "tool_observation_wait", "idle"];
    let queue_all_timely = states.iter().all(|state| {
        probes
            .iter()
            .any(|probe| probe.state == *state && probe.route == "queue" && probe.timely)
    });
    let selected = if queue_all_timely {
        "codex queue".to_owned()
    } else {
        format!(
            "app-server {}",
            states
                .iter()
                .map(|state| format!("{state}={}", chosen_invocation(&probes, state).method))
                .collect::<Vec<_>>()
                .join("; ")
        )
    };
    let summary = summary_root.join("lead-input-comparison.json");
    fs::write(
        &summary,
        serde_json::to_vec_pretty(&json!({
            "version": version,
            "selectedRoute": selected,
            "probes": probes.iter().map(probe_json).collect::<Vec<_>>(),
        }))
        .unwrap(),
    )
    .unwrap();
    eprintln!("lead input comparison: {}", summary.display());
    eprintln!("selected lead input route: {selected}");
    assert_eq!(
        version, "codex-cli 0.157.0",
        "HARNESS_CONTROL_CODEX_EXE must be the installed Codex 0.157.0 binary"
    );
    for probe in &probes {
        assert_eq!(
            probe.delivered,
            probe.native_item && probe.provider_on_thread,
            "acceptance must not define delivery: {} {}",
            probe.state,
            probe.route
        );
    }
    let generation_queue = route_probe(&probes, "generation", "queue");
    let generation_native = route_probe(&probes, "generation", "app-server");
    let tool_queue = route_probe(&probes, "tool_observation_wait", "queue");
    let tool_native = route_probe(&probes, "tool_observation_wait", "app-server");
    let idle_queue = route_probe(&probes, "idle", "queue");
    let idle_native = route_probe(&probes, "idle", "app-server");
    assert!(
        generation_queue.delivered && !generation_queue.timely,
        "0.157.0 queue must reach the owning thread only after active generation"
    );
    assert!(generation_native.timely && generation_native.method == "turn/steer");
    assert!(
        tool_queue.delivered && !tool_queue.timely,
        "0.157.0 queue must reach the owning thread only after a blocked tool"
    );
    assert!(tool_native.timely && tool_native.method == "turn/steer");
    assert!(
        idle_queue.timely,
        "0.157.0 queue must still reach an idle owning thread"
    );
    assert!(idle_native.timely && idle_native.method == "turn/start");
    assert_eq!(
        selected,
        "app-server generation=turn/steer; tool_observation_wait=turn/steer; idle=turn/start"
    );
    for state in states {
        let chosen = chosen_invocation(&probes, state);
        assert!(
            chosen.timely,
            "no timely owning-thread delivery during {state}; selected {selected}; see {}",
            summary.display()
        );
    }
}

fn native_fixture_with_arguments(
    direct: bool,
    background: bool,
    arguments: &[&str],
) -> NativeFixture {
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
    let responses = if background {
        control_responses::Responses::with_background_terminal(root.into())
    } else {
        control_responses::Responses::start(root.into(), direct)
    };
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
    spec.args.extend(arguments.iter().map(Into::into));
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
