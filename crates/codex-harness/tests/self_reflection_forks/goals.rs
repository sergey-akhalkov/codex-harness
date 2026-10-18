//! Native goal-RPC scope and reflection-fork suppression.
//!
//! The installed app-server gates `thread/goal/*` on its process configuration,
//! while `thread/fork` reads a separate fork-local `features.goals` switch to
//! decide whether to inherit the source goal. This check drives the real
//! native 0.163.0 contract with owned state and a loopback provider counter.
//! No model request is made or accepted.

use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, ChildStdin, ChildStdout, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
};

const PROVIDER_KEY_ENV: &str = "GOAL_PROBE_PROVIDER_KEY";
const PROVIDER_KEY: &str = "synthetic-goal-probe-provider-key";
const OBJECTIVE: &str = "preserve the exact source goal while the reflection fork stays inert";
const GOAL_STATUS: &str = "paused";

/// The installed native upstream the launcher registration names. No binary is
/// copied and no global configuration is read or changed.
fn installed_upstream() -> PathBuf {
    let home = harness_core::native_launcher::codex_home().expect("installed kit home");
    let registration = home.join("harness/native-launch.json");
    let value: Value = serde_json::from_slice(&fs::read(&registration).unwrap_or_else(|error| {
        panic!(
            "installed launcher registration {}: {error}; deploy the kit before running this check",
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

/// Writes one owned home whose provider route is a loopback counter. The
/// counter never receives a request in this check; a nonzero count is a
/// failure, not a fixture answer.
fn write_home(home: &Path, port: u16, goals: bool) {
    fs::create_dir_all(home).unwrap();
    let trusted = serde_json::to_string(&home.to_string_lossy().to_lowercase()).unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "gpt-6.1-sol"
model_provider = "goal_counter"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "read-only"
cli_auth_credentials_store = "file"

[model_providers.goal_counter]
name = "Owned zero-request counter"
base_url = "http://127.0.0.1:{port}/v1"
env_key = "{PROVIDER_KEY_ENV}"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0

[features]
goals = {goals}

[windows]
sandbox = "unelevated"

[projects.{trusted}]
trust_level = "trusted"
"#
        ),
    )
    .unwrap();
    fs::write(
        home.join("auth.json"),
        serde_json::to_vec_pretty(&json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": PROVIDER_KEY
        }))
        .unwrap(),
    )
    .unwrap();
}

/// Owned native provider route. `Fail` answers every Responses request with
/// 500; `CompleteThenHold` completes the first source turn and holds later
/// turn requests so an active goal can be inspected before any answer.
#[derive(Clone, Copy)]
enum ProviderMode {
    Fail,
    CompleteThenHold,
}

struct ProviderCounter {
    port: u16,
    hits: Arc<AtomicUsize>,
    turns: Arc<AtomicUsize>,
    titles: Arc<AtomicUsize>,
    held: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl ProviderCounter {
    fn start() -> Self {
        Self::configured(ProviderMode::Fail)
    }

    /// Completes the first Responses request so the source has one real
    /// completed turn, then holds every later request open. An active goal is
    /// only inspectable while its continuation request is in flight; holding
    /// that request leaves the source goal active without answering it.
    fn start_complete_then_hold() -> Self {
        Self::configured(ProviderMode::CompleteThenHold)
    }

    fn configured(mode: ProviderMode) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let turns = Arc::new(AtomicUsize::new(0));
        let titles = Arc::new(AtomicUsize::new(0));
        let held_count = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let counted = Arc::clone(&hits);
        let turn_count = Arc::clone(&turns);
        let titled = Arc::clone(&titles);
        let holding = Arc::clone(&held_count);
        let stopping = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            let mut completed = false;
            let mut held = Vec::new();
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(10)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(10)))
                            .unwrap();
                        let Some((request, body)) = read_request(&mut stream) else {
                            continue;
                        };
                        if request.starts_with("GET ") {
                            respond_models(&mut stream);
                            continue;
                        }
                        counted.fetch_add(1, Ordering::SeqCst);
                        let title = is_title_request(&body);
                        if title {
                            titled.fetch_add(1, Ordering::SeqCst);
                        } else {
                            turn_count.fetch_add(1, Ordering::SeqCst);
                        }
                        match mode {
                            ProviderMode::Fail => {
                                let _ = stream.write_all(
                                    b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                                );
                            }
                            ProviderMode::CompleteThenHold if title => {
                                respond_text(&mut stream, "title", "Owned source");
                            }
                            ProviderMode::CompleteThenHold if !completed => {
                                completed = true;
                                respond_text(
                                    &mut stream,
                                    "source-turn",
                                    "owned source turn complete",
                                );
                            }
                            ProviderMode::CompleteThenHold => {
                                holding.fetch_add(1, Ordering::SeqCst);
                                held.push(stream);
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            port,
            hits,
            turns,
            titles,
            held: held_count,
            stop,
            worker: Some(worker),
        }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    fn turns(&self) -> usize {
        self.turns.load(Ordering::SeqCst)
    }

    fn titles(&self) -> usize {
        self.titles.load(Ordering::SeqCst)
    }

    fn held(&self) -> usize {
        self.held.load(Ordering::SeqCst)
    }

    fn wait_for_turns(&self, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.turns() < expected {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {expected} turn requests; saw {}",
                self.turns()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_held(&self, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.held() < expected {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {expected} held provider requests; saw {}",
                self.held()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// One native HTTP request line and bounded body.
fn read_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut bytes = Vec::new();
    loop {
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&bytes[..end]).into_owned();
            let length = header
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            while bytes.len() < end + 4 + length {
                let mut chunk = [0_u8; 4096];
                let count = stream.read(&mut chunk).ok()?;
                if count == 0 {
                    return None;
                }
                bytes.extend_from_slice(&chunk[..count]);
            }
            return header
                .lines()
                .next()
                .map(|line| (line.to_owned(), bytes[end + 4..end + 4 + length].to_vec()));
        }
        if bytes.len() > 64 * 1024 {
            return None;
        }
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

/// The native title request is a separate system turn. It is answered so it
/// cannot be mistaken for the source goal continuation that this check holds.
fn is_title_request(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .is_some_and(|request| {
            request["input"].as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item["role"] == "user"
                        && item["content"].as_array().is_some_and(|parts| {
                            parts.iter().any(|part| {
                                part["text"].as_str().is_some_and(|text| {
                                    text.starts_with("Generate a concise, single-line task title")
                                })
                            })
                        })
                })
            })
        })
}

fn respond_models(stream: &mut TcpStream) {
    let body = json!({
        "object": "list",
        "data": [{"id": "gpt-6.1-sol", "object": "model", "owned_by": "owned"}]
    })
    .to_string();
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
}

fn respond_text(stream: &mut TcpStream, id: &str, text: &str) {
    let item = json!({
        "type": "message",
        "id": format!("owned-message-{id}"),
        "role": "assistant",
        "content": [{"type": "output_text", "text": text}]
    });
    let events = [
        json!({"type": "response.created", "response": {"id": format!("resp-{id}")}}),
        json!({"type": "response.output_item.done", "item": item}),
        json!({
            "type": "response.completed",
            "response": {
                "id": format!("resp-{id}"),
                "usage": {"input_tokens": 10, "output_tokens": 10, "total_tokens": 20}
            }
        }),
    ];
    let body = events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect::<String>();
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
}

impl Drop for ProviderCounter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// One real stdio app-server control connection. Notifications are retained so
/// the check can prove no turn started.
struct Native {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    id: u64,
    notifications: Vec<Value>,
}

impl Native {
    fn spawn(upstream: &Path, home: &Path, extra: &[&str]) -> Self {
        let log = fs::File::create(home.join("app-server.log")).unwrap();
        let mut child = Command::new(upstream)
            .args(["app-server", "--stdio"])
            .args(extra)
            .env("CODEX_HOME", home)
            .env(PROVIDER_KEY_ENV, PROVIDER_KEY)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            id: 0,
            notifications: Vec::new(),
        }
    }

    fn initialize(&mut self) {
        let response = self.request(
            "initialize",
            json!({
                "clientInfo": {"name": "goal-scope-check", "version": "1"},
                "capabilities": {"experimentalApi": true},
            }),
        );
        assert!(response.get("error").is_none(), "{response}");
        self.notify("initialized");
    }

    fn notify(&mut self, method: &str) {
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "method": method})
        )
        .unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .unwrap();
        self.stdin.flush().unwrap();
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).unwrap();
            assert!(read > 0, "app-server closed before answering {method}");
            let value: Value = serde_json::from_str(&line).unwrap();
            if value.get("method").is_some() {
                self.notifications.push(value);
                continue;
            }
            if value["id"].as_u64() == Some(id) {
                return value;
            }
        }
    }

    fn result(&mut self, method: &str, params: Value) -> Value {
        let response = self.request(method, params);
        assert!(response.get("error").is_none(), "{method}: {response}");
        response["result"].clone()
    }

    fn start_source(&mut self, home: &Path) -> String {
        self.result(
            "thread/start",
            json!({"cwd": home, "approvalPolicy": "never", "sandbox": "read-only"}),
        )["thread"]["id"]
            .as_str()
            .expect("thread/start names its thread")
            .to_owned()
    }

    fn fork(&mut self, home: &Path, source: &str, goals: bool) -> String {
        self.result(
            "thread/fork",
            json!({
                "threadId": source,
                "cwd": home,
                "approvalPolicy": "never",
                "sandbox": "read-only",
                "deferGoalContinuation": true,
                "config": {"features.goals": goals},
            }),
        )["thread"]["id"]
            .as_str()
            .expect("thread/fork names its fork")
            .to_owned()
    }

    fn goal(&mut self, thread: &str) -> Value {
        self.result("thread/goal/get", json!({"threadId": thread}))["goal"].clone()
    }

    fn wait_for_completed_turn(&mut self, thread: &str, turn: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let _ = self.goal(thread);
            if self.notifications.iter().any(|note| {
                note["method"] == "turn/completed"
                    && note["params"]["threadId"].as_str() == Some(thread)
                    && note["params"]["turn"]["id"].as_str() == Some(turn)
            }) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for turn {turn} to complete: {:?}",
                self.notifications
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn wait_for_started_turn_after(&mut self, thread: &str, after: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let _ = self.goal(thread);
            if let Some(turn) = self.notifications.iter().rev().find_map(|note| {
                if note["method"] != "turn/started"
                    || note["params"]["threadId"].as_str() != Some(thread)
                {
                    return None;
                }
                let turn = note["params"]["turn"]["id"].as_str()?;
                (turn != after).then(|| turn.to_owned())
            }) {
                return turn;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for a goal turn after {after}: {:?}",
                self.notifications
            );
            thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Evidence {
    dir: Option<tempfile::TempDir>,
}

impl Evidence {
    fn new(prefix: &str) -> Self {
        Self {
            dir: Some(
                tempfile::Builder::new()
                    .prefix(prefix)
                    .tempdir()
                    .expect("owned goal evidence"),
            ),
        }
    }

    fn path(&self) -> &Path {
        self.dir.as_ref().expect("evidence directory").path()
    }
}

impl Drop for Evidence {
    fn drop(&mut self) {
        if std::thread::panicking() || std::env::var_os("HARNESS_KEEP_WORKSPACE_EVIDENCE").is_some()
        {
            if let Some(dir) = self.dir.take() {
                eprintln!("retained goal evidence: {}", dir.keep().display());
            }
        }
    }
}

#[test]
fn native_goal_rpcs_and_fork_suppression_follow_process_and_fork_scope() {
    let evidence = tempfile::tempdir().expect("owned goal evidence");
    let upstream = installed_upstream();
    let version = Command::new(&upstream)
        .arg("--version")
        .output()
        .expect("installed native version");
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    assert!(
        version.starts_with("codex-cli "),
        "registered upstream did not report its version: {version}"
    );
    fs::write(evidence.path().join("native-version.txt"), &version).unwrap();
    let provider = ProviderCounter::start();

    // The actual controller failure: the source home disables Goals, so the
    // process also refuses the administrative goal read before any model work.
    let disabled_home = evidence.path().join("source-goals-disabled");
    write_home(&disabled_home, provider.port, false);
    let mut native = Native::spawn(&upstream, &disabled_home, &[]);
    native.initialize();
    let source = native.start_source(&disabled_home);
    let refused = native.request("thread/goal/get", json!({"threadId": source}));
    assert_eq!(
        refused["error"]["message"], "goals feature is disabled",
        "{refused}"
    );
    assert_eq!(
        provider.hits(),
        0,
        "the refused read must make no provider request"
    );
    fs::write(
        evidence.path().join("disabled-failure.json"),
        serde_json::to_vec_pretty(&json!({
            "nativeVersion": version,
            "sourceGoals": false,
            "processGoalsOverride": null,
            "goalRead": refused,
            "providerRequests": provider.hits(),
        }))
        .unwrap(),
    )
    .unwrap();
    drop(native);

    // The corrected path keeps the process able to administer goals for both
    // source-home settings, while the fork's own config still suppresses the
    // inherited goal. A goals-enabled control fork proves suppression is real.
    for source_goals in [false, true] {
        let home = evidence.path().join(format!("source-goals-{source_goals}"));
        write_home(&home, provider.port, source_goals);
        let mut native = Native::spawn(&upstream, &home, &["-c", "features.goals=true"]);
        native.initialize();
        let source = native.start_source(&home);
        assert_eq!(
            native.goal(&source),
            Value::Null,
            "a new source starts without a goal"
        );
        let set = native.result(
            "thread/goal/set",
            json!({
                "threadId": source,
                "objective": OBJECTIVE,
                "status": GOAL_STATUS,
                "origin": "user",
            }),
        );
        assert_eq!(set["goal"]["objective"], OBJECTIVE, "{set}");
        assert_eq!(set["goal"]["status"], GOAL_STATUS, "{set}");
        let source_goal = native.goal(&source);
        assert_eq!(source_goal["objective"], OBJECTIVE, "{source_goal}");
        assert_eq!(source_goal["status"], GOAL_STATUS, "{source_goal}");

        let inherited_thread = native.fork(&home, &source, true);
        assert_ne!(inherited_thread, source);
        let inherited_goal = native.goal(&inherited_thread);
        assert_eq!(
            inherited_goal["objective"], OBJECTIVE,
            "the control fork must expose the inheritance the suppression prevents: {inherited_goal}"
        );
        assert_eq!(inherited_goal["status"], GOAL_STATUS, "{inherited_goal}");

        let suppressed_thread = native.fork(&home, &source, false);
        assert_ne!(suppressed_thread, source);
        assert_eq!(
            native.goal(&suppressed_thread),
            Value::Null,
            "the reflection fork must carry no inherited goal"
        );
        assert_eq!(
            native.goal(&source),
            source_goal,
            "the source goal and state must be unchanged by either fork"
        );
        assert_eq!(
            provider.hits(),
            0,
            "fork preparation and goal inspection must make no provider request"
        );
        assert!(
            native
                .notifications
                .iter()
                .all(|note| note["method"] != "turn/started"),
            "no fork may start an autonomous turn: {:?}",
            native.notifications
        );
        fs::write(
            evidence.path().join(format!("goals-{source_goals}.json")),
            serde_json::to_vec_pretty(&json!({
                "nativeVersion": version,
                "sourceGoals": source_goals,
                "processGoalsOverride": true,
                "sourceGoal": source_goal,
                "inheritedForkGoal": inherited_goal,
                "suppressedForkGoal": Value::Null,
                "inheritedFork": inherited_thread,
                "suppressedFork": suppressed_thread,
                "providerRequests": provider.hits(),
            }))
            .unwrap(),
        )
        .unwrap();
        drop(native);
    }
    assert_eq!(provider.hits(), 0);

    if std::env::var_os("HARNESS_KEEP_WORKSPACE_EVIDENCE").is_some() {
        eprintln!("retained goal evidence: {}", evidence.keep().display());
    }
}

#[test]
fn native_active_source_goal_is_preserved_and_fork_stays_inert() {
    let evidence = Evidence::new("goal-active-source-");
    let upstream = installed_upstream();
    let version = Command::new(&upstream)
        .arg("--version")
        .output()
        .expect("installed native version");
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    assert!(version.starts_with("codex-cli "), "{version}");
    fs::write(evidence.path().join("native-version.txt"), &version).unwrap();

    let provider = ProviderCounter::start_complete_then_hold();
    let home = evidence.path().join("source-active");
    write_home(&home, provider.port, false);
    let mut native = Native::spawn(&upstream, &home, &["-c", "features.goals=true"]);
    native.initialize();
    let source = native.start_source(&home);
    let idle_include_turns = native.request(
        "thread/read",
        json!({"threadId": source, "includeTurns": true}),
    );
    fs::write(
        evidence.path().join("idle-thread-read-include-turns.json"),
        serde_json::to_vec_pretty(&idle_include_turns).unwrap(),
    )
    .unwrap();
    let idle_error = idle_include_turns["error"]["message"]
        .as_str()
        .unwrap_or_default();
    assert!(
        idle_error.contains("list_turns")
            || idle_include_turns["result"]["thread"]["turns"]
                .as_array()
                .is_some_and(Vec::is_empty),
        "unexpected idle thread/read answer: {idle_include_turns}"
    );

    let completed_turn = native.result(
        "turn/start",
        json!({
            "threadId": source,
            "input": [{
                "type": "text",
                "text": "complete one owned source turn before activating the goal",
            }],
        }),
    )["turn"]["id"]
        .as_str()
        .expect("turn/start names its turn")
        .to_owned();
    provider.wait_for_turns(1);
    assert_eq!(
        provider.turns(),
        1,
        "the pre-goal source turn must reach the owned provider exactly once"
    );
    native.wait_for_completed_turn(&source, &completed_turn);
    let completed_read = native.result(
        "thread/read",
        json!({"threadId": source, "includeTurns": true}),
    );
    let completed_read_turns = completed_read["thread"]["turns"]
        .as_array()
        .expect("a completed source read carries turns")
        .iter()
        .map(|turn| json!({"id": turn["id"], "status": turn["status"]}))
        .collect::<Vec<_>>();
    assert!(
        completed_read_turns.iter().any(|turn| {
            turn["id"].as_str() == Some(completed_turn.as_str()) && turn["status"] == "completed"
        }),
        "the completed source turn must be readable with includeTurns: {completed_read_turns:?}"
    );

    let set = native.result(
        "thread/goal/set",
        json!({
            "threadId": source,
            "objective": OBJECTIVE,
            "status": "active",
            "origin": "user",
        }),
    );
    assert_eq!(set["goal"]["status"], "active", "{set}");
    provider.wait_for_turns(2);
    assert_eq!(
        provider.turns(),
        2,
        "setting the goal active must start exactly one source continuation request"
    );
    provider.wait_for_held(1);
    assert_eq!(
        provider.held(),
        1,
        "the source goal continuation request must still be in flight"
    );
    let continuation_turn = native.wait_for_started_turn_after(&source, &completed_turn);
    let source_goal_before = native.goal(&source);
    assert_eq!(
        source_goal_before["objective"], OBJECTIVE,
        "{source_goal_before}"
    );
    assert_eq!(
        source_goal_before["status"], "active",
        "{source_goal_before}"
    );
    let setup_provider_requests = provider.hits();
    let setup_provider_turns = provider.turns();
    let setup_provider_titles = provider.titles();

    // The production fork keeps this process able to administer goals, turns
    // goals off in the fork-local config, and defers any inherited
    // continuation. The source goal must remain the exact active goal.
    let fork = native.result(
        "thread/fork",
        json!({
            "threadId": source,
            "lastTurnId": completed_turn,
            "cwd": home,
            "approvalPolicy": "never",
            "sandbox": "read-only",
            "deferGoalContinuation": true,
            "config": {"features.goals": false},
        }),
    )["thread"]["id"]
        .as_str()
        .expect("thread/fork names its fork")
        .to_owned();
    assert_ne!(fork, source);

    let fork_goal_before_clear = native.goal(&fork);
    let goal_cleared = !fork_goal_before_clear.is_null();
    if goal_cleared {
        native.result(
            "thread/goal/clear",
            json!({"threadId": fork, "origin": "user"}),
        );
    }
    let fork_goal_after_clear = native.goal(&fork);
    assert!(
        fork_goal_after_clear.is_null(),
        "the fork must carry no active inherited goal: {fork_goal_after_clear}"
    );

    thread::sleep(Duration::from_millis(1500));
    let source_goal_after = native.goal(&source);
    assert_eq!(
        source_goal_after, source_goal_before,
        "the active source goal must be preserved exactly across the fork"
    );
    assert_eq!(
        provider.turns(),
        setup_provider_turns,
        "the fork must not make a provider turn request"
    );
    assert_eq!(
        provider.hits(),
        setup_provider_requests,
        "the fork must not make any provider request"
    );
    let fork_turn_started = native.notifications.iter().any(|note| {
        note["method"] == "turn/started"
            && note["params"]["threadId"].as_str() == Some(fork.as_str())
    });
    assert!(
        !fork_turn_started,
        "the fork must not start an autonomous turn: {:?}",
        native.notifications
    );

    fs::write(
        evidence.path().join("active-goal.json"),
        serde_json::to_vec_pretty(&json!({
            "nativeVersion": version,
            "sourceHomeGoals": false,
            "processGoalsOverride": true,
            "forkConfigGoals": false,
            "deferGoalContinuation": true,
            "sourceCompletedTurn": completed_turn,
            "sourceContinuationTurn": continuation_turn,
            "sourceGoalBefore": source_goal_before,
            "sourceGoalAfter": source_goal_after,
            "forkThread": fork,
            "forkGoalBeforeClear": fork_goal_before_clear,
            "forkGoalAfterClear": fork_goal_after_clear,
            "goalCleared": goal_cleared,
            "providerRequestsBeforeFork": setup_provider_requests,
            "providerTurnRequestsBeforeFork": setup_provider_turns,
            "providerTitleRequestsBeforeFork": setup_provider_titles,
            "providerHeldBeforeFork": provider.held(),
            "providerRequestsAfterWait": provider.hits(),
            "providerTurnRequestsAfterWait": provider.turns(),
            "idleThreadReadIncludeTurns": idle_include_turns,
            "completedThreadReadTurns": completed_read_turns,
            "forkTurnStarted": fork_turn_started,
        }))
        .unwrap(),
    )
    .unwrap();
}
