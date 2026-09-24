//! Fixture tests for the control-backed executor conversation driver.
//!
//! The app-server side of every check is a canned, model-free WebSocket
//! endpoint owned by the test: it speaks the contract the change's native
//! probes observed (initialize plus initialized, `thread/start`,
//! `thread/name/set`, `turn/start`, `thread/read includeTurns`, and the
//! `thread/*`, `item/*`, `turn/*` and `error` notifications), so startup,
//! binding verification, lifecycle mapping, final-message extraction and the
//! fail-closed paths are exercised without a model, a subscription or the
//! installed CLI.
#![cfg(windows)]
#[path = "fixtures/cache_usage.rs"]
mod cache_usage;
#[path = "../src/executor_control.rs"]
mod executor_control;

use executor_control::{
    BoundIdentity, ControlPaths, ControlPlan, Conversation, Endpoint, FinalMessage, Lifecycle,
    MAX_EVENTS_PER_PUMP, app_server_spec,
};
use harness_core::{
    orchestration_config::ProfileBinding,
    process::{Job, Limits, ProcessIdentity},
    process_service::{self, ServiceProcess},
};
use serde_json::{Value, json};
use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const TOKEN: &str = "9f0c1d2e3a4b5c6d7e8f90123456789abcdef0123456789abcdef0123456789a";
/// The wrong bearer of the refusal check: same shape, different value.
const OTHER_TOKEN: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
const TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f2401";
const FINAL: &str = "CONTROL_FIXTURE_FINAL_MESSAGE";
const MODEL: &str = "deepseek-v4-flash";
const PROVIDER: &str = "deepseek-fixture";
const EFFORT: &str = "max";
const FIXTURE: &str = env!("CARGO_BIN_EXE_harness-executor-fixture");
const WAIT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------- endpoint

#[path = "fixtures/control_endpoint.rs"]
mod control_endpoint;

use control_endpoint::{Answer, Bearer, Server};

// ----------------------------------------------------------------- fixtures

fn identity() -> BoundIdentity {
    BoundIdentity {
        profile: "deepseek".to_owned(),
        model: Some(MODEL.to_owned()),
        model_provider: Some(PROVIDER.to_owned()),
        reasoning_effort: Some(EFFORT.to_owned()),
    }
}

fn thread_start_answer() -> Value {
    json!({
        "thread": {"id": THREAD},
        "model": MODEL,
        "modelProvider": PROVIDER,
        "reasoningEffort": EFFORT
    })
}

fn thread_read_answer() -> Value {
    json!({"thread": {
        "id": THREAD,
        "status": {"type": "idle"},
        "turns": [{
            "id": TURN,
            "status": "completed",
            "items": [
                {"id": "c1", "type": "commandExecution", "command": "pwsh -NoProfile -Command 'echo'", "exitCode": 0},
                {"id": "m1", "type": "agentMessage", "text": FINAL}
            ]
        }]
    }})
}

/// A canned endpoint plus the plan and Job that point the documented command
/// at it. The child is the kit's own executor fixture process: it proves the
/// real spawn, environment and Job ownership, while the protocol answers come
/// from the canned endpoint because the kit has no app-server fixture binary.
struct Fixture {
    _root: tempfile::TempDir,
    slot: PathBuf,
    server: Server,
    plan: ControlPlan,
    marker: PathBuf,
    job: Option<Job>,
}

fn fixture() -> Fixture {
    fixture_with(Answer::Result(thread_start_answer()))
}

fn fixture_with(thread_start: Answer) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let slot = root.path().join("slot");
    fs::create_dir_all(&slot).unwrap();
    let paths = ControlPaths {
        endpoint: root.path().join("control-1.json"),
        token: root.path().join("control-1.token"),
        log: root.path().join("control-1.log"),
    };
    // The canned endpoint reads the bearer from the token file the module
    // writes, exactly as the installed app-server reads it.
    let server = Server::start(Bearer::File(paths.token.clone()));
    server.answer(
        "initialize",
        Answer::Result(json!({"serverInfo": {"name": "canned-executor-control", "version": "1"}})),
    );
    server.answer("thread/start", thread_start);
    server.answer("thread/name/set", Answer::Result(json!({})));
    server.answer(
        "turn/start",
        Answer::Result(json!({"turn": {"id": TURN, "status": "inProgress"}})),
    );
    server.answer("thread/read", Answer::Result(thread_read_answer()));
    let marker = root.path().join("fixture-child.json");
    let mut plan = ControlPlan::new(
        FIXTURE,
        root.path().join("home"),
        &slot,
        "control fixture assignment",
        identity(),
        paths,
    );
    plan.port = Some(server.port);
    plan.approval_policy = Some("never".to_owned());
    plan.sandbox = Some("danger-full-access".to_owned());
    plan.bound = WAIT;
    plan.poll = Duration::from_millis(50);
    plan.env
        .insert("HARNESS_EXECUTOR_FIXTURE_MODE".into(), Some("hang".into()));
    plan.env.insert(
        "HARNESS_EXECUTOR_FIXTURE_STARTED".into(),
        Some(marker.clone().into_os_string()),
    );
    Fixture {
        _root: root,
        slot,
        server,
        plan,
        marker,
        job: Some(Job::new(Limits::default()).unwrap()),
    }
}

impl Fixture {
    fn start(&self) -> std::io::Result<Conversation> {
        Conversation::start(self.job.as_ref().expect("job"), &self.plan)
    }

    fn endpoint_path(&self) -> PathBuf {
        self.plan.paths.endpoint.clone()
    }

    fn child_pid(&self) -> u32 {
        self.child_identity().pid
    }

    /// The identity the fixture child recorded through the plan's environment.
    fn child_identity(&self) -> harness_core::process::ProcessIdentity {
        let until = Instant::now() + WAIT;
        loop {
            if let Ok(bytes) = fs::read(&self.marker) {
                return serde_json::from_slice(&bytes)
                    .expect("the recorded identity is a process identity");
            }
            assert!(
                Instant::now() < until,
                "the fixture child records its identity through plan.env"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    /// The caller's Job stays the sole cleanup authority.
    fn terminate(&mut self) {
        if let Some(job) = self.job.take() {
            let _ = job.terminate(0, Duration::from_secs(5));
        }
    }
}

fn completion_burst(server: &Server) {
    server.push(json!({"method":"thread/started","params":{"thread":{"id":THREAD}}}));
    server.push(
        json!({"method":"thread/status/changed","params":{"threadId":THREAD,"status":{"type":"active"}}}),
    );
    server.push(
        json!({"method":"item/started","params":{"threadId":THREAD,"item":{
        "id":"c1","type":"commandExecution","command":"pwsh -NoProfile -Command 'exit 0'"}}}),
    );
    server.push(json!({"method":"item/completed","params":{"threadId":THREAD,"item":{
        "id":"c1","type":"commandExecution","command":"pwsh -NoProfile -Command 'exit 0'","exitCode":0}}}));
    server.push(
        json!({"method":"item/completed","params":{"threadId":THREAD,"item":{
        "id":"m1","type":"agentMessage","text":FINAL}}}),
    );
    // Token-level deltas must neither render nor occupy the bounded detail
    // file; the completed item above carries the same content.
    server.push(
        json!({"method":"item/agentMessage/delta","params":{"threadId":THREAD,"turnId":TURN,"itemId":"m1","delta":"IGNORED_DELTA"}}),
    );
    server.push(
        json!({"method":"item/reasoning/textDelta","params":{"threadId":THREAD,"turnId":TURN,"itemId":"r1","delta":"IGNORED_REASONING_DELTA"}}),
    );
    server.push(
        json!({"method":"mcpServer/startupStatus/updated","params":{"serverId":"fixture","status":"starting"}}),
    );
    server.push(json!({"method":"remoteControl/status/changed","params":{"attached":false}}));
    server.push(
        json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{"id":TURN,"status":"completed"}}}),
    );
    server.push(
        json!({"method":"thread/status/changed","params":{"threadId":THREAD,"status":{"type":"idle"}}}),
    );
}

