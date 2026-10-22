//! Native goal overload and stale-resume contract; owned model-free Responses.
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

const WAIT: Duration = Duration::from_secs(30);
const OBJECTIVE: &str = "Preserve the owned proof through an overload";

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
        client.request(
            "initialize",
            json!({"clientInfo":{"name":"goal-recovery-contract","version":"1"},
                "capabilities":{"experimentalApi":true}}),
        );
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
                "observation deadline; native goal state is unknown"
            );
            if let Some(value) = self.connection.receive(Duration::from_millis(200)).unwrap() {
                writeln!(self.log, "{value}").unwrap();
                return value;
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.connection
            .send(&json!({"id":id,"method":method,"params":params}), WAIT)
            .unwrap();
        let until = Instant::now() + WAIT;
        loop {
            let value = self.receive(until);
            if value["id"] == id {
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
            assert!(self.pending.len() < 512, "contract pending-event bound");
            self.pending.push_back(value);
        }
    }
}

struct NativeFixture {
    exe: PathBuf,
    root: BrokerRoot,
    home: PathBuf,
    workspace: PathBuf,
    port: u16,
    token: String,
    version: String,
    job: Job,
    responses: control_responses::Responses,
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

fn capture(exe: &Path, home: &Path, workspace: &Path, root: &Path, args: &[&str]) -> String {
    let mut spec = command(exe, home, workspace);
    spec.args = args.iter().map(Into::into).collect();
    let stdout = root.join("version-stdout.txt");
    let stderr = root.join("version-stderr.txt");
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
    assert_eq!(outcome.reason, StopReason::Exited);
    assert_eq!(outcome.exit_code, 0);
    fs::read_to_string(stdout).unwrap()
}

fn native_fixture(overload_at: usize) -> NativeFixture {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    assert!(exe.is_absolute() && exe.is_file());
    let owned_root = BrokerRoot::prepare().unwrap().keep();
    let root = owned_root.path();
    eprintln!("goal recovery evidence: {}", root.display());
    let home = root.join("home");
    let workspace = root.join("workspace");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&workspace).unwrap();
    let version = capture(&exe, &home, &workspace, root, &["--version"])
        .trim()
        .to_owned();
    assert_eq!(version, "codex-cli 0.163.0");
    let responses = control_responses::Responses::with_server_overload(root.into(), overload_at);
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
name = "Owned native goal fixture"
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
        "-c".into(),
        "features.goals=true".into(),
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
    let _server = job.spawn(&spec).unwrap();
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
        version,
        job,
        responses,
    }
}

fn start_thread(client: &mut Client, workspace: &Path, name: &str) -> String {
    let started = client.request(
        "thread/start",
        json!({"cwd":workspace,"model":"gpt-6-astra","modelProvider":"control_fixture",
            "allowProviderModelFallback":false,"approvalPolicy":"never",
            "sandbox":"danger-full-access"}),
    );
    assert_eq!(started["model"], "gpt-6-astra");
    assert_eq!(started["modelProvider"], "control_fixture");
    let thread = started["thread"]["id"].as_str().unwrap().to_owned();
    client.request("thread/name/set", json!({"threadId":thread,"name":name}));
    client.request("thread/resume", json!({"threadId":thread}));
    thread
}

fn goal(client: &mut Client, thread: &str) -> Value {
    client.request("thread/goal/get", json!({"threadId":thread}))["goal"].clone()
}

