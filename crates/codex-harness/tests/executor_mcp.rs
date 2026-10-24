#![cfg(windows)]

#[path = "fixtures/control_endpoint.rs"]
mod control_endpoint;
#[path = "fixtures/synthetic_role_build.rs"]
mod synthetic_role_build;

use control_endpoint::{Answer, Bearer, Server};
use harness_core::{process_service::ServiceProcess, task_worktree};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Barrier, Mutex},
    time::{Duration, Instant},
};

const LEAD: &str = "01a00000-0000-7000-8000-000000000001";
const DIRECTOR: &str = "01a00000-0000-7000-8000-00000000000d";
const OTHER_DIRECTOR: &str = "01a00000-0000-7000-8000-00000000000e";
const OTHER_LEAD: &str = "01a00000-0000-7000-8000-000000000002";
const EXECUTOR: &str = "01a00000-0000-7000-8000-000000000003";
const OTHER_EXECUTOR: &str = "01a00000-0000-7000-8000-000000000004";
const SECOND_LEAD: &str = "01a00000-0000-7000-8000-000000000005";
const SECOND_EXECUTOR: &str = "01a00000-0000-7000-8000-000000000006";
const THIRD_EXECUTOR: &str = "01a00000-0000-7000-8000-000000000007";
const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ALL_TOOLS: [&str; 10] = [
    "executor_ask_lead",
    "executor_dispatch",
    "executor_message",
    "executor_release",
    "executor_reply",
    "executor_resume",
    "executor_status",
    "executor_stop",
    "executor_wait",
    "lead_dispatch",
];
const EXECUTOR_ROLE_TOOLS: [&str; 3] = ["executor_ask_lead", "executor_message", "executor_status"];
const LEAD_ROLE_TOOLS: [&str; 9] = [
    "executor_ask_lead",
    "executor_dispatch",
    "executor_message",
    "executor_release",
    "executor_reply",
    "executor_resume",
    "executor_status",
    "executor_stop",
    "executor_wait",
];

struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    root: tempfile::TempDir,
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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
        self.read()
    }

    fn read(&mut self) -> Value {
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

    /// Closes the MCP transport by ending its process, without an orderly
    /// stdio shutdown. The recorded runs and their shared app-server remain.
    fn disconnect(mut self) {
        self.child.kill().unwrap();
        let _ = self.child.wait();
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
    let structured = &response["result"]["structuredContent"];
    structured["error"]
        .as_str()
        .or_else(|| structured["result"]["error"].as_str())
        .or_else(|| {
            structured["results"]
                .as_array()
                .and_then(|results| results.iter().find_map(|result| result["error"].as_str()))
        })
        .unwrap_or_else(|| panic!("tool error missing: {response}"))
        .to_owned()
}

fn above_one_mebibyte() -> String {
    let text = "日本語 кариллица \"quoted\" $(literal) %PATH%\n".repeat(40 * 1024);
    assert!(text.len() > 1024 * 1024, "{}", text.len());
    text
}

fn assert_no_file_relay(home: &Path) {
    let messages = home.join("harness/messages");
    assert!(
        !messages.exists() || fs::read_dir(&messages).unwrap().next().is_none(),
        "MCP delivery wrote a file relay under {}",
        messages.display()
    );
}

fn turn_requests(server: &Server) -> usize {
    server.requests_for("turn/start").len() + server.requests_for("turn/steer").len()
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

fn git_head(repository: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git rev-parse: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn workspace_slot_record(home: &Path, repository: &Path, index: u32) -> Value {
    let path = task_worktree::pool_state_dir(home, repository)
        .unwrap()
        .join(format!("slot-{index}.json"));
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[derive(Clone)]
struct SimThread {
    id: String,
    cwd: PathBuf,
    workspace_roots: Vec<PathBuf>,
    name: Option<String>,
    model: String,
    model_provider: String,
    reasoning_effort: String,
    tools: Vec<String>,
    turns: Vec<Value>,
    next_turn: u64,
}

impl SimThread {
    fn idle(id: &str, cwd: &Path) -> Self {
        Self {
            id: id.to_owned(),
            cwd: cwd.to_path_buf(),
            workspace_roots: Vec::new(),
            name: None,
            model: "test-model".into(),
            model_provider: "test-provider".into(),
            reasoning_effort: "high".into(),
            tools: ALL_TOOLS.iter().map(|tool| (*tool).to_owned()).collect(),
            turns: Vec::new(),
            next_turn: 1,
        }
    }

    fn active(id: &str, cwd: &Path, turn: &str) -> Self {
        Self {
            id: id.to_owned(),
            cwd: cwd.to_path_buf(),
            workspace_roots: Vec::new(),
            name: None,
            model: "test-model".into(),
            model_provider: "test-provider".into(),
            reasoning_effort: "high".into(),
            tools: ALL_TOOLS.iter().map(|tool| (*tool).to_owned()).collect(),
            turns: vec![json!({"id": turn, "status": "inProgress", "items": []})],
            next_turn: 2,
        }
    }

    fn with_ds(mut self) -> Self {
        self.model = "deepseek-flash".into();
        self.model_provider = "deepseek".into();
        self.reasoning_effort = "max".into();
        self
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
            "name": self.name,
            "model": self.model,
            "modelProvider": self.model_provider,
            "reasoningEffort": self.reasoning_effort,
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

#[derive(Clone)]
struct SimTerminal {
    process_id: String,
    item_id: String,
    command: String,
}

struct NativeSim {
    threads: BTreeMap<String, SimThread>,
    terminals: BTreeMap<String, Vec<SimTerminal>>,
    read_error: Option<String>,
    lose_next_ack: Option<String>,
    suppress_next_evidence: bool,
    refuse_turns: bool,
    interrupt_completes: bool,
    terminate_terminals: bool,
    post_interrupt_terminal: Option<SimTerminal>,
    next_thread: u64,
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
            terminals: BTreeMap::new(),
            read_error: None,
            lose_next_ack: None,
            suppress_next_evidence: false,
            refuse_turns: false,
            interrupt_completes: false,
            terminate_terminals: true,
            post_interrupt_terminal: None,
            next_thread: 1,
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

    fn interrupt(&mut self, thread: &str, turn: &str) -> Result<(), String> {
        let completes = self.interrupt_completes;
        let Some(thread) = self.threads.get_mut(thread) else {
            return Err(format!("unknown thread {thread}"));
        };
        let Some(active) = thread
            .turns
            .iter_mut()
            .find(|active| active["id"].as_str() == Some(turn))
        else {
            return Err(format!("unknown active turn {turn}"));
        };
        active["status"] = Value::String(
            if completes {
                "completed"
            } else {
                "interrupted"
            }
            .into(),
        );
        if let Some(terminal) = self.post_interrupt_terminal.take() {
            self.terminals
                .entry(thread.id.clone())
                .or_default()
                .push(terminal);
        }
        Ok(())
    }
}

fn seed_active_turn(sim: &Arc<Mutex<NativeSim>>, thread: &str, turn: &str) {
    let mut sim = sim.lock().unwrap();
    let cwd = sim.threads.get(thread).expect("thread").cwd.clone();
    sim.threads
        .insert(thread.to_owned(), SimThread::active(thread, &cwd, turn));
}

fn complete_turn(sim: &Arc<Mutex<NativeSim>>, thread: &str, turn_id: &str, text: &str) {
    let mut sim = sim.lock().unwrap();
    let turn = sim
        .threads
        .get_mut(thread)
        .expect("thread")
        .turns
        .iter_mut()
        .find(|turn| turn["id"].as_str() == Some(turn_id))
        .expect("turn");
    turn["status"] = json!("completed");
    turn["items"] = json!([{
        "id": format!("agent-{turn_id}"),
        "type": "agentMessage",
        "text": text
    }]);
}

fn fail_turn(sim: &Arc<Mutex<NativeSim>>, thread: &str, turn_id: &str, message: &str) {
    let mut sim = sim.lock().unwrap();
    let turn = sim
        .threads
        .get_mut(thread)
        .expect("thread")
        .turns
        .iter_mut()
        .find(|turn| turn["id"].as_str() == Some(turn_id))
        .expect("turn");
    turn["status"] = json!("failed");
    turn["error"] = json!({"message": message});
}

fn native_responder(
    sim: Arc<Mutex<NativeSim>>,
) -> impl Fn(&Value) -> Answer + Send + Sync + 'static {
    move |request| {
        let method = request["method"].as_str().unwrap_or_default();
        match method {
            "initialize" => Answer::Result(json!({})),
            "mcpServerStatus/list" => {
                let sim = sim.lock().unwrap();
                let thread = request["params"]["threadId"].as_str().unwrap_or_default();
                let tools: BTreeMap<String, Value> = sim
                    .threads
                    .get(thread)
                    .map(|thread| {
                        thread
                            .tools
                            .iter()
                            .map(|tool| (tool.clone(), json!({})))
                            .collect()
                    })
                    .unwrap_or_default();
                Answer::Result(json!({
                    "data": [{"name": "executors", "tools": tools}],
                    "nextCursor": null
                }))
            }
            "thread/read" => {
                let id = request["params"]["threadId"].as_str().unwrap_or_default();
                let sim = sim.lock().unwrap();
                if let Some(error) = sim.read_error.as_ref() {
                    return Answer::Error(json!({"message": error}));
                }
                match sim.threads.get(id) {
                    Some(thread) => Answer::Result(json!({"thread": thread.record()})),
                    None => Answer::Error(json!({"message": format!("unknown thread {id}")})),
                }
            }
            "thread/start" => {
                let mut sim = sim.lock().unwrap();
                let id = format!("recovery-thread-{}", sim.next_thread);
                sim.next_thread += 1;
                let cwd = request["params"]["cwd"].as_str().unwrap_or_default();
                let mut thread = SimThread::idle(&id, Path::new(cwd));
                thread.workspace_roots = request["params"]["runtimeWorkspaceRoots"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|root| root.as_str().map(PathBuf::from))
                    .collect();
                if let Some(model) = request["params"]["model"].as_str() {
                    thread.model = model.to_owned();
                }
                if let Some(provider) = request["params"]["modelProvider"].as_str() {
                    thread.model_provider = provider.to_owned();
                }
                if let Some(effort) = request["params"]["config"]["model_reasoning_effort"].as_str()
                {
                    thread.reasoning_effort = effort.to_owned();
                }
                if let Some(tools) =
                    request["params"]["config"]["mcp_servers"]["executors"]["enabled_tools"]
                        .as_array()
                {
                    thread.tools = tools
                        .iter()
                        .filter_map(|tool| tool.as_str().map(str::to_owned))
                        .collect();
                }
                let workspace_roots = thread.workspace_roots.clone();
                sim.threads.insert(id.clone(), thread);
                Answer::Result(json!({
                    "thread": {"id": id},
                    "cwd": cwd,
                    "runtimeWorkspaceRoots": workspace_roots,
                    "model": request["params"]["model"],
                    "modelProvider": request["params"]["modelProvider"],
                    "reasoningEffort": request["params"]["config"]["model_reasoning_effort"]
                }))
            }
            "thread/name/set" => {
                let mut sim = sim.lock().unwrap();
                let id = request["params"]["threadId"].as_str().unwrap_or_default();
                let Some(thread) = sim.threads.get_mut(id) else {
                    return Answer::Error(json!({"message": format!("unknown thread {id}")}));
                };
                thread.name = request["params"]["name"].as_str().map(str::to_owned);
                Answer::Result(json!({}))
            }
            "turn/interrupt" => {
                let mut sim = sim.lock().unwrap();
                let thread = request["params"]["threadId"].as_str().unwrap_or_default();
                let turn = request["params"]["turnId"].as_str().unwrap_or_default();
                match sim.interrupt(thread, turn) {
                    Ok(()) => Answer::Result(json!({})),
                    Err(message) => Answer::Error(json!({"message": message})),
                }
            }
            "thread/backgroundTerminals/list" => {
                let sim = sim.lock().unwrap();
                let thread = request["params"]["threadId"].as_str().unwrap_or_default();
                let data: Vec<Value> = sim
                    .terminals
                    .get(thread)
                    .into_iter()
                    .flatten()
                    .map(|terminal| {
                        json!({
                            "processId": terminal.process_id.clone(),
                            "itemId": terminal.item_id.clone(),
                            "command": terminal.command.clone(),
                            "cwd": sim.threads.get(thread).map(|thread| &thread.cwd),
                            "osPid": null
                        })
                    })
                    .collect();
                Answer::Result(json!({"data": data, "nextCursor": null}))
            }
            "thread/backgroundTerminals/terminate" => {
                let mut sim = sim.lock().unwrap();
                let thread = request["params"]["threadId"].as_str().unwrap_or_default();
                let process_id = request["params"]["processId"].as_str().unwrap_or_default();
                let terminated = sim.terminate_terminals
                    && sim.terminals.get_mut(thread).is_some_and(|terminals| {
                        let before = terminals.len();
                        terminals.retain(|terminal| terminal.process_id != process_id);
                        terminals.len() != before
                    });
                Answer::Result(json!({"terminated": terminated}))
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

struct LeadContinuationRun {
    run: SyntheticRun,
    value: Value,
    primary: PathBuf,
    _secondary: tempfile::TempDir,
    roots: Vec<PathBuf>,
}

fn delegated_lead_continuation_run(home: &Path, server: &Server) -> LeadContinuationRun {
    let run = synthetic_run(home, DIRECTOR, LEAD, server);
    endpoint(home, LEAD, LEAD, server.port, 0);
    let primary = run._checkout.path().to_path_buf();
    let secondary = tempfile::tempdir().unwrap();
    let secondary_checkout = secondary.path().join("secondary");
    let secondary_base = git_repo(&secondary_checkout);
    let mut value: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    let run_id = value["run"].as_str().unwrap().to_owned();
    let base = value["base"].as_str().unwrap().to_owned();
    let repository = value["repository"].clone();
    let title = "CLead (ds) - lead-continuation";
    value["role"] = json!("lead");
    value["parentThreadId"] = json!(DIRECTOR);
    value["rootThreadId"] = json!(DIRECTOR);
    value["runGeneration"] = json!(run_id);
    value["title"] = json!(title);
    value["profile"] = json!({
        "profile": "ds",
        "model": "deepseek-flash",
        "modelProvider": "deepseek",
        "reasoningEffort": "max"
    });
    value["workspaces"] = json!([
        {
            "repository": repository,
            "slot": 1,
            "checkout": primary,
            "base": base,
            "owner": run_id
        },
        {
            "repository": secondary_checkout,
            "slot": 1,
            "checkout": secondary_checkout,
            "base": secondary_base,
            "owner": run_id
        }
    ]);
    value["workspaceRoots"] = json!([primary, secondary_checkout]);
    value["control"]["config"] = json!({
        "agents.enabled": false,
        "mcp_servers": {"executors": {"enabled_tools": LEAD_ROLE_TOOLS}}
    });
    fs::write(&run.receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    LeadContinuationRun {
        run,
        value,
        primary: primary.clone(),
        _secondary: secondary,
        roots: vec![primary, secondary_checkout],
    }
}

fn recorded_lead_thread(title: &str, checkout: &Path, with_turn: bool) -> SimThread {
    let mut thread = SimThread::idle(LEAD, checkout);
    thread.name = Some(title.to_owned());
    thread.model = "deepseek-flash".into();
    thread.model_provider = "deepseek".into();
    thread.reasoning_effort = "max".into();
    thread.tools = LEAD_ROLE_TOOLS
        .iter()
        .map(|tool| (*tool).to_owned())
        .collect();
    if with_turn {
        thread.turns = vec![json!({
            "id": "lead-turn",
            "status": "completed",
            "items": [{"id": "lead-message", "type": "agentMessage", "text": "partial result"}]
        })];
    }
    thread
}

fn recorded_executor_child(home: &Path, server: &Server) -> ReleaseRun {
    recorded_executor_child_for(home, server, EXECUTOR)
}

fn recorded_executor_child_for(home: &Path, server: &Server, executor: &str) -> ReleaseRun {
    let run = release_run(home, LEAD, executor, server);
    let mut value: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    let run_id = value["run"].as_str().unwrap().to_owned();
    value["role"] = json!("executor");
    value["parentThreadId"] = json!(LEAD);
    value["leadThreadId"] = json!(LEAD);
    value["rootThreadId"] = json!(DIRECTOR);
    value["runGeneration"] = json!(run_id);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    run
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
    git_repo(repository.path());
    let base = git_repo(checkout.path());
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
            "task": run,
            "handle": "test-handle",
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
        "assignment": {
            "schema": 1,
            "objective": "Prove one bounded executor dispatch",
            "inputs": [],
            "outputs": [],
            "invariants": [],
            "acceptance": ["Return a bounded result"]
        },
        "prompt": "Prove one bounded executor dispatch and return a bounded result.",
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
        "control": {
            "approvalPolicy": "never",
            "sandbox": "danger-full-access",
            "config": {"agents.enabled": false},
            "roleArgs": [],
            "profileOverrides": []
        },
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

fn install_kit(home: &Path, root: &Path) {
    let kit = synthetic_role_build::kit_source(root);
    let plugin = kit.join(".agents/plugins/executors");
    fs::create_dir_all(&plugin).unwrap();
    fs::write(
        plugin.join(".mcp.json"),
        serde_json::to_vec(&json!({
            "mcpServers": {
                "executors": {
                    "command": "codex-harness",
                    "args": ["mcp", "executors"],
                    "tool_timeout_sec": 1860
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        kit.join("global/orchestration.toml"),
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"default\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 6\nworktree_pool_size = 18\n",
    )
    .unwrap();
    fs::write(
        home.join("ds.config.toml"),
        "model = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    fs::write(
        home.join("config.toml"),
        "model = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    let build = synthetic_role_build::recorded_build(root, &kit);
    synthetic_role_build::register(home, root, &build);
    fs::create_dir_all(home.join("harness")).unwrap();
    fs::write(
        home.join("harness/installation.json"),
        serde_json::to_vec(&json!({"settings": {"sourceRoot": kit}})).unwrap(),
    )
    .unwrap();
}

/// One recorded delegated Lead run in a shared repository. The same checkout
/// is the Lead workspace and the repository whose pool the Lead dispatches
/// into; its exact base is available for the child.
fn delegated_lead_run(home: &Path, root: &Path, server: &Server) -> (PathBuf, Value) {
    let repository = root.join("repo");
    let base = git_repo(&repository);
    endpoint(home, DIRECTOR, DIRECTOR, server.port, 0);
    endpoint(home, LEAD, LEAD, server.port, 0);
    let endpoint_value: Value = serde_json::from_slice(
        &fs::read(
            home.join("harness/lead-endpoints")
                .join(format!("{DIRECTOR}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    let state = task_worktree::pool_state_dir(home, &repository).unwrap();
    fs::create_dir_all(&state).unwrap();
    let run = "lead-run";
    fs::write(
        state.join("slot-1.json"),
        serde_json::to_vec_pretty(&json!({
            "ownershipSchema": 1,
            "source": repository,
            "index": 1,
            "path": repository,
            "state": "occupied",
            "owner": run,
            "task": run,
            "handle": "lead-handle",
            "base": base,
            "disposition": null,
            "reason": null,
            "startup": null,
            "assignment": null
        }))
        .unwrap(),
    )
    .unwrap();
    let receipt = state.join("mcp-1.json");
    let value = json!({
        "schema": 1,
        "run": run,
        "role": "lead",
        "parentThreadId": DIRECTOR,
        "rootThreadId": DIRECTOR,
        "runGeneration": "lead-generation",
        "leadThreadId": DIRECTOR,
        "executorThreadId": LEAD,
        "turnId": "lead-turn",
        "assignment": {
            "schema": 1,
            "objective": "Lead one prepared change",
            "inputs": [],
            "outputs": [],
            "invariants": [],
            "acceptance": ["Return a bounded result"]
        },
        "prompt": "Lead one prepared change",
        "title": "CLead (ds) - lead-run",
        "slot": 1,
        "source": repository,
        "repository": repository,
        "poolSize": 6,
        "checkout": repository,
        "base": base,
        "profile": {
            "profile": "ds",
            "model": "deepseek-flash",
            "modelProvider": "deepseek",
            "reasoningEffort": "max"
        },
        "observedStatus": "idle",
        "allowProviderModelFallback": false,
        "control": {
            "approvalPolicy": "never",
            "sandbox": "danger-full-access",
            "config": {"agents.enabled": false},
            "roleArgs": [],
            "profileOverrides": []
        },
        "lease": {
            "slot": 1,
            "owner": run,
            "checkout": repository,
            "appServer": endpoint_value["process"]
        }
    });
    fs::write(&receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    (repository, value)
}

fn delegated_child_run(home: &Path, server: &Server) -> (SyntheticRun, Value, String) {
    let run = synthetic_run(home, LEAD, EXECUTOR, server);
    let request_id = "lead-000000000000000000000001".to_owned();
    let mut value: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    value["role"] = json!("executor");
    value["parentThreadId"] = json!(LEAD);
    value["rootThreadId"] = json!(DIRECTOR);
    value["runGeneration"] = json!("child-generation");
    value["leadMessages"] = json!([{
        "schema": 1,
        "id": request_id,
        "kind": "reply-request",
        "status": "pending",
        "contentId": "pending-content",
        "method": "turn/start",
        "turnId": "initial-turn",
        "attempts": 0,
        "requestedMs": 1
    }]);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    (run, value, request_id)
}

fn bd_executable() -> PathBuf {
    let path = std::env::var_os("PATH").expect("PATH");
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join("bd.exe");
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("bd executable is not installed on PATH");
}

/// A committed prepared change plus a real board issue in one repository.
fn prepared_repository(home: &Path, root: &Path, server: &Server) -> (PathBuf, String, String) {
    let repository = root.join("repo");
    git_repo(&repository);
    let change = repository.join("openspec/changes/lead-sample");
    fs::create_dir_all(change.join("specs/sample")).unwrap();
    fs::write(change.join("proposal.md"), "# Proposal\n\nPrepared.\n").unwrap();
    fs::write(change.join("tasks.md"), "- [ ] implement\n").unwrap();
    fs::write(change.join("design.md"), "# Design\n\nPrepared.\n").unwrap();
    fs::write(
        change.join("specs/sample/spec.md"),
        "## ADDED Requirements\n\n### Requirement: Sample\n\nThe system SHALL work.\n",
    )
    .unwrap();
    let head = commit_checkout(&repository, "prepare the sample change");
    let bin = home.join("harness/bin");
    fs::create_dir_all(&bin).unwrap();
    fs::copy(bd_executable(), bin.join("bd.exe")).unwrap();
    let bd = bin.join("bd.exe");
    let init = Command::new(&bd)
        .args([
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "leadtest",
        ])
        .current_dir(&repository)
        .env("BD_NON_INTERACTIVE", "1")
        .env("BEADS_ACTOR", "executor-mcp-test")
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let created = Command::new(&bd)
        .args(["create", "Prepared Lead change", "--type", "task", "--json"])
        .current_dir(&repository)
        .env("BD_NON_INTERACTIVE", "1")
        .env("BEADS_ACTOR", "executor-mcp-test")
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let issue: Value = serde_json::from_slice(&created.stdout).unwrap();
    let issue = issue["id"].as_str().unwrap().to_owned();
    endpoint(home, DIRECTOR, DIRECTOR, server.port, 0);
    (repository, head, issue)
}

fn write_limits(root: &Path, limits: &str) {
    fs::write(
        root.join("kit/global/orchestration.toml"),
        format!(
            "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"default\"\nexecutor_profiles = [\"ds\"]\n{limits}\nworktree_pool_size = 18\n"
        ),
    )
    .unwrap();
}

fn prepared_lead_assignment(issue: &str, base: &str) -> Value {
    prepared_lead_assignment_with(issue, base, json!([{"path": ".", "base": base}]))
}

fn prepared_lead_assignment_with(issue: &str, base: &str, repositories: Value) -> Value {
    json!({
        "schema": 1,
        "objective": "Lead the prepared sample change",
        "inputs": [],
        "outputs": [],
        "invariants": ["Use the prepared change and its exact base."],
        "acceptance": ["The change is accepted with checked evidence."],
        "board_task": issue,
        "lead_change": {
            "planning_root": ".",
            "base": base,
            "directory": "openspec/changes/lead-sample",
            "repositories": repositories
        }
    })
}

struct ReleaseRun {
    _root: tempfile::TempDir,
    repository: PathBuf,
    checkout: PathBuf,
    slot_state: PathBuf,
    receipt: PathBuf,
    base: String,
    run: String,
}

fn release_run(home: &Path, lead: &str, executor: &str, server: &Server) -> ReleaseRun {
    endpoint(home, lead, lead, server.port, 0);
    let endpoint_value: Value = serde_json::from_slice(
        &fs::read(
            home.join("harness/lead-endpoints")
                .join(format!("{lead}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join("repo");
    git_repo(&repository);
    fs::write(repository.join(".gitignore"), "target/\n").unwrap();
    let base = commit_checkout(&repository, "ignore build cache");
    let run = format!("mcp-test-{executor}");
    let acquired =
        task_worktree::acquire_slot(home, &repository, 1, &run, Some(&base), &|_| false).unwrap();
    let state = task_worktree::pool_state_dir(home, &repository).unwrap();
    let receipt = state.join("mcp-1.json");
    let value = json!({
        "schema": 1,
        "run": run,
        "leadThreadId": lead,
        "executorThreadId": executor,
        "turnId": "initial-turn",
        "assignment": {
            "schema": 1,
            "objective": "Prove one bounded executor dispatch",
            "inputs": [],
            "outputs": [],
            "invariants": [],
            "acceptance": ["Return a bounded result"]
        },
        "prompt": "Prove one bounded executor dispatch and return a bounded result.",
        "title": format!("executor {executor}"),
        "slot": 1,
        "source": repository,
        "repository": repository,
        "poolSize": 1,
        "checkout": acquired.path,
        "base": base,
        "profile": {
            "profile": "test",
            "model": "test-model",
            "modelProvider": "test-provider",
            "reasoningEffort": "high"
        },
        "observedStatus": "idle",
        "allowProviderModelFallback": false,
        "startupReservationReleased": true,
        "control": {
            "approvalPolicy": "never",
            "sandbox": "danger-full-access",
            "config": {"agents.enabled": false},
            "roleArgs": [],
            "profileOverrides": []
        },
        "lease": {
            "slot": 1,
            "owner": run,
            "checkout": acquired.path,
            "appServer": endpoint_value["process"]
        }
    });
    fs::write(&receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    ReleaseRun {
        _root: root,
        repository,
        checkout: acquired.path,
        slot_state: state.join("slot-1.json"),
        receipt,
        base,
        run,
    }
}

fn record_release_payload(home: &Path, run: &ReleaseRun, id: &str) -> (PathBuf, PathBuf) {
    let directory = fs::canonicalize(home).unwrap().join("harness/messages");
    fs::create_dir_all(&directory).unwrap();
    let payload = directory.join(format!("{id}.txt"));
    let sibling = directory.join("another-run.txt");
    fs::write(&payload, "recorded run payload\n").unwrap();
    fs::write(&sibling, "another run payload\n").unwrap();
    let mut receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    receipt["leadMessages"] = json!([{
        "schema": 1,
        "id": id,
        "kind": "reply-request",
        "status": "resolved",
        "contentId": "resolved-request",
        "payloadPath": payload,
        "payloadBytes": 21
    }]);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    (payload, sibling)
}

fn release_tool(
    mcp: &mut Mcp,
    id: u64,
    caller: &str,
    run: Option<&str>,
    disposition: &str,
    reason: &str,
) -> Value {
    let mut arguments = json!({"disposition": disposition, "reason": reason});
    if let Some(run) = run {
        arguments["run"] = json!(run);
    }
    mcp.call_tool(
        id,
        "executor_release",
        arguments,
        Some(json!({"threadId": caller})),
    )
}

fn slot_record(run: &ReleaseRun) -> Value {
    serde_json::from_slice(&fs::read(&run.slot_state).unwrap()).unwrap()
}

fn result(response: &Value) -> &Value {
    assert_eq!(response["result"]["isError"], false, "{response}");
    &response["result"]["structuredContent"]
}

fn structured(response: &Value) -> &Value {
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
                .is_some_and(|path| path.ends_with("consumer-de1"))
        }),
        "the claimed pool worktree must be a sibling of the consumer: {records:?}"
    );
    assert!(
        records.iter().any(|record| record["label"] == "de1"),
        "the direct Executor position records its fixed hierarchy label: {records:?}"
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
        "executor_wait",
        "executor_message",
        "executor_reply",
        "executor_ask_lead",
        "executor_stop",
        "executor_resume",
        "executor_release",
    ] {
        assert!(names.contains(&required), "{listed}");
    }
    let wait = tools
        .iter()
        .find(|tool| tool["name"] == "executor_wait")
        .unwrap();
    assert_eq!(
        wait["inputSchema"]["properties"]["timeoutMs"]["maximum"],
        1_800_000
    );
    assert_eq!(
        wait["inputSchema"]["properties"]["timeoutMs"]["default"],
        1_800_000
    );
    assert_eq!(
        wait["inputSchema"]["properties"]["acknowledge"]["items"]["type"],
        "string"
    );
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
        if !matches!(
            tool["name"].as_str(),
            Some("executor_message")
                | Some("executor_wait")
                | Some("executor_stop")
                | Some("executor_resume")
                | Some("executor_release")
        ) {
            assert!(
                !properties.contains_key("run"),
                "{} must not expose run: {tool}",
                tool["name"]
            );
        } else {
            assert!(
                properties.contains_key("run"),
                "{} must expose run: {tool}",
                tool["name"]
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
        5,
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
fn status_rebinds_an_ended_app_server_for_the_same_lead_thread() {
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

    let endpoint_path = home
        .path()
        .join("harness/lead-endpoints")
        .join(format!("{LEAD}.json"));
    let endpoint: Value = serde_json::from_slice(&fs::read(&endpoint_path).unwrap()).unwrap();
    let mut receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    let previous = json!({
        "pid": 1,
        "creationTime": 1,
        "program": endpoint["process"]["program"]
    });
    receipt["lease"]["appServer"] = previous.clone();
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();

    let mut mcp = Mcp::start(home.path());
    let status = mcp.call_tool(
        2,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    let status = result(&status);
    assert_eq!(status["role"], "lead");
    assert_eq!(status["runs"][0]["run"], receipt["run"]);
    assert_eq!(status["runs"][0]["executorThreadId"], EXECUTOR);
    assert_eq!(status["runs"][0]["slot"], receipt["slot"]);

    let rebound: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert_eq!(rebound["lease"]["appServer"], endpoint["process"]);
    assert_eq!(rebound["appServerRebind"]["from"], previous);
    assert_eq!(rebound["appServerRebind"]["to"], endpoint["process"]);
    for field in [
        "run",
        "leadThreadId",
        "executorThreadId",
        "slot",
        "checkout",
        "base",
        "profile",
    ] {
        assert_eq!(rebound[field], receipt[field], "{field} changed");
    }
    assert!(server.requests_for("turn/start").is_empty());
    assert!(server.requests_for("turn/steer").is_empty());

    drop(run);
    mcp.finish();
}

#[test]
fn status_refuses_a_live_previous_app_server_before_native_effects() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);

    let program = PathBuf::from(r"C:\Windows\System32\ping.exe");
    let previous = ChildGuard(
        Command::new(&program)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let identity = ServiceProcess::observe(
        previous.0.id(),
        &program,
        0,
        &harness_core::process_service::current_user().unwrap(),
    )
    .unwrap()
    .identity();
    let mut receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    receipt["lease"]["appServer"] = json!({
        "pid": identity.pid,
        "creationTime": identity.creation_time,
        "program": program
    });
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let before = fs::read(&run.receipt).unwrap();

    let mut mcp = Mcp::start(home.path());
    let refused = mcp.call_tool(
        2,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&refused).contains("is still live but differs"),
        "{refused}"
    );
    assert_eq!(fs::read(&run.receipt).unwrap(), before);
    assert_eq!(server.connections(), 0);
    assert!(server.requests().is_empty());

    drop(run);
    mcp.finish();
}

#[test]
fn status_reports_unavailable_native_detail_without_blocking_on_silent_endpoint() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let first = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let second = synthetic_run(home.path(), LEAD, THIRD_EXECUTOR, &server);
    server.answer_fn(|_| Answer::Silence);
    let mut mcp = Mcp::start(home.path());

    let started = Instant::now();
    let response = mcp.call_tool(
        2,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    let elapsed = started.elapsed();
    let status = result(&response);
    assert_eq!(status["state"], "ok");
    let runs = status["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    for run in runs {
        assert_eq!(run["native"]["status"], "unavailable", "{run}");
        assert_eq!(run["executorNative"]["status"], "unavailable", "{run}");
    }
    assert!(
        elapsed < Duration::from_secs(5),
        "status took {elapsed:?} with a silent endpoint"
    );

    drop(first);
    drop(second);
    mcp.finish();
}

#[test]
fn status_keeps_a_released_run_readable_after_its_app_server_is_replaced() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let mut value: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    value["lease"]["appServer"]["pid"] = json!(1);
    value["lease"]["appServer"]["creationTime"] = json!(1);
    value["release"] = json!({
        "schema": 1,
        "run": value["run"],
        "slot": 1,
        "disposition": "merged",
        "reason": "test release",
        "base": value["base"],
        "outcome": "reused",
        "slotState": "released",
        "slotFreed": true
    });
    fs::write(&run.receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    let slot = run.receipt.parent().unwrap().join("slot-1.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&slot).unwrap()).unwrap();
    record["state"] = json!("released");
    record["owner"] = Value::Null;
    record["task"] = Value::Null;
    record["handle"] = Value::Null;
    fs::write(&slot, serde_json::to_vec_pretty(&record).unwrap()).unwrap();

    let mut mcp = Mcp::start(home.path());
    let response = mcp.call_tool(
        2,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    let status = result(&response);
    assert_eq!(status["runs"][0]["release"]["slotFreed"], true);
    assert_eq!(status["runs"][0]["native"]["status"], "unavailable");
    assert!(
        status["runs"][0]["native"]["error"]
            .as_str()
            .unwrap()
            .contains("released run lease"),
        "{status}"
    );

    drop(run);
    mcp.finish();
}

#[test]
fn delegated_lead_dispatches_an_executor_and_executor_cannot_dispatch() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, lead) = delegated_lead_run(home.path(), root.path(), &server);
    let base = lead["base"].as_str().unwrap().to_owned();
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        "unused",
        &repository,
        false,
        false,
    )));
    server.answer_fn(native_responder(sim));
    let mut mcp = Mcp::start(home.path());

    let starts_before = server.requests_for("thread/start").len();
    let dispatched = mcp.call(
        2,
        json!({
            "owner": "delegated-child",
            "base": base,
            "assignment": assignment("Dispatch one nested executor")
        }),
        Some(json!({"threadId": LEAD})),
    );
    let dispatched = result(&dispatched);
    assert_eq!(dispatched["role"], "executor", "{dispatched}");
    assert_eq!(dispatched["parentThreadId"], LEAD, "{dispatched}");
    assert_eq!(dispatched["rootThreadId"], DIRECTOR, "{dispatched}");
    assert_eq!(
        server.requests_for("thread/start").len(),
        starts_before + 1,
        "{dispatched}"
    );
    let child = dispatched["executorThreadId"].as_str().unwrap().to_owned();

    let starts_before = server.requests_for("thread/start").len();
    let refused = mcp.call_tool(
        3,
        "lead_dispatch",
        json!({"assignment": assignment("nested lead")}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&refused).contains("only a verified Director"),
        "{refused}"
    );
    assert_eq!(
        server.requests_for("thread/start").len(),
        starts_before,
        "a delegated Lead must not create another Lead"
    );

    let refused = mcp.call(
        4,
        json!({
            "owner": "nested-executor",
            "base": base,
            "assignment": assignment("nested executor")
        }),
        Some(json!({"threadId": child})),
    );
    assert!(
        tool_error(&refused).contains("recorded executor"),
        "{refused}"
    );
    let refused = mcp.call_tool(
        5,
        "lead_dispatch",
        json!({"assignment": assignment("nested lead")}),
        Some(json!({"threadId": child})),
    );
    assert!(
        tool_error(&refused).contains("only a verified Director"),
        "{refused}"
    );

    drop(lead);
    mcp.finish();
}

#[test]
fn director_dispatches_a_prepared_change_to_a_lead_that_dispatches_an_executor() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, base, issue) = prepared_repository(home.path(), root.path(), &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        DIRECTOR,
        "unused",
        &repository,
        false,
        false,
    )));
    server.answer_fn(native_responder(sim));
    let mut mcp = Mcp::start(home.path());

    let lead = mcp.call_tool(
        2,
        "lead_dispatch",
        json!({
            "requestId": "lead-request-1",
            "assignment": {
                "schema": 1,
                "objective": "Lead the prepared sample change",
                "inputs": [],
                "outputs": [],
                "invariants": ["Use the prepared change and its exact base."],
                "acceptance": ["The change is accepted with checked evidence."],
                "board_task": issue,
                "lead_change": {
                    "planning_root": ".",
                    "base": base,
                    "directory": "openspec/changes/lead-sample",
                    "repositories": [{"path": ".", "base": base}]
                }
            }
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    let lead = result(&lead);
    assert_eq!(lead["role"], "lead", "{lead}");
    assert_eq!(lead["parentThreadId"], DIRECTOR, "{lead}");
    assert_eq!(lead["rootThreadId"], DIRECTOR, "{lead}");
    assert_eq!(lead["leadChange"]["board_task"], issue, "{lead}");
    assert_eq!(lead["profile"]["profile"], "ds", "{lead}");
    assert_eq!(lead["slotLabel"], "l1", "{lead}");
    assert_eq!(lead["workspaces"][0]["label"], "l1", "{lead}");
    let lead_thread = lead["executorThreadId"].as_str().unwrap().to_owned();
    let endpoint_path = home
        .path()
        .join("harness/lead-endpoints")
        .join(format!("{lead_thread}.json"));
    assert!(
        endpoint_path.is_file(),
        "the Lead endpoint was not published"
    );

    let starts_before_retry = server.requests_for("thread/start").len();
    let retried = mcp.call_tool(
        3,
        "lead_dispatch",
        json!({
            "requestId": "lead-request-1",
            "assignment": {
                "schema": 1,
                "objective": "Lead the prepared sample change",
                "inputs": [],
                "outputs": [],
                "invariants": ["Use the prepared change and its exact base."],
                "acceptance": ["The change is accepted with checked evidence."],
                "board_task": issue,
                "lead_change": {
                    "planning_root": ".",
                    "base": base,
                    "directory": "openspec/changes/lead-sample",
                    "repositories": [{"path": ".", "base": base}]
                }
            }
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    let retried = result(&retried);
    assert_eq!(retried["reconciled"], true, "{retried}");
    assert_eq!(retried["requestId"], "lead-request-1", "{retried}");
    assert_eq!(retried["executorThreadId"], lead_thread, "{retried}");
    assert_eq!(
        server.requests_for("thread/start").len(),
        starts_before_retry,
        "a repeated Lead request must not create another thread"
    );

    let child = mcp.call(
        4,
        json!({
            "owner": "lead-child",
            "requestId": "child-request-1",
            "base": base,
            "assignment": assignment("Dispatch one executor from the Lead")
        }),
        Some(json!({"threadId": lead_thread})),
    );
    let child = result(&child);
    assert_eq!(child["role"], "executor", "{child}");
    assert_eq!(child["parentThreadId"], lead_thread, "{child}");
    assert_eq!(child["rootThreadId"], DIRECTOR, "{child}");
    assert_eq!(child["base"], base, "{child}");
    assert_eq!(child["slotLabel"], "l1e1", "{child}");
    assert_eq!(child["workspaces"][0]["label"], "l1e1", "{child}");
    assert_ne!(child["checkout"], lead["checkout"], "{child}");
    assert_ne!(
        child["executorThreadId"], lead["executorThreadId"],
        "{child}"
    );
    let starts_before_child_retry = server.requests_for("thread/start").len();
    let child_retry = mcp.call(
        5,
        json!({
            "owner": "lead-child",
            "requestId": "child-request-1",
            "base": base,
            "assignment": assignment("Dispatch one executor from the Lead")
        }),
        Some(json!({"threadId": lead_thread})),
    );
    let child_retry = result(&child_retry);
    assert_eq!(child_retry["reconciled"], true, "{child_retry}");
    assert_eq!(
        child_retry["executorThreadId"], child["executorThreadId"],
        "{child_retry}"
    );
    assert_eq!(
        server.requests_for("thread/start").len(),
        starts_before_child_retry,
        "a repeated Executor request must not create another thread"
    );

    let conflict = mcp.call_tool(
        6,
        "lead_dispatch",
        json!({
            "requestId": "lead-request-1",
            "assignment": {
                "schema": 1,
                "objective": "A different Lead objective",
                "inputs": [],
                "outputs": [],
                "invariants": ["Different intent."],
                "acceptance": ["Different intent."],
                "board_task": issue,
                "lead_change": {
                    "planning_root": ".",
                    "base": base,
                    "directory": "openspec/changes/lead-sample",
                    "repositories": [{"path": ".", "base": base}]
                }
            }
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert!(
        tool_error(&conflict).contains("different dispatch intent"),
        "{conflict}"
    );

    // A direct Director-to-Executor dispatch remains compatible.
    let direct = mcp.call(
        7,
        json!({
            "owner": "direct-child",
            "requestId": "direct-request-1",
            "base": base,
            "assignment": assignment("Dispatch one direct executor")
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    let direct = result(&direct);
    assert_eq!(direct["role"], "executor", "{direct}");
    assert_eq!(direct["parentThreadId"], DIRECTOR, "{direct}");
    assert_eq!(direct["rootThreadId"], DIRECTOR, "{direct}");
    assert_eq!(direct["slotLabel"], "de1", "{direct}");
    assert_ne!(direct["checkout"], lead["checkout"], "{direct}");

    let starts = server.requests_for("thread/start");
    assert_eq!(starts.len(), 3, "{starts:?}");
    for start in &starts {
        assert_eq!(
            start["params"]["config"]["agents.enabled"], false,
            "{start}"
        );
    }
    let lead_tools = starts[0]["params"]["config"]["mcp_servers"]["executors"]["enabled_tools"]
        .as_array()
        .unwrap();
    assert!(lead_tools.iter().any(|tool| tool == "executor_dispatch"));
    assert!(!lead_tools.iter().any(|tool| tool == "lead_dispatch"));
    for index in [1, 2] {
        let child_tools =
            starts[index]["params"]["config"]["mcp_servers"]["executors"]["enabled_tools"]
                .as_array()
                .unwrap();
        assert!(child_tools.iter().any(|tool| tool == "executor_status"));
        assert!(!child_tools.iter().any(|tool| tool == "executor_dispatch"));
        assert!(!child_tools.iter().any(|tool| tool == "lead_dispatch"));
    }

    // A vague Lead assignment is refused before any new thread.
    let starts_before = server.requests_for("thread/start").len();
    let refused = mcp.call_tool(
        8,
        "lead_dispatch",
        json!({"assignment": assignment("vague lead")}),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert!(tool_error(&refused).contains("lead_change"), "{refused}");
    assert_eq!(server.requests_for("thread/start").len(), starts_before);

    mcp.finish();
}

#[test]
fn director_lead_dispatch_records_distinct_repository_workspaces() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, base, issue) = prepared_repository(home.path(), root.path(), &server);
    let consumer = repository.join("consumer");
    let consumer_base = git_repo(&consumer);
    let outside = root.path().join("outside");
    git_repo(&outside);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        DIRECTOR,
        "unused",
        &repository,
        false,
        false,
    )));
    let observed_sim = Arc::clone(&sim);
    server.answer_fn(native_responder(sim));
    let mut mcp = Mcp::start(home.path());

    let starts_before = server.requests_for("thread/start").len();
    let duplicate = mcp.call_tool(
        2,
        "lead_dispatch",
        json!({
            "assignment": prepared_lead_assignment_with(
                &issue,
                &base,
                json!([
                    {"path": ".", "base": base},
                    {"path": "./", "base": base}
                ])
            )
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert!(
        tool_error(&duplicate).contains("repeats the already declared repository"),
        "{duplicate}"
    );
    let foreign = mcp.call_tool(
        3,
        "lead_dispatch",
        json!({
            "assignment": prepared_lead_assignment_with(
                &issue,
                &base,
                json!([
                    {"path": ".", "base": base},
                    {"path": "../outside", "base": base}
                ])
            )
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert!(
        tool_error(&foreign).contains("must not traverse with '..'"),
        "{foreign}"
    );
    assert_eq!(
        server.requests_for("thread/start").len(),
        starts_before,
        "invalid repository declarations must not start a Lead"
    );

    let lead = mcp.call_tool(
        4,
        "lead_dispatch",
        json!({
            "requestId": "lead-multi-repository",
            "assignment": prepared_lead_assignment_with(
                &issue,
                &base,
                json!([
                    {"path": "consumer", "base": consumer_base},
                    {"path": ".", "base": base}
                ])
            )
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    let lead = result(&lead);
    assert_eq!(lead["role"], "lead", "{lead}");
    assert_eq!(lead["parentThreadId"], DIRECTOR, "{lead}");
    assert_eq!(lead["rootThreadId"], DIRECTOR, "{lead}");
    let workspaces = lead["workspaces"].as_array().unwrap();
    assert_eq!(workspaces.len(), 2, "{lead}");
    assert_eq!(lead["slotLabel"], "l1", "{lead}");
    assert_eq!(workspaces[0]["label"], "l1", "{lead}");
    assert_eq!(workspaces[1]["label"], "l1", "{lead}");
    assert_eq!(workspaces[0]["base"], consumer_base, "{lead}");
    assert_eq!(workspaces[1]["base"], base, "{lead}");
    assert_ne!(workspaces[0]["checkout"], workspaces[1]["checkout"]);
    assert_eq!(
        PathBuf::from(workspaces[0]["repository"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        consumer.canonicalize().unwrap()
    );
    assert_eq!(
        PathBuf::from(workspaces[1]["repository"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        repository.canonicalize().unwrap()
    );
    for workspace in workspaces {
        assert_eq!(workspace["owner"], lead["run"], "{lead}");
        assert_eq!(
            git_head(Path::new(workspace["checkout"].as_str().unwrap())),
            workspace["base"].as_str().unwrap()
        );
    }
    assert_eq!(lead["primaryCwd"], workspaces[0]["checkout"], "{lead}");
    assert_eq!(lead["checkout"], workspaces[0]["checkout"], "{lead}");
    let starts = server.requests_for("thread/start");
    assert_eq!(starts.len(), starts_before + 1, "{starts:?}");
    assert_eq!(
        PathBuf::from(starts[0]["params"]["cwd"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        PathBuf::from(workspaces[0]["checkout"].as_str().unwrap())
            .canonicalize()
            .unwrap()
    );
    let declared_roots: Vec<PathBuf> = starts[0]["params"]["runtimeWorkspaceRoots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|root| PathBuf::from(root.as_str().unwrap()))
        .collect();
    let expected_roots: Vec<PathBuf> = workspaces
        .iter()
        .map(|workspace| PathBuf::from(workspace["checkout"].as_str().unwrap()))
        .collect();
    assert_eq!(declared_roots, expected_roots, "{starts:?}");
    assert!(
        !declared_roots
            .iter()
            .any(|root| root.canonicalize().unwrap() == outside.canonicalize().unwrap()),
        "an undeclared repository leaked into the Lead workspace roots"
    );
    let lead_thread = lead["executorThreadId"].as_str().unwrap();
    assert_eq!(
        observed_sim.lock().unwrap().threads[lead_thread].workspace_roots,
        declared_roots,
        "the Lead's tool context retained another session's roots"
    );
    let slot = u32::try_from(lead["slot"].as_u64().unwrap()).unwrap();
    let primary = workspace_slot_record(home.path(), &consumer, slot);
    let root = workspace_slot_record(home.path(), &repository, slot);
    assert_eq!(primary["owner"], lead["run"], "{primary}");
    assert_eq!(root["owner"], lead["run"], "{root}");
    assert_eq!(primary["base"], consumer_base, "{primary}");
    assert_eq!(root["base"], base, "{root}");
    assert_eq!(primary["label"], "l1", "{primary}");
    assert_eq!(root["label"], "l1", "{root}");
    let receipt: Value =
        serde_json::from_slice(&fs::read(lead["receipt"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(receipt["workspaces"], lead["workspaces"], "{receipt}");
    assert_eq!(
        receipt["workspaceRoots"],
        json!(declared_roots),
        "{receipt}"
    );
    assert_eq!(receipt["primaryCwd"], lead["primaryCwd"], "{receipt}");

    let lead_thread = lead["executorThreadId"].as_str().unwrap();
    let child = mcp.call(
        8,
        json!({
            "owner": "lead-multi-child",
            "requestId": "lead-multi-child-request",
            "assignment": assignment("Implement the child slice in every Lead repository")
        }),
        Some(json!({"threadId": lead_thread})),
    );
    let child = result(&child);
    assert_eq!(child["role"], "executor", "{child}");
    assert_eq!(child["parentThreadId"], lead_thread, "{child}");
    assert_eq!(child["slot"], 7, "{child}");
    assert_eq!(child["slotLabel"], "l1e1", "{child}");
    let child_workspaces = child["workspaces"].as_array().unwrap();
    assert_eq!(child_workspaces.len(), 2, "{child}");
    for (index, workspace) in child_workspaces.iter().enumerate() {
        assert_eq!(workspace["slot"], 7, "{child}");
        assert_eq!(workspace["label"], "l1e1", "{child}");
        assert_ne!(
            workspace["checkout"], workspaces[index]["checkout"],
            "{child}"
        );
        assert_eq!(workspace["owner"], child["run"], "{child}");
    }
    let child_roots: Vec<PathBuf> = child_workspaces
        .iter()
        .map(|workspace| PathBuf::from(workspace["checkout"].as_str().unwrap()))
        .collect();
    assert_eq!(
        child["workspaceRoots"],
        json!(&child_roots),
        "the child owns both declared worktrees"
    );
    assert_eq!(
        observed_sim.lock().unwrap().threads[child["executorThreadId"].as_str().unwrap()]
            .workspace_roots,
        child_roots
    );
    assert_eq!(
        child["workspaces"][0]["repository"], workspaces[0]["repository"],
        "{child}"
    );
    assert_eq!(
        child["workspaces"][1]["repository"], workspaces[1]["repository"],
        "{child}"
    );
    assert_eq!(
        workspace_slot_record(home.path(), &consumer, 7)["label"],
        "l1e1"
    );
    assert_eq!(
        workspace_slot_record(home.path(), &repository, 7)["label"],
        "l1e1"
    );

    mcp.finish();
}

#[test]
fn simultaneous_lead_workspace_contexts_do_not_retarget() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, base, issue) = prepared_repository(home.path(), root.path(), &server);
    endpoint(home.path(), OTHER_DIRECTOR, OTHER_DIRECTOR, server.port, 0);
    let first_consumer = repository.join("first-consumer");
    let first_base = git_repo(&first_consumer);
    let second_consumer = repository.join("second-consumer");
    let second_base = git_repo(&second_consumer);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        DIRECTOR,
        OTHER_DIRECTOR,
        &repository,
        false,
        false,
    )));
    let observed_sim = Arc::clone(&sim);
    server.answer_fn(native_responder(sim));
    let mut mcp = Mcp::start(home.path());

    let first = mcp.call_tool(
        2,
        "lead_dispatch",
        json!({
            "requestId": "lead-context-first",
            "assignment": prepared_lead_assignment_with(
                &issue,
                &base,
                json!([
                    {"path": "first-consumer", "base": first_base},
                    {"path": ".", "base": base}
                ])
            )
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    let first = result(&first);
    let second = mcp.call_tool(
        3,
        "lead_dispatch",
        json!({
            "requestId": "lead-context-second",
            "assignment": prepared_lead_assignment_with(
                &issue,
                &base,
                json!([
                    {"path": "second-consumer", "base": second_base},
                    {"path": ".", "base": base}
                ])
            )
        }),
        Some(json!({"threadId": OTHER_DIRECTOR})),
    );
    let second = result(&second);
    assert_eq!(first["slotLabel"], "l1", "{first}");
    assert_eq!(second["slotLabel"], "l2", "{second}");
    for workspace in first["workspaces"].as_array().unwrap() {
        assert_eq!(workspace["label"], "l1", "{first}");
    }
    for workspace in second["workspaces"].as_array().unwrap() {
        assert_eq!(workspace["label"], "l2", "{second}");
    }
    let second_thread = second["executorThreadId"].as_str().unwrap();
    let child = mcp.call(
        4,
        json!({
            "owner": "second-lead-child",
            "requestId": "second-lead-child-request",
            "assignment": assignment("Work only in the second Lead's own child worktrees")
        }),
        Some(json!({"threadId": second_thread})),
    );
    let child = result(&child);
    assert_eq!(child["slot"], 13, "{child}");
    assert_eq!(child["slotLabel"], "l2e1", "{child}");
    let child_roots: Vec<PathBuf> = child["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|workspace| {
            assert_eq!(workspace["slot"], 13, "{child}");
            assert_eq!(workspace["label"], "l2e1", "{child}");
            PathBuf::from(workspace["checkout"].as_str().unwrap())
        })
        .collect();
    assert_eq!(child_roots.len(), 2, "{child}");
    assert!(
        !child_roots
            .iter()
            .any(|root| root.canonicalize().unwrap() == second_consumer.canonicalize().unwrap()),
        "the Executor must not write in its Lead checkout: {child}"
    );
    assert!(
        !child_roots
            .iter()
            .any(|root| root.canonicalize().unwrap() == repository.canonicalize().unwrap()),
        "the Executor must not write in its Lead checkout: {child}"
    );
    assert_eq!(
        observed_sim.lock().unwrap().threads[child["executorThreadId"].as_str().unwrap()]
            .workspace_roots,
        child_roots
    );

    let expected = |lead: &Value| -> Vec<PathBuf> {
        lead["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|workspace| PathBuf::from(workspace["checkout"].as_str().unwrap()))
            .collect()
    };
    let first_roots = expected(&first);
    let second_roots = expected(&second);
    let observed = observed_sim.lock().unwrap();
    assert_eq!(
        observed.threads[first["executorThreadId"].as_str().unwrap()].workspace_roots,
        first_roots
    );
    assert_eq!(
        observed.threads[second["executorThreadId"].as_str().unwrap()].workspace_roots,
        second_roots
    );
    assert!(
        !second_roots
            .iter()
            .any(|root| root.canonicalize().unwrap() == first_consumer.canonicalize().unwrap())
    );
    assert!(
        !first_roots
            .iter()
            .any(|root| root.canonicalize().unwrap() == second_consumer.canonicalize().unwrap())
    );
    drop(observed);

    let starts = server.requests_for("thread/start");
    assert_eq!(starts.len(), 3, "{starts:?}");
    assert_eq!(
        starts[0]["params"]["runtimeWorkspaceRoots"],
        json!(first_roots),
        "{starts:?}"
    );
    assert_eq!(
        starts[1]["params"]["runtimeWorkspaceRoots"],
        json!(second_roots),
        "{starts:?}"
    );
    assert_eq!(
        starts[2]["params"]["runtimeWorkspaceRoots"],
        json!(&child_roots),
        "{starts:?}"
    );

    mcp.finish();
}

#[test]
fn lead_dispatch_refuses_a_thread_that_drops_a_declared_workspace() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, base, issue) = prepared_repository(home.path(), root.path(), &server);
    let consumer = repository.join("consumer");
    let consumer_base = git_repo(&consumer);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        DIRECTOR,
        "unused",
        &repository,
        false,
        false,
    )));
    let responder = native_responder(sim);
    server.answer_fn(move |request| {
        let mut answer = responder(request);
        if request["method"] == "thread/start"
            && let Answer::Result(value) = &mut answer
            && let Some(roots) = value["runtimeWorkspaceRoots"].as_array_mut()
        {
            roots.pop();
        }
        answer
    });
    let mut mcp = Mcp::start(home.path());

    let refused = mcp.call_tool(
        2,
        "lead_dispatch",
        json!({
            "assignment": prepared_lead_assignment_with(
                &issue,
                &base,
                json!([
                    {"path": "consumer", "base": consumer_base},
                    {"path": ".", "base": base}
                ])
            )
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert!(
        tool_error(&refused).contains("declared workspace set was not preserved"),
        "{refused}"
    );
    assert!(
        server.requests_for("turn/start").is_empty(),
        "workspace validation must precede model work"
    );
    for repository in [&repository, &consumer] {
        let inventory = task_worktree::task_inventory(home.path(), repository, 18).unwrap();
        assert!(
            inventory
                .iter()
                .any(|slot| slot.state != task_worktree::TaskState::Free),
            "an uncertain started thread keeps its workspace: {inventory:?}"
        );
    }

    mcp.finish();
}

#[test]
fn lead_second_workspace_failure_releases_the_unused_primary_claim() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    fs::write(
        root.path().join("kit/global/orchestration.toml"),
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"default\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 6\nworktree_pool_size = 18\n",
    )
    .unwrap();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, base, issue) = prepared_repository(home.path(), root.path(), &server);
    let consumer = repository.join("consumer");
    let consumer_base = git_repo(&consumer);
    let startup = task_worktree::current_startup_reservation().unwrap();
    task_worktree::acquire_hierarchy_task_for_startup(
        home.path(),
        &consumer,
        18,
        5,
        "prior-review",
        Some(&consumer_base),
        &startup,
        &|_| false,
    )
    .unwrap();
    let prior = workspace_slot_record(home.path(), &consumer, 5);
    let prior_checkout = PathBuf::from(prior["path"].as_str().unwrap());
    let preserved = prior_checkout.join("keep.txt");
    fs::write(&preserved, "reviewed work\n").unwrap();
    let dirty_legacy = repository.join("consumer-wt6");
    let output = Command::new("git")
        .args([
            "worktree",
            "add",
            "--detach",
            dirty_legacy.to_str().unwrap(),
            consumer_base.as_str(),
        ])
        .current_dir(&consumer)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git worktree add: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dirty_file = dirty_legacy.join("keep.txt");
    fs::write(&dirty_file, "unreleased partial work\n").unwrap();
    let sim = Arc::new(Mutex::new(NativeSim::new(
        DIRECTOR,
        "unused",
        &repository,
        false,
        false,
    )));
    server.answer_fn(native_responder(sim));
    let mut mcp = Mcp::start(home.path());

    let starts_before = server.requests_for("thread/start").len();
    let failed = mcp.call_tool(
        2,
        "lead_dispatch",
        json!({
            "assignment": prepared_lead_assignment_with(
                &issue,
                &base,
                json!([
                    {"path": ".", "base": base},
                    {"path": "consumer", "base": consumer_base}
                ])
            )
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert!(tool_error(&failed).contains("is not clean"), "{failed}");
    assert_eq!(
        server.requests_for("thread/start").len(),
        starts_before,
        "workspace preparation failure must not start a Lead"
    );
    let primary = task_worktree::task_inventory(home.path(), &repository, 18).unwrap();
    let primary_position = primary.iter().find(|slot| slot.index == 6).unwrap();
    assert_eq!(
        primary_position.state,
        task_worktree::TaskState::Free,
        "the unused primary claim must be returned"
    );
    assert_eq!(primary_position.owner, None);
    assert_eq!(workspace_slot_record(home.path(), &consumer, 5), prior);
    assert!(
        dirty_file.is_file(),
        "the dirty legacy position must be preserved instead of reset"
    );
    assert!(
        preserved.is_file(),
        "pre-existing reviewed work must remain in its workspace"
    );

    mcp.finish();
}

#[test]
fn immediate_parent_limits_refuse_excess_children_without_stopping_admitted_work() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    write_limits(
        root.path(),
        "director_max_leads = 1\ndirector_max_executors = 4\nlead_max_executors = 1\nmax_concurrent_executors = 6",
    );
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, base, issue) = prepared_repository(home.path(), root.path(), &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        DIRECTOR,
        "unused",
        &repository,
        false,
        false,
    )));
    server.answer_fn(native_responder(sim));
    let mut mcp = Mcp::start(home.path());

    let lead = mcp.call_tool(
        2,
        "lead_dispatch",
        json!({"requestId": "lead-limit-1", "assignment": prepared_lead_assignment(&issue, &base)}),
        Some(json!({"threadId": DIRECTOR})),
    );
    let lead = result(&lead);
    let lead_thread = lead["executorThreadId"].as_str().unwrap().to_owned();
    let starts_before = server.requests_for("thread/start").len();
    let second_lead = mcp.call_tool(
        3,
        "lead_dispatch",
        json!({"requestId": "lead-limit-2", "assignment": prepared_lead_assignment(&issue, &base)}),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert!(
        tool_error(&second_lead).contains("lead limit is full: 1 of 1"),
        "{second_lead}"
    );
    assert_eq!(server.requests_for("thread/start").len(), starts_before);

    let child = mcp.call(
        4,
        json!({
            "owner": "lead-child-1",
            "requestId": "lead-child-1",
            "base": base,
            "assignment": assignment("first child")
        }),
        Some(json!({"threadId": lead_thread})),
    );
    let child = result(&child);
    let child_thread = child["executorThreadId"].as_str().unwrap().to_owned();
    let starts_before = server.requests_for("thread/start").len();
    let second_child = mcp.call(
        5,
        json!({
            "owner": "lead-child-2",
            "requestId": "lead-child-2",
            "base": base,
            "assignment": assignment("second child")
        }),
        Some(json!({"threadId": lead_thread})),
    );
    assert!(
        tool_error(&second_child).contains("executor limit is full: 1 of 1"),
        "{second_child}"
    );
    assert_eq!(server.requests_for("thread/start").len(), starts_before);
    assert!(
        child_thread != lead_thread,
        "the admitted child remains distinct and preserved"
    );

    mcp.finish();
}

#[test]
fn installation_wide_executor_limit_refuses_a_second_child() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    write_limits(
        root.path(),
        "director_max_leads = 2\ndirector_max_executors = 4\nlead_max_executors = 6\nmax_concurrent_executors = 1",
    );
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, base, issue) = prepared_repository(home.path(), root.path(), &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        DIRECTOR,
        "unused",
        &repository,
        false,
        false,
    )));
    server.answer_fn(native_responder(sim));
    let mut mcp = Mcp::start(home.path());

    let lead = mcp.call_tool(
        2,
        "lead_dispatch",
        json!({"requestId": "global-lead-1", "assignment": prepared_lead_assignment(&issue, &base)}),
        Some(json!({"threadId": DIRECTOR})),
    );
    let lead = result(&lead);
    let lead_thread = lead["executorThreadId"].as_str().unwrap().to_owned();
    let child = mcp.call(
        3,
        json!({
            "owner": "global-child-1",
            "requestId": "global-child-1",
            "base": base,
            "assignment": assignment("first global child")
        }),
        Some(json!({"threadId": lead_thread})),
    );
    assert_eq!(result(&child)["role"], "executor");
    let starts_before = server.requests_for("thread/start").len();
    let second = mcp.call(
        4,
        json!({
            "owner": "global-child-2",
            "requestId": "global-child-2",
            "base": base,
            "assignment": assignment("second global child")
        }),
        Some(json!({"threadId": lead_thread})),
    );
    assert!(
        tool_error(&second).contains("installation-wide executor admission is full: 1 of 1"),
        "{second}"
    );
    assert_eq!(server.requests_for("thread/start").len(), starts_before);

    mcp.finish();
}

#[test]
fn cross_repository_admission_race_keeps_one_global_allowance() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    write_limits(
        root.path(),
        "director_max_leads = 2\ndirector_max_executors = 4\nlead_max_executors = 6\nmax_concurrent_executors = 1",
    );
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (first_repository, first_base, _issue) =
        prepared_repository(home.path(), root.path(), &server);
    let second_repository = root.path().join("second-repo");
    let second_base = git_repo(&second_repository);
    endpoint(home.path(), OTHER_DIRECTOR, OTHER_DIRECTOR, server.port, 0);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        DIRECTOR,
        OTHER_DIRECTOR,
        &first_repository,
        false,
        false,
    )));
    sim.lock()
        .unwrap()
        .threads
        .get_mut(OTHER_DIRECTOR)
        .unwrap()
        .cwd = second_repository.clone();
    server.answer_fn(native_responder(sim));

    let barrier = Arc::new(Barrier::new(3));
    let first_barrier = Arc::clone(&barrier);
    let first_home = home.path().to_path_buf();
    let first_first_base = first_base.clone();
    let first = std::thread::spawn(move || {
        let mut mcp = Mcp::start(&first_home);
        first_barrier.wait();
        let response = mcp.call(
            2,
            json!({
                "owner": "cross-repository-first",
                "requestId": "cross-repository-first",
                "base": first_first_base,
                "assignment": assignment("first cross-repository child")
            }),
            Some(json!({"threadId": DIRECTOR})),
        );
        (
            mcp,
            response,
            DIRECTOR.to_owned(),
            first_first_base,
            "cross-repository-first",
        )
    });
    let second_home = home.path().to_path_buf();
    let second_barrier = Arc::clone(&barrier);
    let second_second_base = second_base.clone();
    let second = std::thread::spawn(move || {
        let mut mcp = Mcp::start(&second_home);
        second_barrier.wait();
        let response = mcp.call(
            3,
            json!({
                "owner": "cross-repository-second",
                "requestId": "cross-repository-second",
                "base": second_second_base,
                "assignment": assignment("second cross-repository child")
            }),
            Some(json!({"threadId": OTHER_DIRECTOR})),
        );
        (
            mcp,
            response,
            OTHER_DIRECTOR.to_owned(),
            second_second_base,
            "cross-repository-second",
        )
    });
    barrier.wait();
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    let first_admitted = first.1["result"]["isError"] == false;
    let second_admitted = second.1["result"]["isError"] == false;
    assert!(
        first_admitted ^ second_admitted,
        "exactly one cross-repository dispatch must consume the single allowance: first={} second={}",
        first.1,
        second.1
    );
    let (winner_mcp, winner, mut loser_mcp, mut loser, loser_thread, loser_base, loser_owner) =
        if first_admitted {
            (
                first.0, first.1, second.0, second.1, second.2, second.3, second.4,
            )
        } else {
            (
                second.0, second.1, first.0, first.1, first.2, first.3, first.4,
            )
        };
    assert_eq!(result(&winner)["role"], "executor", "{winner}");
    if loser["result"]["structuredContent"]["state"] == "busy" {
        loser = loser_mcp.call(
            4,
            json!({
                "owner": format!("{loser_owner}-retry"),
                "requestId": format!("{loser_owner}-retry"),
                "base": loser_base,
                "assignment": assignment("retry the refused cross-repository child")
            }),
            Some(json!({"threadId": loser_thread})),
        );
    }
    assert!(
        tool_error(&loser).contains("installation-wide executor admission is full: 1 of 1"),
        "{loser}"
    );
    assert_eq!(server.requests_for("thread/start").len(), 1);
    let occupied: Vec<_> = [&first_repository, &second_repository]
        .into_iter()
        .flat_map(|repository| task_worktree::task_inventory(home.path(), repository, 6).unwrap())
        .filter(|slot| slot.state != task_worktree::TaskState::Free)
        .collect();
    assert_eq!(occupied.len(), 1, "{occupied:?}");
    assert_eq!(
        occupied[0].owner.as_deref(),
        result(&winner)["run"].as_str()
    );

    winner_mcp.finish();
    loser_mcp.finish();
}

#[test]
fn director_executor_limit_refuses_a_second_direct_child() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    write_limits(
        root.path(),
        "director_max_leads = 2\ndirector_max_executors = 1\nlead_max_executors = 6\nmax_concurrent_executors = 6",
    );
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, base, _issue) = prepared_repository(home.path(), root.path(), &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        DIRECTOR,
        "unused",
        &repository,
        false,
        false,
    )));
    server.answer_fn(native_responder(sim));
    let mut mcp = Mcp::start(home.path());

    let first = mcp.call(
        2,
        json!({
            "owner": "direct-limit-1",
            "requestId": "direct-limit-1",
            "base": base,
            "assignment": assignment("first direct child")
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert_eq!(result(&first)["role"], "executor");
    let starts_before = server.requests_for("thread/start").len();
    let second = mcp.call(
        3,
        json!({
            "owner": "direct-limit-2",
            "requestId": "direct-limit-2",
            "base": base,
            "assignment": assignment("second direct child")
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert!(
        tool_error(&second).contains("executor limit is full: 1 of 1"),
        "{second}"
    );
    assert_eq!(server.requests_for("thread/start").len(), starts_before);
    let first_run = result(&first)["run"].as_str().unwrap().to_owned();
    let stopped = stop_run(&mut mcp, 4, DIRECTOR, &first_run);
    assert_eq!(result(&stopped)["outcome"], "stopped", "{stopped}");
    let released = release_tool(
        &mut mcp,
        5,
        DIRECTOR,
        Some(&first_run),
        "discarded",
        "free the allowance for a replacement",
    );
    assert_eq!(result(&released)["released"], true, "{released}");
    let replacement = mcp.call(
        6,
        json!({
            "owner": "direct-limit-3",
            "requestId": "direct-limit-3",
            "base": base,
            "assignment": assignment("replacement direct child")
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert_eq!(
        result(&replacement)["role"],
        "executor",
        "a released started request must not consume the parent allowance: {replacement}"
    );

    mcp.finish();
}

#[test]
fn lowering_a_parent_limit_preserves_admitted_children_and_blocks_new_ones() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    write_limits(
        root.path(),
        "director_max_leads = 2\ndirector_max_executors = 2\nlead_max_executors = 6\nmax_concurrent_executors = 6",
    );
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, base, _issue) = prepared_repository(home.path(), root.path(), &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        DIRECTOR,
        "unused",
        &repository,
        false,
        false,
    )));
    server.answer_fn(native_responder(sim));
    let mut mcp = Mcp::start(home.path());

    let first = mcp.call(
        2,
        json!({
            "owner": "reduce-child-1",
            "requestId": "reduce-child-1",
            "base": base,
            "assignment": assignment("first admitted child")
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    let first = result(&first);
    let second = mcp.call(
        3,
        json!({
            "owner": "reduce-child-2",
            "requestId": "reduce-child-2",
            "base": base,
            "assignment": assignment("second admitted child")
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    let second = result(&second);
    write_limits(
        root.path(),
        "director_max_leads = 2\ndirector_max_executors = 1\nlead_max_executors = 6\nmax_concurrent_executors = 6",
    );
    let starts_before = server.requests_for("thread/start").len();
    let third = mcp.call(
        4,
        json!({
            "owner": "reduce-child-3",
            "requestId": "reduce-child-3",
            "base": base,
            "assignment": assignment("third child after reduction")
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert!(
        tool_error(&third).contains("executor limit is full: 2 of 1"),
        "{third}"
    );
    assert_eq!(server.requests_for("thread/start").len(), starts_before);
    assert!(first["executorThreadId"].as_str().is_some());
    assert!(second["executorThreadId"].as_str().is_some());

    mcp.finish();
}

#[test]
fn a_missing_role_tool_catalogue_fails_before_the_turn() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (_repository, lead) = delegated_lead_run(home.path(), root.path(), &server);
    let base = lead["base"].as_str().unwrap().to_owned();
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        "unused",
        Path::new(lead["checkout"].as_str().unwrap()),
        false,
        false,
    )));
    let responder = native_responder(sim);
    server.answer_fn(move |request| {
        if request["method"] == "mcpServerStatus/list" {
            Answer::Result(json!({"data": [], "nextCursor": null}))
        } else {
            responder(request)
        }
    });
    let mut mcp = Mcp::start(home.path());
    let starts = server.requests_for("thread/start").len();
    let turns = server.requests_for("turn/start").len();
    let refused = mcp.call(
        2,
        json!({
            "owner": "unfiltered-child",
            "requestId": "missing-catalogue-1",
            "base": base,
            "assignment": assignment("should not start a turn")
        }),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&refused).contains("no 'executors' server entry"),
        "{refused}"
    );
    assert_eq!(server.requests_for("thread/start").len(), starts + 1);
    assert_eq!(server.requests_for("turn/start").len(), turns);
    let retried = mcp.call(
        3,
        json!({
            "owner": "unfiltered-child",
            "requestId": "missing-catalogue-1",
            "base": base,
            "assignment": assignment("should not start a turn")
        }),
        Some(json!({"threadId": LEAD})),
    );
    let structured = structured(&retried);
    assert_eq!(structured["state"], "uncertain", "{retried}");
    assert_eq!(
        server.requests_for("thread/start").len(),
        starts + 1,
        "an uncertain request must not allocate a duplicate child"
    );
    mcp.finish();
}

#[test]
fn director_status_inspects_descendants_without_acknowledging_the_lead_result() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, lead) = delegated_lead_run(home.path(), root.path(), &server);
    let (child, child_value, request_id) = delegated_child_run(home.path(), &server);
    let mut sim = NativeSim::new(DIRECTOR, EXECUTOR, &repository, false, false);
    sim.threads.insert(
        LEAD.to_owned(),
        SimThread::idle(LEAD, Path::new(lead["checkout"].as_str().unwrap())),
    );
    sim.threads.insert(
        EXECUTOR.to_owned(),
        SimThread::active(
            EXECUTOR,
            child._checkout.path(),
            child_value["turnId"].as_str().unwrap(),
        ),
    );
    server.answer_fn(native_responder(Arc::new(Mutex::new(sim))));
    let mut mcp = Mcp::start(home.path());

    let status = mcp.call_tool(
        2,
        "executor_status",
        json!({}),
        Some(json!({"threadId": DIRECTOR})),
    );
    let status = result(&status);
    let runs = status["runs"].as_array().unwrap();
    assert!(runs.iter().any(|run| run["run"] == lead["run"]), "{status}");
    let child_summary = runs
        .iter()
        .find(|run| run["run"] == child_value["run"])
        .unwrap_or_else(|| panic!("descendant missing: {status}"));
    assert_eq!(child_summary["parentThreadId"], LEAD, "{child_summary}");
    assert_eq!(child_summary["rootThreadId"], DIRECTOR, "{child_summary}");
    assert_eq!(
        child_summary["pendingRequests"][0]["requestId"], request_id,
        "{child_summary}"
    );

    let waited = mcp.call_tool(
        3,
        "executor_wait",
        json!({"run": child_value["run"], "timeoutMs": 1000}),
        Some(json!({"threadId": LEAD})),
    );
    let waited = result(&waited);
    assert_eq!(waited["state"], "question", "{waited}");
    assert_eq!(waited["pendingRequests"][0]["requestId"], request_id);

    let again = mcp.call_tool(
        4,
        "executor_status",
        json!({}),
        Some(json!({"threadId": DIRECTOR})),
    );
    let again = result(&again);
    let child_again = again["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["run"] == child_value["run"])
        .unwrap();
    assert_eq!(
        child_again["pendingRequests"][0]["requestId"], request_id,
        "Director inspection must not acknowledge the Lead's result: {again}"
    );

    drop(child);
    mcp.finish();
}

#[test]
fn delegated_lead_asks_upward_and_the_director_replies() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, _lead) = delegated_lead_run(home.path(), root.path(), &server);
    let mut sim = NativeSim::new(DIRECTOR, LEAD, &repository, false, false);
    sim.threads.insert(
        DIRECTOR.to_owned(),
        SimThread::idle(DIRECTOR, &repository).with_ds(),
    );
    sim.threads.insert(
        LEAD.to_owned(),
        SimThread::idle(LEAD, &repository).with_ds(),
    );
    let sim = Arc::new(Mutex::new(sim));
    server.answer_fn(native_responder(sim));
    let mut mcp = Mcp::start(home.path());

    let asked = mcp.call_tool(
        2,
        "executor_ask_lead",
        json!({"text": "upward decision?", "clientMessageId": "lead-up"}),
        Some(json!({"threadId": LEAD})),
    );
    let asked = result(&asked);
    assert_eq!(asked["delivery"]["status"], "delivered", "{asked}");
    let request_id = asked["requestId"].as_str().unwrap().to_owned();
    let replied = mcp.call_tool(
        3,
        "executor_reply",
        json!({"requestId": request_id, "text": "director answer"}),
        Some(json!({"threadId": DIRECTOR})),
    );
    let replied = result(&replied);
    assert_eq!(replied["delivery"]["status"], "delivered", "{replied}");

    mcp.finish();
}

#[test]
fn delegated_lead_messages_downward_and_a_sibling_lead_is_rejected() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, _lead) = delegated_lead_run(home.path(), root.path(), &server);
    let (child, child_value, _) = delegated_child_run(home.path(), &server);
    let sibling_run = synthetic_run(home.path(), OTHER_LEAD, OTHER_EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        child._checkout.path(),
        false,
        false,
    )));
    server.answer_fn(native_responder(sim));
    let mut mcp = Mcp::start(home.path());

    let sent = mcp.call_tool(
        2,
        "executor_message",
        json!({"run": child_value["run"], "text": "downward instruction"}),
        Some(json!({"threadId": LEAD})),
    );
    let sent = result(&sent);
    assert_eq!(sent["delivery"]["status"], "delivered", "{sent}");

    let sibling = mcp.call_tool(
        3,
        "executor_message",
        json!({"run": child_value["run"], "text": "cross-team"}),
        Some(json!({"threadId": OTHER_LEAD})),
    );
    assert!(
        tool_error(&sibling).contains("owned by the calling lead"),
        "{sibling}"
    );

    drop(child);
    drop(sibling_run);
    drop(repository);
    mcp.finish();
}

#[test]
fn stopping_a_lead_closes_its_subtree_and_leaves_a_direct_sibling_running() {
    let home = home();
    let root = tempfile::tempdir().unwrap();
    install_kit(home.path(), root.path());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let (repository, lead) = delegated_lead_run(home.path(), root.path(), &server);
    let (child, child_value, _) = delegated_child_run(home.path(), &server);
    let sibling = synthetic_run(home.path(), DIRECTOR, OTHER_EXECUTOR, &server);
    let mut sim = NativeSim::new(DIRECTOR, LEAD, &repository, false, false);
    sim.threads.insert(
        LEAD.to_owned(),
        SimThread::active(LEAD, &repository, "lead-turn").with_ds(),
    );
    sim.threads.insert(
        EXECUTOR.to_owned(),
        SimThread::active(
            EXECUTOR,
            child._checkout.path(),
            child_value["turnId"].as_str().unwrap(),
        ),
    );
    sim.threads.insert(
        OTHER_EXECUTOR.to_owned(),
        SimThread::active(OTHER_EXECUTOR, sibling._checkout.path(), "initial-turn"),
    );
    let sim = Arc::new(Mutex::new(sim));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let stopped = mcp.call_tool(
        2,
        "executor_stop",
        json!({"run": lead["run"]}),
        Some(json!({"threadId": DIRECTOR})),
    );
    let stopped = result(&stopped);
    assert_eq!(stopped["subtree"], true, "{stopped}");
    assert_eq!(stopped["outcome"], "stopped", "{stopped}");
    assert_eq!(stopped["lead"]["outcome"], "stopped", "{stopped}");
    assert!(
        stopped["runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|run| run["run"] == child_value["run"]),
        "{stopped}"
    );
    assert_eq!(sim.lock().unwrap().threads[LEAD].state(), "idle");
    assert_eq!(sim.lock().unwrap().threads[EXECUTOR].state(), "idle");
    assert_eq!(
        sim.lock().unwrap().threads[OTHER_EXECUTOR].state(),
        "active",
        "a direct Executor outside the Lead subtree must keep running"
    );
    let marker = home
        .path()
        .join("harness/executor-pool/requests")
        .join(LEAD)
        .join("STOPPED.json");
    assert!(marker.is_file(), "the subtree stop marker was not written");

    let starts_before = server.requests_for("thread/start").len();
    let refused = mcp.call(
        3,
        json!({
            "owner": "after-stop-child",
            "requestId": "after-stop-child",
            "base": child_value["base"],
            "assignment": assignment("must not start after stop")
        }),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&refused).contains("subtree") && tool_error(&refused).contains("stopped"),
        "{refused}"
    );
    assert_eq!(server.requests_for("thread/start").len(), starts_before);

    let resumed = mcp.call_tool(
        4,
        "executor_resume",
        json!({"run": lead["run"], "mode": "exact"}),
        Some(json!({"threadId": DIRECTOR})),
    );
    let resumed = result(&resumed);
    assert!(resumed["subtreeStopError"].is_null(), "{resumed}");
    assert!(
        !marker.exists(),
        "explicit Lead resume must clear the subtree stop marker"
    );

    drop(child);
    drop(sibling);
    mcp.finish();
}

#[test]
fn releasing_a_lead_workspace_waits_for_its_unreviewed_children() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let lead = release_run(home.path(), DIRECTOR, LEAD, &server);
    let mut lead_value: Value = serde_json::from_slice(&fs::read(&lead.receipt).unwrap()).unwrap();
    lead_value["role"] = json!("lead");
    lead_value["parentThreadId"] = json!(DIRECTOR);
    lead_value["rootThreadId"] = json!(DIRECTOR);
    fs::write(
        &lead.receipt,
        serde_json::to_vec_pretty(&lead_value).unwrap(),
    )
    .unwrap();
    let child = release_run(home.path(), LEAD, EXECUTOR, &server);
    let mut sim = NativeSim::new(DIRECTOR, LEAD, &lead.checkout, false, false);
    sim.threads.insert(
        EXECUTOR.to_owned(),
        SimThread::idle(EXECUTOR, &child.checkout),
    );
    server.answer_fn(native_responder(Arc::new(Mutex::new(sim))));
    let mut mcp = Mcp::start(home.path());

    let refused = mcp.call_tool(
        2,
        "executor_release",
        json!({
            "run": lead.run,
            "disposition": "discarded",
            "reason": "director review"
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert!(
        tool_error(&refused).contains("unreleased descendant"),
        "{refused}"
    );
    assert!(tool_error(&refused).contains(&child.run), "{refused}");

    let child_released = mcp.call_tool(
        3,
        "executor_release",
        json!({
            "run": child.run,
            "disposition": "discarded",
            "reason": "lead accepted the child"
        }),
        Some(json!({"threadId": LEAD})),
    );
    let child_released = result(&child_released);
    assert_eq!(child_released["released"], true, "{child_released}");

    let lead_released = mcp.call_tool(
        4,
        "executor_release",
        json!({
            "run": lead.run,
            "disposition": "discarded",
            "reason": "director accepted the lead"
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    let lead_released = result(&lead_released);
    assert_eq!(lead_released["released"], true, "{lead_released}");
    assert_eq!(lead_released["slotFreed"], true, "{lead_released}");

    mcp.finish();
}

#[test]
fn wait_returns_a_pending_question_and_a_terminal_result_separately() {
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
    seed_active_turn(&sim, EXECUTOR, "initial-turn");
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let question = mcp.call_tool(
        2,
        "executor_ask_lead",
        json!({"text": "which result should I return?", "clientMessageId": "wait-question"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let request_id = result(&question)["requestId"].as_str().unwrap().to_owned();
    complete_turn(
        &sim,
        EXECUTOR,
        "initial-turn",
        "result while question is open",
    );

    let waited = mcp.call_tool(
        3,
        "executor_wait",
        json!({"timeoutMs": 1000}),
        Some(json!({"threadId": LEAD})),
    );
    let waited = result(&waited);
    assert_eq!(waited["state"], "question", "{waited}");
    assert_eq!(
        waited["pendingRequests"][0]["requestId"], request_id,
        "{waited}"
    );
    assert_eq!(
        waited["pendingRequests"][0]["kind"], "reply-request",
        "{waited}"
    );
    assert_eq!(waited["results"][0]["state"], "completed", "{waited}");
    assert_eq!(
        waited["results"][0]["finalMessage"],
        "result while question is open"
    );

    drop(run);
    mcp.finish();
}

#[test]
fn an_actionable_question_interrupts_a_wait_without_starting_work() {
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
    seed_active_turn(&sim, EXECUTOR, "initial-turn");
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    let started = Instant::now();

    let (waited, request_id) = std::thread::scope(|scope| {
        let question = scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            let mut executor = Mcp::start(home.path());
            let asked = executor.call_tool(
                2,
                "executor_ask_lead",
                json!({"text": "interrupting question", "clientMessageId": "interrupt-question"}),
                Some(json!({"threadId": EXECUTOR})),
            );
            let id = result(&asked)["requestId"].as_str().unwrap().to_owned();
            executor.finish();
            id
        });
        let waited = mcp.call_tool(
            2,
            "executor_wait",
            json!({"timeoutMs": 10000}),
            Some(json!({"threadId": LEAD})),
        );
        (waited, question.join().unwrap())
    });

    let waited = result(&waited);
    assert_eq!(waited["state"], "question", "{waited}");
    assert_eq!(
        waited["pendingRequests"][0]["requestId"], request_id,
        "{waited}"
    );
    assert!(waited["result"].is_null(), "{waited}");
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "the question did not interrupt the wait: {:?}",
        started.elapsed()
    );

    drop(run);
    mcp.finish();
}

#[test]
fn wait_times_out_without_model_work_and_refuses_an_executor_caller() {
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
    seed_active_turn(&sim, EXECUTOR, "initial-turn");
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    let before = server.requests_for("turn/start").len() + server.requests_for("turn/steer").len();
    let started = Instant::now();

    let waited = mcp.call_tool(
        2,
        "executor_wait",
        json!({"timeoutMs": 200}),
        Some(json!({"threadId": LEAD})),
    );
    let waited = result(&waited);
    assert_eq!(waited["state"], "timeout", "{waited}");
    assert!(waited["result"].is_null(), "{waited}");
    let timeout_bytes = serde_json::to_string(waited).unwrap();
    println!("quiet timeout response bytes={}", timeout_bytes.len());
    assert!(timeout_bytes.len() < 128, "{waited}");
    assert!(
        started.elapsed() >= Duration::from_millis(100),
        "the wait returned before its bounded interval: {:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the wait exceeded its bound: {:?}",
        started.elapsed()
    );
    assert_eq!(
        server.requests_for("turn/start").len() + server.requests_for("turn/steer").len(),
        before
    );

    let mut executor = Mcp::start(home.path());
    let denied = executor.call_tool(
        2,
        "executor_wait",
        json!({"timeoutMs": 100}),
        Some(json!({"threadId": EXECUTOR})),
    );
    assert!(tool_error(&denied).contains("verified lead"), "{denied}");

    drop(run);
    executor.finish();
    mcp.finish();
}

#[test]
fn wait_does_not_repeat_a_reply_that_was_already_delivered() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = release_run(home.path(), LEAD, EXECUTOR, &server);
    let mut receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    receipt["leadMessages"] = json!([{
        "schema": 1,
        "id": "answered-request",
        "kind": "reply-request",
        "status": "delivered",
        "contentId": "answered"
    }]);
    receipt["messages"] = json!([{
        "schema": 1,
        "id": "reply-attempt",
        "replyTo": "answered-request",
        "status": "queued"
    }]);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        &run.checkout,
        false,
        false,
    )));
    seed_active_turn(&sim, EXECUTOR, "initial-turn");
    complete_turn(&sim, EXECUTOR, "initial-turn", "completed after the reply");
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let waited = mcp.call_tool(
        2,
        "executor_wait",
        json!({"timeoutMs": 200}),
        Some(json!({"threadId": LEAD})),
    );
    let waited = result(&waited);
    assert_ne!(waited["state"], "question", "{waited}");
    assert!(
        waited["pendingRequests"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "{waited}"
    );
    let released = release_tool(
        &mut mcp,
        3,
        LEAD,
        Some(&run.run),
        "discarded",
        "the referenced question was answered",
    );
    assert_eq!(result(&released)["released"], true, "{released}");

    mcp.finish();
    drop(run);
}

#[test]
fn wait_returns_the_exact_current_final_message_and_keeps_a_native_locator() {
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
    seed_active_turn(&sim, EXECUTOR, "initial-turn");
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    let completion = std::thread::spawn({
        let sim = Arc::clone(&sim);
        move || {
            std::thread::sleep(Duration::from_millis(150));
            complete_turn(&sim, EXECUTOR, "initial-turn", "done\n\u{2603}");
        }
    });

    let waited = mcp.call_tool(
        2,
        "executor_wait",
        json!({"run": format!("mcp-test-{EXECUTOR}"), "timeoutMs": 1000}),
        Some(json!({"threadId": LEAD})),
    );
    completion.join().unwrap();
    let waited = result(&waited);
    assert_eq!(waited["state"], "result", "{waited}");
    assert_eq!(waited["result"]["state"], "completed", "{waited}");
    assert_eq!(waited["result"]["outcome"], "completed", "{waited}");
    assert_eq!(
        waited["result"]["finalMessage"], "done\n\u{2603}",
        "{waited}"
    );
    assert_eq!(waited["result"]["output"], "present", "{waited}");
    assert_eq!(waited["result"]["finalMessageTruncated"], false);
    assert_eq!(waited["result"]["native"]["threadId"], EXECUTOR);
    assert_eq!(waited["result"]["native"]["turnId"], "initial-turn");
    assert_eq!(
        waited["result"]["detailLocator"]["statusTool"],
        "executor_status"
    );

    let status = mcp.call_tool(
        3,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    let status = result(&status);
    assert_eq!(status["runs"][0]["executorThreadId"], EXECUTOR);
    assert_eq!(status["runs"][0]["executorNative"]["status"], "idle");

    let repeated = mcp.call_tool(
        4,
        "executor_wait",
        json!({"run": format!("mcp-test-{EXECUTOR}"), "timeoutMs": 1000}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(
        result(&repeated)["result"]["finalMessage"],
        "done\n\u{2603}"
    );

    drop(run);
    mcp.finish();
}

#[test]
fn wait_distinguishes_empty_output_from_provider_and_read_failures() {
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
    seed_active_turn(&sim, EXECUTOR, "initial-turn");
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    complete_turn(&sim, EXECUTOR, "initial-turn", "");
    let empty_response = mcp.call_tool(
        2,
        "executor_wait",
        json!({"timeoutMs": 1000}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(
        empty_response["result"]["isError"], true,
        "{empty_response}"
    );
    let empty_error = tool_error(&empty_response);
    let empty = structured(&empty_response);
    assert_eq!(empty["results"][0]["state"], "output-defect", "{empty}");
    assert_eq!(empty["results"][0]["outcome"], "empty-output", "{empty}");
    assert!(empty["results"][0]["finalMessage"].is_null(), "{empty}");
    assert!(
        empty_error.contains("empty final assistant message"),
        "{empty}"
    );

    fail_turn(&sim, EXECUTOR, "initial-turn", "provider route failed");
    let failed_response = mcp.call_tool(
        3,
        "executor_wait",
        json!({"timeoutMs": 1000}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(
        failed_response["result"]["isError"], true,
        "{failed_response}"
    );
    let failed = structured(&failed_response);
    assert_eq!(failed["results"][0]["state"], "failed", "{failed}");
    assert_eq!(failed["results"][0]["outcome"], "failed", "{failed}");
    assert_eq!(
        failed["results"][0]["error"], "provider route failed",
        "{failed}"
    );

    sim.lock().unwrap().read_error = Some("native thread read rejected".into());
    let started = Instant::now();
    let unreadable_response = mcp.call_tool(
        4,
        "executor_wait",
        json!({"timeoutMs": 3000}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(
        unreadable_response["result"]["isError"], true,
        "{unreadable_response}"
    );
    let unreadable_error = tool_error(&unreadable_response);
    let unreadable = structured(&unreadable_response);
    assert_eq!(unreadable["state"], "observation-error", "{unreadable}");
    assert!(
        unreadable_error.contains("native thread read rejected"),
        "{unreadable}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "a read failure was reported as a wait timeout: {:?}",
        started.elapsed()
    );

    drop(run);
    mcp.finish();
}

#[test]
fn wait_across_owned_runs_returns_results_and_acknowledges_receipt() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let first = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let second = synthetic_run(home.path(), LEAD, OTHER_EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        first._checkout.path(),
        false,
        false,
    )));
    sim.lock().unwrap().threads.insert(
        OTHER_EXECUTOR.to_owned(),
        SimThread::active(OTHER_EXECUTOR, second._checkout.path(), "initial-turn"),
    );
    seed_active_turn(&sim, EXECUTOR, "initial-turn");
    complete_turn(&sim, EXECUTOR, "initial-turn", "first done");
    complete_turn(&sim, OTHER_EXECUTOR, "initial-turn", "second done");
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let waited = mcp.call_tool(
        2,
        "executor_wait",
        json!({"timeoutMs": 0}),
        Some(json!({"threadId": LEAD})),
    );
    let waited = result(&waited);
    assert_eq!(waited["state"], "result", "{waited}");
    let results = waited["results"].as_array().unwrap();
    assert_eq!(results.len(), 2, "{waited}");
    assert!(results.iter().all(|result| result["run"].is_string()));
    assert!(results.iter().all(|result| result["resultId"].is_string()));
    let first_id = results
        .iter()
        .find(|result| result["run"] == format!("mcp-test-{EXECUTOR}"))
        .unwrap()["resultId"]
        .as_str()
        .unwrap()
        .to_owned();
    let second_id = results
        .iter()
        .find(|result| result["run"] == format!("mcp-test-{OTHER_EXECUTOR}"))
        .unwrap()["resultId"]
        .as_str()
        .unwrap()
        .to_owned();

    let one = mcp.call_tool(
        3,
        "executor_wait",
        json!({"timeoutMs": 0, "acknowledge": [first_id]}),
        Some(json!({"threadId": LEAD})),
    );
    let one = result(&one);
    assert_eq!(one["results"].as_array().unwrap().len(), 1, "{one}");
    assert_eq!(one["results"][0]["resultId"], second_id, "{one}");

    let settled = mcp.call_tool(
        4,
        "executor_wait",
        json!({"timeoutMs": 0, "acknowledge": [second_id]}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&settled)["state"], "idle", "{settled}");

    let retry = mcp.call_tool(
        5,
        "executor_wait",
        json!({"timeoutMs": 0, "acknowledge": [first_id, second_id]}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&retry)["state"], "idle", "{retry}");
    mcp.finish();

    let mut reconnected = Mcp::start(home.path());
    let quiet = reconnected.call_tool(
        6,
        "executor_wait",
        json!({"timeoutMs": 0}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&quiet)["state"], "idle", "{quiet}");

    let reread = reconnected.call_tool(
        7,
        "executor_wait",
        json!({"run": format!("mcp-test-{EXECUTOR}"), "timeoutMs": 0}),
        Some(json!({"threadId": LEAD})),
    );
    let reread = result(&reread);
    assert_eq!(reread["state"], "result", "{reread}");
    assert_eq!(reread["result"]["resultId"], first_id, "{reread}");

    drop(first);
    drop(second);
    reconnected.finish();
}

#[test]
fn wait_across_runs_returns_another_runs_question_without_waiting() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let first = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let second = synthetic_run(home.path(), LEAD, OTHER_EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        first._checkout.path(),
        false,
        false,
    )));
    sim.lock().unwrap().threads.insert(
        OTHER_EXECUTOR.to_owned(),
        SimThread::active(OTHER_EXECUTOR, second._checkout.path(), "initial-turn"),
    );
    seed_active_turn(&sim, EXECUTOR, "initial-turn");
    complete_turn(&sim, EXECUTOR, "initial-turn", "first done");
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    let started = Instant::now();

    let (waited, request_id) = std::thread::scope(|scope| {
        let question = scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(150));
            let mut executor = Mcp::start(home.path());
            let asked = executor.call_tool(
                2,
                "executor_ask_lead",
                json!({"text": "which result should I use?", "clientMessageId": "all-run-question"}),
                Some(json!({"threadId": OTHER_EXECUTOR})),
            );
            let id = result(&asked)["requestId"].as_str().unwrap().to_owned();
            executor.finish();
            id
        });
        let waited = mcp.call_tool(
            3,
            "executor_wait",
            json!({"timeoutMs": 15000}),
            Some(json!({"threadId": LEAD})),
        );
        (waited, question.join().unwrap())
    });

    let waited = result(&waited);
    assert_eq!(waited["state"], "question", "{waited}");
    assert_eq!(
        waited["pendingRequests"][0]["run"],
        format!("mcp-test-{OTHER_EXECUTOR}"),
        "{waited}"
    );
    assert_eq!(
        waited["pendingRequests"][0]["requestId"], request_id,
        "{waited}"
    );
    assert!(
        waited["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["run"] == format!("mcp-test-{EXECUTOR}")),
        "{waited}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "another run's question did not interrupt the wait: {:?}",
        started.elapsed()
    );

    drop(first);
    drop(second);
    mcp.finish();
}

#[test]
fn invalid_acknowledgement_has_no_effect_and_a_later_turn_is_not_hidden() {
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
    seed_active_turn(&sim, EXECUTOR, "initial-turn");
    complete_turn(&sim, EXECUTOR, "initial-turn", "first result");
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let waited = mcp.call_tool(
        2,
        "executor_wait",
        json!({"timeoutMs": 0}),
        Some(json!({"threadId": LEAD})),
    );
    let result_id = result(&waited)["results"][0]["resultId"]
        .as_str()
        .unwrap()
        .to_owned();
    let before: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert!(before["waitAcknowledgements"].is_null(), "{before}");

    let invalid = mcp.call_tool(
        3,
        "executor_wait",
        json!({"timeoutMs": 0, "acknowledge": ["wait-result-000000000000000000000000"]}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&invalid).contains("not exactly one current terminal result"),
        "{invalid}"
    );
    let after: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert!(after["waitAcknowledgements"].is_null(), "{after}");

    let acknowledged = mcp.call_tool(
        4,
        "executor_wait",
        json!({"timeoutMs": 0, "acknowledge": [result_id]}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&acknowledged)["state"], "idle", "{acknowledged}");

    {
        let mut sim = sim.lock().unwrap();
        let thread = sim.threads.get_mut(EXECUTOR).unwrap();
        thread.turns.push(json!({
            "id": "later-turn",
            "status": "inProgress",
            "items": []
        }));
    }
    let mut receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    receipt["turnId"] = json!("later-turn");
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();

    let pending = mcp.call_tool(
        5,
        "executor_wait",
        json!({"timeoutMs": 0}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&pending)["state"], "timeout", "{pending}");

    complete_turn(&sim, EXECUTOR, "later-turn", "later result");
    let later = mcp.call_tool(
        6,
        "executor_wait",
        json!({"timeoutMs": 1000}),
        Some(json!({"threadId": LEAD})),
    );
    let later = result(&later);
    assert_eq!(later["state"], "result", "{later}");
    assert_eq!(later["results"][0]["finalMessage"], "later result");
    assert_ne!(later["results"][0]["resultId"], result_id);

    drop(run);
    mcp.finish();
}

#[test]
fn a_pending_wait_is_cancelled_without_executor_effects() {
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
    seed_active_turn(&sim, EXECUTOR, "initial-turn");
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    let before = turn_requests(&server);

    mcp.send(json!({
        "jsonrpc": "2.0",
        "id": 40,
        "method": "tools/call",
        "params": {
            "name": "executor_wait",
            "arguments": {"timeoutMs": 1_800_000},
            "_meta": {"threadId": LEAD}
        }
    }));
    std::thread::sleep(Duration::from_millis(250));
    mcp.send(json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": {"requestId": 40}
    }));
    std::thread::sleep(Duration::from_millis(250));
    mcp.send(json!({"jsonrpc":"2.0","id":41,"method":"ping"}));
    let reply = mcp.read();
    assert_eq!(reply["id"], 41, "{reply}");
    assert_eq!(reply["result"], json!({}), "{reply}");
    assert_eq!(turn_requests(&server), before);
    assert!(server.requests_for("turn/interrupt").is_empty());
    assert_eq!(sim.lock().unwrap().threads[EXECUTOR].state(), "active");

    drop(run);
    mcp.finish();
}

#[test]
fn wait_reports_idle_without_owned_runs_and_rejects_an_over_limit_bound() {
    let home = home();
    let mut mcp = Mcp::start(home.path());
    let idle = mcp.call_tool(
        2,
        "executor_wait",
        json!({"timeoutMs": 0}),
        Some(json!({"threadId": LEAD})),
    );
    let idle = result(&idle);
    assert_eq!(idle["state"], "idle", "{idle}");
    let idle_bytes = serde_json::to_string(idle).unwrap();
    println!("quiet idle response bytes={}", idle_bytes.len());
    assert!(idle_bytes.len() <= 32, "{idle}");

    let over = mcp.call_tool(
        3,
        "executor_wait",
        json!({"timeoutMs": 1_800_001}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(tool_error(&over).contains("limit is 1800000"), "{over}");
    mcp.finish();
}

#[test]
fn an_older_completion_cannot_settle_newer_work_or_change_a_timeout_boundary() {
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
    {
        let mut sim_state = sim.lock().unwrap();
        let mut thread = SimThread::active(EXECUTOR, run._checkout.path(), "initial-turn");
        thread.turns.insert(
            0,
            json!({
                "id": "older-turn",
                "status": "completed",
                "items": [{
                    "id": "older-message",
                    "type": "agentMessage",
                    "text": "an older result"
                }]
            }),
        );
        sim_state.threads.insert(EXECUTOR.to_owned(), thread);
    }
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let first = mcp.call_tool(
        2,
        "executor_wait",
        json!({"timeoutMs": 200}),
        Some(json!({"threadId": LEAD})),
    );
    let first = result(&first);
    assert_eq!(first["state"], "timeout", "{first}");
    assert_eq!(first["result"], Value::Null, "{first}");

    complete_turn(&sim, EXECUTOR, "initial-turn", "the newer result");
    let later = mcp.call_tool(
        3,
        "executor_wait",
        json!({"timeoutMs": 1000}),
        Some(json!({"threadId": LEAD})),
    );
    let later = result(&later);
    assert_eq!(later["state"], "result", "{later}");
    assert_eq!(later["results"][0]["finalMessage"], "the newer result");
    assert_ne!(later["results"][0]["finalMessage"], "an older result");

    drop(run);
    mcp.finish();
}

#[test]
fn executor_notices_reach_the_lead_once_and_an_indeterminate_retry_is_not_resent() {
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
        json!({
            "text": "bounded terminal result",
            "clientMessageId": "result-notice"
        }),
        Some(json!({"threadId": EXECUTOR})),
    );
    let first = result(&first);
    let message_id = first["messageId"].as_str().unwrap().to_owned();
    assert_eq!(first["delivery"]["status"], "delivered", "{first}");
    assert_eq!(first["awaitingReply"], false, "{first}");
    assert!(
        first["delivery"]["detail"]
            .as_str()
            .unwrap()
            .contains("whether the recipient applied it"),
        "{first}"
    );
    let sent = server.requests_for("turn/start").len() + server.requests_for("turn/steer").len();

    let repeated = mcp.call_tool(
        3,
        "executor_message",
        json!({
            "text": "bounded terminal result",
            "clientMessageId": "result-notice"
        }),
        Some(json!({"threadId": EXECUTOR})),
    );
    let repeated = result(&repeated);
    assert_eq!(repeated["messageId"], message_id, "{repeated}");
    assert_eq!(repeated["delivery"]["status"], "delivered", "{repeated}");
    assert_eq!(
        server.requests_for("turn/start").len() + server.requests_for("turn/steer").len(),
        sent,
        "a delivered result was sent again"
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert_eq!(
        receipt["leadMessages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["id"].as_str() == Some(message_id.as_str()))
            .count(),
        1
    );
    let lead_items = sim.lock().unwrap().threads[LEAD]
        .turns
        .iter()
        .flat_map(|turn| turn["items"].as_array().into_iter().flatten())
        .filter(|item| item["clientId"].as_str() == Some(message_id.as_str()))
        .count();
    assert_eq!(
        lead_items, 1,
        "the lead did not receive exactly one native item"
    );

    sim.lock().unwrap().lose_next_ack = Some("turn/steer".to_owned());
    let lost = mcp.call_tool(
        4,
        "executor_message",
        json!({
            "text": "result with unknown acceptance",
            "clientMessageId": "result-lost"
        }),
        Some(json!({"threadId": EXECUTOR})),
    );
    assert_eq!(lost["result"]["isError"], true, "{lost}");
    let lost = structured(&lost);
    assert_eq!(lost["delivery"]["status"], "indeterminate", "{lost}");
    let after_lost =
        server.requests_for("turn/start").len() + server.requests_for("turn/steer").len();
    let retry = mcp.call_tool(
        5,
        "executor_message",
        json!({
            "text": "result with unknown acceptance",
            "clientMessageId": "result-lost"
        }),
        Some(json!({"threadId": EXECUTOR})),
    );
    let retry = structured(&retry);
    assert_eq!(retry["delivery"]["status"], "indeterminate", "{retry}");
    assert_eq!(
        server.requests_for("turn/start").len() + server.requests_for("turn/steer").len(),
        after_lost,
        "an indeterminate result was resent"
    );

    drop(run);
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
fn above_one_mebibyte_literal_message_survives_reconnect_without_replay() {
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

    let large = above_one_mebibyte();
    let sent = mcp.call_tool(
        10,
        "executor_message",
        json!({"text": large, "clientMessageId": "message-above-one-mebibyte"}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&sent)["delivery"]["status"], "delivered");
    let native = server
        .requests()
        .into_iter()
        .rev()
        .find(|request| {
            matches!(
                request["method"].as_str(),
                Some("turn/start" | "turn/steer")
            )
        })
        .unwrap();
    let delivered = native["params"]["input"][0]["text"].as_str().unwrap();
    assert_eq!(delivered.len(), large.len());
    assert!(
        delivered == large,
        "the literal message changed in transport"
    );
    let before_reconnect = turn_requests(&server);
    assert_no_file_relay(home.path());

    mcp.finish();
    let mut reconnected = Mcp::start(home.path());
    let repeated = reconnected.call_tool(
        11,
        "executor_message",
        json!({"text": large, "clientMessageId": "message-above-one-mebibyte"}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&repeated)["delivery"]["status"], "delivered");
    assert_eq!(
        turn_requests(&server),
        before_reconnect,
        "reconnect replayed the accepted message"
    );
    let status = reconnected.call_tool(
        12,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        result(&status)["runs"][0]["pendingRequests"]
            .as_array()
            .unwrap()
            .is_empty(),
        "a phantom request was recorded"
    );
    assert_no_file_relay(home.path());

    let receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    let messages = receipt["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1, "reconnect duplicated the attempt record");
    let entry = messages.last().unwrap();
    assert_eq!(entry["status"], "delivered");
    assert_eq!(entry["bytes"].as_u64(), Some(large.len() as u64));
    assert!(entry["payloadPath"].is_null());

    drop(run);
    reconnected.finish();
}

#[test]
fn above_one_mebibyte_question_and_reply_use_no_file_relay() {
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

    let question_text = above_one_mebibyte();
    let asked = mcp.call_tool(
        20,
        "executor_ask_lead",
        json!({"text": question_text, "clientMessageId": "large-question"}),
        Some(json!({"threadId": EXECUTOR})),
    );
    let asked = result(&asked);
    assert_eq!(asked["delivery"]["status"], "delivered");
    assert_eq!(asked["awaitingReply"], true);
    let request_id = asked["requestId"].as_str().unwrap().to_owned();
    let native_question = server
        .requests()
        .into_iter()
        .rev()
        .find(|request| request["method"] == "turn/steer")
        .unwrap();
    let delivered_question = native_question["params"]["input"][0]["text"]
        .as_str()
        .unwrap();
    assert!(delivered_question.ends_with(&question_text));
    assert_no_file_relay(home.path());

    let reply_text = above_one_mebibyte();
    let replied = mcp.call_tool(
        21,
        "executor_reply",
        json!({"requestId": request_id, "text": reply_text}),
        Some(json!({"threadId": LEAD})),
    );
    let replied = result(&replied);
    assert_eq!(replied["delivery"]["status"], "delivered");
    assert_eq!(replied["resolved"], true);
    let native_reply = server
        .requests()
        .into_iter()
        .rev()
        .find(|request| request["method"] == "turn/start")
        .unwrap();
    let delivered_reply = native_reply["params"]["input"][0]["text"].as_str().unwrap();
    assert!(delivered_reply.ends_with(&reply_text));
    assert_no_file_relay(home.path());

    let receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert_eq!(receipt["leadMessages"].as_array().unwrap().len(), 1);
    assert!(receipt["leadMessages"][0]["payloadPath"].is_null());
    assert_eq!(receipt["messages"].as_array().unwrap().len(), 1);
    let reply = receipt["messages"].as_array().unwrap().last().unwrap();
    assert!(reply["payloadPath"].is_null());
    assert!(reply["bytes"].as_u64().unwrap() > 1024 * 1024, "{reply}");

    drop(run);
    mcp.finish();
}

#[test]
fn above_one_mebibyte_native_record_is_reread_after_interruption_and_reconnect() {
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
    seed_active_turn(&sim, EXECUTOR, "initial-turn");
    let record = above_one_mebibyte();
    complete_turn(&sim, EXECUTOR, "initial-turn", &record);
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    let before_reconnect = turn_requests(&server);

    sim.lock().unwrap().read_error = Some("interrupted partial native record".into());
    let interrupted = mcp.call_tool(
        30,
        "executor_wait",
        json!({"timeoutMs": 1000}),
        Some(json!({"threadId": LEAD})),
    );
    let interrupted = structured(&interrupted);
    assert_eq!(interrupted["state"], "observation-error");
    assert!(
        interrupted["error"]
            .as_str()
            .unwrap()
            .contains("interrupted partial native record")
    );

    sim.lock().unwrap().read_error = None;
    mcp.finish();
    let mut reconnected = Mcp::start(home.path());
    let reread = reconnected.call_tool(
        31,
        "executor_wait",
        json!({"timeoutMs": 1000}),
        Some(json!({"threadId": LEAD})),
    );
    let reread = result(&reread);
    assert_eq!(reread["state"], "result");
    assert_eq!(
        reread["results"][0]["finalMessageBytes"].as_u64(),
        Some(record.len() as u64)
    );
    assert_eq!(reread["results"][0]["finalMessageTruncated"], true);
    assert!(reread["results"][0]["finalMessage"].as_str().unwrap().len() <= 32 * 1024);
    assert_eq!(
        turn_requests(&server),
        before_reconnect,
        "re-reading a native record started or replayed model work"
    );
    assert!(
        reread["pendingRequests"].as_array().unwrap().is_empty(),
        "a reconnect created a phantom request"
    );
    assert_no_file_relay(home.path());

    let status = reconnected.call_tool(
        32,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&status)["runs"][0]["executorThreadId"], EXECUTOR);
    assert!(
        result(&status)["runs"][0]["pendingRequests"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    drop(run);
    reconnected.finish();
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
    receipt["lease"]["appServer"]["pid"] = json!(0);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let stale = mcp.call_tool(
        12,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&stale).contains("could not be proved ended"),
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

fn commit_checkout(root: &Path, message: &str) -> String {
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(["-C", root.to_str().unwrap()])
            .args(["-c", "user.name=Executor MCP Test"])
            .args(["-c", "user.email=executor-mcp@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    git(&["add", "-A"]);
    git(&["commit", "-qm", message]);
    String::from_utf8(git(&["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned()
}

fn terminal(process_id: &str, command: &str) -> SimTerminal {
    SimTerminal {
        process_id: process_id.into(),
        item_id: format!("item-{process_id}"),
        command: command.into(),
    }
}

fn stop_run(mcp: &mut Mcp, id: u64, caller: &str, run: &str) -> Value {
    mcp.call_tool(
        id,
        "executor_stop",
        json!({"run": run}),
        Some(json!({"threadId": caller})),
    )
}

fn cache_stamp() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    chrono::DateTime::from_timestamp_millis(millis)
        .unwrap()
        .to_rfc3339()
}

fn append_cache_usage(home: &Path, session: &str, records: &[(u64, u64, u64)]) {
    let path = home
        .join("sessions/2026/10/01")
        .join(format!("rollout-{session}.jsonl"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap();
    if file.metadata().unwrap().len() == 0 {
        writeln!(
            file,
            "{}",
            json!({"type": "session_meta", "payload": {"id": session}})
        )
        .unwrap();
    }
    for (input, cached, total) in records {
        writeln!(
            file,
            "{}",
            json!({
                "type": "token_usage_record",
                "timestamp": cache_stamp(),
                "payload": {
                    "thread_id": session,
                    "response_id": format!("response-{total}"),
                    "usage": {
                        "input_tokens": input,
                        "cached_input_tokens": cached
                    },
                    "thread_token_usage": {"input_tokens": total}
                }
            })
        )
        .unwrap();
    }
}

fn cache_guard(receipt: &Path) -> Value {
    serde_json::from_slice::<Value>(&fs::read(receipt).unwrap()).unwrap()["cacheGuard"].clone()
}

fn wait_cache(
    receipt: &Path,
    timeout: Duration,
    ready: impl Fn(&Value) -> bool,
    reason: &str,
) -> Value {
    let until = Instant::now() + timeout;
    loop {
        let guard = cache_guard(receipt);
        if ready(&guard) {
            return guard;
        }
        if Instant::now() >= until {
            panic!("cache guard never reached {reason}: {guard}");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn cache_loss_stops_only_the_exact_run_and_preserves_exact_continuation() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let first = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let second = synthetic_run(home.path(), SECOND_LEAD, SECOND_EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        first._checkout.path(),
        false,
        true,
    )));
    {
        let mut sim = sim.lock().unwrap();
        sim.threads.insert(
            SECOND_LEAD.to_owned(),
            SimThread::idle(SECOND_LEAD, second._checkout.path()),
        );
        sim.threads.insert(
            SECOND_EXECUTOR.to_owned(),
            SimThread::active(SECOND_EXECUTOR, second._checkout.path(), "sibling-turn"),
        );
        sim.terminals.insert(
            SECOND_EXECUTOR.to_owned(),
            vec![terminal("6101", "sibling command")],
        );
        sim.terminals.insert(
            EXECUTOR.to_owned(),
            vec![terminal("6100", "owned cache command")],
        );
    }
    server.answer_fn(native_responder(Arc::clone(&sim)));
    fs::write(first._checkout.path().join("preserved.txt"), "committed\n").unwrap();
    let preserved = commit_checkout(first._checkout.path(), "preserved cache work");
    fs::write(first._checkout.path().join("untracked.txt"), "partial\n").unwrap();
    let mut mcp = Mcp::start(home.path());

    let started = mcp.call_tool(
        2,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&started)["runs"][0]["executorThreadId"], EXECUTOR);
    let waiting = wait_cache(
        &first.receipt,
        Duration::from_secs(10),
        |guard| guard["status"] == "waiting-for-usage",
        "waiting-for-usage",
    );
    assert_eq!(waiting["diagnosticDelivery"], "no-native-consumer");

    append_cache_usage(
        home.path(),
        EXECUTOR,
        &[
            (200_000, 199_000, 200_000),
            (200_000, 0, 400_000),
            (200_000, 0, 600_000),
            (200_000, 0, 800_000),
        ],
    );
    let guard = wait_cache(
        &first.receipt,
        Duration::from_secs(15),
        |guard| guard["stopOutcome"] == "stopped",
        "an exact-thread cache-loss stop",
    );
    assert_eq!(guard["status"], "stopped");
    assert_eq!(guard["run"], format!("mcp-test-{EXECUTOR}"));
    assert_eq!(guard["threadId"], EXECUTOR);
    assert_eq!(guard["turnId"], "executor-active");
    assert_eq!(guard["consecutiveMisses"], 3);
    assert_eq!(guard["lastInputTokens"], 200_000);
    assert_eq!(guard["lastCachedTokens"], 0);
    assert_eq!(guard["lastMissTokens"], 200_000);
    assert_eq!(guard["runtimeSupport"], "proven");
    assert_eq!(guard["diagnosticDelivery"], "no-native-consumer");
    assert_eq!(guard["threshold"]["warmupInputTokens"], 100_000);
    assert_eq!(guard["threshold"]["warmupCachedPercent"], 90);
    assert_eq!(guard["threshold"]["consecutiveMisses"], 3);
    assert_eq!(guard["threshold"]["missInputTokens"], 100_000);
    assert_eq!(guard["threshold"]["missInputPercent"], 50);
    let warning = guard["warning"].as_str().unwrap();
    assert!(
        warning.contains("cache loss on provider test-provider"),
        "{warning}"
    );
    assert!(
        warning.contains("thread 01a00000-0000-7000-8000-000000000003"),
        "{warning}"
    );
    assert!(warning.contains("turn executor-active"), "{warning}");
    assert!(warning.contains("threshold"), "{warning}");
    let receipt: Value = serde_json::from_slice(&fs::read(&first.receipt).unwrap()).unwrap();
    assert_eq!(guard["lease"], receipt["lease"]);
    assert_eq!(guard["turnId"], receipt["stopTurnId"]);
    assert_eq!(receipt["stop"]["outcome"], "stopped");
    assert_eq!(receipt["executorThreadId"], EXECUTOR);
    assert_eq!(
        receipt["assignment"]["objective"],
        "Prove one bounded executor dispatch"
    );
    assert_eq!(receipt["checkout"], json!(first._checkout.path()));
    assert_eq!(receipt["slot"], 1);
    assert_eq!(
        server
            .requests_for("turn/interrupt")
            .iter()
            .filter_map(|request| request["params"]["threadId"].as_str())
            .collect::<Vec<_>>(),
        vec![EXECUTOR]
    );

    let sim_state = sim.lock().unwrap();
    assert_eq!(
        sim_state.threads[EXECUTOR].turns.last().unwrap()["status"],
        "interrupted"
    );
    assert_eq!(sim_state.threads[SECOND_EXECUTOR].state(), "active");
    assert_eq!(sim_state.terminals[SECOND_EXECUTOR].len(), 1);
    assert!(
        sim_state.terminals[EXECUTOR].is_empty(),
        "the stopped run's owned terminal must be gone"
    );
    drop(sim_state);

    let alive = mcp.call_tool(
        3,
        "executor_status",
        json!({}),
        Some(json!({"threadId": SECOND_LEAD})),
    );
    assert_eq!(
        result(&alive)["runs"][0]["executorThreadId"],
        SECOND_EXECUTOR
    );
    let resumed = mcp.call_tool(
        4,
        "executor_resume",
        json!({
            "run": format!("mcp-test-{EXECUTOR}"),
            "mode": "exact",
            "progress": "A cache stop interrupted this turn; verify partial work before reuse."
        }),
        Some(json!({"threadId": LEAD})),
    );
    let resumed = result(&resumed);
    assert_eq!(resumed["threadId"], EXECUTOR, "{resumed}");
    assert_eq!(resumed["predecessorRetained"], false);
    let starts = server.requests_for("turn/start");
    let text = starts.last().unwrap()["params"]["input"][0]["text"]
        .as_str()
        .unwrap();
    assert!(
        text.starts_with("Continue the preserved assignment"),
        "{text}"
    );
    assert!(!text.contains("Original assignment:"), "{text}");
    assert_eq!(server.requests_for("thread/start").len(), 0);
    let receipt: Value = serde_json::from_slice(&fs::read(&first.receipt).unwrap()).unwrap();
    assert_eq!(receipt["executorThreadId"], EXECUTOR);
    assert_eq!(receipt["stop"]["continuedBy"], "exact");
    assert_eq!(
        Command::new("git")
            .args(["-C", first._checkout.path().to_str().unwrap()])
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
        format!("{preserved}\n").into_bytes()
    );
    assert_eq!(
        fs::read_to_string(first._checkout.path().join("untracked.txt")).unwrap(),
        "partial\n"
    );
    drop(first);
    drop(second);
    mcp.finish();
}

#[test]
fn cache_guard_ignores_cold_start_and_a_broken_miss_sequence() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        run._checkout.path(),
        false,
        true,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    let status = mcp.call_tool(
        2,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&status)["runs"][0]["executorThreadId"], EXECUTOR);
    wait_cache(
        &run.receipt,
        Duration::from_secs(10),
        |guard| guard["status"] == "waiting-for-usage",
        "waiting-for-usage",
    );
    append_cache_usage(
        home.path(),
        EXECUTOR,
        &[
            (200_000, 0, 200_000),
            (200_000, 0, 400_000),
            (200_000, 0, 600_000),
        ],
    );
    let cold = wait_cache(
        &run.receipt,
        Duration::from_secs(10),
        |guard| guard["responses"] == 3,
        "three cold responses",
    );
    assert!(cold["stopOutcome"].is_null(), "{cold}");
    assert_eq!(cold["consecutiveMisses"], 0);

    append_cache_usage(
        home.path(),
        EXECUTOR,
        &[
            (200_000, 199_000, 800_000),
            (200_000, 0, 1_000_000),
            (200_000, 199_000, 1_200_000),
            (200_000, 0, 1_400_000),
            (200_000, 0, 1_600_000),
        ],
    );
    let guard = wait_cache(
        &run.receipt,
        Duration::from_secs(10),
        |guard| guard["responses"] == 8,
        "eight classified responses",
    );
    assert!(guard["stopOutcome"].is_null(), "{guard}");
    assert_eq!(guard["consecutiveMisses"], 2);
    assert_eq!(guard["runtimeSupport"], "proven");
    assert_eq!(sim.lock().unwrap().threads[EXECUTOR].state(), "active");
    assert!(server.requests_for("turn/interrupt").is_empty());
    drop(run);
    mcp.finish();
}

#[test]
fn cache_guard_does_not_replay_historical_loss_but_stops_new_misses() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    append_cache_usage(
        home.path(),
        EXECUTOR,
        &[
            (200_000, 199_000, 200_000),
            (200_000, 0, 400_000),
            (200_000, 0, 600_000),
            (200_000, 0, 800_000),
        ],
    );
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        run._checkout.path(),
        false,
        true,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    let status = mcp.call_tool(
        2,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&status)["runs"][0]["executorThreadId"], EXECUTOR);
    let historical = wait_cache(
        &run.receipt,
        Duration::from_secs(10),
        |guard| guard["responses"] == 4,
        "historical usage warmup",
    );
    assert!(historical["stopOutcome"].is_null(), "{historical}");
    assert_eq!(historical["consecutiveMisses"], 0);
    assert!(server.requests_for("turn/interrupt").is_empty());

    append_cache_usage(
        home.path(),
        EXECUTOR,
        &[
            (200_000, 0, 1_000_000),
            (200_000, 0, 1_200_000),
            (200_000, 0, 1_400_000),
        ],
    );
    let stopped = wait_cache(
        &run.receipt,
        Duration::from_secs(15),
        |guard| guard["stopOutcome"] == "stopped",
        "new post-history misses",
    );
    assert_eq!(stopped["consecutiveMisses"], 3);
    assert_eq!(stopped["diagnosticDelivery"], "no-native-consumer");
    assert_eq!(
        server
            .requests_for("turn/interrupt")
            .iter()
            .filter_map(|request| request["params"]["threadId"].as_str())
            .collect::<Vec<_>>(),
        vec![EXECUTOR]
    );
    drop(run);
    mcp.finish();
}

#[test]
fn stop_interrupts_only_the_exact_thread_and_contains_its_terminal() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let first_run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let second_run = synthetic_run(home.path(), SECOND_LEAD, SECOND_EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        first_run._checkout.path(),
        false,
        true,
    )));
    {
        let mut sim = sim.lock().unwrap();
        sim.threads.insert(
            SECOND_LEAD.to_owned(),
            SimThread::idle(SECOND_LEAD, second_run._checkout.path()),
        );
        sim.threads.insert(
            SECOND_EXECUTOR.to_owned(),
            SimThread::active(SECOND_EXECUTOR, second_run._checkout.path(), "sibling-turn"),
        );
        sim.post_interrupt_terminal = Some(terminal("4101", "owned stop command"));
        sim.terminals.insert(
            SECOND_EXECUTOR.to_owned(),
            vec![terminal("4102", "sibling command")],
        );
    }
    server.answer_fn(native_responder(Arc::clone(&sim)));

    fs::write(first_run._checkout.path().join("committed.txt"), "kept\n").unwrap();
    let head = commit_checkout(first_run._checkout.path(), "partial work");
    fs::write(
        first_run._checkout.path().join("untracked.txt"),
        "untracked kept\n",
    )
    .unwrap();
    let receipt_before: Value =
        serde_json::from_slice(&fs::read(&first_run.receipt).unwrap()).unwrap();
    let slot_before = receipt_before["lease"].clone();
    let slot_state = task_worktree::pool_state_dir(home.path(), first_run._repository.path())
        .unwrap()
        .join("slot-1.json");
    let record_before: Value = serde_json::from_slice(&fs::read(&slot_state).unwrap()).unwrap();
    let mut receipt = receipt_before.clone();
    receipt["leadMessages"] = json!([
        {
            "schema": 1,
            "id": "pending-stop-message",
            "kind": "message",
            "status": "pending",
            "contentId": "pending"
        },
        {
            "schema": 1,
            "id": "delivered-stop-request",
            "kind": "reply-request",
            "status": "delivered",
            "contentId": "delivered"
        }
    ]);
    fs::write(
        &first_run.receipt,
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();

    let run_id = format!("mcp-test-{EXECUTOR}");
    let mut mcp = Mcp::start(home.path());
    let response = stop_run(&mut mcp, 2, LEAD, &run_id);
    let stopped = result(&response);
    assert_eq!(stopped["outcome"], "stopped", "{response}");
    assert_eq!(stopped["executorThreadId"], EXECUTOR);
    assert_eq!(stopped["interruptedTurnId"], "executor-active");
    assert_eq!(stopped["threadState"], "idle");
    assert_eq!(stopped["containment"]["status"], "verified");
    assert_eq!(stopped["containment"]["listed"], 1);
    assert_eq!(stopped["containment"]["ended"], 1);
    assert_eq!(
        stopped["cancelledMessages"],
        json!(["pending-stop-message", "delivered-stop-request"])
    );

    let interrupts = server.requests_for("turn/interrupt");
    assert_eq!(interrupts.len(), 1, "{interrupts:?}");
    assert_eq!(interrupts[0]["params"]["threadId"], EXECUTOR);
    assert_eq!(interrupts[0]["params"]["turnId"], "executor-active");
    let terminated = server.requests_for("thread/backgroundTerminals/terminate");
    assert_eq!(terminated.len(), 1, "{terminated:?}");
    assert_eq!(terminated[0]["params"]["threadId"], EXECUTOR);
    assert_eq!(terminated[0]["params"]["processId"], "4101");
    assert!(
        server.requests_for("turn/start").is_empty(),
        "stop must not start model work"
    );
    assert!(
        server.requests_for("thread/start").is_empty(),
        "stop must not create a thread"
    );
    for inventory in server.requests_for("thread/backgroundTerminals/list") {
        assert_eq!(
            inventory["params"]["threadId"], EXECUTOR,
            "stop read a sibling inventory: {inventory}"
        );
    }
    {
        let sim = sim.lock().unwrap();
        assert_eq!(sim.threads[EXECUTOR].state(), "idle");
        assert_eq!(sim.threads[SECOND_EXECUTOR].state(), "active");
        assert_eq!(sim.terminals.get(EXECUTOR).unwrap().len(), 0);
        assert_eq!(sim.terminals.get(SECOND_EXECUTOR).unwrap().len(), 1);
    }
    assert_eq!(
        String::from_utf8(
            Command::new("git")
                .args([
                    "-C",
                    first_run._checkout.path().to_str().unwrap(),
                    "rev-parse",
                    "HEAD"
                ])
                .output()
                .unwrap()
                .stdout
        )
        .unwrap()
        .trim(),
        head
    );
    assert_eq!(
        fs::read_to_string(first_run._checkout.path().join("untracked.txt")).unwrap(),
        "untracked kept\n"
    );
    let receipt_after: Value =
        serde_json::from_slice(&fs::read(&first_run.receipt).unwrap()).unwrap();
    assert_eq!(receipt_after["lease"], slot_before);
    assert_eq!(receipt_after["turnId"], "executor-active");
    assert_eq!(receipt_after["stopTurnId"], "executor-active");
    assert_eq!(
        receipt_after["leadMessages"][0]["status"], "cancelled",
        "{receipt_after}"
    );
    let record_after: Value = serde_json::from_slice(&fs::read(&slot_state).unwrap()).unwrap();
    assert_eq!(record_after, record_before);
    let still_running = mcp.call_tool(
        3,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(result(&still_running)["runs"][0]["run"], run_id);
    assert!(
        result(&still_running)["runs"][0]["pendingRequests"]
            .as_array()
            .unwrap()
            .is_empty(),
        "cancelled requests are not pending"
    );

    let before_retry = server.requests_for("turn/start").len();
    let refused_message = mcp.call_tool(
        4,
        "executor_message",
        json!({"text": "restart? no", "clientMessageId": "after-stop"}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&refused_message).contains("exact run is stopped"),
        "{refused_message}"
    );
    assert_eq!(server.requests_for("turn/start").len(), before_retry);

    let repeated = stop_run(&mut mcp, 5, LEAD, &run_id);
    let repeated = result(&repeated);
    assert_eq!(repeated["outcome"], "already-stopped", "{repeated}");
    assert_eq!(repeated["stop"]["repeats"], 1);
    assert_eq!(server.requests_for("turn/interrupt").len(), 1);

    let forbidden = stop_run(&mut mcp, 6, EXECUTOR, &run_id);
    assert!(
        tool_error(&forbidden).contains("owned by the calling lead"),
        "{forbidden}"
    );
    assert_eq!(server.requests_for("turn/interrupt").len(), 1);
    assert_eq!(
        sim.lock().unwrap().terminals[SECOND_EXECUTOR].len(),
        1,
        "the sibling command survives"
    );

    drop(first_run);
    drop(second_run);
    mcp.finish();
}

#[test]
fn stop_reports_completion_race_and_partial_containment_honestly() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let raced = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let raced_sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        raced._checkout.path(),
        false,
        true,
    )));
    raced_sim.lock().unwrap().interrupt_completes = true;
    server.answer_fn(native_responder(Arc::clone(&raced_sim)));
    let mut mcp = Mcp::start(home.path());
    let response = stop_run(&mut mcp, 2, LEAD, &format!("mcp-test-{EXECUTOR}"));
    let completed = result(&response);
    assert_eq!(completed["outcome"], "already-completed", "{response}");
    assert_eq!(
        raced_sim.lock().unwrap().threads[EXECUTOR]
            .turns
            .last()
            .unwrap()["status"],
        "completed"
    );
    assert!(
        server
            .requests_for("thread/backgroundTerminals/terminate")
            .is_empty()
    );
    mcp.finish();
    drop(raced);

    let partial = synthetic_run(home.path(), SECOND_LEAD, SECOND_EXECUTOR, &server);
    let partial_sim = Arc::new(Mutex::new(NativeSim::new(
        SECOND_LEAD,
        SECOND_EXECUTOR,
        partial._checkout.path(),
        false,
        true,
    )));
    {
        let mut sim = partial_sim.lock().unwrap();
        sim.terminate_terminals = false;
        sim.terminals.insert(
            SECOND_EXECUTOR.to_owned(),
            vec![terminal("5101", "cannot stop this command")],
        );
    }
    server.answer_fn(native_responder(Arc::clone(&partial_sim)));
    let mut mcp = Mcp::start(home.path());
    let response = stop_run(
        &mut mcp,
        3,
        SECOND_LEAD,
        &format!("mcp-test-{SECOND_EXECUTOR}"),
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
    let partial_value = &response["result"]["structuredContent"];
    assert_eq!(partial_value["outcome"], "partial", "{response}");
    assert_eq!(partial_value["containment"]["status"], "partial");
    assert_eq!(partial_value["stop"]["survivors"][0]["surface"], "5101");
    assert!(
        server
            .requests_for("turn/interrupt")
            .iter()
            .any(|request| request["params"]["threadId"] == SECOND_EXECUTOR),
        "the partial stop interrupted its exact thread"
    );
    assert!(
        server.requests_for("turn/start").is_empty(),
        "partial containment must not start new work"
    );
    partial_sim.lock().unwrap().terminate_terminals = true;
    let cleaned = stop_run(
        &mut mcp,
        4,
        SECOND_LEAD,
        &format!("mcp-test-{SECOND_EXECUTOR}"),
    );
    let cleaned = result(&cleaned);
    assert_eq!(cleaned["outcome"], "stopped", "{cleaned}");
    assert_eq!(cleaned["containment"]["status"], "verified");
    assert!(cleaned["stop"]["survivors"].as_array().unwrap().is_empty());
    let usable = mcp.call_tool(
        5,
        "executor_status",
        json!({}),
        Some(json!({"threadId": SECOND_LEAD})),
    );
    assert_eq!(
        result(&usable)["runs"][0]["executorThreadId"],
        SECOND_EXECUTOR,
        "the shared app-server remains usable"
    );
    mcp.finish();
    drop(partial);
}

#[test]
fn exact_continuation_reuses_the_thread_and_never_replays_the_assignment() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        run._checkout.path(),
        false,
        true,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    let run_id = format!("mcp-test-{EXECUTOR}");
    let stopped = stop_run(&mut mcp, 2, LEAD, &run_id);
    let stopped = result(&stopped);
    assert_eq!(stopped["outcome"], "stopped");
    let starts_before = server.requests_for("thread/start").len();
    let turns_before = server.requests_for("turn/start").len();

    let resumed = mcp.call_tool(
        3,
        "executor_resume",
        json!({
            "run": run_id,
            "mode": "exact",
            "progress": "The child command was interrupted; keep the committed and untracked files."
        }),
        Some(json!({"threadId": LEAD})),
    );
    let resumed = result(&resumed);
    assert_eq!(resumed["mode"], "exact", "{resumed}");
    assert_eq!(resumed["threadId"], EXECUTOR);
    assert_eq!(resumed["preTurnTurns"], 1);
    assert_eq!(
        resumed["predecessorRetained"], false,
        "exact continuation is the same thread"
    );
    assert_eq!(server.requests_for("thread/start").len(), starts_before);
    assert_eq!(server.requests_for("turn/start").len(), turns_before + 1);
    let starts = server.requests_for("turn/start");
    let text = starts[0]["params"]["input"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("Continue the preserved assignment in this same thread"),
        "{text}"
    );
    assert!(text.contains("The child command was interrupted"), "{text}");
    assert!(!text.contains("Original assignment:"), "{text}");
    assert_eq!(
        sim.lock().unwrap().threads[EXECUTOR].state(),
        "active",
        "explicit continuation starts the same thread"
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert_eq!(receipt["executorThreadId"], EXECUTOR);
    assert_eq!(receipt["turnId"], resumed["turnId"]);
    assert_eq!(receipt["continuation"]["mode"], "exact");
    assert_eq!(receipt["continuation"]["fromThreadId"], EXECUTOR);
    assert!(receipt.get("predecessorThreadId").is_none());
    assert_eq!(receipt["slot"], 1);
    let allowed = mcp.call_tool(
        4,
        "executor_message",
        json!({"text": "post-resume guidance", "clientMessageId": "post-resume"}),
        Some(json!({"threadId": LEAD})),
    );
    assert_eq!(
        allowed["result"]["isError"], false,
        "explicit continuation clears the stopped-delivery gate: {allowed}"
    );
    assert_eq!(
        server.requests_for("turn/start").len(),
        turns_before + 1,
        "post-resume delivery must not start a duplicate turn"
    );
    let stopped_again = stop_run(&mut mcp, 5, LEAD, &format!("mcp-test-{EXECUTOR}"));
    let stopped_again = result(&stopped_again);
    assert_eq!(stopped_again["outcome"], "stopped", "{stopped_again}");
    let receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert!(receipt["stop"].get("continuedMs").is_none(), "{receipt}");
    let refused_again = mcp.call_tool(
        6,
        "executor_message",
        json!({"text": "after the new stop", "clientMessageId": "after-second-stop"}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&refused_again).contains("exact run is stopped"),
        "{refused_again}"
    );
    assert_eq!(server.requests_for("turn/start").len(), turns_before + 1);
    mcp.finish();
    drop(run);
}

#[test]
fn fresh_recovery_creates_one_new_thread_and_retains_the_predecessor() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        run._checkout.path(),
        false,
        true,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    let run_id = format!("mcp-test-{EXECUTOR}");
    let stopped = stop_run(&mut mcp, 2, LEAD, &run_id);
    let stopped = result(&stopped);
    assert_eq!(stopped["outcome"], "stopped");
    let lease_before = stopped["lease"].clone();
    let starts_before = server.requests_for("thread/start").len();
    let resumed = mcp.call_tool(
        3,
        "executor_resume",
        json!({
            "run": run_id,
            "mode": "fresh",
            "progress": "Inspect the interrupted command before reusing its output."
        }),
        Some(json!({"threadId": LEAD})),
    );
    let resumed = result(&resumed);
    let new_thread = resumed["threadId"].as_str().unwrap();
    assert_eq!(resumed["mode"], "fresh", "{resumed}");
    assert_ne!(new_thread, EXECUTOR);
    assert_eq!(resumed["predecessorThreadId"], EXECUTOR);
    assert_eq!(resumed["predecessorRetained"], true);
    assert_eq!(server.requests_for("thread/start").len(), starts_before + 1);
    assert_eq!(server.requests_for("turn/start").len(), 1);
    let starts = server.requests_for("turn/start");
    let text = starts[0]["params"]["input"][0]["text"].as_str().unwrap();
    assert!(text.contains("Original assignment:"), "{text}");
    assert!(
        text.contains("Prove one bounded executor dispatch"),
        "{text}"
    );
    assert!(text.contains("Inspect the interrupted command"), "{text}");
    let sim = sim.lock().unwrap();
    assert!(sim.threads.contains_key(EXECUTOR), "predecessor retained");
    assert_eq!(sim.threads[new_thread].state(), "active");
    drop(sim);
    let receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert_eq!(receipt["executorThreadId"], new_thread);
    assert_eq!(receipt["predecessorThreadId"], EXECUTOR);
    assert_eq!(receipt["predecessor"]["threadId"], EXECUTOR);
    assert_eq!(receipt["predecessor"]["retained"], true);
    assert_eq!(receipt["continuation"]["mode"], "fresh");
    assert_eq!(receipt["slot"], 1);
    assert_eq!(receipt["lease"], lease_before);
    mcp.finish();
    drop(run);
}

#[test]
fn exact_lead_continuation_preserves_recorded_hierarchy_and_workspace_context() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let lead = delegated_lead_continuation_run(home.path(), &server);
    let title = lead.value["title"].as_str().unwrap();
    let mut sim = NativeSim::new(DIRECTOR, LEAD, &lead.primary, false, false);
    sim.threads.insert(
        LEAD.to_owned(),
        recorded_lead_thread(title, &lead.primary, true),
    );
    server.answer_fn(native_responder(Arc::new(Mutex::new(sim))));
    let partial = lead.primary.join("partial-work.txt");
    fs::write(&partial, "interrupted executor output\n").unwrap();
    let mut mcp = Mcp::start(home.path());

    let starts_before = server.requests_for("thread/start").len();
    let turns_before = server.requests_for("turn/start").len();
    let resumed = mcp.call_tool(
        2,
        "executor_resume",
        json!({
            "run": lead.value["run"],
            "mode": "exact",
            "progress": "The recorded check was interrupted; keep the partial work."
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    let resumed = result(&resumed);
    assert_eq!(resumed["mode"], "exact", "{resumed}");
    assert_eq!(resumed["threadId"], LEAD);
    assert_eq!(resumed["role"], "lead", "{resumed}");
    assert_eq!(resumed["parentThreadId"], DIRECTOR, "{resumed}");
    assert_eq!(resumed["rootThreadId"], DIRECTOR, "{resumed}");
    assert_eq!(resumed["profile"], lead.value["profile"], "{resumed}");
    assert_eq!(
        resumed["workspaceRoots"], lead.value["workspaceRoots"],
        "{resumed}"
    );
    assert_eq!(resumed["workspaces"], lead.value["workspaces"], "{resumed}");
    assert_eq!(server.requests_for("thread/start").len(), starts_before);
    assert_eq!(server.requests_for("turn/start").len(), turns_before + 1);
    let turns = server.requests_for("turn/start");
    let text = turns[0]["params"]["input"][0]["text"].as_str().unwrap();
    assert!(!text.contains("Original assignment:"), "{text}");
    assert!(
        text.contains("Do not replay or resubmit the assignment"),
        "{text}"
    );
    assert_eq!(
        fs::read_to_string(&partial).unwrap(),
        "interrupted executor output\n"
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&lead.run.receipt).unwrap()).unwrap();
    assert_eq!(receipt["role"], "lead", "{receipt}");
    assert_eq!(receipt["parentThreadId"], DIRECTOR, "{receipt}");
    assert_eq!(receipt["rootThreadId"], DIRECTOR, "{receipt}");
    assert_eq!(receipt["workspaceRoots"], lead.value["workspaceRoots"]);
    assert_eq!(receipt["workspaces"], lead.value["workspaces"]);
    assert_eq!(receipt["continuation"]["mode"], "exact");
    assert_eq!(receipt["continuation"]["role"], "lead");
    assert_eq!(receipt["continuation"]["parentThreadId"], DIRECTOR);
    assert_eq!(receipt["continuation"]["rootThreadId"], DIRECTOR);
    mcp.finish();
    drop(lead);
}

#[test]
fn fresh_lead_recovery_restores_hierarchy_workspace_set_and_ds_binding() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let lead = delegated_lead_continuation_run(home.path(), &server);
    let title = lead.value["title"].as_str().unwrap();
    let mut sim = NativeSim::new(DIRECTOR, LEAD, &lead.primary, false, false);
    sim.threads.insert(
        LEAD.to_owned(),
        recorded_lead_thread(title, &lead.primary, false),
    );
    server.answer_fn(native_responder(Arc::new(Mutex::new(sim))));
    let partial = lead.primary.join("partial-work.txt");
    fs::write(&partial, "uncommitted recovery input\n").unwrap();
    let mut mcp = Mcp::start(home.path());
    let old_endpoint = home
        .path()
        .join("harness/lead-endpoints")
        .join(format!("{LEAD}.json"));
    assert!(old_endpoint.is_file());

    let starts_before = server.requests_for("thread/start").len();
    let resumed = mcp.call_tool(
        2,
        "executor_resume",
        json!({
            "run": lead.value["run"],
            "mode": "fresh",
            "progress": "The recorded check was interrupted; do not assume it passed."
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    let resumed = result(&resumed);
    let new_thread = resumed["threadId"].as_str().unwrap();
    assert_eq!(resumed["mode"], "fresh", "{resumed}");
    assert_ne!(new_thread, LEAD);
    assert_eq!(resumed["role"], "lead", "{resumed}");
    assert_eq!(resumed["parentThreadId"], DIRECTOR, "{resumed}");
    assert_eq!(resumed["rootThreadId"], DIRECTOR, "{resumed}");
    assert_eq!(resumed["profile"], lead.value["profile"], "{resumed}");
    assert_eq!(
        resumed["workspaceRoots"], lead.value["workspaceRoots"],
        "{resumed}"
    );
    assert_eq!(resumed["workspaces"], lead.value["workspaces"], "{resumed}");
    assert_eq!(server.requests_for("thread/start").len(), starts_before + 1);
    let starts = server.requests_for("thread/start");
    let params = &starts[0]["params"];
    assert_eq!(
        params["runtimeWorkspaceRoots"],
        serde_json::to_value(&lead.roots).unwrap(),
        "{params}"
    );
    assert_eq!(params["model"], "deepseek-flash", "{params}");
    assert_eq!(params["modelProvider"], "deepseek", "{params}");
    assert_eq!(
        params["config"]["model_reasoning_effort"], "max",
        "{params}"
    );
    assert_eq!(
        params["config"]["mcp_servers"]["executors"]["enabled_tools"],
        json!(LEAD_ROLE_TOOLS),
        "{params}"
    );
    let new_endpoint = home
        .path()
        .join("harness/lead-endpoints")
        .join(format!("{new_thread}.json"));
    let endpoint: Value = serde_json::from_slice(&fs::read(&new_endpoint).unwrap()).unwrap();
    assert_eq!(endpoint["threadId"], new_thread, "{endpoint}");
    assert!(
        !old_endpoint.exists(),
        "fresh recovery left the predecessor addressable as authority"
    );
    assert_eq!(
        fs::read_to_string(&partial).unwrap(),
        "uncommitted recovery input\n"
    );
    let turns = server.requests_for("turn/start");
    let text = turns[0]["params"]["input"][0]["text"].as_str().unwrap();
    assert!(text.contains("Original assignment:"), "{text}");
    assert!(
        text.contains("do not assume a prior command completed"),
        "{text}"
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&lead.run.receipt).unwrap()).unwrap();
    assert_eq!(receipt["executorThreadId"], new_thread, "{receipt}");
    assert_eq!(receipt["predecessorThreadId"], LEAD, "{receipt}");
    assert_eq!(receipt["role"], "lead", "{receipt}");
    assert_eq!(receipt["parentThreadId"], DIRECTOR, "{receipt}");
    assert_eq!(receipt["rootThreadId"], DIRECTOR, "{receipt}");
    assert_eq!(receipt["profile"], lead.value["profile"]);
    assert_eq!(receipt["workspaceRoots"], lead.value["workspaceRoots"]);
    assert_eq!(receipt["workspaces"], lead.value["workspaces"]);
    assert_eq!(receipt["continuation"]["role"], "lead");
    assert_eq!(receipt["continuation"]["parentThreadId"], DIRECTOR);
    assert_eq!(receipt["continuation"]["rootThreadId"], DIRECTOR);
    mcp.finish();
    drop(lead);
}

#[test]
fn fresh_lead_recovery_reattaches_preexisting_children_to_the_successor() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let lead = delegated_lead_continuation_run(home.path(), &server);
    let child = recorded_executor_child(home.path(), &server);
    let released_child = recorded_executor_child_for(home.path(), &server, OTHER_EXECUTOR);
    let mut released_value: Value =
        serde_json::from_slice(&fs::read(&released_child.receipt).unwrap()).unwrap();
    released_value["release"] = json!({
        "schema": 1,
        "run": released_value["run"],
        "slot": 1,
        "disposition": "discarded",
        "slotFreed": true
    });
    fs::write(
        &released_child.receipt,
        serde_json::to_vec_pretty(&released_value).unwrap(),
    )
    .unwrap();
    let title = lead.value["title"].as_str().unwrap();
    let mut sim = NativeSim::new(DIRECTOR, LEAD, &lead.primary, false, false);
    sim.threads.insert(
        LEAD.to_owned(),
        recorded_lead_thread(title, &lead.primary, false),
    );
    sim.threads.insert(
        EXECUTOR.to_owned(),
        SimThread::idle(EXECUTOR, &child.checkout),
    );
    let sim = Arc::new(Mutex::new(sim));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());

    let resumed = mcp.call_tool(
        2,
        "executor_resume",
        json!({"run": lead.value["run"], "mode": "fresh"}),
        Some(json!({"threadId": DIRECTOR})),
    );
    let resumed = result(&resumed);
    let successor = resumed["threadId"].as_str().unwrap();
    let child_value: Value = serde_json::from_slice(&fs::read(&child.receipt).unwrap()).unwrap();
    assert_ne!(successor, LEAD);
    assert_eq!(resumed["successorAttachment"]["count"], 1, "{resumed}");
    assert_eq!(
        resumed["successorAttachment"]["errors"],
        json!([]),
        "{resumed}"
    );
    assert_eq!(
        resumed["successorAttachment"]["runs"][0], child_value["run"],
        "the attachment report must name the exact child run"
    );
    assert_eq!(resumed["successorAttachment"]["fromThreadId"], LEAD);
    assert_eq!(resumed["successorAttachment"]["toThreadId"], successor);
    assert!(
        sim.lock().unwrap().threads.contains_key(LEAD),
        "the predecessor remains retained"
    );
    assert_eq!(child_value["parentThreadId"], successor, "{child_value}");
    assert_eq!(child_value["leadThreadId"], successor, "{child_value}");
    assert_eq!(child_value["rootThreadId"], DIRECTOR, "{child_value}");
    assert_eq!(
        child_value["successorAttachment"]["fromParentThreadId"], LEAD,
        "{child_value}"
    );
    let released_after: Value =
        serde_json::from_slice(&fs::read(&released_child.receipt).unwrap()).unwrap();
    assert_eq!(
        released_after, released_value,
        "a released child is retained evidence and must not be rebound"
    );

    let status = mcp.call_tool(
        3,
        "executor_status",
        json!({}),
        Some(json!({"threadId": successor})),
    );
    let status = result(&status);
    assert_eq!(status["callerRole"], "lead", "{status}");
    let child_status = status["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["run"] == child_value["run"])
        .unwrap();
    assert_eq!(child_status["parentThreadId"], successor, "{child_status}");
    let recorded_parent = child_status["parentThreadId"].as_str().unwrap();
    harness_core::task_orchestrate::validate_acceptance_relation(
        "executor",
        "lead",
        Some(recorded_parent),
        successor,
    )
    .unwrap();

    let child_before_old_attempts: Value =
        serde_json::from_slice(&fs::read(&child.receipt).unwrap()).unwrap();
    let starts_before_old_attempts = server.requests_for("thread/start").len();
    let turns_before_old_attempts = server.requests_for("turn/start").len();
    let old_message = mcp.call_tool(
        4,
        "executor_message",
        json!({"run": child_value["run"], "text": "stale parent message"}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&old_message).contains("no recorded MCP executor run"),
        "{old_message}"
    );
    let old_release = release_tool(
        &mut mcp,
        5,
        LEAD,
        child_value["run"].as_str(),
        "discarded",
        "stale parent review",
    );
    assert!(
        tool_error(&old_release).contains("owned by the calling lead"),
        "{old_release}"
    );
    let old_dispatch = mcp.call(
        6,
        json!({
            "owner": "stale-parent-dispatch",
            "assignment": assignment("must not start from the predecessor")
        }),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&old_dispatch).contains("retained continuation predecessor"),
        "{old_dispatch}"
    );
    assert_eq!(
        server.requests_for("thread/start").len(),
        starts_before_old_attempts
    );
    assert_eq!(
        server.requests_for("turn/start").len(),
        turns_before_old_attempts
    );
    let child_after_old_attempts: Value =
        serde_json::from_slice(&fs::read(&child.receipt).unwrap()).unwrap();
    assert_eq!(
        child_after_old_attempts, child_before_old_attempts,
        "the retained predecessor mutated the child receipt"
    );

    let message = mcp.call_tool(
        7,
        "executor_message",
        json!({
            "run": child_value["run"],
            "text": "continue under the recorded successor",
            "clientMessageId": "successor-child-message"
        }),
        Some(json!({"threadId": successor})),
    );
    let message = result(&message);
    assert_eq!(message["delivery"]["status"], "delivered", "{message}");
    let child_turn = message["delivery"]["turnId"].as_str().unwrap();
    complete_turn(
        &sim,
        EXECUTOR,
        child_turn,
        "the child completed under its successor",
    );

    let released = release_tool(
        &mut mcp,
        8,
        successor,
        child_value["run"].as_str(),
        "discarded",
        "successor reviewed the child",
    );
    let released = result(&released);
    assert_eq!(released["leadThreadId"], successor, "{released}");
    assert_eq!(released["executorThreadId"], EXECUTOR, "{released}");
    assert_eq!(released["state"], "released", "{released}");
    mcp.finish();
    drop(released_child);
    drop(child);
    drop(lead);
}

#[test]
fn reopen_and_explicit_resume_preserve_child_work_and_reconcile_an_uncertain_start() {
    const COMPLETED_TEXT: &str = "completed but not accepted";
    const LOST_CHILD: &str = "01a00000-0000-7000-8000-000000000008";
    const LOST_OWNER: &str = "lost-child-start";
    const LOST_REQUEST: &str = "lost-child-request";

    let home = home();
    let kit = tempfile::tempdir().unwrap();
    install_kit(home.path(), kit.path());
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let lead = delegated_lead_continuation_run(home.path(), &server);
    let (child, child_value, pending_request) = delegated_child_run(home.path(), &server);
    let request_path = home
        .path()
        .join("harness/executor-pool/requests")
        .join(LEAD)
        .join(format!("{LOST_REQUEST}.json"));
    fs::create_dir_all(request_path.parent().unwrap()).unwrap();
    fs::write(
        &request_path,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "requestId": LOST_REQUEST,
            "parentThreadId": LEAD,
            "rootThreadId": DIRECTOR,
            "role": "executor",
            "intent": "a child start whose acknowledgement was lost",
            "owner": LOST_OWNER,
            "status": "thread-started",
            "createdMs": 1,
            "childThreadId": LOST_CHILD,
            "receipt": null,
            "error": null,
            "process": null
        }))
        .unwrap(),
    )
    .unwrap();
    let partial = child._checkout.path().join("partial-work.txt");
    fs::write(&partial, "uncommitted partial work\n").unwrap();

    let mut sim = NativeSim::new(DIRECTOR, LEAD, &lead.primary, false, false);
    sim.threads.insert(
        LEAD.to_owned(),
        recorded_lead_thread(lead.value["title"].as_str().unwrap(), &lead.primary, false),
    );
    let mut completed = SimThread::idle(EXECUTOR, child._checkout.path());
    completed.turns = vec![json!({
        "id": child_value["turnId"],
        "status": "completed",
        "items": [{"id": "completed-result", "type": "agentMessage", "text": COMPLETED_TEXT}]
    })];
    sim.threads.insert(EXECUTOR.to_owned(), completed);
    sim.threads.insert(
        LOST_CHILD.to_owned(),
        SimThread::idle(LOST_CHILD, child._checkout.path()),
    );
    let sim = Arc::new(Mutex::new(sim));
    server.answer_fn(native_responder(Arc::clone(&sim)));

    let lifecycle = |value: &Value| {
        json!({
            "observation": value["observation"],
            "stop": value["stop"],
            "release": value["release"],
            "turnId": value["turnId"],
            "leadMessages": value["leadMessages"],
            "waitAcknowledgements": value["waitAcknowledgements"]
        })
    };
    let child_before = lifecycle(&child_value);
    let lead_before: Value = serde_json::from_slice(&fs::read(&lead.run.receipt).unwrap()).unwrap();
    let lead_before = lifecycle(&lead_before);
    let starts_before = server.requests_for("thread/start").len();
    let turns_before = turn_requests(&server);
    let resumes_before = server.requests_for("thread/resume").len();

    let inspect = |mcp: &mut Mcp, next: u64| {
        let status = mcp.call_tool(
            next,
            "executor_status",
            json!({}),
            Some(json!({"threadId": DIRECTOR})),
        );
        let status = result(&status);
        let child_status = status["runs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|run| run["run"] == child_value["run"])
            .unwrap_or_else(|| panic!("preserved child missing after reopen: {status}"));
        assert_eq!(
            child_status["pendingRequests"][0]["requestId"], pending_request,
            "{child_status}"
        );
        assert!(child_status["stop"].is_null(), "{child_status}");
        assert!(child_status["release"].is_null(), "{child_status}");

        let waited = mcp.call_tool(
            next + 1,
            "executor_wait",
            json!({"run": child_value["run"], "timeoutMs": 0}),
            Some(json!({"threadId": LEAD})),
        );
        let waited = result(&waited);
        assert_eq!(waited["state"], "question", "{waited}");
        assert_eq!(
            waited["pendingRequests"][0]["requestId"], pending_request,
            "{waited}"
        );
        assert_eq!(waited["result"]["finalMessage"], COMPLETED_TEXT, "{waited}");
    };

    let mut orderly = Mcp::start(home.path());
    inspect(&mut orderly, 2);
    orderly.finish();
    assert_eq!(server.requests_for("thread/start").len(), starts_before);
    assert_eq!(turn_requests(&server), turns_before);
    assert_eq!(server.requests_for("thread/resume").len(), resumes_before);
    assert_eq!(
        lifecycle(&serde_json::from_slice::<Value>(&fs::read(&child.receipt).unwrap()).unwrap()),
        child_before,
        "orderly MCP closure changed the child's work or delivery state"
    );

    let mut disconnected = Mcp::start(home.path());
    inspect(&mut disconnected, 4);
    disconnected.disconnect();
    assert_eq!(server.requests_for("thread/start").len(), starts_before);
    assert_eq!(turn_requests(&server), turns_before);
    assert_eq!(server.requests_for("thread/resume").len(), resumes_before);
    assert_eq!(
        lifecycle(&serde_json::from_slice::<Value>(&fs::read(&child.receipt).unwrap()).unwrap()),
        child_before,
        "abrupt MCP disconnection changed the child's work or delivery state"
    );
    assert_eq!(
        fs::read_to_string(&partial).unwrap(),
        "uncommitted partial work\n"
    );

    let mut recovered = Mcp::start(home.path());
    let resumed = recovered.call_tool(
        6,
        "executor_resume",
        json!({
            "run": lead.value["run"],
            "mode": "fresh",
            "progress": "The controlling CLI closed after the child completed; inspect before reusing it."
        }),
        Some(json!({"threadId": DIRECTOR})),
    );
    let resumed = result(&resumed);
    let successor = resumed["threadId"].as_str().unwrap();
    assert_eq!(
        resumed["uncertainRequests"][0]["requestId"], LOST_REQUEST,
        "{resumed}"
    );
    assert_eq!(
        resumed["uncertainRequests"][0]["run"], LOST_OWNER,
        "{resumed}"
    );
    assert_eq!(
        resumed["uncertainRequests"][0]["childThreadId"], LOST_CHILD,
        "{resumed}"
    );
    assert_eq!(
        resumed["uncertainRequests"][0]["state"], "uncertain",
        "{resumed}"
    );
    assert_eq!(resumed["successorAttachment"]["count"], 1, "{resumed}");
    assert_eq!(server.requests_for("thread/start").len(), starts_before + 1);
    assert_eq!(turn_requests(&server), turns_before + 1);
    let uncertain: Value = serde_json::from_slice(&fs::read(&request_path).unwrap()).unwrap();
    assert_eq!(uncertain["status"], "uncertain", "{uncertain}");
    assert_eq!(uncertain["childThreadId"], LOST_CHILD, "{uncertain}");
    let child_after: Value = serde_json::from_slice(&fs::read(&child.receipt).unwrap()).unwrap();
    assert_eq!(child_after["parentThreadId"], successor, "{child_after}");
    assert_eq!(child_after["leadThreadId"], successor, "{child_after}");
    assert_eq!(
        lifecycle(&child_after),
        child_before,
        "fresh recovery replayed or accepted the completed child result"
    );
    assert_eq!(
        fs::read_to_string(&partial).unwrap(),
        "uncommitted partial work\n"
    );
    let lead_after: Value = serde_json::from_slice(&fs::read(&lead.run.receipt).unwrap()).unwrap();
    assert_eq!(
        json!({
            "observation": lead_after["observation"],
            "stop": lead_after["stop"],
            "release": lead_after["release"],
            "leadMessages": lead_after["leadMessages"],
            "waitAcknowledgements": lead_after["waitAcknowledgements"]
        }),
        json!({
            "observation": lead_before["observation"],
            "stop": lead_before["stop"],
            "release": lead_before["release"],
            "leadMessages": lead_before["leadMessages"],
            "waitAcknowledgements": lead_before["waitAcknowledgements"]
        }),
        "fresh recovery changed the preserved Lead lifecycle"
    );

    let status = recovered.call_tool(
        7,
        "executor_status",
        json!({}),
        Some(json!({"threadId": successor})),
    );
    let status = result(&status);
    let child_status = status["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["run"] == child_value["run"])
        .unwrap_or_else(|| panic!("attached child missing from its successor: {status}"));
    assert_eq!(
        child_status["pendingRequests"][0]["requestId"], pending_request,
        "{child_status}"
    );
    assert!(
        child_after["waitAcknowledgements"].is_null(),
        "inspection acknowledged the completed child result: {child_after}"
    );

    let retry_starts = server.requests_for("thread/start").len();
    let retry_turns = turn_requests(&server);
    let retry = recovered.call_tool(
        8,
        "executor_dispatch",
        json!({
            "owner": LOST_OWNER,
            "requestId": LOST_REQUEST,
            "base": child_value["base"],
            "assignment": assignment("must not allocate a duplicate writer")
        }),
        Some(json!({"threadId": successor})),
    );
    assert!(
        tool_error(&retry).contains("unresolved dispatch request"),
        "{retry}"
    );
    assert_eq!(
        server.requests_for("thread/start").len(),
        retry_starts,
        "an uncertain child owner allocated a duplicate thread"
    );
    assert_eq!(
        turn_requests(&server),
        retry_turns,
        "an uncertain child owner started duplicate model work"
    );
    recovered.finish();
    drop(child);
    drop(lead);
}

#[test]
fn refused_fresh_lead_turn_leaves_no_unrecorded_endpoint_or_new_authority() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let lead = delegated_lead_continuation_run(home.path(), &server);
    let title = lead.value["title"].as_str().unwrap();
    let mut sim = NativeSim::new(DIRECTOR, LEAD, &lead.primary, false, false);
    sim.threads.insert(
        LEAD.to_owned(),
        recorded_lead_thread(title, &lead.primary, false),
    );
    sim.refuse_turns = true;
    server.answer_fn(native_responder(Arc::new(Mutex::new(sim))));
    let partial = lead.primary.join("partial-work.txt");
    fs::write(&partial, "keep the interrupted work\n").unwrap();
    let slot_path = lead.run.receipt.parent().unwrap().join("slot-1.json");
    let slot_before: Value = serde_json::from_slice(&fs::read(&slot_path).unwrap()).unwrap();
    let mut mcp = Mcp::start(home.path());

    let response = mcp.call_tool(
        2,
        "executor_resume",
        json!({"run": lead.value["run"], "mode": "fresh"}),
        Some(json!({"threadId": DIRECTOR})),
    );
    assert!(
        tool_error(&response).contains("native refusal"),
        "{response}"
    );
    let endpoints = fs::read_dir(home.path().join("harness/lead-endpoints"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(endpoints.len(), 2, "{endpoints:?}");
    assert!(endpoints.contains(&format!("{LEAD}.json")), "{endpoints:?}");
    assert!(
        endpoints.contains(&format!("{DIRECTOR}.json")),
        "{endpoints:?}"
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&lead.run.receipt).unwrap()).unwrap();
    assert_eq!(receipt, lead.value, "failed recovery changed the receipt");
    let slot: Value = serde_json::from_slice(&fs::read(&slot_path).unwrap()).unwrap();
    assert_eq!(slot, slot_before, "failed recovery changed the lease");
    assert_eq!(
        fs::read_to_string(&partial).unwrap(),
        "keep the interrupted work\n"
    );
    mcp.finish();
    drop(lead);
}

#[test]
fn stale_or_mismatched_lead_authority_fails_before_native_effects() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let lead = delegated_lead_continuation_run(home.path(), &server);
    server.answer_fn(|_| Answer::Error(json!({"message": "no native effect is allowed"})));
    let partial = lead.primary.join("partial-work.txt");
    fs::write(&partial, "preserve this interrupted work\n").unwrap();
    let slot_path = lead.run.receipt.parent().unwrap().join("slot-1.json");
    let slot_before: Value = serde_json::from_slice(&fs::read(&slot_path).unwrap()).unwrap();
    let mut mcp = Mcp::start(home.path());
    let cases = [
        ("exact", 0, "root thread"),
        ("exact", 1, "generation"),
        ("exact", 2, "profile binding"),
        ("fresh", 3, "workspace"),
        ("fresh", 4, "role tool"),
        ("exact", 5, "Lead endpoint"),
    ];
    for (mode, case, expected) in cases {
        let mut value = lead.value.clone();
        match case {
            0 => value["rootThreadId"] = json!(OTHER_LEAD),
            1 => {
                value.as_object_mut().unwrap().remove("runGeneration");
            }
            2 => {
                value.as_object_mut().unwrap().remove("profile");
            }
            3 => {
                value.as_object_mut().unwrap().remove("workspaceRoots");
            }
            4 => {
                value["control"]["config"]["mcp_servers"]["executors"]["enabled_tools"] =
                    json!(EXECUTOR_ROLE_TOOLS);
            }
            5 => endpoint(home.path(), LEAD, LEAD, server.port, 1),
            _ => unreachable!(),
        }
        fs::write(
            &lead.run.receipt,
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
        let response = mcp.call_tool(
            2,
            "executor_resume",
            json!({"run": lead.value["run"], "mode": mode}),
            Some(json!({"threadId": DIRECTOR})),
        );
        let error = tool_error(&response);
        assert!(
            error.contains(expected),
            "case {case} ({mode}): {error}\n{response}"
        );
        assert_eq!(
            server.connections(),
            0,
            "case {case} ({mode}) contacted the app-server"
        );
        assert!(
            server.requests_for("thread/start").is_empty(),
            "case {case} ({mode}) started a thread"
        );
        assert!(
            server.requests_for("turn/start").is_empty(),
            "case {case} ({mode}) started model work"
        );
        let receipt: Value = serde_json::from_slice(&fs::read(&lead.run.receipt).unwrap()).unwrap();
        assert_eq!(receipt, value, "case {case} ({mode}) changed its receipt");
        let slot: Value = serde_json::from_slice(&fs::read(&slot_path).unwrap()).unwrap();
        assert_eq!(slot, slot_before, "case {case} ({mode}) changed its lease");
        assert_eq!(
            fs::read_to_string(&partial).unwrap(),
            "preserve this interrupted work\n",
            "case {case} ({mode}) changed partial work"
        );
    }
    mcp.finish();
    drop(lead);
}

#[test]
fn exact_continuation_reports_missing_pre_turn_history_without_starting_a_turn() {
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
    let response = mcp.call_tool(
        2,
        "executor_resume",
        json!({"mode": "exact"}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&response).contains("no pre-turn history"),
        "{response}"
    );
    assert!(server.requests_for("turn/start").is_empty());
    assert!(server.requests_for("thread/start").is_empty());
    mcp.finish();
    drop(run);
}

#[test]
fn continuation_refuses_an_active_thread_before_starting_new_work() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = synthetic_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        run._checkout.path(),
        false,
        true,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    for (id, mode) in [(2, "exact"), (3, "fresh")] {
        let response = mcp.call_tool(
            id,
            "executor_resume",
            json!({"mode": mode}),
            Some(json!({"threadId": LEAD})),
        );
        assert!(
            tool_error(&response).contains("still has active turn"),
            "{mode}: {response}"
        );
    }
    assert!(server.requests_for("turn/start").is_empty());
    assert!(server.requests_for("thread/start").is_empty());
    assert_eq!(sim.lock().unwrap().threads[EXECUTOR].state(), "active");
    mcp.finish();
    drop(run);
}

#[test]
fn release_requires_explicit_review_and_refuses_foreign_or_ambiguous_targets() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = release_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        &run.checkout,
        false,
        false,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let marker = run.checkout.join("unreviewed.txt");
    fs::write(&marker, "do not delete\n").unwrap();
    let mut mcp = Mcp::start(home.path());

    let missing = mcp.call_tool(
        2,
        "executor_release",
        json!({"run": run.run}),
        Some(json!({"threadId": LEAD})),
    );
    assert!(
        tool_error(&missing).contains("arguments are invalid"),
        "{missing}"
    );
    let empty = release_tool(&mut mcp, 3, LEAD, Some(&run.run), "merged", "  ");
    assert!(tool_error(&empty).contains("nonempty reason"), "{empty}");
    let foreign = release_tool(
        &mut mcp,
        4,
        OTHER_LEAD,
        Some(&run.run),
        "discarded",
        "not this lead's run",
    );
    assert!(
        tool_error(&foreign).contains("no recorded MCP executor run"),
        "{foreign}"
    );
    let executor_caller = release_tool(
        &mut mcp,
        5,
        EXECUTOR,
        Some(&run.run),
        "discarded",
        "an executor cannot release its own run",
    );
    assert!(
        tool_error(&executor_caller).contains("no recorded MCP executor run"),
        "{executor_caller}"
    );
    assert_eq!(slot_record(&run)["state"], "occupied");
    assert!(marker.is_file(), "a refused release reset the checkout");

    let state = run.receipt.parent().unwrap();
    fs::write(
        state.join("mcp-2.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "run": "another-run",
            "leadThreadId": LEAD,
            "executorThreadId": SECOND_EXECUTOR,
            "slot": 2
        }))
        .unwrap(),
    )
    .unwrap();
    let ambiguous = release_tool(&mut mcp, 6, LEAD, None, "discarded", "ambiguous target");
    assert!(tool_error(&ambiguous).contains("ambiguous"), "{ambiguous}");
    assert!(
        marker.is_file(),
        "an ambiguous release reset the only concrete slot"
    );
    assert_eq!(slot_record(&run)["state"], "occupied");
    assert!(server.requests_for("turn/start").is_empty());
    mcp.finish();
    drop(run);
}

#[test]
fn release_refuses_an_active_turn_or_unanswered_request_before_effects() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = release_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        &run.checkout,
        false,
        true,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let marker = run.checkout.join("active-work.txt");
    fs::write(&marker, "active work\n").unwrap();
    let mut mcp = Mcp::start(home.path());

    let active = release_tool(
        &mut mcp,
        2,
        LEAD,
        Some(&run.run),
        "discarded",
        "reviewed while still running",
    );
    assert!(
        tool_error(&active).contains("still has active turn"),
        "{active}"
    );
    assert!(marker.is_file());
    assert_eq!(slot_record(&run)["state"], "occupied");

    sim.lock().unwrap().set_active(EXECUTOR, false);
    let mut receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    receipt["leadMessages"] = json!([{
        "schema": 1,
        "id": "unanswered-release-question",
        "kind": "reply-request",
        "status": "pending",
        "contentId": "unanswered"
    }]);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let pending = release_tool(
        &mut mcp,
        3,
        LEAD,
        Some(&run.run),
        "discarded",
        "reviewed with a question still open",
    );
    assert!(
        tool_error(&pending).contains("unresolved request"),
        "{pending}"
    );
    assert!(marker.is_file());
    assert_eq!(slot_record(&run)["state"], "occupied");
    assert!(server.requests_for("turn/start").is_empty());
    mcp.finish();
    drop(run);
}

#[test]
fn release_refuses_a_live_executor_session_lease_before_reset() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = release_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        &run.checkout,
        false,
        false,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let program = std::env::current_exe().unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    let identity = ServiceProcess::observe(std::process::id(), &program, 0, &user)
        .unwrap()
        .identity();
    let lease = run.receipt.parent().unwrap().join("lease-1.json");
    fs::write(
        &lease,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "owner": run.run,
            "index": 1,
            "path": run.checkout,
            "pid": identity.pid,
            "created": identity.creation_time,
            "program": program
        }))
        .unwrap(),
    )
    .unwrap();
    let marker = run.checkout.join("leased-work.txt");
    fs::write(&marker, "live session work\n").unwrap();
    let mut mcp = Mcp::start(home.path());

    let refused = release_tool(
        &mut mcp,
        2,
        LEAD,
        Some(&run.run),
        "discarded",
        "live session still holds the slot",
    );
    assert!(
        tool_error(&refused).contains("live executor-session lease"),
        "{refused}"
    );
    assert!(marker.is_file(), "a live lease was force-reset");
    assert_eq!(slot_record(&run)["state"], "occupied");
    mcp.finish();
    drop(run);
}

#[test]
fn merged_release_frees_the_slot_records_status_keeps_cache_and_repeats_without_reset() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = release_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        &run.checkout,
        false,
        false,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    fs::create_dir_all(run.checkout.join("target")).unwrap();
    fs::write(run.checkout.join("target/artifact"), "warm cache\n").unwrap();
    let (payload, sibling) = record_release_payload(home.path(), &run, "this-run");
    let mut mcp = Mcp::start(home.path());

    let response = release_tool(
        &mut mcp,
        2,
        LEAD,
        Some(&run.run),
        "merged",
        "accepted merged result",
    );
    let released = result(&response);
    assert_eq!(released["state"], "released", "{released}");
    assert_eq!(released["outcome"], "reused", "{released}");
    assert_eq!(released["released"], true, "{released}");
    assert_eq!(released["disposition"], "merged", "{released}");
    assert_eq!(released["base"], run.base, "{released}");
    assert_eq!(released["messagePayloadsRemoved"], 1, "{released}");
    assert!(run.checkout.join("target/artifact").is_file());
    assert!(!payload.exists(), "the run's own payload survived release");
    assert!(sibling.is_file(), "another run's payload was removed");
    let record = slot_record(&run);
    assert_eq!(record["state"], "released", "{record}");
    assert_eq!(record["disposition"], "merged", "{record}");
    assert_eq!(record["reason"], "accepted merged result", "{record}");
    let receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert_eq!(receipt["release"]["outcome"], "reused", "{receipt}");
    assert_eq!(receipt["release"]["slotFreed"], true, "{receipt}");

    let status = mcp.call_tool(
        3,
        "executor_status",
        json!({}),
        Some(json!({"threadId": LEAD})),
    );
    let status = result(&status);
    assert_eq!(
        status["runs"][0]["release"]["outcome"], "reused",
        "{status}"
    );
    assert_eq!(
        status["runs"][0]["release"]["disposition"], "merged",
        "{status}"
    );

    let sentinel = run.checkout.join("post-release.txt");
    fs::write(&sentinel, "must survive a repeated release\n").unwrap();
    let repeated = release_tool(&mut mcp, 4, LEAD, None, "merged", "same accepted result");
    let repeated = result(&repeated);
    assert_eq!(repeated["idempotent"], true, "{repeated}");
    assert_eq!(repeated["run"], run.run, "{repeated}");
    assert_eq!(repeated["released"], true, "{repeated}");
    assert!(
        sentinel.is_file(),
        "a repeated release reset the slot again"
    );
    assert_eq!(slot_record(&run)["state"], "released");
    mcp.finish();
    drop(run);
}

#[test]
fn merged_release_preserves_dirty_work_and_explicit_discard_resets_it() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = release_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        &run.checkout,
        false,
        false,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let scratch = run.checkout.join("scratch.txt");
    fs::write(&scratch, "unreviewed work\n").unwrap();
    fs::create_dir_all(run.checkout.join("target")).unwrap();
    fs::write(run.checkout.join("target/artifact"), "warm cache\n").unwrap();
    let (payload, sibling) = record_release_payload(home.path(), &run, "dirty-run");
    let mut mcp = Mcp::start(home.path());

    let preserved = release_tool(
        &mut mcp,
        2,
        LEAD,
        Some(&run.run),
        "merged",
        "merged claim with local work",
    );
    let preserved = result(&preserved);
    assert_eq!(preserved["state"], "preserved", "{preserved}");
    assert_eq!(preserved["slotFreed"], false, "{preserved}");
    assert!(scratch.is_file(), "merged force-reset dirty work");
    assert!(payload.is_file(), "a preserved slot removed its messages");
    assert_eq!(slot_record(&run)["state"], "occupied");
    let receipt: Value = serde_json::from_slice(&fs::read(&run.receipt).unwrap()).unwrap();
    assert_eq!(receipt["release"]["outcome"], "preserved", "{receipt}");

    let discarded = release_tool(
        &mut mcp,
        3,
        LEAD,
        Some(&run.run),
        "discarded",
        "explicitly discarded after review",
    );
    let discarded = result(&discarded);
    assert_eq!(discarded["state"], "released", "{discarded}");
    assert_eq!(discarded["outcome"], "reused", "{discarded}");
    assert!(!scratch.exists(), "explicit discard did not reset the slot");
    assert!(run.checkout.join("target/artifact").is_file());
    assert!(!payload.exists(), "the run's payload was not removed");
    assert!(sibling.is_file(), "another run's payload was removed");
    let record = slot_record(&run);
    assert_eq!(record["state"], "released", "{record}");
    assert_eq!(record["disposition"], "discarded", "{record}");
    mcp.finish();
    drop(run);
}

#[test]
fn archived_or_missing_native_thread_alone_leaves_the_slot_occupied() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = release_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        &run.checkout,
        false,
        false,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let marker = run.checkout.join("archived-work.txt");
    fs::write(&marker, "preserved after archive\n").unwrap();
    sim.lock().unwrap().threads.remove(EXECUTOR);
    let mut mcp = Mcp::start(home.path());

    let refused = release_tool(
        &mut mcp,
        2,
        LEAD,
        Some(&run.run),
        "discarded",
        "archive is not release",
    );
    let error = tool_error(&refused);
    assert!(error.contains("native archive alone"), "{error}");
    assert!(
        marker.is_file(),
        "archive alone reset or cleaned the checkout"
    );
    let record = slot_record(&run);
    assert_eq!(record["state"], "occupied", "{record}");
    assert_eq!(record["owner"], run.run, "{record}");
    mcp.finish();
    drop(run);
}

#[test]
fn an_old_release_reference_cannot_reset_a_later_occupant() {
    let home = home();
    let server = Server::start(Bearer::Value(TOKEN.to_owned()));
    let run = release_run(home.path(), LEAD, EXECUTOR, &server);
    let sim = Arc::new(Mutex::new(NativeSim::new(
        LEAD,
        EXECUTOR,
        &run.checkout,
        false,
        false,
    )));
    server.answer_fn(native_responder(Arc::clone(&sim)));
    let mut mcp = Mcp::start(home.path());
    let released = release_tool(
        &mut mcp,
        2,
        LEAD,
        Some(&run.run),
        "merged",
        "accepted first run",
    );
    assert_eq!(result(&released)["state"], "released");

    task_worktree::acquire_slot(
        home.path(),
        &run.repository,
        1,
        "successor-owner",
        Some(&run.base),
        &|_| false,
    )
    .unwrap();
    let successor = run.checkout.join("successor-work.txt");
    fs::write(&successor, "later occupant work\n").unwrap();
    let stale = release_tool(
        &mut mcp,
        3,
        LEAD,
        Some(&run.run),
        "merged",
        "stale release of a reused position",
    );
    assert!(tool_error(&stale).contains("recorded ownership"), "{stale}");
    assert!(successor.is_file(), "the later occupant's work was reset");
    let record = slot_record(&run);
    assert_eq!(record["owner"], "successor-owner", "{record}");
    assert_eq!(record["state"], "occupied", "{record}");
    mcp.finish();
    drop(run);
}