/// The terminal record of one turn that did not complete the assignment, with
/// the native status and failure message the thread reported.
fn terminal_turn(server: &Server, status: &str, message: Option<&str>) {
    server.push(json!({"method":"thread/started","params":{"thread":{"id":THREAD}}}));
    server.push(json!({"method":"turn/started","params":{"threadId":THREAD}}));
    let mut turn = json!({"id": TURN, "status": status});
    if let Some(message) = message {
        turn["error"] = json!({"message": message});
    }
    server.push(json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":turn}}));
}

fn drain(conversation: &mut Conversation, expected: usize) -> Vec<executor_control::ControlEvent> {
    let until = Instant::now() + WAIT;
    let mut events = Vec::new();
    while events.len() < expected && Instant::now() < until {
        events.extend(conversation.pump().unwrap());
    }
    assert_eq!(
        events.len(),
        expected,
        "control records: {:?}",
        events
            .iter()
            .map(|event| event.method.clone())
            .collect::<Vec<_>>()
    );
    events
}

/// Bounded wait for a fixture observation that a worker thread records.
fn wait_for(mut condition: impl FnMut() -> bool, reason: &str) {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("{reason}");
}

// ------------------------------------------------------- startup and binding

#[test]
fn spawned_child_serves_the_documented_command_environment_and_endpoint() {
    let mut fixture = fixture();
    let conversation = fixture.start().expect("the canned endpoint must answer");
    assert_ne!(
        fixture.child_pid(),
        0,
        "the child ran with the plan's environment"
    );
    assert!(
        conversation.process().unwrap().is_running().unwrap(),
        "the app-server child stays inside the caller's Job"
    );
    assert!(conversation.process_identity().is_some());
    assert_eq!(
        conversation.identity().unwrap().model.as_deref(),
        Some(MODEL)
    );
    assert_eq!(conversation.lifecycle(), None);
    assert_eq!(conversation.thread_id(), THREAD);
    assert!(conversation.defect().is_none());
    assert!(conversation.failure().is_none());
    assert!(conversation.turn().is_none());

    // The endpoint is recorded beside the receipt with exactly the bearer the
    // token file carries - the canned endpoint accepted that bearer, so the
    // token file, the client and the record agree - and the bearer stays out
    // of every diagnostic shape.
    let token = fs::read_to_string(&fixture.plan.paths.token).unwrap();
    assert_eq!(token.len(), 64, "32 bytes of OS randomness as hex");
    assert!(token.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    let endpoint = Endpoint::read(&fixture.endpoint_path()).unwrap();
    assert_eq!(endpoint.port(), fixture.server.port);
    assert_eq!(endpoint.token(), token);
    assert_eq!(endpoint.thread_id.as_deref(), Some(THREAD));
    assert_eq!(conversation.endpoint().token(), token);
    assert!(!format!("{endpoint:?}").contains(&token));
    // The recorded process identity is the app-server child this session
    // spawned, in the exact shape `executor stop` reads (pid, creation time
    // and image), so the stop path can end the child that owns the thread.
    let child = conversation.process_identity().unwrap();
    let recorded = endpoint
        .process
        .as_ref()
        .expect("a recorded child identity");
    assert_eq!(recorded.pid, child.pid);
    assert_eq!(recorded.creation_time, child.creation_time);
    assert_eq!(recorded.program, PathBuf::from(FIXTURE));

    // The child ran the exact documented command environment.
    let requests = fixture.server.requests();
    let methods: Vec<&str> = requests
        .iter()
        .filter_map(|request| request["method"].as_str())
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "thread/start",
            "thread/name/set"
        ]
    );
    assert_eq!(
        requests[0]["params"]["capabilities"]["experimentalApi"],
        true
    );
    assert!(
        requests[1].get("id").is_none(),
        "initialized is a notification, not a request"
    );
    let start = &requests[2]["params"];
    assert_eq!(start["cwd"], json!(fixture.slot), "{start}");
    assert_eq!(start["model"], MODEL);
    assert_eq!(start["modelProvider"], PROVIDER);
    assert_eq!(
        start["allowProviderModelFallback"], false,
        "a model fallback would change the routed model"
    );
    assert_eq!(start["approvalPolicy"], "never");
    assert_eq!(start["sandbox"], "danger-full-access");
    assert_eq!(start["config"]["model_reasoning_effort"], EFFORT);
    assert_eq!(requests[3]["params"]["threadId"], THREAD);
    assert_eq!(requests[3]["params"]["name"], "control fixture assignment");
    drop(conversation);
    fixture.terminate();
}

#[test]
fn plan_spawns_the_documented_app_server_command() {
    let fixture = fixture();
    let spec = app_server_spec(&fixture.plan, 51234);
    assert_eq!(spec.program, PathBuf::from(FIXTURE));
    assert_eq!(spec.current_dir.as_deref(), Some(fixture.slot.as_path()));
    let args: Vec<String> = spec
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "app-server".to_owned(),
            "--listen".to_owned(),
            "ws://127.0.0.1:51234".to_owned(),
            "--ws-auth".to_owned(),
            "capability-token".to_owned(),
            "--ws-token-file".to_owned(),
            fixture.plan.paths.token.to_string_lossy().into_owned(),
            "-c".to_owned(),
            "agents.enabled=false".to_owned(),
        ]
    );
    assert_eq!(
        spec.env
            .get(&std::ffi::OsString::from("CODEX_HOME"))
            .cloned()
            .flatten(),
        Some(fixture.plan.home.clone().into_os_string())
    );
    assert_eq!(
        spec.env
            .get(&std::ffi::OsString::from("HARNESS_EXECUTOR_SESSION"))
            .cloned()
            .flatten(),
        Some(std::ffi::OsString::from("1"))
    );
}

#[test]
fn start_refuses_a_thread_that_reports_another_binding() {
    let mut fixture = fixture_with(Answer::Result(json!({
        "thread": {"id": THREAD},
        "model": "other-model",
        "modelProvider": PROVIDER,
        "reasoningEffort": EFFORT
    })));
    let error = fixture.start().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("binding was not preserved: the started thread reports model=other-model"),
        "{error}"
    );
    assert!(
        fixture.server.requests_for("thread/name/set").is_empty(),
        "a refused thread must not be named or driven further"
    );
    assert!(
        !fixture.endpoint_path().exists(),
        "a refused conversation must not be recorded as addressable"
    );
    fixture.terminate();
}

#[test]
fn binding_verification_covers_every_bound_field() {
    let bound = identity();
    assert!(bound.verify(&thread_start_answer()).is_ok());
    assert!(
        bound
            .verify(&json!({"model": MODEL, "modelProvider": PROVIDER}))
            .is_err(),
        "a thread that reports no effort cannot be assumed to run the bound one"
    );
    assert!(
        bound
            .verify(&json!({
                "model": MODEL.to_uppercase(), "modelProvider": PROVIDER, "reasoningEffort": EFFORT
            }))
            .is_ok()
    );
    for (field, value) in [
        ("model", json!("other-model")),
        ("modelProvider", json!("other-provider")),
        ("reasoningEffort", json!("low")),
    ] {
        let mut answer = thread_start_answer();
        if field == "model" {
            answer["modelProvider"] = json!(PROVIDER);
        }
        answer[field] = value;
        let mismatch = bound.verify(&answer).unwrap_err();
        assert!(mismatch.contains(field), "{mismatch}");
    }
    // An unbound profile pins nothing, so nothing can mismatch.
    let open = BoundIdentity {
        profile: "default".to_owned(),
        model: None,
        model_provider: None,
        reasoning_effort: None,
    };
    assert!(open.verify(&json!({})).is_ok());
}