fn wait_goal(client: &mut Client, thread: &str, predicate: impl Fn(&Value) -> bool) -> Value {
    let until = Instant::now() + WAIT;
    loop {
        let current = goal(client, thread);
        if predicate(&current) {
            return current;
        }
        assert!(
            Instant::now() < until,
            "goal did not reach the expected state: {current}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn tui(fixture: &NativeFixture, thread: &str) -> ConsoleSession {
    let mut command = command(&fixture.exe, &fixture.home, &fixture.workspace);
    command.args = vec![
        "--remote".into(),
        format!("ws://127.0.0.1:{}", fixture.port).into(),
        "--remote-auth-token-env".into(),
        "HARNESS_CONTROL_TOKEN".into(),
        "--no-alt-screen".into(),
        "resume".into(),
        thread.into(),
    ];
    command.env.insert(
        "HARNESS_CONTROL_TOKEN".into(),
        Some(fixture.token.clone().into()),
    );
    ConsoleSession::spawn(ConsoleSpec::new(command)).unwrap()
}

fn visible(transcript: &str) -> String {
    let bytes = transcript.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'[' => {
                    i += 2;
                    while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    i += usize::from(i < bytes.len());
                    continue;
                }
                b']' => {
                    i += 2;
                    while i < bytes.len()
                        && bytes[i] != 0x07
                        && !(bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\'))
                    {
                        i += 1;
                    }
                    i += if bytes.get(i) == Some(&0x1b) { 2 } else { 1 };
                    continue;
                }
                _ => {}
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn wait_visible(session: &ConsoleSession, evidence: &Path, patterns: &[&str]) {
    let until = Instant::now() + WAIT;
    loop {
        let transcript = session.transcript();
        let text = visible(&transcript);
        let _ = fs::write(evidence, &transcript);
        if patterns.iter().any(|pattern| text.contains(pattern)) {
            return;
        }
        assert!(
            Instant::now() < until,
            "missing {patterns:?}; inspect {}",
            evidence.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn send_tui_command(session: &ConsoleSession, terminal: &Path, command: &str) {
    wait_visible(
        session,
        terminal,
        &["Ask Codex to do anything", "Type a message"],
    );
    if visible(&session.transcript()).contains("Resume paused goal?") {
        session.send("\x1b").unwrap();
        std::thread::sleep(Duration::from_millis(500));
    }
    session.send(&format!("{command}\r")).unwrap();
    wait_visible(session, terminal, &[command]);
}

fn provider_requests(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("provider-") && name.ends_with(".json"))
        })
        .collect();
    files.sort();
    files
}

fn finish(fixture: NativeFixture, evidence_name: &str, mut evidence: Value, terminal: String) {
    let root = fixture.root.path().to_path_buf();
    let exe_hash = format!("{:x}", Sha256::digest(fs::read(&fixture.exe).unwrap()));
    let NativeFixture {
        version,
        job,
        responses,
        ..
    } = fixture;
    drop(responses);
    let cleanup = job.terminate(0, Duration::from_secs(2)).unwrap();
    evidence["cliVersion"] = json!(version);
    evidence["cliSha256"] = json!(exe_hash);
    evidence["tuiTranscript"] = json!(terminal);
    evidence["cleanup"] = json!({"activeProcesses":cleanup.active_processes});
    fs::write(
        root.join(evidence_name),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned native goal overload and recovery probe"]
fn native_overload_blocks_goal_and_status_only_resume_recovers() {
    let fixture = native_fixture(2);
    let root = fixture.root.path();
    let mut first = Client::connect(fixture.port, &fixture.token, root, "first");
    let mut observer = Client::connect(fixture.port, &fixture.token, root, "observer");
    let thread = start_thread(&mut first, &fixture.workspace, "Owned goal recovery");
    let set = first.request(
        "thread/goal/set",
        json!({"threadId":thread,"objective":OBJECTIVE,"status":"active",
            "tokenBudget":10000,"origin":"user"}),
    );
    assert_eq!(set["goal"]["status"], "active");
    first.request(
        "turn/start",
        json!({"threadId":thread,"input":[{"type":"text",
            "text":"Perform the owned proof command and return its consumed result."}]}),
    );
    let session = tui(&fixture, &thread);
    let failed = first.event(|event| {
        event["method"] == "turn/completed"
            && event["params"]["threadId"] == thread
            && event["params"]["turn"]["status"] == "failed"
    });
    assert_eq!(
        failed["params"]["turn"]["error"]["codexErrorInfo"], "serverOverloaded",
        "{failed}"
    );
    let failed_turn = failed["params"]["turn"]["id"].as_str().unwrap().to_owned();
    let before = wait_goal(&mut first, &thread, |goal| goal["status"] == "blocked");
    assert_eq!(before["threadId"], thread);
    assert_eq!(before["objective"], OBJECTIVE);
    assert_eq!(before["tokenBudget"], 10000);
    let tokens_before = before["tokensUsed"].as_i64().unwrap();
    let terminal = root.join("terminal-overload.txt");
    wait_visible(&session, &terminal, &["Goal stalled (/goal resume)"]);

    let resumed = observer.request(
        "thread/goal/set",
        json!({"threadId":thread,"status":"active","origin":"automatic"}),
    )["goal"]
        .clone();
    assert_eq!(resumed["status"], "active");
    assert_eq!(resumed["threadId"], thread);
    assert_eq!(resumed["createdAt"], before["createdAt"]);
    assert_eq!(resumed["objective"], OBJECTIVE);
    assert_eq!(resumed["tokenBudget"], 10000);
    let completed = first.event(|event| {
        event["method"] == "turn/completed"
            && event["params"]["threadId"] == thread
            && event["params"]["turn"]["status"] == "completed"
            && event["params"]["turn"]["id"] != failed_turn
    });
    assert_eq!(
        completed["params"]["turn"]["status"], "completed",
        "{completed}"
    );
    let after = wait_goal(&mut first, &thread, |goal| {
        goal["status"] == "complete" && goal["tokensUsed"].as_i64() > Some(tokens_before)
    });
    assert_eq!(after["threadId"], thread);
    assert_eq!(after["createdAt"], before["createdAt"]);
    assert_eq!(after["objective"], OBJECTIVE);
    assert_eq!(after["tokenBudget"], 10000);
    assert!(
        after["tokensUsed"].as_i64().unwrap() > tokens_before,
        "native accounting must advance: before={before} after={after}"
    );
    wait_visible(&session, &terminal, &[control_responses::FINAL]);
    assert_eq!(
        fs::read_to_string(fixture.workspace.join("proof.txt")).unwrap(),
        "one",
        "the owned tool effect must happen exactly once"
    );
    let requests = provider_requests(root);
    assert_eq!(requests.len(), 4, "{requests:?}");
    for request in &requests {
        let body: Value = serde_json::from_slice(&fs::read(request).unwrap()).unwrap();
        assert_eq!(body["client_metadata"]["thread_id"], thread);
    }
    let overload = fs::read_to_string(root.join("overload-response.json")).unwrap();
    assert!(overload.contains("server_is_overloaded"));
    drop(session);
    drop(first);
    drop(observer);
    finish(
        fixture,
        "goal-recovery-result.json",
        json!({
            "schema":1,
            "threadId":thread,
            "goalBefore":before,
            "goalAfter":after,
            "providerRequests":requests.len(),
            "toolEffect":"one"
        }),
        terminal.to_string_lossy().into_owned(),
    );
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned native goal pause-race probe"]
fn native_stale_automatic_resume_wins_after_user_pause() {
    let fixture = native_fixture(1);
    let root = fixture.root.path();
    let mut first = Client::connect(fixture.port, &fixture.token, root, "pause-owner");
    let mut observer = Client::connect(fixture.port, &fixture.token, root, "pause-observer");
    let thread = start_thread(&mut first, &fixture.workspace, "Owned goal pause race");
    let paused = first.request(
        "thread/goal/set",
        json!({"threadId":thread,"objective":OBJECTIVE,"status":"paused",
            "tokenBudget":10000,"origin":"user"}),
    )["goal"]
        .clone();
    assert_eq!(paused["status"], "paused");
    let initial_pause = first.event(|event| {
        event["method"] == "thread/goal/updated"
            && event["params"]["threadId"] == thread
            && event["params"]["goal"]["status"] == "paused"
    });
    assert_eq!(initial_pause["params"]["goal"]["status"], "paused");
    let before = goal(&mut first, &thread);
    assert_eq!(before["status"], "paused");
    let session = tui(&fixture, &thread);
    let terminal = root.join("terminal-stale-resume.txt");
    wait_visible(&session, &terminal, &["Goal paused (/goal resume)"]);
    send_tui_command(&session, &terminal, "/goal pause");
    let user_pause = first.event(|event| {
        event["method"] == "thread/goal/updated"
            && event["params"]["threadId"] == thread
            && event["params"]["goal"]["status"] == "paused"
    });
    assert_eq!(user_pause["params"]["goal"]["status"], "paused");
    let after_user = goal(&mut first, &thread);
    assert_eq!(after_user["status"], "paused");
    assert!(
        provider_requests(root).is_empty(),
        "the user pause must not start model work"
    );
    let stale = observer.request(
        "thread/goal/set",
        json!({"threadId":thread,"status":"active","origin":"automatic"}),
    )["goal"]
        .clone();
    assert_eq!(stale["status"], "active");
    assert_eq!(stale["threadId"], thread);
    assert_eq!(stale["createdAt"], before["createdAt"]);
    assert_eq!(stale["objective"], OBJECTIVE);
    assert_eq!(stale["tokenBudget"], 10000);
    let after_failure = wait_goal(&mut first, &thread, |goal| goal["status"] == "blocked");
    let requests = provider_requests(root);
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(
        requests[0].ends_with("provider-1.json"),
        "the stale activation must start exactly one native turn"
    );
    fs::write(&terminal, session.transcript()).unwrap();
    assert!(
        visible(&fs::read_to_string(&terminal).unwrap()).contains("/goal pause"),
        "the user command must reach the owned native terminal"
    );
    drop(session);
    drop(first);
    drop(observer);
    finish(
        fixture,
        "stale-resume-result.json",
        json!({
            "schema":1,
            "threadId":thread,
            "sequence":[
                {"step":"user-pause-before","status":after_user["status"]},
                {"step":"no-provider-request-before-stale","status":"paused"},
                {"step":"stale-automatic-resume","origin":"automatic","status":stale["status"]},
                {"step":"stale-activation-started-native-work","status":after_failure["status"]}
            ],
            "goalBefore":before,
            "goalAfter":after_failure,
            "acceptedUpdate":stale,
            "providerRequests":requests.len(),
            "race":"A status read followed by an unchecked automatic update is not an ordering boundary."
        }),
        terminal.to_string_lossy().into_owned(),
    );
}
