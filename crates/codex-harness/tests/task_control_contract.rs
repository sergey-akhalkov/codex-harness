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