#[test]
fn startup_fails_closed_when_the_child_exits_before_serving() {
    let root = tempfile::tempdir().unwrap();
    let slot = root.path().join("slot");
    fs::create_dir_all(&slot).unwrap();
    let paths = ControlPaths {
        endpoint: root.path().join("control-1.json"),
        token: root.path().join("control-1.token"),
        log: root.path().join("control-1.log"),
    };
    let mut plan = ControlPlan::new(
        FIXTURE,
        root.path().join("home"),
        &slot,
        "exiting child",
        identity(),
        paths,
    );
    plan.bound = Duration::from_secs(5);
    plan.poll = Duration::from_millis(50);
    plan.env.insert(
        "HARNESS_EXECUTOR_FIXTURE_MODE".into(),
        Some("complete".into()),
    );
    let job = Job::new(Limits::default()).unwrap();
    let error = Conversation::start(&job, &plan).unwrap_err();
    assert!(
        error.to_string().contains("exited with exit code 0"),
        "{error}"
    );
    assert!(
        error.to_string().contains("control-1.log"),
        "the failure names the log to read: {error}"
    );
    let token = fs::read_to_string(&plan.paths.token).unwrap();
    assert_eq!(token.len(), 64, "the capability token is 32 bytes of hex");
    assert!(token.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    assert!(plan.paths.log.exists(), "the child log exists");
    let _ = job.terminate(0, Duration::from_secs(5));
}

#[test]
fn control_state_sits_beside_the_receipt_and_carries_the_resolved_binding() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("checkout");
    fs::create_dir_all(&source).unwrap();
    let paths = ControlPaths::for_slot(&root.path().join("home"), &source, 3).unwrap();
    // The name is the convention the stop path reads (`endpoint-<index>.json`
    // beside the receipt): a record under another name leaves a live run
    // unaddressable for `executor stop` and `executor message`.
    assert_eq!(paths.endpoint.file_name().unwrap(), "endpoint-3.json");
    assert_eq!(paths.token.file_name().unwrap(), "endpoint-3.token");
    assert_eq!(paths.log.file_name().unwrap(), "endpoint-3.log");
    let directory = paths.endpoint.parent().unwrap();
    assert_eq!(directory, paths.token.parent().unwrap());
    assert_eq!(directory, paths.log.parent().unwrap());
    assert_eq!(
        paths.endpoint.with_file_name("spawn-3.json").parent(),
        Some(directory),
        "the endpoint record is written where the dispatch receipt of the same slot lives"
    );

    let binding = BoundIdentity::resolve(&ProfileBinding {
        profile: "deepseek".to_owned(),
        model: Some(MODEL.to_owned()),
        model_provider: Some(PROVIDER.to_owned()),
        reasoning_effort: Some(EFFORT.to_owned()),
    });
    assert_eq!(binding, identity());
}

#[test]
fn startup_fails_closed_when_the_child_never_serves_the_endpoint() {
    let root = tempfile::tempdir().unwrap();
    let slot = root.path().join("slot");
    fs::create_dir_all(&slot).unwrap();
    let marker = root.path().join("child.json");
    let paths = ControlPaths {
        endpoint: root.path().join("control-1.json"),
        token: root.path().join("control-1.token"),
        log: root.path().join("control-1.log"),
    };
    let mut plan = ControlPlan::new(
        FIXTURE,
        root.path().join("home"),
        &slot,
        "silent child",
        identity(),
        paths,
    );
    plan.bound = Duration::from_secs(1);
    plan.poll = Duration::from_millis(50);
    plan.env
        .insert("HARNESS_EXECUTOR_FIXTURE_MODE".into(), Some("hang".into()));
    plan.env.insert(
        "HARNESS_EXECUTOR_FIXTURE_STARTED".into(),
        Some(marker.clone().into_os_string()),
    );
    let job = Job::new(Limits::default()).unwrap();
    let error = Conversation::start(&job, &plan).unwrap_err();
    assert!(error.to_string().contains("did not serve"), "{error}");
    assert!(error.to_string().contains("control-1.log"), "{error}");
    // The child is still running: the module never reaps what the caller's Job
    // owns, and the failure names it instead of hiding it.
    let user = harness_core::process_service::current_user().unwrap();
    let recorded: harness_core::process::ProcessIdentity =
        serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    let child = harness_core::process_service::ServiceProcess::observe(
        recorded.pid,
        Path::new(FIXTURE),
        recorded.creation_time,
        &user,
    )
    .unwrap();
    assert!(
        child.is_running().unwrap(),
        "the caller's Job still owns the child"
    );
    let _ = job.terminate(0, Duration::from_secs(5));
}

// ---------------------------------------------------- attached conversations

/// A recorded endpoint plus the canned server behind it: exactly the recorded
/// state `executor message` and `executor stop` read, so adopting it is tested
/// where those commands will use it.
struct Recorded {
    _root: tempfile::TempDir,
    server: Server,
    endpoint: Endpoint,
    record: PathBuf,
}

fn recorded() -> Recorded {
    recorded_with(thread_read_answer())
}

fn recorded_with(read: Value) -> Recorded {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    server.answer("initialize", Answer::Result(json!({})));
    server.answer("thread/read", Answer::Result(read));
    server.answer(
        "turn/start",
        Answer::Result(json!({"turn": {"id": TURN, "status": "inProgress"}})),
    );
    let record = root.path().join("control-1.json");
    fs::write(
        &record,
        serde_json::to_vec_pretty(&json!({
            "schema": 1, "port": server.port, "token": TOKEN, "threadId": THREAD
        }))
        .unwrap(),
    )
    .unwrap();
    let endpoint = Endpoint::read(&record).unwrap();
    Recorded {
        _root: root,
        server,
        endpoint,
        record,
    }
}

impl Recorded {
    fn attach(&self) -> Conversation {
        Conversation::attach(&self.endpoint, WAIT).unwrap()
    }
}

#[test]
fn lifecycle_mapping_follows_the_thread_records() {
    let mut fixture = fixture();
    let mut conversation = fixture.start().unwrap();
    completion_burst(&fixture.server);
    let events = drain(&mut conversation, 11);
    let states: Vec<Option<Lifecycle>> = events.iter().map(|event| event.lifecycle).collect();
    assert_eq!(
        states,
        [
            Some(Lifecycle::NativeStart),
            Some(Lifecycle::Running),
            Some(Lifecycle::Running),
            Some(Lifecycle::Running),
            Some(Lifecycle::Running),
            None,
            None,
            None,
            None,
            Some(Lifecycle::Completed),
            None
        ],
        "idle after a completed turn changes nothing"
    );
    assert!(events.iter().all(|event| event.deviation.is_none()));
    assert_eq!(conversation.lifecycle(), Some(Lifecycle::Completed));
    assert!(Lifecycle::Completed.is_terminal());
    assert_eq!(
        conversation.lifecycle().unwrap().receipt_state(),
        "completed"
    );
    assert_eq!(conversation.failure(), None);
    assert_eq!(conversation.defect(), None);
    assert_eq!(Lifecycle::NativeStart.receipt_state(), "native-start");
    assert_eq!(Lifecycle::Defect.receipt_state(), "defect");
    assert!(!Lifecycle::Running.is_terminal());

    let mut rendered = Vec::new();
    for event in &events {
        event.render(&mut rendered).unwrap();
    }
    let text = String::from_utf8(rendered).unwrap();
    assert!(
        text.contains(&format!("state: native start (session {THREAD})")),
        "{text}"
    );
    assert!(text.contains("state: thread active"), "{text}");
    assert!(
        text.contains("command: pwsh -NoProfile -Command 'exit 0' (exit 0)"),
        "{text}"
    );
    assert!(text.contains(&format!("assistant: {FINAL}")), "{text}");
    assert!(text.contains("state: turn completed (completed)"), "{text}");

    // The final message comes from the thread items, and the read is the
    // verified shape.
    assert_eq!(
        conversation.final_message().unwrap(),
        FinalMessage::Present(FINAL.to_owned())
    );
    let read = fixture.server.requests_for("thread/read");
    assert_eq!(read[0]["params"]["includeTurns"], true);
    assert_eq!(read[0]["params"]["threadId"], THREAD);
    drop(conversation);
    fixture.terminate();
}

#[test]
fn failed_and_interrupted_turns_are_distinguished() {
    let recorded = recorded();
    let mut conversation = recorded.attach();
    recorded.server.push(json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{
        "id":TURN,"status":"failed",
        "error":{"message":"provider refused the request","codexErrorInfo":"usageLimitExceeded"}}}}));
    let events = drain(&mut conversation, 1);
    assert_eq!(events[0].lifecycle, Some(Lifecycle::Failed));
    assert_eq!(conversation.failure(), Some("provider refused the request"));
    assert_eq!(conversation.lifecycle().unwrap().receipt_state(), "failed");
    assert!(conversation.lifecycle().unwrap().is_terminal());
    recorded.server.push(
        json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{
        "id":TURN,"status":"interrupted"}}}),
    );
    let events = drain(&mut conversation, 1);
    assert_eq!(events[0].lifecycle, Some(Lifecycle::Interrupted));
    assert_eq!(
        conversation.lifecycle().unwrap().receipt_state(),
        "interrupted"
    );
    // Item activity after an interrupted turn never reopens the state.
    recorded.server.push(
        json!({"method":"item/completed","params":{"threadId":THREAD,"item":{
        "id":"c1","type":"commandExecution","command":"pwsh","exitCode":0}}}),
    );
    let events = drain(&mut conversation, 1);
    assert_eq!(events[0].lifecycle, None);
    assert_eq!(conversation.lifecycle(), Some(Lifecycle::Interrupted));
}

