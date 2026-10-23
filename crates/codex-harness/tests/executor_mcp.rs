#![cfg(windows)]

#[path = "fixtures/control_endpoint.rs"]
mod control_endpoint;

use control_endpoint::{Answer, Bearer, Server};
use harness_core::{process_service::ServiceProcess, task_worktree};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

const LEAD: &str = "01a00000-0000-7000-8000-000000000001";
const OTHER_LEAD: &str = "01a00000-0000-7000-8000-000000000002";
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
        let mut params = json!({
            "name": "executor_dispatch",
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
