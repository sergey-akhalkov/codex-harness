#![cfg(windows)]

#[path = "fixtures/control_endpoint.rs"]
mod control_endpoint;

use control_endpoint::{Answer, Bearer, Server};
use harness_core::{process_service::ServiceProcess, task_worktree};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const LEAD: &str = "01a00000-0000-7000-8000-000000000001";
const OTHER_LEAD: &str = "01a00000-0000-7000-8000-000000000002";
const EXECUTOR: &str = "01a00000-0000-7000-8000-000000000003";
const OTHER_EXECUTOR: &str = "01a00000-0000-7000-8000-000000000004";
const SECOND_LEAD: &str = "01a00000-0000-7000-8000-000000000005";
const SECOND_EXECUTOR: &str = "01a00000-0000-7000-8000-000000000006";
const THIRD_EXECUTOR: &str = "01a00000-0000-7000-8000-000000000007";
const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    root: tempfile::TempDir,
}

impl Mcp {
    fn start(home: &Path) -> Self {
        let root = tempfile::tempdir().unwrap();
        let stderr = File::create(root.path().join("stderr")).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["mcp", "executors"])
            .env("CODEX_HOME", home)
            .env_remove("HARNESS_EXECUTOR_SESSION")
            .env_remove("HARNESS_BOUNDED_LEAD_SESSION")
            .env_remove("HARNESS_REFLECTION_SESSION")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut server = Self {
            child,
            stdin,
            stdout,
            root,
        };
        let initialized = server.request(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "executor-mcp-test", "version": "1"}
            }
        }));
        assert_eq!(
            initialized["result"]["serverInfo"]["name"], "executors",
            "{initialized}"
        );
        server.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        server
    }

    fn send(&mut self, value: Value) {
        let mut line = serde_json::to_vec(&value).unwrap();
        line.push(b'\n');
        self.stdin.as_mut().unwrap().write_all(&line).unwrap();
        self.stdin.as_mut().unwrap().flush().unwrap();
    }

    fn request(&mut self, value: Value) -> Value {
        self.send(value);
        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).unwrap();
        if read == 0 {
            panic!(
                "executor MCP server exited before replying: {}",
                self.stderr()
            );
        }
        serde_json::from_str(&line).unwrap()
    }

    fn call(&mut self, id: u64, arguments: Value, metadata: Option<Value>) -> Value {
        self.call_tool(id, "executor_dispatch", arguments, metadata)
    }

    fn call_tool(
        &mut self,
        id: u64,
        tool: &str,
        arguments: Value,
        metadata: Option<Value>,
    ) -> Value {
        let mut params = json!({
            "name": tool,
            "arguments": arguments
        });
        if let Some(metadata) = metadata {
            params["_meta"] = metadata;
        }
        self.request(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": params
        }))
    }

    fn stderr(&self) -> String {
        fs::read_to_string(self.root.path().join("stderr")).unwrap_or_default()
    }

    fn finish(mut self) {
        drop(self.stdin.take());
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "executor MCP stderr: {}", self.stderr());
                return;
            }
            if Instant::now() >= until {
                let _ = self.child.kill();
                panic!("executor MCP server did not exit: {}", self.stderr());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

fn home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("harness/lead-endpoints")).unwrap();
    home
}

fn assignment(objective: &str) -> Value {
    json!({
        "schema": 1,
        "objective": objective,
        "inputs": [],
        "outputs": [],
        "invariants": [],
        "acceptance": ["Return a bounded result"]
    })
}

fn arguments(owner: &str) -> Value {
    json!({"owner": owner, "assignment": assignment("Prove one bounded executor dispatch")})
}