#[test]
fn protocol_deviations_are_recorded_and_never_invent_state() {
    let recorded = recorded();
    let mut conversation = recorded.attach();
    recorded.server.push(
        json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{
        "id":TURN,"status":"cancelled-by-a-future-build"}}}),
    );
    recorded
        .server
        .push(json!({"method":"item/completed","params":{"threadId":THREAD,"item":{"id":"x"}}}));
    recorded.server.push(json!({"method":"thread/status/changed","params":{"threadId":THREAD,"status":{"type":"hibernating"}}}));
    recorded
        .server
        .push(json!({"method":"codex/future-notification","params":{"threadId":THREAD}}));
    recorded.server.push(json!({"method":"item/completed","params":{"threadId":"another-thread","item":{"id":"y","type":"agentMessage","text":"not ours"}}}));
    recorded.server.push(json!({"method":"error","params":{"threadId":THREAD,"willRetry":true,"error":{"message":"transient"}}}));
    let events = drain(&mut conversation, 6);
    assert_eq!(events[0].lifecycle, Some(Lifecycle::Defect));
    assert!(
        events[0]
            .deviation
            .as_deref()
            .unwrap()
            .contains("unknown turn status"),
        "{:?}",
        events[0].deviation
    );
    assert!(
        events[1]
            .deviation
            .as_deref()
            .unwrap()
            .contains("no item type")
    );
    assert!(
        events[2]
            .deviation
            .as_deref()
            .unwrap()
            .contains("unknown status type"),
        "{:?}",
        events[2].deviation
    );
    assert_eq!(
        events[3].lifecycle, None,
        "an unknown method is not a deviation"
    );
    assert_eq!(events[3].deviation, None);
    assert_eq!(
        events[4].lifecycle, None,
        "another thread is not this conversation"
    );
    assert_eq!(events[4].deviation, None);
    assert_eq!(events[5].lifecycle, None, "a retrying error is not a state");
    assert_eq!(conversation.lifecycle(), Some(Lifecycle::Defect));
    assert!(
        conversation
            .defect()
            .unwrap()
            .contains("unknown turn status"),
        "the first deviation is retained"
    );
    assert_eq!(
        conversation.failure(),
        None,
        "a retrying error is not a recorded cause"
    );
    let mut rendered = Vec::new();
    events[5].render(&mut rendered).unwrap();
    assert!(
        String::from_utf8(rendered)
            .unwrap()
            .contains("error: transient (retrying)")
    );
}

#[test]
fn assignment_turn_start_records_the_turn_and_rejects_a_missing_turn() {
    let recorded = recorded();
    let mut conversation = recorded.attach();
    let turn = conversation
        .assign("apply the correction to the same conversation")
        .unwrap();
    assert_eq!(turn.turn_id, TURN);
    assert_eq!(turn.status, "inProgress");
    assert_eq!(conversation.turn().unwrap().turn_id, TURN);
    let requests = recorded.server.requests_for("turn/start");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["params"]["threadId"], THREAD);
    assert_eq!(requests[0]["params"]["input"][0]["type"], "text");
    assert_eq!(
        requests[0]["params"]["input"][0]["text"],
        "apply the correction to the same conversation"
    );
    assert!(
        conversation.assign("").is_err(),
        "an empty input is refused"
    );
    recorded.server.answer(
        "turn/start",
        Answer::Result(json!({"turn": {"status": "inProgress"}})),
    );
    let error = conversation.assign("second submission").unwrap_err();
    assert!(
        error.to_string().contains("without a turn identity"),
        "{error}"
    );
}

#[test]
fn call_returns_one_bounded_native_answer() {
    let recorded = recorded();
    recorded.server.answer(
        "turn/interrupt",
        Answer::Result(json!({"interrupted": true})),
    );
    let mut conversation = recorded.attach();
    let answer = conversation
        .call(
            "turn/interrupt",
            json!({"threadId": THREAD, "turnId": TURN}),
        )
        .unwrap();
    assert_eq!(answer["interrupted"], true);
    let seen = recorded.server.requests_for("turn/interrupt");
    assert_eq!(seen[0]["params"]["turnId"], TURN);
}

#[test]
fn final_message_distinguishes_present_empty_and_missing() {
    let recorded = recorded_with(thread_read_answer());
    let mut conversation = recorded.attach();
    assert_eq!(
        conversation.final_message().unwrap(),
        FinalMessage::Present(FINAL.to_owned())
    );
    recorded.server.answer(
        "thread/read",
        Answer::Result(json!({"thread": {"id": THREAD, "turns": [
            {"id": TURN, "status": "completed", "items": [
                {"id": "m1", "type": "agentMessage", "text": "   "}]}]}})),
    );
    assert_eq!(conversation.final_message().unwrap(), FinalMessage::Empty);
    recorded.server.answer(
        "thread/read",
        Answer::Result(json!({"thread": {"id": THREAD, "turns": [
            {"id": TURN, "status": "completed", "items": [
                {"id": "c1", "type": "commandExecution"}]}]}})),
    );
    assert_eq!(conversation.final_message().unwrap(), FinalMessage::Missing);
    recorded.server.answer(
        "thread/read",
        Answer::Result(json!({"thread": {"id": "another-thread", "turns": []}})),
    );
    let error = conversation.final_message().unwrap_err();
    assert!(error.to_string().contains("instead of"), "{error}");
}

#[test]
fn protocol_errors_fail_the_call_and_a_deadline_is_not_progress() {
    let recorded = recorded();
    let mut conversation = recorded.attach();
    recorded.server.answer(
        "thread/read",
        Answer::Record(json!({"id": "@request", "result": {"thread": {"id": THREAD}}})),
    );
    assert!(conversation.thread_state().is_ok());
    recorded.server.answer(
        "thread/read",
        Answer::Record(json!({"id": 4242, "result": {}})),
    );
    let error = conversation.thread_state().unwrap_err();
    assert!(
        error.to_string().contains("carried request identity 4242"),
        "{error}"
    );
    recorded.server.answer(
        "thread/read",
        Answer::Error(json!({"code": -32601, "message": "no such method"})),
    );
    let error = conversation.thread_state().unwrap_err();
    assert!(error.to_string().contains("was rejected"), "{error}");
    recorded
        .server
        .answer("thread/read", Answer::Record(json!({"id": "@request"})));
    let error = conversation.thread_state().unwrap_err();
    assert!(error.to_string().contains("carried no result"), "{error}");
    recorded
        .server
        .answer("thread/read", Answer::Record(json!("not a control object")));
    let error = conversation.thread_state().unwrap_err();
    assert!(error.to_string().contains("not a JSON object"), "{error}");
    recorded.server.answer("thread/read", Answer::Silence);
    let mut silent = Conversation::attach(&recorded.endpoint, Duration::from_millis(300)).unwrap();
    let error = silent.thread_state().unwrap_err();
    assert!(
        error.to_string().contains("was not answered within"),
        "{error}"
    );
    assert_eq!(silent.lifecycle(), None, "a deadline establishes no state");
}

