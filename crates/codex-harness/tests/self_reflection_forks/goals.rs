//! Native goal-RPC scope and reflection-fork suppression.
//!
//! The installed app-server gates `thread/goal/*` on its process configuration,
//! while `thread/fork` reads a separate fork-local `features.goals` switch to
//! decide whether to inherit the source goal. This check drives the real
//! native 0.163.0 contract with owned state and a loopback provider counter.
//! No model request is made or accepted.

use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
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

/// Counts every connection the native provider route opens. It answers 500 so
/// an accidental turn cannot be mistaken for a completed one.
struct ProviderCounter {
    port: u16,
    hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl ProviderCounter {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let counted = Arc::clone(&hits);
        let stopping = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        counted.fetch_add(1, Ordering::SeqCst);
                        let _ = stream.write_all(
                            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
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
            stop,
            worker: Some(worker),
        }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
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
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