fn endpoint(home: &Path, requested: &str, recorded: &str, port: u16, creation_delta: u64) {
    let program = std::env::current_exe().unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    let identity = ServiceProcess::observe(std::process::id(), &program, 0, &user)
        .unwrap()
        .identity();
    let value = json!({
        "schema": 1,
        "port": port,
        "token": TOKEN,
        "threadId": recorded,
        "process": {
            "pid": identity.pid,
            "creationTime": identity.creation_time + creation_delta,
            "program": program
        }
    });
    fs::write(
        home.join("harness/lead-endpoints")
            .join(format!("{requested}.json")),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
}

fn live_server() -> Server {
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    server.answer("initialize", Answer::Result(json!({})));
    server
}

fn assert_no_effects(home: &Path, server: &Server) {
    assert_eq!(server.connections(), 0, "an unusable identity connected");
    assert!(
        server.requests().is_empty(),
        "an unusable identity sent a native request: {:?}",
        server.requests()
    );
    let pool = home.join("harness/executor-pool");
    assert!(
        !pool.exists() || fs::read_dir(&pool).unwrap().next().is_none(),
        "an unusable identity created pool state at {}",
        pool.display()
    );
}

fn tool_error(response: &Value) -> String {
    assert_eq!(response["result"]["isError"], true, "{response}");
    response["result"]["structuredContent"]["error"]
        .as_str()
        .unwrap_or_else(|| panic!("tool error missing: {response}"))
        .to_owned()
}

fn git_repo(root: &Path) -> String {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("README.md"), "synthetic consumer\n").unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    git(&["init", "-q"]);
    git(&["add", "README.md"]);
    git(&[
        "-c",
        "user.name=Executor MCP Test",
        "-c",
        "user.email=executor-mcp@example.invalid",
        "commit",
        "-q",
        "-m",
        "consumer base",
    ]);
    String::from_utf8(git(&["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned()
}

#[derive(Clone)]
struct SimThread {
    id: String,
    cwd: PathBuf,
    turns: Vec<Value>,
    next_turn: u64,
}

impl SimThread {
    fn idle(id: &str, cwd: &Path) -> Self {
        Self {
            id: id.to_owned(),
            cwd: cwd.to_path_buf(),
            turns: Vec::new(),
            next_turn: 1,
        }
    }

    fn active(id: &str, cwd: &Path, turn: &str) -> Self {
        Self {
            id: id.to_owned(),
            cwd: cwd.to_path_buf(),
            turns: vec![json!({"id": turn, "status": "inProgress", "items": []})],
            next_turn: 2,
        }
    }

    fn state(&self) -> &'static str {
        if self.turns.iter().any(|turn| turn["status"] == "inProgress") {
            "active"
        } else {
            "idle"
        }
    }

    fn record(&self) -> Value {
        json!({
            "id": self.id,
            "status": {"type": self.state()},
            "cwd": self.cwd,
            "model": "test-model",
            "modelProvider": "test-provider",
            "reasoningEffort": "high",
            "turns": self.turns
        })
    }

    fn deliver(&mut self, turn_id: Option<&str>, client_id: &str, text: &str) -> String {
        let turn = match turn_id {
            Some(turn) => turn.to_owned(),
            None => {
                let id = format!("turn-{}", self.next_turn);
                self.next_turn += 1;
                self.turns.push(json!({
                    "id": id,
                    "status": "inProgress",
                    "items": []
                }));
                id
            }
        };
        if let Some(existing) = self
            .turns
            .iter_mut()
            .find(|existing| existing["id"].as_str() == Some(turn.as_str()))
        {
            let items = existing["items"].as_array_mut().expect("items");
            items.push(json!({
                "id": format!("item-{}", items.len() + 1),
                "type": "userMessage",
                "clientId": client_id,
                "content": [{"type": "text", "text": text}]
            }));
        } else {
            return format!("unknown active turn {turn}");
        }
        turn
    }
}

struct NativeSim {
    threads: BTreeMap<String, SimThread>,
    lose_next_ack: Option<String>,
    suppress_next_evidence: bool,
    refuse_turns: bool,
}

impl NativeSim {
    fn new(
        lead: &str,
        executor: &str,
        checkout: &Path,
        lead_active: bool,
        executor_active: bool,
    ) -> Self {
        let mut threads = BTreeMap::new();
        threads.insert(
            lead.to_owned(),
            if lead_active {
                SimThread::active(lead, checkout, "lead-active")
            } else {
                SimThread::idle(lead, checkout)
            },
        );
        threads.insert(
            executor.to_owned(),
            if executor_active {
                SimThread::active(executor, checkout, "executor-active")
            } else {
                SimThread::idle(executor, checkout)
            },
        );
        Self {
            threads,
            lose_next_ack: None,
            suppress_next_evidence: false,
            refuse_turns: false,
        }
    }

    fn set_active(&mut self, thread: &str, active: bool) {
        let current = self.threads.get(thread).expect("thread");
        self.threads.insert(
            thread.to_owned(),
            if active {
                SimThread::active(thread, &current.cwd, &format!("{thread}-active"))
            } else {
                SimThread::idle(thread, &current.cwd)
            },
        );
    }
}

fn native_responder(
    sim: Arc<Mutex<NativeSim>>,
) -> impl Fn(&Value) -> Answer + Send + Sync + 'static {
    move |request| {
        let method = request["method"].as_str().unwrap_or_default();
        match method {
            "initialize" => Answer::Result(json!({})),
            "thread/read" => {
                let id = request["params"]["threadId"].as_str().unwrap_or_default();
                let sim = sim.lock().unwrap();
                match sim.threads.get(id) {
                    Some(thread) => Answer::Result(json!({"thread": thread.record()})),
                    None => Answer::Error(json!({"message": format!("unknown thread {id}")})),
                }
            }
            "turn/steer" | "turn/start" => {
                let mut sim = sim.lock().unwrap();
                if sim.refuse_turns {
                    return Answer::Error(json!({"message": "native refusal"}));
                }
                if sim.lose_next_ack.as_deref() == Some(method) {
                    sim.lose_next_ack = None;
                    return Answer::Record(json!({"id": request["id"].clone()}));
                }
                if sim.suppress_next_evidence {
                    sim.suppress_next_evidence = false;
                    let turn = request["params"]["expectedTurnId"]
                        .as_str()
                        .unwrap_or("queued-turn");
                    return if method == "turn/steer" {
                        Answer::Result(json!({"turnId": turn}))
                    } else {
                        Answer::Result(json!({"turn": {"id": turn, "status": "inProgress"}}))
                    };
                }
                let thread_id = request["params"]["threadId"].as_str().unwrap_or_default();
                let client = request["params"]["clientUserMessageId"]
                    .as_str()
                    .unwrap_or_default();
                let text = request["params"]["input"][0]["text"]
                    .as_str()
                    .unwrap_or_default();
                let Some(thread) = sim.threads.get_mut(thread_id) else {
                    return Answer::Error(
                        json!({"message": format!("unknown thread {thread_id}")}),
                    );
                };
                let expected = request["params"]["expectedTurnId"].as_str();
                let turn = thread.deliver(expected, client, text);
                if method == "turn/steer" {
                    Answer::Result(json!({"turnId": turn}))
                } else {
                    Answer::Result(json!({"turn": {"id": turn, "status": "inProgress"}}))
                }
            }
            _ => Answer::Result(json!({})),
        }
    }
}

struct SyntheticRun {
    _repository: tempfile::TempDir,
    _checkout: tempfile::TempDir,
    receipt: PathBuf,
}

fn synthetic_run(home: &Path, lead: &str, executor: &str, server: &Server) -> SyntheticRun {
    endpoint(home, lead, lead, server.port, 0);
    let endpoint_value: Value = serde_json::from_slice(
        &fs::read(
            home.join("harness/lead-endpoints")
                .join(format!("{lead}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    let repository = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    let base = git_repo(repository.path());
    let slot = 1u32;
    let run = format!("mcp-test-{executor}");
    let state = task_worktree::pool_state_dir(home, repository.path()).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::write(
        state.join(format!("slot-{slot}.json")),
        serde_json::to_vec_pretty(&json!({
            "ownershipSchema": 1,
            "source": repository.path(),
            "index": slot,
            "path": checkout.path(),
            "state": "occupied",
            "owner": run,
            "task": null,
            "handle": null,
            "base": base,
            "disposition": null,
            "reason": null,
            "startup": null,
            "assignment": null
        }))
        .unwrap(),
    )
    .unwrap();
    let receipt = state.join(format!("mcp-{slot}.json"));
    let value = json!({
        "schema": 1,
        "run": run,
        "leadThreadId": lead,
        "executorThreadId": executor,
        "turnId": "initial-turn",
        "title": format!("executor {executor}"),
        "slot": slot,
        "source": repository.path(),
        "repository": repository.path(),
        "poolSize": 1,
        "checkout": checkout.path(),
        "base": base,
        "profile": {
            "profile": "test",
            "model": "test-model",
            "modelProvider": "test-provider",
            "reasoningEffort": "high"
        },
        "observedStatus": "active",
        "allowProviderModelFallback": false,
        "startupReservationReleased": true,
        "lease": {
            "slot": slot,
            "owner": run,
            "checkout": checkout.path(),
            "appServer": endpoint_value["process"]
        }
    });
    fs::write(&receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    SyntheticRun {
        _repository: repository,
        _checkout: checkout,
        receipt,
    }
}

fn result(response: &Value) -> &Value {
    assert_eq!(response["result"]["isError"], false, "{response}");
    &response["result"]["structuredContent"]
}

fn valid_request_shape(id: &str) -> bool {
    id.strip_prefix("lead-").is_some_and(|hex| {
        hex.len() == 24
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[test]
fn absent_and_forged_metadata_fail_before_endpoint_claim_or_thread() {
    let home = home();
    let server = live_server();
    endpoint(home.path(), OTHER_LEAD, OTHER_LEAD, server.port, 0);
    let mut mcp = Mcp::start(home.path());

    let missing = mcp.call(2, arguments("missing-meta"), None);
    assert!(tool_error(&missing).contains("_meta.threadId"), "{missing}");

    let forged = mcp.call(
        3,
        arguments("forged-meta"),
        Some(json!({"threadId": "01a00000-0000-7000-8000-000000000000"})),
    );
    assert!(
        tool_error(&forged).contains("no lead endpoint is registered"),
        "{forged}"
    );
    assert_no_effects(home.path(), &server);
    mcp.finish();
}

#[test]
fn a_mismatched_endpoint_record_is_not_an_address() {
    let home = home();
    let server = live_server();
    endpoint(home.path(), LEAD, OTHER_LEAD, server.port, 0);
    let mut mcp = Mcp::start(home.path());

    let response = mcp.call(
        2,
        arguments("mismatched-endpoint"),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&response).contains("belongs to another conversation"),
        "{response}"
    );
    assert_no_effects(home.path(), &server);
    mcp.finish();
}

#[test]
fn a_dead_app_server_is_refused_before_effects() {
    let home = home();
    let server = live_server();
    endpoint(home.path(), LEAD, LEAD, server.port, 1);
    let mut mcp = Mcp::start(home.path());

    let response = mcp.call(
        2,
        arguments("dead-endpoint"),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&response).contains("no longer live"),
        "{response}"
    );
    assert_no_effects(home.path(), &server);
    mcp.finish();
}

#[test]
fn forged_arguments_cannot_name_another_lead_or_run() {
    let home = home();
    let server = live_server();
    endpoint(home.path(), LEAD, LEAD, server.port, 0);
    let mut mcp = Mcp::start(home.path());

    for (id, field, value) in [
        (2, "leadThreadId", json!(OTHER_LEAD)),
        (3, "run", json!("another-lead-run")),
        (4, "threadId", json!(OTHER_LEAD)),
    ] {
        let mut forged = arguments("argument-authority");
        forged[field] = value;
        let response = mcp.call(id, forged, Some(json!({"threadId": LEAD})));
        assert!(
            tool_error(&response).contains("unknown field"),
            "{field}: {response}"
        );
    }
    assert_no_effects(home.path(), &server);
    mcp.finish();
}

#[test]
fn invalid_assignment_fails_before_claim_or_thread_creation() {
    let home = home();
    let server = live_server();
    endpoint(home.path(), LEAD, LEAD, server.port, 0);
    let mut mcp = Mcp::start(home.path());

    let mut invalid = arguments("invalid-assignment");
    invalid["assignment"]["objective"] = json!("");
    let response = mcp.call(2, invalid, Some(json!({"threadId": LEAD})));
    assert!(tool_error(&response).contains("objective"), "{response}");
    assert_no_effects(home.path(), &server);
    mcp.finish();
}

#[test]
fn installed_kit_source_and_consumer_repository_stay_separate_without_an_owner() {
    let root = tempfile::tempdir().unwrap();
    let kit = root.path().join("kit");
    let consumer = root.path().join("consumer");
    fs::create_dir_all(kit.join("global")).unwrap();
    fs::write(
        kit.join("global/orchestration.toml"),
        concat!(
            "schema = 1\n",
            "lead_profile = \"default\"\n",
            "successor_lead_profile = \"default\"\n",
            "executor_profiles = [\"configured\"]\n",
            "max_concurrent_executors = 2\n",
            "worktree_pool_size = 2\n",
            "vote_threshold = 2\n",
            "incubator_size_cap = 2\n",
            "feedback_batch_limit = 1\n",
            "worktree_limit = 2\n",
        ),
    )
    .unwrap();
    assert!(!consumer.join("global/orchestration.toml").exists());
    let base = git_repo(&consumer);

    let home = home();
    fs::write(
        home.path().join("harness/installation.json"),
        serde_json::to_vec_pretty(&json!({
            "schemaVersion": 2,
            "settings": {"sourceRoot": kit}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        home.path().join("configured.config.toml"),
        "model = \"test-model\"\nmodel_provider = \"test-provider\"\nmodel_reasoning_effort = \"high\"\n",
    )
    .unwrap();

    let server = live_server();
    server.answer(
        "thread/read",
        Answer::Result(json!({
            "thread": {
                "id": LEAD,
                "status": {"type": "idle"},
                "cwd": consumer,
                "name": null
            }
        })),
    );
    endpoint(home.path(), LEAD, LEAD, server.port, 0);
    let mut mcp = Mcp::start(home.path());
    let response = mcp.call(
        2,
        json!({
            "base": base,
            "assignment": assignment(
                "Run git rev-parse HEAD in the consumer pool and make no changes."
            )
        }),
        Some(json!({"threadId": LEAD})),
    );
    let error = tool_error(&response);
    assert!(
        error.contains("native launch registration") || error.contains("role context"),
        "the kit source should have reached role resolution: {error}"
    );
    assert!(
        !error.contains("orchestration configuration"),
        "the consumer repository must not be read as the kit source: {error}"
    );
    assert!(
        server.requests_for("thread/start").is_empty(),
        "role resolution precedes thread creation"
    );

    let state = task_worktree::pool_state_dir(home.path(), &consumer).unwrap();
    let records = fs::read_dir(&state)
        .unwrap()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("slot-") && name.ends_with(".json")
        })
        .map(|entry| serde_json::from_slice::<Value>(&fs::read(entry.path()).unwrap()).unwrap())
        .collect::<Vec<_>>();
    assert!(
        records.iter().any(|record| {
            record["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("consumer-wt1"))
        }),
        "the claimed pool worktree must be a sibling of the consumer: {records:?}"
    );
    mcp.finish();
}

#[test]
fn mcp_tools_declare_closed_schemas_with_scoped_run_selection() {
    let home = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(home.path());
    let listed = mcp.request(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    }));
    let tools = listed["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for required in [
        "executor_dispatch",
        "executor_status",
        "executor_message",
        "executor_reply",
        "executor_ask_lead",
    ] {
        assert!(names.contains(&required), "{listed}");
    }
    for tool in tools {
        assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{tool}");
        let properties = tool["inputSchema"]["properties"].as_object().unwrap();
        for forbidden in ["threadId", "leadThreadId", "endpoint", "recipient"] {
            assert!(
                !properties.contains_key(forbidden),
                "{} exposes {forbidden}: {tool}",
                tool["name"]
            );
        }
        if tool["name"] != "executor_message" {
            assert!(
                !properties.contains_key("run"),
                "{} must not expose run: {tool}",
                tool["name"]
            );
        } else {
            assert!(
                properties.contains_key("run"),
                "executor_message must expose run: {tool}"
            );
        }
    }
    let missing = mcp.call_tool(3, "executor_status", json!({}), None);
    assert!(tool_error(&missing).contains("_meta.threadId"), "{missing}");
    mcp.finish();
}

#[test]
fn status_is_bounded_to_the_verified_calling_thread_and_rejects_forged_arguments() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let second_run = synthetic_run(home.path(), SECOND_LEAD, SECOND_EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        run._checkout.path(),
        true,
        false,
    )));
    sim.lock().unwrap().threads.insert(
        SECOND_LEAD.to_owned(),
        SimThread::idle(SECOND_LEAD, second_run._checkout.path()),
    );
    sim.lock().unwrap().threads.insert(
        SECOND_EXECUTOR.to_owned(),
        SimThread::idle(SECOND_EXECUTOR, second_run._checkout.path()),
    );
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let lead_status = mcp.call_tool(
        2,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    let lead_status = result(&lead_status);
    assert_eq!(lead_status["role"], "lead");
    assert_eq!(lead_status["runs"].as_array().unwrap().len(), 1);
    assert_eq!(lead_status["runs"][0]["executorThreadId"], EXECUTOR);
    assert_eq!(lead_status["runs"][0]["native"]["status"], "active");
    assert_eq!(lead_status["runs"][0]["executorNative"]["status"], "idle");
    assert_eq!(
        lead_status["runs"][0]["pendingRequests"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let executor_status = mcp.call_tool(
        3,
        "executor_status",
        json!({}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let executor_status = result(&executor_status);
    assert_eq!(executor_status["role"], "executor");
    assert_eq!(
        executor_status["runs"][0]["run"],
        "mcp-test-01a00000-0000-7000-8000-000000000003"
    );

    let second_status = mcp.call_tool(
        4,
        "executor_status",
        json!({}),
        Some(json!({"threadId": SECOND_LEAD})),
    );
    let second_status = result(&second_status);
    assert_eq!(second_status["runs"].as_array().unwrap().len(), 1);
    assert_eq!(
        second_status["runs"][0]["executorThreadId"],
        SECOND_EXECUTOR
    );
    let sibling_status = mcp.call_tool(
        5,
        "executor_status",
        json!({}),
        Some(json!({"threadId": SECOND_EXECUTOR})),
    );
    let sibling_status = result(&sibling_status);
    assert_eq!(sibling_status["runs"].as_array().unwrap().len(), 1);
    assert_eq!(
        sibling_status["runs"][0]["executorThreadId"],
        SECOND_EXECUTOR
    );

    let foreign = mcp.call_tool(
        6,
        "executor_status",
        json!({}),
        Some(json!({"threadId": OTHER_LEAD})),
    );
    let foreign = result(&foreign);
    assert_eq!(foreign["state"], "empty");
    assert!(foreign["runs"].as_array().unwrap().is_empty());

    let sibling = mcp.call_tool(
        7,
        "executor_status",
        json!({}),
        Some(json!({"threadId": OTHER_EXECUTOR})),
    );
    assert_eq!(result(&sibling)["state"], "empty");

    let forged = mcp.call_tool(
        8,
        "executor_status",
        json!({"run": "mcp-test-01a00000-0000-7000-8000-000000000003"}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(tool_error(&forged).contains("unknown field"), "{forged}");

    let forged_message = mcp.call_tool(
        9,
        "executor_message",
        json!({"text": "hello", "threadId": EXECUTOR}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&forged_message).contains("unknown field"),
        "{forged_message}"
    );

    let ambiguous = synthetic_run(home.path(), LEAD, THIRD_EXECUTOR, &server);
    let before = server.requests_for("turn/start").len() + server.requests_for("turn/steer").len();
    let refused = mcp.call_tool(
        10,
        "executor_message",
        json!({"text": "which run?"}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(tool_error(&refused).contains("ambiguous"), "{refused}");
    assert_eq!(
        server.requests_for("turn/start").len() + server.requests_for("turn/steer").len(),
        before
    );

    drop(run);
    drop(second_run);
    drop(ambiguous);
    mcp.finish();
}

#[test]
fn questions_and_replies_use_active_steering_and_idle_turns_in_both_directions() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        run._checkout.path(),
        true,
        false,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let asked = mcp.call_tool(
        2,
        "executor_ask_lead",
        json!({"text": "Which migration order?", "clientMessageId": "question-one"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let asked = result(&asked);
    assert_eq!(asked["delivery"]["status"], "delivered");
    assert_eq!(asked["awaitingReply"], true);
    let request = asked["requestId"].as_str().unwrap().to_owned();
    assert_eq!(server.requests_for("turn/steer").len(), 1);
    assert!(server.requests_for("turn/start").is_empty());
    let steer = &server.requests_for("turn/steer")[0];
    let steer_text = steer["params"]["input"][0]["text"].as_str().unwrap();
    assert!(steer_text.contains(&request), "{steer_text}");
    assert!(
        steer_text.contains("Which migration order?"),
        "{steer_text}"
    );

    let answered = mcp.call_tool(
        3,
        "executor_reply",
        json!({"requestId": request, "text": "Schema first."}),
        Some(json!({"threadId": LEAD})),
    );
    let answered = result(&answered);
    assert_eq!(answered["delivery"]["status"], "delivered");
    assert_eq!(answered["resolved"], true);
    assert_eq!(server.requests_for("turn/start").len(), 1);
    let reply = &server.requests_for("turn/start")[0];
    let reply_text = reply["params"]["input"][0]["text"].as_str().unwrap();
    assert!(reply_text.contains("Schema first."), "{reply_text}");

    let receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert_eq!(receipt["leadMessages"][0]["status"], "resolved");
    assert_eq!(receipt["messages"][0]["status"], "delivered");
    assert_eq!(receipt["messages"][0]["replyTo"], request);

    let lead_pending = mcp.call_tool(
        4,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(
        result(&lead_pending)["runs"][0]["pendingRequests"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    sim.lock().unwrap().set_active(LEAD, false);
    sim.lock().unwrap().set_active(EXECUTOR, true);
    let starts = server.requests_for("turn/start").len();
    let steers = server.requests_for("turn/steer").len();
    let second = mcp.call_tool(
        5,
        "executor_ask_lead",
        json!({"text": "Is the idle path visible?", "clientMessageId": "question-two"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let second = result(&second);
    assert_eq!(second["delivery"]["status"], "delivered");
    assert_eq!(server.requests_for("turn/start").len(), starts + 1);
    assert_eq!(server.requests_for("turn/steer").len(), steers);
    let second_request = second["requestId"].as_str().unwrap().to_owned();

    let second_reply = mcp.call_tool(
        6,
        "executor_reply",
        json!({"requestId": second_request, "text": "Yes, as a native turn."}),
        Some(json!({"threadId": LEAD})),
    );
    let second_reply = result(&second_reply);
    assert_eq!(second_reply["delivery"]["status"], "delivered");
    assert_eq!(second_reply["resolved"], true);
    assert_eq!(server.requests_for("turn/steer").len(), steers + 1);

    drop(run);
    mcp.finish();
}

#[test]
fn duplicate_payloads_are_not_repeated_but_identical_intentional_messages_are_distinct() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        run._checkout.path(),
        false,
        false,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let first = mcp.call_tool(
        2,
        "executor_message",
        json!({"text": "one correction", "clientMessageId": "message-one"}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&first)["delivery"]["status"], "delivered");
    assert_eq!(server.requests_for("turn/start").len(), 1);

    let duplicate = mcp.call_tool(
        3,
        "executor_message",
        json!({"text": "one correction", "clientMessageId": "message-one"}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&duplicate)["delivery"]["status"], "delivered");
    assert_eq!(server.requests_for("turn/start").len(), 1);
    assert_eq!(server.requests_for("turn/steer").len(), 0);

    let distinct = mcp.call_tool(
        4,
        "executor_message",
        json!({"text": "one correction", "clientMessageId": "message-two"}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&distinct)["delivery"]["status"], "delivered");
    assert_eq!(server.requests_for("turn/steer").len(), 1);

    let large = format!(
        "{}\n{}",
        "日本語 кариллица \"quoted\" $(not-evaluated) %PATH%".repeat(180),
        "final line with a literal newline"
    );
    let large_response = mcp.call_tool(
        5,
        "executor_message",
        json!({"text": large, "clientMessageId": "message-large"}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&large_response)["delivery"]["status"], "delivered");
    let sent = server
        .requests_for("turn/steer")
        .last()
        .and_then(|request| request["params"]["input"][0]["text"].as_str())
        .unwrap()
        .to_owned();
    assert_eq!(sent, large);

    sim.lock().unwrap().lose_next_ack = Some("turn/steer".to_owned());
    let lost = mcp.call_tool(
        6,
        "executor_message",
        json!({"text": "may have arrived", "clientMessageId": "message-lost"}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(tool_error(&lost).contains("unknown"), "{lost}");
    let after_loss = server.requests_for("turn/steer").len();
    let repeat = mcp.call_tool(
        7,
        "executor_message",
        json!({"text": "may have arrived", "clientMessageId": "message-lost"}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(tool_error(&repeat).contains("unknown"), "{repeat}");
    assert_eq!(server.requests_for("turn/steer").len(), after_loss);
    let receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert_eq!(
        receipt["messages"].as_array().unwrap().last().unwrap()["status"],
        "indeterminate"
    );

    drop(run);
    mcp.finish();
}

#[test]
fn two_questions_correlate_and_stale_or_sibling_identities_fail_before_send() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        run._checkout.path(),
        false,
        false,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let first = mcp.call_tool(
        2,
        "executor_ask_lead",
        json!({"text": "first question", "clientMessageId": "q-one"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let first_id = result(&first)["requestId"].as_str().unwrap().to_owned();
    let second = mcp.call_tool(
        3,
        "executor_ask_lead",
        json!({"text": "second question", "clientMessageId": "q-two"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let second_id = result(&second)["requestId"].as_str().unwrap().to_owned();
    assert_ne!(first_id, second_id);
    let pending = mcp.call_tool(
        4,
        "executor_status",
        json!({}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let pending = result(&pending);
    let pending_requests = pending["runs"][0]["pendingRequests"].as_array().unwrap();
    assert_eq!(pending_requests.len(), 2, "{pending}");

    let first_reply = mcp.call_tool(
        5,
        "executor_reply",
        json!({"requestId": first_id, "text": "first answer"}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&first_reply)["resolved"], true);
    let pending = mcp.call_tool(
        6,
        "executor_status",
        json!({}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let pending = result(&pending);
    let pending_requests = pending["runs"][0]["pendingRequests"].as_array().unwrap();
    assert_eq!(pending_requests.len(), 1, "{pending}");
    assert_eq!(pending_requests[0]["requestId"], second_id);

    let notice = mcp.call_tool(
        20,
        "executor_message",
        json!({"text": "progress without a hold", "clientMessageId": "notice-one"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let notice = result(&notice);
    assert_eq!(notice["delivery"]["status"], "delivered");
    assert_eq!(notice["awaitingReply"], false);
    let still_pending = mcp.call_tool(
        21,
        "executor_status",
        json!({}),
        Some(json!({"threadId": EXECUTOR})),
    );
    assert_eq!(
        result(&still_pending)["runs"][0]["pendingRequests"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert_eq!(
        receipt["leadMessages"].as_array().unwrap().last().unwrap()["kind"],
        "notification"
    );

    sim.lock().unwrap().lose_next_ack = Some("turn/steer".to_owned());
    let lost = mcp.call_tool(
        7,
        "executor_ask_lead",
        json!({"text": "third question", "clientMessageId": "q-three"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    assert!(tool_error(&lost).contains("unknown"), "{lost}");
    let sends = server.requests_for("turn/steer").len();
    let repeat = mcp.call_tool(
        8,
        "executor_ask_lead",
        json!({"text": "third question", "clientMessageId": "q-three"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    assert!(tool_error(&repeat).contains("unknown"), "{repeat}");
    assert_eq!(server.requests_for("turn/steer").len(), sends);

    let endpoint_path = home
        .path()
        .join("harness/lead-endpoints")
        .join(format!("{LEAD}.json"));
    let saved_endpoint = fs::read(&endpoint_path).unwrap();
    fs::remove_file(&endpoint_path).unwrap();
    let absent = mcp.call_tool(
        9,
        "executor_ask_lead",
        json!({"text": "queued while absent", "clientMessageId": "q-absent"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let absent_text = tool_error(&absent);
    assert!(
        absent_text.contains("no verified live endpoint"),
        "{absent}"
    );
    let absent_id = absent["result"]["structuredContent"]["requestId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(valid_request_shape(&absent_id), "{absent_id}");
    fs::write(&endpoint_path, saved_endpoint).unwrap();
    let recovered = mcp.call_tool(
        10,
        "executor_ask_lead",
        json!({"text": "queued while absent", "clientMessageId": "q-absent"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    assert_eq!(result(&recovered)["delivery"]["status"], "delivered");
    assert_eq!(result(&recovered)["requestId"], absent_id);

    let before_sibling = server.requests_for("turn/steer").len();
    let sibling = mcp.call_tool(
        11,
        "executor_reply",
        json!({"requestId": second_id, "text": "sibling override"}),
        Some(json!({"threadId": OTHER_LEAD})),
    );
    assert!(
        tool_error(&sibling).contains("not the recorded lead or executor"),
        "{sibling}"
    );
    assert_eq!(server.requests_for("turn/steer").len(), before_sibling);

    let unknown_request = mcp.call_tool(
        13,
        "executor_reply",
        json!({"requestId": "lead-000000000000000000000000", "text": "stale"}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&unknown_request).contains("is not recorded"),
        "{unknown_request}"
    );
    assert_eq!(server.requests_for("turn/steer").len(), before_sibling);

    let mut receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    receipt["lease"]["appServer"]["creationTime"] = json!(
        receipt["lease"]["appServer"]["creationTime"]
            .as_u64()
            .unwrap()
            + 1
    );
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let stale = mcp.call_tool(
        12,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&stale).contains("no longer matches the exact run lease"),
        "{stale}"
    );
    assert_eq!(server.requests_for("turn/steer").len(), before_sibling);

    drop(run);
    mcp.finish();
}

#[test]
fn questions_are_bound_to_their_exact_run_across_two_leads() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let first_run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let second_run = synthetic_run(home.path(), SECOND_LEAD, SECOND_EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        first_run._checkout.path(),
        false,
        false,
    )));
    sim.lock().unwrap().threads.insert(
        SECOND_LEAD.to_owned(),
        SimThread::idle(SECOND_LEAD, second_run._checkout.path()),
    );
    sim.lock().unwrap().threads.insert(
        SECOND_EXECUTOR.to_owned(),
        SimThread::idle(SECOND_EXECUTOR, second_run._checkout.path()),
    );
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let first = mcp.call_tool(
        2,
        "executor_ask_lead",
        json!({"text": "same question", "clientMessageId": "same-id"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let first_id = result(&first)["requestId"].as_str().unwrap().to_owned();
    let second = mcp.call_tool(
        3,
        "executor_ask_lead",
        json!({"text": "same question", "clientMessageId": "same-id"}),
        Some(json!({"threadId": SECOND_EXECUTOR})),
    );
    let second_id = result(&second)["requestId"].as_str().unwrap().to_owned();
    assert_ne!(first_id, second_id);

    let first_status = mcp.call_tool(
        4,
        "executor_status",
        json!({}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let first_requests = result(&first_status)["runs"][0]["pendingRequests"]
        .as_array()
        .unwrap();
    assert_eq!(first_requests.len(), 1);
    assert_eq!(first_requests[0]["requestId"], first_id);
    let second_status = mcp.call_tool(
        5,
        "executor_status",
        json!({}),
        Some(json!({"threadId": SECOND_EXECUTOR})),
    );
    let second_requests = result(&second_status)["runs"][0]["pendingRequests"]
        .as_array()
        .unwrap();
    assert_eq!(second_requests.len(), 1);
    assert_eq!(second_requests[0]["requestId"], second_id);

    let foreign_reply = mcp.call_tool(
        6,
        "executor_reply",
        json!({"requestId": first_id, "text": "wrong lead"}),
        Some(json!({"threadId": SECOND_LEAD})),
    );
    assert!(
        tool_error(&foreign_reply).contains("not the recorded lead or executor"),
        "{foreign_reply}"
    );
    let reply = mcp.call_tool(
        7,
        "executor_reply",
        json!({"requestId": first_id, "text": "first answer"}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&reply)["resolved"], true);
    let first_status = mcp.call_tool(
        8,
        "executor_status",
        json!({}),
        Some(json!({"threadId": EXECUTOR})),
    );
    assert!(
        result(&first_status)["runs"][0]["pendingRequests"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let second_status = mcp.call_tool(
        9,
        "executor_status",
        json!({}),
        Some(json!({"threadId": SECOND_EXECUTOR})),
    );
    let second_requests = result(&second_status)["runs"][0]["pendingRequests"]
        .as_array()
        .unwrap();
    assert_eq!(second_requests.len(), 1);
    assert_eq!(second_requests[0]["requestId"], second_id);

    drop(first_run);
    drop(second_run);
    mcp.finish();
}

#[test]
fn queue_acceptance_and_native_refusal_are_classified_without_claiming_delivery() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        run._checkout.path(),
        false,
        false,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    sim.lock().unwrap().suppress_next_evidence = true;
    let queued = mcp.call_tool(
        2,
        "executor_message",
        json!({"text": "accepted but not observed", "clientMessageId": "queued-one"}),
        Some(json!({"threadId": LEAD})),
    );
    let queued = result(&queued);
    assert_eq!(queued["delivery"]["status"], "queued");
    assert_eq!(queued["delivery"]["evidence"], Value::Null);
    let sends = server.requests_for("turn/start").len();
    let repeat = mcp.call_tool(
        3,
        "executor_message",
        json!({"text": "accepted but not observed", "clientMessageId": "queued-one"}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&repeat)["delivery"]["status"], "queued");
    assert_eq!(server.requests_for("turn/start").len(), sends);

    sim.lock().unwrap().refuse_turns = true;
    let refused = mcp.call_tool(
        4,
        "executor_message",
        json!({"text": "refused by the endpoint", "clientMessageId": "refused-one"}),
        Some(json!({"threadId": LEAD})),
    );
    let refused_text = tool_error(&refused);
    assert!(refused_text.contains("refused after 3 rounds"), "{refused}");
    assert_eq!(server.requests_for("turn/start").len(), sends + 3);

    sim.lock().unwrap().refuse_turns = false;
    let retried = mcp.call_tool(
        5,
        "executor_message",
        json!({"text": "refused by the endpoint", "clientMessageId": "refused-one"}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&retried)["delivery"]["status"], "delivered");
    assert_eq!(server.requests_for("turn/start").len(), sends + 4);

    drop(run);
    mcp.finish();
}

#[test]
fn a_lead_selects_one_run_and_mismatched_or_foreign_targets_have_no_effect() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let first_run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let second_run = synthetic_run(home.path(), LEAD, SECOND_EXECUTOR, &server);
    let foreign_run = synthetic_run(home.path(), OTHER_LEAD, THIRD_EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        first_run._checkout.path(),
        false,
        false,
    )));
    sim.lock().unwrap().threads.insert(
        SECOND_EXECUTOR.to_owned(),
        SimThread::idle(SECOND_EXECUTOR, second_run._checkout.path()),
    );
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let status = mcp.call_tool(
        2,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    let status = result(&status);
    let runs = status["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2, "{status}");
    let first_id = runs
        .iter()
        .find(|run| run["executorThreadId"] == EXECUTOR)
        .unwrap()["run"]
        .as_str()
        .unwrap()
        .to_owned();
    let second_id = runs
        .iter()
        .find(|run| run["executorThreadId"] == SECOND_EXECUTOR)
        .unwrap()["run"]
        .as_str()
        .unwrap()
        .to_owned();

    let turns = |thread: &str| {
        server
            .requests_for("turn/start")
            .into_iter()
            .chain(server.requests_for("turn/steer"))
            .filter(|request| request["params"]["threadId"].as_str() == Some(thread))
            .count()
    };

    let ambiguous = mcp.call_tool(
        3,
        "executor_message",
        json!({"text": "which one?"}),
        Some(json!({"threadId": LEAD})),
    );
    let ambiguous_text = tool_error(&ambiguous);
    assert!(ambiguous_text.contains("ambiguous"), "{ambiguous}");
    assert!(ambiguous_text.contains(&first_id), "{ambiguous}");
    assert!(ambiguous_text.contains(&second_id), "{ambiguous}");
    assert_eq!(turns(EXECUTOR), 0);
    assert_eq!(turns(SECOND_EXECUTOR), 0);

    let selected = mcp.call_tool(
        4,
        "executor_message",
        json!({"text": "second only", "run": second_id}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&selected)["delivery"]["status"], "delivered");
    assert_eq!(turns(EXECUTOR), 0);
    assert_eq!(turns(SECOND_EXECUTOR), 1);
    let first_receipt: Value =
        serde_json::from_slice(&fs::read(&first_run.receipt).unwrap()).unwrap();
    assert!(
        first_receipt
            .get("messages")
            .is_none_or(|messages| messages.as_array().unwrap().is_empty()),
        "{first_receipt}"
    );
    let second_receipt: Value =
        serde_json::from_slice(&fs::read(&second_run.receipt).unwrap()).unwrap();
    assert_eq!(second_receipt["messages"].as_array().unwrap().len(), 1);

    let before = turns(SECOND_EXECUTOR) + turns(EXECUTOR);
    let unknown = mcp.call_tool(
        5,
        "executor_message",
        json!({"text": "unknown", "run": "missing-run"}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&unknown).contains("is owned by the calling lead"),
        "{unknown}"
    );
    let foreign = mcp.call_tool(
        6,
        "executor_message",
        json!({"text": "foreign", "run": format!("mcp-test-{THIRD_EXECUTOR}")}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&foreign).contains("is owned by the calling lead"),
        "{foreign}"
    );
    assert_eq!(turns(SECOND_EXECUTOR) + turns(EXECUTOR), before);

    let asked = mcp.call_tool(
        7,
        "executor_ask_lead",
        json!({"text": "first run question", "clientMessageId": "run-question"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let request_id = result(&asked)["requestId"].as_str().unwrap().to_owned();
    let before_mismatch = turns(SECOND_EXECUTOR) + turns(EXECUTOR);
    let mismatch = mcp.call_tool(
        8,
        "executor_message",
        json!({"text": "mismatch", "run": second_id, "requestId": request_id}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&mismatch).contains("name different runs"),
        "{mismatch}"
    );
    assert_eq!(turns(SECOND_EXECUTOR) + turns(EXECUTOR), before_mismatch);

    drop(first_run);
    drop(second_run);
    drop(foreign_run);
    mcp.finish();
}

#[test]
fn repeated_question_text_is_distinct_with_a_new_client_id_and_stable_when_omitted() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        run._checkout.path(),
        false,
        false,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let first = mcp.call_tool(
        2,
        "executor_ask_lead",
        json!({"text": "same question", "clientMessageId": "question-a"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let first_id = result(&first)["requestId"].as_str().unwrap().to_owned();
    let second = mcp.call_tool(
        3,
        "executor_ask_lead",
        json!({"text": "same question", "clientMessageId": "question-b"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let second_id = result(&second)["requestId"].as_str().unwrap().to_owned();
    assert_ne!(first_id, second_id);

    let omitted = mcp.call_tool(
        4,
        "executor_ask_lead",
        json!({"text": "same question"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let omitted_id = result(&omitted)["requestId"].as_str().unwrap().to_owned();
    assert_ne!(omitted_id, first_id);
    assert_ne!(omitted_id, second_id);
    let sends = server.requests_for("turn/start").len() + server.requests_for("turn/steer").len();
    let repeat = mcp.call_tool(
        5,
        "executor_ask_lead",
        json!({"text": "same question"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let repeat = result(&repeat);
    assert_eq!(repeat["requestId"], omitted_id);
    assert_eq!(repeat["delivery"]["status"], "delivered");
    assert_eq!(
        server.requests_for("turn/start").len() + server.requests_for("turn/steer").len(),
        sends
    );
    let status = mcp.call_tool(
        6,
        "executor_status",
        json!({}),
        Some(json!({"threadId": EXECUTOR})),
    );
    assert_eq!(
        result(&status)["runs"][0]["pendingRequests"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    drop(run);
    mcp.finish();
}