#[test]
fn one_pump_is_bounded_and_leaves_the_rest_queued() {
    let recorded = recorded();
    let mut conversation = recorded.attach();
    for index in 0..MAX_EVENTS_PER_PUMP + 8 {
        recorded.server.push(
            json!({"method":"item/completed","params":{"threadId":THREAD,"item":{
            "id":format!("m{index}"),"type":"agentMessage","text":format!("line {index}")}}}),
        );
    }
    let mut first = conversation.pump().unwrap();
    while first.len() < MAX_EVENTS_PER_PUMP {
        let batch = conversation.pump().unwrap();
        assert!(batch.len() <= MAX_EVENTS_PER_PUMP);
        first.extend(batch);
    }
    assert_eq!(first.len(), MAX_EVENTS_PER_PUMP);
    assert!(
        first
            .iter()
            .all(|event| event.lifecycle == Some(Lifecycle::Running))
    );
    let rest = drain(&mut conversation, 8);
    assert_eq!(rest.len(), 8);
    assert_eq!(conversation.lifecycle(), Some(Lifecycle::Running));
}

#[test]
fn endpoint_records_are_validated_and_a_thread_is_required() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("control-2.json");
    assert!(
        Endpoint::read(&path).is_err(),
        "a missing record is not an endpoint"
    );
    for record in [
        json!({"schema": 2, "port": 1234, "token": TOKEN, "threadId": null}),
        json!({"schema": 1, "port": 0, "token": TOKEN, "threadId": null}),
        json!({"schema": 1, "port": 1234, "token": "short", "threadId": null}),
        json!({"schema": 1, "port": 1234, "token": TOKEN, "threadId": ""}),
        json!({"schema": 1, "port": 1234, "token": TOKEN, "threadId": null, "extra": true}),
    ] {
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(Endpoint::read(&path).is_err(), "{record}");
    }
    fs::write(
        &path,
        serde_json::to_vec(&json!({"schema": 1, "port": 1234, "token": TOKEN, "threadId": null}))
            .unwrap(),
    )
    .unwrap();
    let endpoint = Endpoint::read(&path).unwrap();
    let error = Conversation::attach(&endpoint, Duration::from_millis(300)).unwrap_err();
    assert!(error.to_string().contains("names no thread"), "{error}");
    let recorded = recorded();
    let round_trip = Endpoint::read(&recorded.record).unwrap();
    assert_eq!(round_trip.token(), TOKEN);
    assert_eq!(round_trip.thread_id.as_deref(), Some(THREAD));
}

#[test]
fn attach_refuses_another_bearer() {
    let recorded = recorded();
    fs::write(
        &recorded.record,
        serde_json::to_vec(&json!({
            "schema": 1, "port": recorded.server.port, "token": OTHER_TOKEN, "threadId": THREAD
        }))
        .unwrap(),
    )
    .unwrap();
    let foreign = Endpoint::read(&recorded.record).unwrap();
    assert_ne!(foreign.token(), recorded.endpoint.token());
    assert!(Conversation::attach(&foreign, Duration::from_millis(500)).is_err());
    wait_for(
        || recorded.server.refusals() == 1,
        "the endpoint refused the foreign bearer",
    );
    assert!(
        recorded.server.requests().is_empty(),
        "no request crosses a refused handshake"
    );
}

// ------------------------------------------------------ pooled exec host

/// The real `codex-harness` binary: these checks host a pooled exec receipt
/// through the same `executor run --file` entry point the dispatcher's
/// terminal-tab and owned-console routes run.
fn manager() -> PathBuf {
    std::env::var_os("HARNESS_OBSERVATION_MANAGER_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_codex-harness")))
}

/// The owner of the fixture slot binding.
const OWNER: &str = "exec-deepseek-host";
/// The assignment one fixture dispatch carries.
const ASSIGNMENT: &str = "fixture control assignment text";

/// One pooled exec dispatch as the dispatcher records it: the launcher, the
/// resolved profile binding, the slot binding, the prepared shell, the run
/// observation and the control route, hosted against the canned endpoint this
/// test owns. The child the host spawns is the kit's own executor fixture
/// process (it proves the real spawn, Job ownership and process record), while
/// the protocol answers come from the canned endpoint because the kit has no
/// app-server fixture binary.
struct Pooled {
    _root: tempfile::TempDir,
    home: PathBuf,
    slot: PathBuf,
    state: PathBuf,
    receipt: PathBuf,
    server: Server,
}

impl Pooled {
    /// `pinned` selects the acceptance shape that owns the endpoint itself: the
    /// receipt then records the canned port, exactly as `ControlPlan::port`
    /// documents for a caller that provides the server. Without it the host
    /// reserves a free port and the spawned child must serve that port itself,
    /// which is the ordinary dispatch shape and the startup-failure shape.
    fn new(name: &str, thread_start: Answer, pinned: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join(format!("source-{name}"));
        fs::create_dir_all(&source).unwrap();
        let source = source.canonicalize().unwrap();
        let home = root.path().join("home");
        let slot = root.path().join("slot");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&slot).unwrap();
        let paths = ControlPaths::for_slot(&home, &source, 1).unwrap();
        let state = paths.endpoint.parent().unwrap().to_path_buf();
        fs::create_dir_all(&state).unwrap();
        // The bound slot record the host's lease reconciliation reads.
        fs::write(
            state.join("slot-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "source": source,
                "index": 1,
                "path": slot,
                "state": "occupied",
                "owner": OWNER,
                "base": "abc123",
                "disposition": null,
                "reason": null,
            }))
            .unwrap(),
        )
        .unwrap();
        let server = Server::start(Bearer::File(paths.token.clone()));
        server.answer("initialize", Answer::Result(json!({})));
        server.answer("thread/start", thread_start);
        server.answer("thread/name/set", Answer::Result(json!({})));
        server.answer(
            "turn/start",
            Answer::Result(json!({"turn": {"id": TURN, "status": "inProgress"}})),
        );
        server.answer("thread/read", Answer::Result(thread_read_answer()));
        let receipt = state.join("spawn-1.json");
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "launcher": FIXTURE,
                "profile": "deepseek",
                "mode": "exec",
                "args": [],
                "visible": true,
                "host": "owned-console",
                "control": {
                    "schema": 1,
                    "assignment": ASSIGNMENT,
                    "identity": {
                        "profile": "deepseek",
                        "model": MODEL,
                        "modelProvider": PROVIDER,
                        "reasoningEffort": EFFORT,
                    },
                    "port": pinned.then_some(server.port),
                },
                "terminal": null,
                "isolation": false,
                "slot": {
                    "index": 1,
                    "path": slot,
                    "source": source,
                    "owner": OWNER,
                    "base": "abc123",
                    "remote": "origin",
                    "branch": "main",
                },
                "model": MODEL,
                "modelProvider": PROVIDER,
                "reasoningEffort": EFFORT,
                "window": null,
                "shell": {
                    "path": std::env::var_os("PATH").unwrap(),
                    "executable": FIXTURE,
                    "version": "fixture",
                    "sandbox_mode": "danger-full-access",
                },
                "observation": {
                    "schema": 1,
                    "coverage": "native",
                    "state": "dispatch-accepted",
                    "result": state.join("message-1.txt"),
                    "detail": state.join("stream-1.jsonl"),
                },
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            _root: root,
            home,
            slot,
            state,
            receipt,
            server,
        }
    }

    /// Starts the real host entry point for this receipt. The fixture child
    /// mode reaches the spawned app-server double through the inherited
    /// environment, exactly as the host's own environment reaches its child.
    fn host(&self, child_mode: &str) -> std::process::Child {
        let mut command = Command::new(manager());
        command
            .args(["executor", "run", "--file"])
            .arg(&self.receipt)
            .env("CODEX_HOME", &self.home)
            .env("HARNESS_EXECUTOR_FIXTURE_MODE", child_mode)
            .env_remove("HARNESS_EXECUTOR_SESSION")
            .env_remove("HARNESS_EXECUTOR_RUN_LOG")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.spawn().unwrap()
    }

    fn receipt(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.receipt).unwrap()).unwrap()
    }

    fn endpoint_path(&self) -> PathBuf {
        self.state.join("endpoint-1.json")
    }

    fn result_path(&self) -> PathBuf {
        self.state.join("message-1.txt")
    }

    fn detail_path(&self) -> PathBuf {
        self.state.join("stream-1.jsonl")
    }

    fn lease_path(&self) -> PathBuf {
        self.state.join("lease-1.json")
    }
}

fn output_text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Bounded wait until the exact recorded process identity is gone. A pid alone
/// is never enough: the creation time and image must match the record.
fn wait_gone(pid: u32, creation_time: u64, program: &Path, reason: &str) {
    let user = process_service::current_user().unwrap();
    let until = Instant::now() + WAIT;
    loop {
        match ServiceProcess::inspect(ProcessIdentity { pid, creation_time }, program, &user) {
            Ok(None) => return,
            Ok(Some(_)) if Instant::now() < until => thread::sleep(Duration::from_millis(25)),
            Ok(Some(_)) => panic!("{reason}"),
            Err(error) => panic!("{reason}: {error}"),
        }
    }
}

#[test]
fn a_hosted_exec_dispatch_converses_through_the_control_driver_and_records_its_result() {
    let pooled = Pooled::new("host-success", Answer::Result(thread_start_answer()), true);
    let host = pooled.host("hang");
    let host_pid = host.id();
    // The assignment reaches the thread as one native turn; only then does the
    // canned endpoint answer, exactly as the native server answers a turn that
    // is still in progress.
    wait_for(
        || !pooled.server.requests_for("turn/start").is_empty(),
        "the host submits the assignment through turn/start",
    );
    assert!(
        pooled.lease_path().exists(),
        "the host holds the slot's lease for the session's lifetime"
    );
    completion_burst(&pooled.server);
    let output = host.wait_with_output().unwrap();
    let text = output_text(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");

    // The visible surface renders the conversation: the dispatched identity,
    // the assignment, the records as they arrive and the terminal state.
    assert!(
        text.contains("conversation=control (codex app-server)"),
        "{text}"
    );
    assert!(text.contains("assignment:"), "{text}");
    assert!(text.contains(ASSIGNMENT), "{text}");
    assert!(
        text.contains(&format!("state: native start (session {THREAD})")),
        "{text}"
    );
    assert!(text.contains("state: turn completed (completed)"), "{text}");
    assert!(text.contains(&format!("assistant: {FINAL}")), "{text}");
    assert!(text.contains("result: completed (events="), "{text}");
    assert!(
        text.contains(&format!(
            "result message: {}",
            pooled.result_path().display()
        )),
        "{text}"
    );

    // The receipt records the exact thread identity, the lifecycle, the exit
    // code, the host that observed it and the final-message locator.
    let receipt = pooled.receipt();
    let observation = &receipt["observation"];
    assert_eq!(observation["state"], "completed", "{receipt}");
    assert_eq!(observation["session"], THREAD, "{receipt}");
    assert_eq!(observation["exitCode"], 0, "{receipt}");
    assert_eq!(observation["messages"], 1, "{receipt}");
    assert_eq!(observation["toolCalls"], 1, "{receipt}");
    assert!(observation["events"].as_u64().unwrap() >= 7, "{receipt}");
    assert_eq!(observation["host"]["pid"], host_pid, "{receipt}");
    assert_eq!(
        fs::read_to_string(pooled.result_path()).unwrap(),
        FINAL,
        "the thread's own final message is the recorded result"
    );
    let detail = fs::read_to_string(pooled.detail_path()).unwrap();
    assert!(detail.contains("\"method\":\"thread/started\""), "{detail}");
    assert!(detail.contains("\"method\":\"turn/completed\""), "{detail}");
    assert!(!detail.contains("agentMessage/delta"), "{detail}");
    assert!(!detail.contains("textDelta"), "{detail}");
    assert!(!detail.contains("mcpServer"), "{detail}");
    assert!(!text.contains("event: item/agentMessage/delta"), "{text}");
    assert!(!text.contains("event: "), "{text}");
    assert!(!text.contains("IGNORED_DELTA"), "{text}");
    assert!(!text.contains("IGNORED_REASONING_DELTA"), "{text}");

    // The endpoint record is the address `executor stop` and
    // `executor message` read: beside the dispatch receipt, under the name the
    // stop path looks for, with the app-server child's exact identity.
    let endpoint: Value =
        serde_json::from_slice(&fs::read(pooled.endpoint_path()).unwrap()).unwrap();
    assert_eq!(endpoint["schema"], 1, "{endpoint}");
    assert_eq!(endpoint["port"], pooled.server.port, "{endpoint}");
    assert_eq!(endpoint["threadId"], THREAD, "{endpoint}");
    assert_eq!(endpoint["process"]["program"], FIXTURE, "{endpoint}");
    let token = fs::read_to_string(pooled.state.join("endpoint-1.token")).unwrap();
    assert_eq!(endpoint["token"], token.trim(), "{endpoint}");
    let child_pid = endpoint["process"]["pid"].as_u64().unwrap() as u32;
    assert_ne!(child_pid, 0);
    assert_ne!(child_pid, host_pid, "the recorded child is not its host");
    // The child this host owned ended with it: no stray app-server is left
    // behind, and the slot's lease ends with the run.
    wait_gone(
        child_pid,
        endpoint["process"]["creationTime"].as_u64().unwrap(),
        Path::new(FIXTURE),
        "the app-server child ends with its host",
    );
    assert!(!pooled.lease_path().exists(), "the lease ends with the run");

    // The conversation ran on the bound slot with the recorded assignment.
    let start = pooled.server.requests_for("thread/start");
    assert_eq!(start.len(), 1, "{start:?}");
    assert_eq!(start[0]["params"]["cwd"], json!(pooled.slot), "{start:?}");
    assert_eq!(start[0]["params"]["model"], MODEL, "{start:?}");
    assert_eq!(start[0]["params"]["modelProvider"], PROVIDER, "{start:?}");
    assert_eq!(
        start[0]["params"]["config"]["model_reasoning_effort"], EFFORT,
        "{start:?}"
    );
    assert_eq!(
        start[0]["params"]["sandbox"], "danger-full-access",
        "the prepared shell's sandbox is pinned on the thread: {start:?}"
    );
    assert_eq!(start[0]["params"]["approvalPolicy"], "never", "{start:?}");
    let name = pooled.server.requests_for("thread/name/set");
    assert_eq!(
        name[0]["params"]["name"],
        format!("CEx (deepseek) - {OWNER}"),
        "the thread carries the same assignment title as the run's surface: {name:?}"
    );
    let turn = pooled.server.requests_for("turn/start");
    assert_eq!(turn.len(), 1, "{turn:?}");
    assert_eq!(turn[0]["params"]["threadId"], THREAD, "{turn:?}");
    assert_eq!(
        turn[0]["params"]["input"],
        json!([{"type": "text", "text": ASSIGNMENT}]),
        "{turn:?}"
    );
    let read = pooled.server.requests_for("thread/read");
    assert_eq!(read[0]["params"]["threadId"], THREAD, "{read:?}");

    // The recorded lifecycle is what the existing review path reads: watch
    // reports the completed run, the exact session and the returned message
    // from the same receipt and result locator.
    let watched = Command::new(manager())
        .args(["executor", "watch", "--receipt"])
        .arg(&pooled.receipt)
        .env_remove("HARNESS_EXECUTOR_SESSION")
        .output()
        .unwrap();
    let watched_text = output_text(&watched);
    assert_eq!(watched.status.code(), Some(0), "{watched_text}");
    assert!(watched_text.contains("state=completed"), "{watched_text}");
    assert!(
        watched_text.contains(&format!("session: {THREAD}")),
        "{watched_text}"
    );
    assert!(watched_text.contains(FINAL), "{watched_text}");
}

#[test]
fn cache_loss_interrupts_the_control_turn_and_preserves_the_slot() {
    let pooled = Pooled::new("cache-loss", Answer::Result(thread_start_answer()), true);
    pooled
        .server
        .answer("turn/interrupt", Answer::Result(json!({})));
    let preserved = pooled.slot.join("partial.txt");
    fs::write(&preserved, "partial work stays").unwrap();
    let mut host = pooled.host("complete");
    wait_for(
        || !pooled.server.requests_for("turn/start").is_empty(),
        "cache-loss assignment started",
    );
    let rollout = cache_usage::write_loss(&pooled.home, THREAD);
    let until = Instant::now() + WAIT;
    loop {
        if host.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= until {
            let _ = host.kill();
            panic!("cache-loss host did not terminate within its test deadline");
        }
        thread::sleep(Duration::from_millis(25));
    }
    let output = host.wait_with_output().unwrap();
    let text = output_text(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("DeepSeek cache loss"), "{text}");
    assert!(text.contains("no process remained"), "{text}");
    let interrupts = pooled.server.requests_for("turn/interrupt");
    assert!(
        interrupts.is_empty(),
        "cache loss must terminate without waiting for a control reply"
    );
    let receipt = pooled.receipt();
    assert_eq!(receipt["observation"]["state"], "stopped", "{receipt}");
    assert_eq!(receipt["cacheGuard"]["lastMissTokens"], 394_000);
    assert_eq!(fs::read_to_string(preserved).unwrap(), "partial work stays");
    assert!(rollout.exists());
    assert!(pooled.state.join("slot-1.json").exists());
}

#[path = "fixtures/succession_responses.rs"]
mod cache_responses;

#[test]
#[ignore = "requires native Codex and owner PowerShell 7; all Responses are local canned events"]
fn native_control_cache_loss_stops_a_fresh_session_and_preserves_work() {
    let exe =
        PathBuf::from(std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("native Codex path"));
    let shell = PathBuf::from(
        std::env::var_os("HARNESS_ACCEPTANCE_POWERSHELL").expect("owner PowerShell 7"),
    );
    let pooled = Pooled::new("native-cache", Answer::Result(thread_start_answer()), false);
    let evidence = pooled._root.path().join("evidence");
    fs::create_dir_all(&evidence).unwrap();
    fs::write(evidence.join("cache-loss"), "synthetic counters").unwrap();
    fs::write(pooled.slot.join("partial.txt"), "previous work preserved").unwrap();
    let responses = cache_responses::Responses::start(evidence.clone());
    fs::write(pooled.home.join("config.toml"), format!(
        "approval_policy = 'never'\nsandbox_mode = 'danger-full-access'\nmodel_context_window = 1000000\nmodel_auto_compact_token_limit = 990000\n[profiles.deepseek]\nmodel = 'gpt-6-astra'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'low'\n[model_providers.deepseek]\nname = 'Owned cache fixture'\nbase_url = 'http://127.0.0.1:{}/v1'\nwire_api = 'responses'\nenv_key = 'HARNESS_CONTROL_FIXTURE_KEY'\nrequires_openai_auth = false\nrequest_max_retries = 0\nstream_max_retries = 0\nsupports_websockets = false\n[analytics]\nenabled = false\n[projects.'{}']\ntrust_level = 'trusted'\n", responses.port, pooled.slot.to_string_lossy())).unwrap();
    let mut receipt = pooled.receipt();
    receipt["launcher"] = json!(exe);
    for (key, value) in [
        ("model", "gpt-6-astra"),
        ("modelProvider", "deepseek"),
        ("reasoningEffort", "low"),
    ] {
        receipt[key] = json!(value);
        receipt["control"]["identity"][key] = json!(value);
    }
    receipt["shell"]["executable"] = json!(shell);
    receipt["shell"]["version"] = json!("owner PowerShell 7");
    receipt["observation"]["previousSession"] = json!(THREAD);
    receipt["control"]["assignment"] = json!(
        "Continue the original fixture task in this preserved checkout; keep partial.txt and check previous work before using results."
    );
    fs::write(
        &pooled.receipt,
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    let stdout = fs::File::create(evidence.join("host-stdout.txt")).unwrap();
    let stderr = fs::File::create(evidence.join("host-stderr.txt")).unwrap();
    let mut host = Command::new(manager())
        .args(["executor", "run", "--file"])
        .arg(&pooled.receipt)
        .env("CODEX_HOME", &pooled.home)
        .env("HARNESS_CONTROL_FIXTURE_KEY", "synthetic-owned-fixture")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("HARNESS_EXECUTOR_SESSION")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(45);
    loop {
        if host.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= until {
            let _ = host.kill();
            panic!(
                "native control cache host timed out; evidence: {}",
                evidence.display()
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
    let status = host.wait().unwrap();
    let output = fs::read_to_string(evidence.join("host-stdout.txt")).unwrap();
    let error = fs::read_to_string(evidence.join("host-stderr.txt")).unwrap();
    let receipt = pooled.receipt();
    assert_eq!(status.code(), Some(1), "{output}\n{error}");
    assert_eq!(
        receipt["observation"]["state"], "stopped",
        "{receipt}\n{output}\n{error}"
    );
    assert_eq!(receipt["cacheGuard"]["consecutiveMisses"], 3);
    assert!(
        receipt["observation"]["session"]
            .as_str()
            .is_some_and(|session| session != THREAD)
    );
    assert_eq!(
        fs::read_to_string(pooled.slot.join("partial.txt")).unwrap(),
        "previous work preserved"
    );
    let requests = fs::read_dir(&evidence)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("provider-"))
        .count();
    assert!(
        (4..=5).contains(&requests),
        "requests after stop: {requests}"
    );
    println!(
        "native app-server cache stop: {requests} local requests; fresh session identity and preserved work verified"
    );
}

/// A turn that did not complete the assignment is never reported as a
/// completed run: the receipt keeps the state the thread's own status
/// established, keeps the measured exit code of that state, and records no
/// final message. A record that arrives after the terminal state is still
/// rendered and never reopens it.
#[test]
fn a_hosted_turn_that_did_not_complete_is_recorded_honestly() {
    for (status, message, expected_state, expected_exit) in [
        ("interrupted", None, "interrupted", 1),
        ("failed", Some("fixture native failure"), "failed", 1),
    ] {
        let pooled = Pooled::new(
            &format!("host-{status}"),
            Answer::Result(thread_start_answer()),
            true,
        );
        let host = pooled.host("hang");
        wait_for(
            || !pooled.server.requests_for("turn/start").is_empty(),
            "the host submits the assignment through turn/start",
        );
        terminal_turn(&pooled.server, status, message);
        pooled.server.push(
            json!({"method":"item/completed","params":{"threadId":THREAD,"item":{
            "id":"late","type":"commandExecution","command":"fixture late command","exitCode":0}}}),
        );
        let output = host.wait_with_output().unwrap();
        let text = output_text(&output);
        assert_eq!(
            output.status.code(),
            Some(expected_exit),
            "{status}: {text}"
        );
        assert!(
            text.contains(&format!("result: {expected_state}:")),
            "{status}: {text}"
        );
        assert!(
            text.contains("command: fixture late command"),
            "a record after the terminal state is still rendered: {status}: {text}"
        );
        let receipt = pooled.receipt();
        let observation = &receipt["observation"];
        assert_eq!(observation["state"], expected_state, "{status}: {receipt}");
        assert_eq!(
            observation["exitCode"], expected_exit,
            "{status}: {receipt}"
        );
        assert_eq!(observation["session"], THREAD, "{status}: {receipt}");
        if let Some(message) = message {
            assert!(
                observation["cause"].as_str().unwrap().contains(message),
                "{status}: {receipt}"
            );
        }
        assert!(
            !pooled.result_path().exists(),
            "no completed result is fabricated: {status}"
        );
        let endpoint: Value =
            serde_json::from_slice(&fs::read(pooled.endpoint_path()).unwrap()).unwrap();
        wait_gone(
            endpoint["process"]["pid"].as_u64().unwrap() as u32,
            endpoint["process"]["creationTime"].as_u64().unwrap(),
            Path::new(FIXTURE),
            "the app-server child ends with its host",
        );
    }
}

/// A completed turn whose final message is empty is an output defect, not a
/// successful run: the host records the defect with the documented cause, exits
/// with the defect code and writes no result.
#[test]
fn a_hosted_empty_final_message_is_an_output_defect() {
    let pooled = Pooled::new("host-empty", Answer::Result(thread_start_answer()), true);
    let mut read = thread_read_answer();
    read["thread"]["turns"][0]["items"] = json!([{"id":"m1","type":"agentMessage","text":""}]);
    pooled.server.answer("thread/read", Answer::Result(read));
    let host = pooled.host("hang");
    wait_for(
        || !pooled.server.requests_for("turn/start").is_empty(),
        "the host submits the assignment through turn/start",
    );
    completion_burst(&pooled.server);
    let output = host.wait_with_output().unwrap();
    let text = output_text(&output);
    assert_eq!(output.status.code(), Some(3), "{text}");
    assert!(text.contains("result: defect:"), "{text}");
    assert!(
        text.contains("an empty completion is an output defect"),
        "{text}"
    );
    let receipt = pooled.receipt();
    let observation = &receipt["observation"];
    assert_eq!(observation["state"], "defect", "{receipt}");
    assert_eq!(observation["exitCode"], 3, "{receipt}");
    assert!(
        observation["cause"]
            .as_str()
            .unwrap()
            .contains("the final message is empty"),
        "{receipt}"
    );
    assert!(!pooled.result_path().exists(), "{receipt}");
}

#[test]
fn a_hosted_exec_dispatch_refuses_a_thread_with_another_binding() {
    let pooled = Pooled::new(
        "host-binding",
        Answer::Result(json!({
            "thread": {"id": THREAD},
            "model": "other-model",
            "modelProvider": PROVIDER,
            "reasoningEffort": EFFORT
        })),
        true,
    );
    let output = pooled.host("hang").wait_with_output().unwrap();
    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("binding was not preserved"), "{text}");
    assert!(
        text.contains("the owned child tree was terminated"),
        "{text}"
    );

    // The honest failed state: no session identity, no fabricated exit code
    // and no addressable endpoint for a conversation that never started.
    let receipt = pooled.receipt();
    let observation = &receipt["observation"];
    assert_eq!(observation["state"], "failed", "{receipt}");
    assert!(observation["session"].is_null(), "{receipt}");
    assert!(observation["exitCode"].is_null(), "{receipt}");
    assert!(
        observation["cause"]
            .as_str()
            .unwrap()
            .contains("binding was not preserved"),
        "{receipt}"
    );
    assert!(!pooled.endpoint_path().exists(), "{receipt}");
    assert!(!pooled.result_path().exists(), "{receipt}");
    // No fallback run: the receipt records no launcher arguments for this
    // route, and the refused assignment never reached the thread.
    assert_eq!(receipt["args"], json!([]), "{receipt}");
    assert!(
        pooled.server.requests_for("turn/start").is_empty(),
        "a refused conversation submits nothing"
    );
    assert!(!pooled.lease_path().exists(), "the lease ends with the run");
}

#[test]
fn a_hosted_exec_dispatch_fails_closed_when_the_child_never_serves() {
    // No pinned endpoint: the spawned child must serve the reserved port, and
    // a child that exits before serving fails the run with its own cause.
    let pooled = Pooled::new("host-startup", Answer::Result(thread_start_answer()), false);
    let output = pooled.host("nonzero").wait_with_output().unwrap();
    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("the control-backed session did not start"),
        "{text}"
    );
    assert!(text.contains("exited with exit code 19"), "{text}");
    assert!(
        text.contains("fixture launcher fails before any native event"),
        "the control log is where the cause is read: {text}"
    );
    assert!(text.contains("control log:"), "{text}");
    assert!(
        text.contains("no fallback run was started"),
        "a control startup failure never falls back to another backend: {text}"
    );

    let receipt = pooled.receipt();
    let observation = &receipt["observation"];
    assert_eq!(observation["state"], "failed", "{receipt}");
    assert!(observation["session"].is_null(), "{receipt}");
    assert!(observation["exitCode"].is_null(), "{receipt}");
    assert!(
        observation["cause"]
            .as_str()
            .unwrap()
            .contains("did not start"),
        "{receipt}"
    );
    assert!(!pooled.endpoint_path().exists(), "{receipt}");
    assert!(!pooled.result_path().exists(), "{receipt}");
    assert_eq!(receipt["args"], json!([]), "{receipt}");
    assert!(!pooled.lease_path().exists(), "the lease ends with the run");
}

// --------------------------------------------------------- opt-in native CLI

/// Opt-in check against the installed Codex CLI: the real `codex app-server`
/// accepts this driver's launch command, initialize handshake, `thread/start`
/// binding (cwd, model, provider, effort) and recording, and the recorded
/// endpoint is addressable again for the later `message`/`stop` slices.
///
/// It makes no model request: no turn is submitted, and the provider the
/// configuration names points at a closed loopback port.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned model-free native app-server"]
fn native_app_server_binds_the_resolved_profile_and_records_an_addressable_endpoint() {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    assert!(exe.is_absolute() && exe.is_file(), "{exe:?}");
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let slot = root.path().join("slot");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&slot).unwrap();
    let closed = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let closed_port = closed.local_addr().unwrap().port();
    drop(closed);
    let trusted = serde_json::to_string(&slot.to_string_lossy()).unwrap();
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
name = "Owned control fixture"
base_url = "http://127.0.0.1:{closed_port}/v1"
wire_api = "responses"
env_key = "HARNESS_CONTROL_FIXTURE_KEY"
requires_openai_auth = false
request_max_retries = 0
stream_max_retries = 0
supports_websockets = false
[analytics]
enabled = false
[projects.{trusted}]
trust_level = "trusted"
"#
        ),
    )
    .unwrap();
    let paths = ControlPaths {
        endpoint: root.path().join("control-1.json"),
        token: root.path().join("control-1.token"),
        log: root.path().join("control-1.log"),
    };
    let mut plan = ControlPlan::new(
        &exe,
        &home,
        &slot,
        "native control fixture assignment",
        BoundIdentity {
            profile: "default".to_owned(),
            model: Some("gpt-6-astra".to_owned()),
            model_provider: Some("control_fixture".to_owned()),
            reasoning_effort: Some("low".to_owned()),
        },
        paths,
    );
    plan.approval_policy = Some("never".to_owned());
    plan.sandbox = Some("danger-full-access".to_owned());
    plan.bound = Duration::from_secs(60);
    plan.poll = Duration::from_millis(200);
    for key in [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "CODEX_ACCESS_TOKEN",
        "HARNESS_CONTROL_CODEX_EXE",
    ] {
        plan.env.insert(key.into(), None);
    }
    plan.env.insert(
        "HARNESS_CONTROL_FIXTURE_KEY".into(),
        Some("synthetic-owned-fixture".into()),
    );
    let path = std::env::var_os("PATH").map(|path| {
        std::env::join_paths(std::env::split_paths(&path).filter(|entry| {
            !entry
                .to_string_lossy()
                .to_lowercase()
                .contains("windowsapps")
        }))
        .unwrap()
    });
    plan.env.insert("PATH".into(), path);
    let job = Job::new(Limits::default()).unwrap();
    let mut conversation = Conversation::start(&job, &plan).expect("native app-server startup");
    let thread = conversation.thread_id().to_owned();
    assert!(!thread.is_empty());
    assert!(
        conversation.endpoint().thread_id.as_deref() == Some(thread.as_str()),
        "the recorded endpoint names the started thread"
    );
    let recorded = Endpoint::read(&plan.paths.endpoint).unwrap();
    assert_eq!(recorded.thread_id.as_deref(), Some(thread.as_str()));
    assert_eq!(recorded.token(), conversation.endpoint().token());
    let token = fs::read_to_string(&plan.paths.token).unwrap();
    assert_eq!(token.len(), 64);
    assert!(conversation.process().unwrap().is_running().unwrap());
    // The real CLI announces the thread it just started.
    let observed: Vec<String> = Vec::new();
    let until = Instant::now() + WAIT;
    let native_start = loop {
        if let Some(event) = conversation
            .pump()
            .unwrap()
            .into_iter()
            .find(|event| event.lifecycle == Some(Lifecycle::NativeStart))
        {
            break event;
        }
        assert!(
            Instant::now() < until,
            "the native thread start was never observed; records: {observed:?}"
        );
    };
    assert_eq!(native_start.method.as_deref(), Some("thread/started"));
    // The recorded endpoint answers a second client: the addressing the later
    // `executor message` and `executor stop` slices depend on.
    let mut attached = Conversation::attach(
        &Endpoint::read(&plan.paths.endpoint).unwrap(),
        Duration::from_secs(30),
    )
    .expect("the recorded endpoint is addressable");
    assert_eq!(attached.thread_id(), thread);
    let state = attached
        .call(
            "thread/read",
            json!({"threadId": thread, "includeTurns": true}),
        )
        .unwrap();
    assert_eq!(state["thread"]["id"], thread);
    assert!(
        state["thread"]["status"]["type"].is_string(),
        "the thread reports a native status: {state}"
    );
    drop(attached);
    drop(conversation);
    let _ = job.terminate(0, Duration::from_secs(10));
}
