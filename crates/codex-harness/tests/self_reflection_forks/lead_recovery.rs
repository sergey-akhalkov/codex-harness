//! Native same-thread lead/controller replacement.
//!
//! One managed lead host owns the real native app-server for one thread. The
//! supported successor is `lead start --session THREAD`: the stale endpoint is
//! retired, the same thread is resumed by a different verified process, and the
//! previous endpoint and its app-server must be gone. Only Responses are
//! canned here; no model, subscription or global installation is used.

use super::*;
#[path = "../fixtures/control_responses.rs"]
mod control_responses;

use harness_core::{
    board_cli, build_identity,
    delivery_reflection::{
        ACCEPTED_CLOSE_REASON, AcceptanceBinding, ActivationRequest,
        DeliveryState as RecordedDeliveryState, JournalPolicy, Purpose, ReflectionController,
        ReflectionState, StageState as RecordedStageState, acceptance_metadata,
    },
    native_launcher,
    process::{ProcessIdentity, StopReason},
    process_service::{self, ServiceProcess},
    task_control::ControlConnection,
};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const PROVIDER_KEY_ENV: &str = "LEAD_RECOVERY_PROVIDER_KEY";
const PROVIDER_KEY: &str = "synthetic-lead-recovery-provider-key";
const FIRST_PROMPT: &str = "LEAD-RECOVERY-OWNED-FIRST-TURN: reply with LEAD-RECOVERY-FIRST-REPLY.";
const FIRST_REPLY: &str = "LEAD-RECOVERY-FIRST-REPLY";
const EXECUTOR_SOURCE_PROMPT: &str =
    "LEAD-RECOVERY-OWNED-EXECUTOR-SOURCE: reply with LEAD-RECOVERY-EXECUTOR-SOURCE-REPLY.";
const EXECUTOR_SOURCE_REPLY: &str = "LEAD-RECOVERY-EXECUTOR-SOURCE-REPLY";
const EXECUTOR_RESULT_TEXT: &str = "LEAD-RECOVERY-EXECUTOR-RESULT";
const LEAD_RESULT_TEXT: &str = "LEAD-RECOVERY-LEAD-RESULT";
const TITLE_PROMPT: &str = "Generate a concise, single-line task title";
const WAIT: Duration = Duration::from_secs(120);
const EXIT_WAIT: Duration = Duration::from_secs(120);

/// True for the native title-generation request, which carries its own
/// provider identity but must not be mistaken for a reflection stage turn.
fn title_generation_request(request: &Value) -> bool {
    request["input"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["role"] == "user"
                && item["content"].as_array().is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part["text"]
                            .as_str()
                            .is_some_and(|text| text.starts_with(TITLE_PROMPT))
                    })
                })
        })
    })
}

fn message(text: &str) -> Value {
    json!({
        "type": "message",
        "id": format!("lead-recovery-{}", text.len()),
        "role": "assistant",
        "content": [{"type": "output_text", "text": text}],
    })
}

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

/// Record one build directory whose `codex.exe` is this checkout's real
/// launcher, beside the integrity stubs the record names. `lead start` launches
/// exactly `<home>/harness/bin/codex.exe`, so the registration must select that
/// same directory and its source root must be this checkout.
fn launcher_build(home: &Path, source: &Path) -> PathBuf {
    let build = home.join("harness/bin");
    fs::create_dir_all(&build).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_codex"), build.join("codex.exe")).unwrap();
    for binary in build_identity::BINARIES {
        let path = build.join(binary);
        if !path.exists() {
            fs::write(&path, binary.as_bytes()).unwrap();
        }
    }
    let record = build_identity::BuildRecord {
        schema: build_identity::SCHEMA,
        source_root: source.to_path_buf(),
        source: build_identity::source_identity(source).unwrap(),
        rustc: "fixture".into(),
        cargo: "fixture".into(),
        target: "x86_64-pc-windows-msvc".into(),
        profile: "release".into(),
        binaries: build_identity::BINARIES
            .iter()
            .map(|name| {
                (
                    (*name).to_string(),
                    build_identity::hash_file(&build.join(name)).unwrap(),
                )
            })
            .collect(),
    };
    fs::write(
        build.join("build.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    build
}

fn write_registration(home: &Path, build: &Path, upstream: &Path) {
    let registration = native_launcher::Registration {
        schema: 2,
        state: None,
        build: Some(build.to_path_buf()),
        task_control: false,
        upstream: native_launcher::Upstream {
            executable: upstream.to_path_buf(),
            sha256: build_identity::hash_file(upstream).unwrap(),
            package: None,
        },
    };
    fs::write(
        home.join("harness/native-launch.json"),
        serde_json::to_vec_pretty(&registration).unwrap(),
    )
    .unwrap();
}

/// One owned home: the lead profile and the executor profile both route to the
/// same canned local Responses server. The executor stage keeps the kit's `ds`
/// routing (deepseek-flash/max); the lead host keeps the default GPT-6 Astra
/// routing. Both are local fixture names, never a provider request.
fn write_config(home: &Path, workspace: &Path, port: u16, goals: bool) {
    let trusted = serde_json::to_string(&workspace.to_str().unwrap().to_lowercase()).unwrap();
    let provider = |id: &str| {
        format!(
            r#"[model_providers.{id}]
name = "Canned lead recovery Responses"
base_url = "http://127.0.0.1:{port}/v1"
env_key = "{PROVIDER_KEY_ENV}"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
"#
        )
    };
    fs::write(
        home.join("config.toml"),
        format!(
            r#"model = "gpt-6-astra"
model_provider = "lead_recovery"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
web_search = "disabled"
model_auto_compact_token_limit = 1000000
model_context_window = 20000
cli_auth_credentials_store = "file"
{}
{}
[windows]
sandbox = "unelevated"
[features]
hooks = false
apps = false
multi_agent = false
memories = false
goals = {goals}
plugins = true
skill_search = true
[projects.{trusted}]
trust_level = "trusted"
"#,
            provider("lead_recovery"),
            provider("deepseek"),
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
    // The reflection executor stage runs under the kit's `ds` profile, which
    // must exist in this owned home and route to the same canned server.
    fs::write(
        home.join("ds.config.toml"),
        "model = \"deepseek-flash\"\nmodel_provider = \"deepseek\"\nmodel_reasoning_effort = \"max\"\n",
    )
    .unwrap();
}

/// The endpoint records the process program path; Windows path spellings can
/// differ in separators or case, so compare canonically when possible.
fn same_program(actual: &Path, expected: &Path) -> bool {
    if actual == expected {
        return true;
    }
    match (actual.canonicalize(), expected.canonicalize()) {
        (Ok(actual), Ok(expected)) => actual == expected,
        _ => actual
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.to_string_lossy()),
    }
}

fn filtered_path() -> OsString {
    let child_path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .filter(|entry| {
            !entry.components().any(|part| {
                part.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("WindowsApps")
            })
        })
        .collect::<Vec<_>>();
    std::env::join_paths(child_path).unwrap()
}

fn lead_spec(
    entry: &Path,
    home: &Path,
    checkout: &Path,
    session: Option<&str>,
    prompt: Option<&str>,
    evidence: &Path,
) -> CommandSpec {
    let mut spec = CommandSpec::new(entry);
    spec.args = vec![
        "lead".into(),
        "start".into(),
        "--source".into(),
        checkout.as_os_str().to_owned(),
        "--codex-home".into(),
        home.as_os_str().to_owned(),
    ];
    if let Some(session) = session {
        spec.args.push("--session".into());
        spec.args.push(session.into());
    }
    if let Some(prompt) = prompt {
        spec.args.push("--exec".into());
        spec.args.push(prompt.into());
    }
    spec.current_dir = Some(checkout.to_owned());
    spec.env
        .insert("CODEX_HOME".into(), Some(home.as_os_str().to_owned()));
    spec.env
        .insert(PROVIDER_KEY_ENV.into(), Some(PROVIDER_KEY.into()));
    spec.env.insert("PATH".into(), Some(filtered_path()));
    spec.env.insert(
        "CODEX_HARNESS_CPU_ACCOUNT".into(),
        Some(evidence.join("cpu-account").into_os_string()),
    );
    for key in [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "CODEX_SESSION_ID",
        "CODEX_THREAD_ID",
        "HARNESS_EXECUTOR_SESSION",
        "HARNESS_EXECUTOR_RUN",
        "HARNESS_CONTEXT_ROLE",
        "HARNESS_CONTEXT_REVISION",
        "HARNESS_ORIGINATING_LEAD",
        "HARNESS_LEAD_THREAD",
        "HARNESS_LEAD_RECIPIENT",
        "WT_SESSION",
    ] {
        spec.env.insert(key.into(), None);
    }
    spec
}

fn spawn_lead(
    entry: &Path,
    home: &Path,
    checkout: &Path,
    session: Option<&str>,
    prompt: Option<&str>,
    evidence: &Path,
) -> ConsoleSession {
    let mut console = ConsoleSpec::new(lead_spec(entry, home, checkout, session, prompt, evidence));
    console.limits.memory_bytes = Some(1024 * 1024 * 1024);
    console.limits.cpu_percent = Some(50.0);
    ConsoleSession::spawn(console).unwrap()
}

fn send_line(session: &ConsoleSession, text: &str) -> std::io::Result<()> {
    session.send(text)?;
    std::thread::sleep(Duration::from_millis(250));
    session.send("\r")
}

fn wait_exit(session: ConsoleSession) -> ConsoleOutcome {
    session
        .wait(
            Deadline::after(EXIT_WAIT).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap()
}

/// Ends one managed lead frontend. A resumed native TUI can still be finishing
/// its startup when the first Enter arrives, so the quit is only sent after the
/// prompt is visible and a bare Enter is repeated while the host is alive.
fn quit_session(session: ConsoleSession, entry: &Path) -> ConsoleOutcome {
    let _ = wait_until(Duration::from_secs(15), || {
        session.transcript().contains("Ask Codex to do anything")
    });
    send_line(&session, "/quit").unwrap();
    std::thread::sleep(Duration::from_secs(2));
    if process_is_live(session.identity(), entry) {
        let _ = session.send("\r");
    }
    wait_exit(session)
}

fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    predicate()
}

/// The exact endpoint record of one live managed host. The registry must hold
/// one record for this thread, never two addresses for the same conversation.
fn wait_for_endpoint(registry: &Path, timeout: Duration) -> (PathBuf, Value) {
    let mut found = None;
    let discovered = wait_until(timeout, || {
        let Ok(entries) = fs::read_dir(registry) else {
            return false;
        };
        let mut records: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        if records.len() != 1 {
            return false;
        }
        let path = records.pop().unwrap();
        let Ok(bytes) = fs::read(&path) else {
            return false;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            return false;
        };
        if value["port"].as_u64().is_none() || value["threadId"].as_str().is_none() {
            return false;
        }
        found = Some((path, value));
        true
    });
    assert!(
        discovered,
        "the managed lead host published no single endpoint within {timeout:?}"
    );
    found.unwrap()
}

/// The published endpoint records, ignoring transient lock or staging files the
/// registry keeps beside them.
fn endpoint_records(registry: &Path) -> Vec<PathBuf> {
    fs::read_dir(registry)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "json")
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Waits for the one endpoint record whose process is not the retired owner.
/// A deliberately restored stale record must never be mistaken for a live
/// successor that merely reloaded the same address.
fn wait_for_replacement_endpoint(
    registry: &Path,
    retired_pid: u32,
    timeout: Duration,
) -> (PathBuf, Value) {
    let mut found = None;
    let discovered = wait_until(timeout, || {
        let records = endpoint_records(registry);
        if records.len() != 1 {
            return false;
        }
        let path = &records[0];
        let Ok(bytes) = fs::read(path) else {
            return false;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            return false;
        };
        let pid = value["process"]["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok());
        if pid == Some(retired_pid)
            || value["port"].as_u64().is_none()
            || value["threadId"].as_str().is_none()
        {
            return false;
        }
        found = Some((path.clone(), value));
        true
    });
    assert!(
        discovered,
        "no replacement endpoint owned by a different process appeared within {timeout:?}"
    );
    found.unwrap()
}

fn endpoint_process(endpoint: &Value) -> (ProcessIdentity, PathBuf) {
    let identity = ProcessIdentity {
        pid: u32::try_from(endpoint["process"]["pid"].as_u64().unwrap()).unwrap(),
        creation_time: endpoint["process"]["creationTime"].as_u64().unwrap(),
    };
    (
        identity,
        PathBuf::from(endpoint["process"]["program"].as_str().unwrap()),
    )
}

fn process_is_live(identity: ProcessIdentity, program: &Path) -> bool {
    let user = process_service::current_user().unwrap();
    ServiceProcess::inspect(identity, program, &user)
        .map(|process| process.is_some())
        .unwrap_or(false)
}

fn wait_process_gone(identity: ProcessIdentity, program: &Path, what: &str) {
    let gone = wait_until(Duration::from_secs(30), || {
        !process_is_live(identity, program)
    });
    assert!(gone, "{what} (pid {}) is still running", identity.pid);
}

/// The first native probe of the supported successor path: one live managed
/// host, a refused overlapping owner, the old host stopped with its endpoint
/// retained as a stale record, and `--session THREAD` resuming that exact
/// conversation in a different verified app-server process.
#[test]
#[ignore = "requires the installed native upstream and the checkout-built launcher; owned canned Responses; no live model"]
fn native_lead_start_replaces_the_same_thread_without_a_second_owner() {
    let evidence = tempfile::Builder::new()
        .prefix("lead-recovery-probe-")
        .tempdir()
        .unwrap()
        .keep();
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let home = evidence.join("home");
    fs::create_dir_all(home.join("harness")).unwrap();
    let upstream = installed_upstream();
    let build = launcher_build(&home, &checkout);
    write_registration(&home, &build, &upstream);
    let responses = control_responses::Responses::routed(evidence.join("provider"), |request| {
        if title_generation_request(request) {
            return message("Lead recovery fixture");
        }
        message(FIRST_REPLY)
    });
    write_config(&home, &checkout, responses.port, false);
    let entry = manager();
    let registry = home.join("harness/lead-endpoints");
    fs::create_dir_all(&registry).unwrap();
    println!("lead recovery probe evidence: {}", evidence.display());

    // The old owner: a real managed lead host creates the conversation and
    // publishes its verified app-server endpoint after the frontend attaches.
    let old = spawn_lead(
        &entry,
        &home,
        &checkout,
        None,
        Some(FIRST_PROMPT),
        &evidence,
    );
    let (endpoint_path, endpoint) = wait_for_endpoint(&registry, WAIT);
    let old_thread = endpoint["threadId"].as_str().unwrap().to_owned();
    let old_port = u16::try_from(endpoint["port"].as_u64().unwrap()).unwrap();
    let old_token = endpoint["token"].as_str().unwrap().to_owned();
    let (old_identity, old_program) = endpoint_process(&endpoint);
    assert!(
        same_program(&old_program, &build.join("codex.exe")),
        "the published endpoint must record the registered launcher"
    );
    assert!(
        process_is_live(old_identity, &old_program),
        "the old host's app-server must be live while it owns the thread"
    );
    let saved_endpoint = fs::read(&endpoint_path).unwrap();

    // A second owner of the same thread is refused before it starts an
    // app-server, a frontend or a journal consumer, and it leaves the single
    // published address untouched.
    let mut refused_command = Command::new(&entry);
    refused_command
        .args([
            "lead",
            "start",
            "--source",
            checkout.to_str().unwrap(),
            "--codex-home",
            home.to_str().unwrap(),
            "--session",
            &old_thread,
        ])
        .current_dir(&checkout)
        .env("CODEX_HOME", &home)
        .env(PROVIDER_KEY_ENV, PROVIDER_KEY);
    for key in [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "CODEX_SESSION_ID",
        "CODEX_THREAD_ID",
        "HARNESS_EXECUTOR_SESSION",
        "HARNESS_EXECUTOR_RUN",
        "HARNESS_CONTEXT_ROLE",
        "HARNESS_CONTEXT_REVISION",
        "HARNESS_ORIGINATING_LEAD",
        "HARNESS_LEAD_THREAD",
        "HARNESS_LEAD_RECIPIENT",
        "WT_SESSION",
    ] {
        refused_command.env_remove(key);
    }
    let refused = refused_command.output().unwrap();
    let refused_text = format!(
        "{}{}",
        String::from_utf8_lossy(&refused.stdout),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(
        !refused.status.success(),
        "a second live owner must be refused: {refused_text}"
    );
    assert!(
        refused_text.contains("is already served by a live managed lead session"),
        "the refusal must name the live owner: {refused_text}"
    );
    assert_eq!(
        fs::read(&endpoint_path).unwrap(),
        saved_endpoint,
        "a refused second owner must not replace the published address"
    );
    assert_eq!(
        endpoint_records(&registry).len(),
        1,
        "a refused second owner must not publish another endpoint"
    );

    // Ending the frontend ends its host: the app-server is gone and the
    // address is retired. The saved record is then restored deliberately as a
    // stale address of the dead process; the successor must retire it.
    let old_outcome = quit_session(old, &entry);
    assert_eq!(
        old_outcome.outcome.reason,
        StopReason::Exited,
        "{}",
        old_outcome.transcript
    );
    assert_eq!(
        old_outcome.outcome.exit_code, 0,
        "{}",
        old_outcome.transcript
    );
    assert!(
        old_outcome
            .transcript
            .contains(&format!("lead session: ended thread {old_thread}")),
        "{}",
        old_outcome.transcript
    );
    assert!(
        !endpoint_path.exists(),
        "the ended host must retire its endpoint record"
    );
    wait_process_gone(old_identity, &old_program, "the old app-server");
    assert!(
        harness_core::task_control::ControlConnection::connect(
            old_port,
            &old_token,
            Duration::from_millis(500)
        )
        .is_err(),
        "the retired address must refuse a new connection"
    );
    fs::write(&endpoint_path, &saved_endpoint).unwrap();

    // The supported successor: the same thread, a different verified process,
    // and one published address for that conversation.
    let successor = spawn_lead(&entry, &home, &checkout, Some(&old_thread), None, &evidence);
    let (successor_path, successor_endpoint) =
        wait_for_replacement_endpoint(&registry, old_identity.pid, WAIT);
    assert_eq!(
        successor_path, endpoint_path,
        "the successor must publish the one registry key of this thread"
    );
    assert_eq!(
        successor_endpoint["threadId"].as_str(),
        Some(old_thread.as_str()),
        "the successor must resume the exact same thread"
    );
    let (successor_identity, successor_program) = endpoint_process(&successor_endpoint);
    assert!(
        same_program(&successor_program, &build.join("codex.exe")),
        "the successor endpoint must record the registered launcher"
    );
    assert_ne!(
        successor_identity.pid, old_identity.pid,
        "the successor must be a different app-server process"
    );
    assert!(
        process_is_live(successor_identity, &successor_program),
        "the successor endpoint must record its live app-server"
    );
    assert_eq!(
        endpoint_records(&registry).len(),
        1,
        "the stale record must be replaced by exactly one successor address"
    );
    assert!(
        successor
            .transcript()
            .contains("lead session: profile=default"),
        "the successor must report the resumed managed session: {}",
        successor.transcript()
    );

    let successor_outcome = quit_session(successor, &entry);
    assert_eq!(
        successor_outcome.outcome.reason,
        StopReason::Exited,
        "{}",
        successor_outcome.transcript
    );
    assert_eq!(
        successor_outcome.outcome.exit_code, 0,
        "{}",
        successor_outcome.transcript
    );
    assert!(
        successor_outcome
            .transcript
            .contains(&format!("lead session: ended thread {old_thread}")),
        "{}",
        successor_outcome.transcript
    );
    assert!(
        !endpoint_path.exists(),
        "the successor must retire its published address"
    );
    wait_process_gone(
        successor_identity,
        &successor_program,
        "the successor app-server",
    );
    assert!(
        endpoint_records(&registry).is_empty(),
        "no endpoint record may outlive either host"
    );
    eprintln!(
        "lead recovery probe: thread {old_thread} old pid {} successor pid {}; evidence {}",
        old_identity.pid,
        successor_identity.pid,
        evidence.display()
    );
}

fn native_output(program: &str, args: &[&str], cwd: &Path) -> std::process::Output {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env("BD_NON_INTERACTIVE", "1")
        .env("BEADS_ACTOR", "lead-recovery-fixture")
        .output()
        .unwrap_or_else(|error| panic!("{program} {args:?}: {error}"));
    assert!(
        output.status.success(),
        "{program} {args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// One initialized native app-server control connection. The upstream refuses
/// every request before the `initialize`/`initialized` handshake, so listing
/// turns must not open a raw `ControlConnection`.
struct NativeClient {
    connection: ControlConnection,
    id: u64,
}

impl NativeClient {
    fn connect(port: u16, token: &str) -> Self {
        let mut client = Self {
            connection: ControlConnection::connect(port, token, Duration::from_secs(10)).unwrap(),
            id: 0,
        };
        client.request(
            "initialize",
            json!({
                "clientInfo": {"name": "lead-recovery-native", "version": "1"},
                "capabilities": {"experimentalApi": true},
            }),
        );
        client
            .connection
            .send(&json!({"method": "initialized"}), Duration::from_secs(10))
            .unwrap();
        client
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.connection
            .send(
                &json!({"id": id, "method": method, "params": params}),
                Duration::from_secs(10),
            )
            .unwrap();
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(Instant::now() < until, "no response to {method}");
            let Some(value) = self.connection.receive(Duration::from_millis(500)).unwrap() else {
                continue;
            };
            if value["id"] == id {
                assert!(value.get("error").is_none(), "{method}: {value}");
                return value["result"].clone();
            }
        }
    }
}

/// The exact completed turn the acceptance binding can name. The installed
/// app-server does not implement `thread/read includeTurns`, so this uses the
/// same paginated turn listing as the existing native lead checks.
fn completed_turn(port: u16, token: &str, thread: &str) -> String {
    let mut connection = NativeClient::connect(port, token);
    let until = Instant::now() + WAIT;
    loop {
        let mut turns = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..16 {
            let mut params = json!({
                "threadId": thread,
                "itemsView": "full",
                "sortDirection": "asc",
                "limit": 100,
            });
            if let Some(cursor) = &cursor {
                params["cursor"] = json!(cursor);
            }
            let page = connection.request("thread/turns/list", params);
            turns.extend(page["data"].as_array().cloned().unwrap_or_default());
            cursor = page["nextCursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        if let Some(turn) = turns
            .iter()
            .find(|turn| turn["status"] == "completed" && turn["id"].is_string())
        {
            return turn["id"].as_str().unwrap().to_owned();
        }
        assert!(
            Instant::now() < until,
            "thread {thread} has no completed turn: {turns:?}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Closes a real task through the native board owner and lets the production
/// controller derive the accepted record. The source identities are the live
/// managed thread's exact completed turn; nothing is relabelled by hand.
fn accepted_board(
    home: &Path,
    checkout: &Path,
    executor_thread: &str,
    executor_turn: &str,
    lead_thread: &str,
    lead_turn: &str,
) -> (PathBuf, String, String) {
    fs::create_dir_all(checkout).unwrap();
    native_output("git", &["init", "-q", "--initial-branch=main"], checkout);
    native_output(
        "git",
        &["config", "user.email", "fixture@example.test"],
        checkout,
    );
    native_output("git", &["config", "user.name", "Fixture"], checkout);
    fs::write(checkout.join("README.md"), "synthetic accepted task\n").unwrap();
    native_output("git", &["add", "README.md"], checkout);
    native_output("git", &["commit", "-qm", "seed"], checkout);
    native_output(
        "bd.exe",
        &[
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "rfl",
        ],
        checkout,
    );
    native_output(
        "bd.exe",
        &["config", "set", "events-journal", "true"],
        checkout,
    );
    let created = native_output(
        "bd.exe",
        &[
            "create",
            "Accepted ordinary task",
            "--type",
            "task",
            "--json",
        ],
        checkout,
    );
    let task: Value = serde_json::from_slice(&created.stdout).unwrap();
    let task_id = task["id"].as_str().unwrap().to_owned();
    let controller = ReflectionController::new("bd.exe", checkout, home);
    controller
        .activate(&ActivationRequest {
            lead_session: lead_thread.to_owned(),
            run: None,
            boundary: None,
            journal: JournalPolicy::RequireExisting,
            limits: None,
        })
        .unwrap();
    let binding = AcceptanceBinding::new(
        &task_id,
        "replacement-run",
        "replacement-assignment",
        "replacement-result",
        "replacement-revision",
        vec![executor_thread.to_owned()],
        lead_thread,
        "lead-recovery-fixture",
        Purpose::Ordinary,
    )
    .with_turns(
        Some(executor_thread.to_owned()),
        Some(executor_turn.to_owned()),
        Some(lead_turn.to_owned()),
    );
    board_cli::close_accepted(
        Path::new("bd.exe"),
        checkout,
        "lead-recovery-fixture",
        &task_id,
        &acceptance_metadata(&binding).unwrap(),
    )
    .unwrap();
    let state_path = controller.state_file().unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        if let Ok(bytes) = fs::read(&state_path)
            && let Ok(state) = serde_json::from_slice::<ReflectionState>(&bytes)
            && state.records.len() == 1
        {
            let acceptance = state.records.keys().next().unwrap().clone();
            return (state_path, task_id, acceptance);
        }
        assert!(
            Instant::now() < deadline,
            "the managed host did not derive an accepted record at {}",
            state_path.display()
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Every `.jsonl` rollout under the isolated home. A headless native
/// `codex exec` turn is the real executor source; its thread and turn are
/// read from the new rollout rather than relabelled by hand.
fn rollout_files(root: &Path) -> std::collections::BTreeSet<PathBuf> {
    let mut files = std::collections::BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                files.insert(path);
            }
        }
    }
    files
}

/// Starts one ordinary executor conversation through the installed native
/// `codex exec` path against the canned provider and returns its exact thread
/// and completed turn from the persisted rollout.
fn native_executor_source(home: &Path, build: &Path, evidence: &Path) -> (String, String) {
    let sessions = home.join("sessions");
    let before = rollout_files(&sessions);
    let cwd = evidence.join("executor-source");
    fs::create_dir_all(&cwd).unwrap();
    let output = Command::new(build.join("codex.exe"))
        .args(["exec", "-p", "ds", "--json", "--skip-git-repo-check", "-C"])
        .arg(&cwd)
        .arg(EXECUTOR_SOURCE_PROMPT)
        .current_dir(&cwd)
        .env("CODEX_HOME", home)
        .env(PROVIDER_KEY_ENV, PROVIDER_KEY)
        .env("PATH", filtered_path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "codex exec executor source: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let created: Vec<PathBuf> = rollout_files(&sessions)
        .difference(&before)
        .cloned()
        .collect();
    assert_eq!(
        created.len(),
        1,
        "the executor source must create one native rollout: {created:?}"
    );
    let rollout = fs::read_to_string(&created[0]).unwrap();
    let mut thread = None;
    let mut turn = None;
    for line in rollout.lines() {
        let value: Value = serde_json::from_str(line).unwrap();
        match value["type"].as_str() {
            Some("session_meta") => {
                thread = value["payload"]["session_id"].as_str().map(str::to_owned);
            }
            Some("turn_context") => {
                turn = value["payload"]["turn_id"].as_str().map(str::to_owned);
            }
            _ => {}
        }
    }
    (
        thread.expect("the executor rollout records its thread"),
        turn.expect("the executor rollout records its completed turn"),
    )
}

/// Counts provider requests that carry a reflection stage prompt. The host's
/// refusal must happen before any model work, so this stays zero.
fn reflection_requests(evidence: &Path) -> usize {
    let mut found = 0;
    if let Ok(entries) = fs::read_dir(evidence.join("provider")) {
        for entry in entries.flatten() {
            if let Ok(body) = fs::read_to_string(entry.path())
                && (body.contains("executor self-reflection")
                    || body.contains("lead self-reflection"))
            {
                found += 1;
            }
        }
    }
    found
}

/// The managed host's own lifecycle status plus its consumer log, selected by
/// the exact lead thread rather than by an inferred file name.
fn host_status(home: &Path, lead: &str) -> Option<(PathBuf, Value, String)> {
    let directory = home.join("harness/delivery-reflection/hosts");
    for entry in fs::read_dir(directory).ok()?.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if value["lead"].as_str() != Some(lead) {
            continue;
        }
        let log = value["log"]
            .as_str()
            .map(|path| fs::read_to_string(path).unwrap_or_default())
            .unwrap_or_default();
        return Some((path, value, log));
    }
    None
}

/// Establishes where the real managed host's model-free consumer stops when a
/// real accepted closure is bound to its live thread and completed turn. The
/// host is left running until its outcome is captured, then ended normally.
#[test]
#[ignore = "requires the installed native upstream and the checkout-built launcher; owned canned Responses; no live model"]
fn native_lead_host_attempts_the_pending_executor_stage() {
    let evidence = tempfile::Builder::new()
        .prefix("lead-recovery-pending-")
        .tempdir()
        .unwrap()
        .keep();
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let home = evidence.join("home");
    fs::create_dir_all(home.join("harness")).unwrap();
    let upstream = installed_upstream();
    let build = launcher_build(&home, &checkout);
    write_registration(&home, &build, &upstream);
    let responses = control_responses::Responses::routed(evidence.join("provider"), |request| {
        if title_generation_request(request) {
            return message("Lead recovery fixture");
        }
        let prompt = request["input"].to_string();
        if prompt.contains("LEAD-RECOVERY-OWNED-EXECUTOR-SOURCE") {
            return message(EXECUTOR_SOURCE_REPLY);
        }
        if prompt.contains("executor self-reflection") {
            return message(EXECUTOR_RESULT_TEXT);
        }
        if prompt.contains("lead self-reflection") {
            return message(LEAD_RESULT_TEXT);
        }
        message(FIRST_REPLY)
    });
    // The reflection runner queries the source goal through `thread/goal/get`
    // before it forks. Enable that upstream feature in this isolated home so
    // the pending-stage probe reaches the controller's actual frontend
    // boundary; no model request is admitted by this feature.
    write_config(&home, &checkout, responses.port, true);
    let entry = manager();
    let registry = home.join("harness/lead-endpoints");
    fs::create_dir_all(&registry).unwrap();
    println!("pending reflection evidence: {}", evidence.display());

    // The accepted source is a real, independent headless executor turn in
    // this same native home, not a relabelled lead conversation.
    let (executor_thread, executor_turn) = native_executor_source(&home, &build, &evidence);

    let host = spawn_lead(
        &entry,
        &home,
        &checkout,
        None,
        Some(FIRST_PROMPT),
        &evidence,
    );
    let (endpoint_path, endpoint) = wait_for_endpoint(&registry, WAIT);
    let thread = endpoint["threadId"].as_str().unwrap().to_owned();
    let port = u16::try_from(endpoint["port"].as_u64().unwrap()).unwrap();
    let token = endpoint["token"].as_str().unwrap().to_owned();
    let (old_identity, old_program) = endpoint_process(&endpoint);
    assert!(process_is_live(old_identity, &old_program));
    let saved_endpoint = fs::read(&endpoint_path).unwrap();
    let first_turn = completed_turn(port, &token, &thread);
    let board = evidence.join("board");
    let (state_path, task, acceptance) = accepted_board(
        &home,
        &board,
        &executor_thread,
        &executor_turn,
        &thread,
        &first_turn,
    );
    eprintln!(
        "accepted board: task {task} acceptance {acceptance} executor {executor_thread}/{executor_turn} lead {thread}/{first_turn}"
    );

    let deadline = Instant::now() + WAIT;
    let mut observed_status = Value::Null;
    let mut observed_log = String::new();
    let mut observed = RecordedStageState::Pending;
    loop {
        if let Ok(bytes) = fs::read(&state_path)
            && let Ok(state) = serde_json::from_slice::<ReflectionState>(&bytes)
            && let Some(record) = state.records.get(&acceptance)
        {
            observed = record.executor.state;
        }
        if let Some((_, status, log)) = host_status(&home, &thread) {
            observed_status = status;
            observed_log = log;
        }
        if matches!(
            observed,
            RecordedStageState::Retained
                | RecordedStageState::Blocked
                | RecordedStageState::Unknown
        ) || observed_status["state"] == "failed"
            || observed_log.contains("no live terminal surface")
        {
            break;
        }
        if Instant::now() >= deadline {
            panic!(
                "the managed host did not attempt the pending executor stage; status={observed_status}\nlog:\n{observed_log}"
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    eprintln!(
        "host status: {observed_status}\nexecutor state: {observed:?}\nhost log:\n{observed_log}"
    );
    let state: ReflectionState = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    let record = state.records.get(&acceptance).unwrap();
    assert_eq!(
        record.executor.state,
        RecordedStageState::Unknown,
        "{record:?}"
    );
    assert_eq!(record.executor.usage.attempts, 1, "{record:?}");
    assert!(record.executor.outcome.is_none(), "{record:?}");
    assert_eq!(record.lead.state, RecordedStageState::Pending, "{record:?}");
    assert_eq!(
        record.delivery,
        RecordedDeliveryState::Pending,
        "{record:?}"
    );
    assert_eq!(
        record.binding.executor_session.as_deref(),
        Some(executor_thread.as_str())
    );
    assert_eq!(
        record.binding.executor_turn.as_deref(),
        Some(executor_turn.as_str())
    );
    assert_eq!(record.binding.lead_session, thread);
    assert_eq!(
        record.binding.lead_turn.as_deref(),
        Some(first_turn.as_str())
    );
    assert_eq!(record.binding.task, task);
    assert_eq!(record.acceptance_id, acceptance);
    assert_eq!(
        reflection_requests(&evidence),
        0,
        "the terminal-surface refusal must happen before any reflection model request"
    );
    let issue = board_cli::show_issue(Path::new("bd.exe"), &board, &task).unwrap();
    assert_eq!(issue["status"], "closed", "{issue}");
    assert_eq!(issue["close_reason"], ACCEPTED_CLOSE_REASON, "{issue}");
    assert!(
        observed_log.contains("no live terminal surface")
            && observed_log.contains("run it in its own terminal instead of a redirected pipe"),
        "the managed host consumer did not retain the terminal-surface refusal at attach_owned_frontend; executor={observed:?} status={observed_status}\nlog:\n{observed_log}"
    );

    // The supported successor takes the same thread after the old controller
    // has retired. The retained executor is Unknown, so the new consumer must
    // stay idle without starting a second stage or delivery and leave the
    // accepted task intact.
    let old_controller = host_status(&home, &thread)
        .map(|(_, status, _)| status["process"].clone())
        .expect("the old host recorded its model-free controller");
    let old_outcome = quit_session(host, &entry);
    assert_eq!(
        old_outcome.outcome.reason,
        StopReason::Exited,
        "{}",
        old_outcome.transcript
    );
    assert_eq!(
        old_outcome.outcome.exit_code, 0,
        "{}",
        old_outcome.transcript
    );
    assert!(
        !endpoint_path.exists(),
        "the ended host must retire its endpoint record"
    );
    wait_process_gone(old_identity, &old_program, "the old app-server");
    fs::write(&endpoint_path, &saved_endpoint).unwrap();
    let log_path = host_status(&home, &thread)
        .map(|(path, _, _)| path.with_extension("log"))
        .expect("the old host retained a status log");
    let before_successor = fs::read(&log_path).unwrap_or_default().len();

    let successor = spawn_lead(&entry, &home, &checkout, Some(&thread), None, &evidence);
    let (successor_path, successor_endpoint) =
        wait_for_replacement_endpoint(&registry, old_identity.pid, WAIT);
    assert_eq!(
        successor_path, endpoint_path,
        "the successor must publish the one registry key of this thread"
    );
    assert_eq!(
        successor_endpoint["threadId"].as_str(),
        Some(thread.as_str()),
        "the successor must resume the exact same thread"
    );
    let (successor_identity, successor_program) = endpoint_process(&successor_endpoint);
    assert_ne!(successor_identity.pid, old_identity.pid);
    assert!(same_program(&successor_program, &build.join("codex.exe")));
    assert!(process_is_live(successor_identity, &successor_program));
    assert_eq!(endpoint_records(&registry).len(), 1);

    let deadline = Instant::now() + WAIT;
    let successor_status = loop {
        if let Some((_, status, _)) = host_status(&home, &thread)
            && status["state"] == "running"
            && status["process"]["pid"] != old_controller["pid"]
        {
            let identity = ProcessIdentity {
                pid: u32::try_from(status["process"]["pid"].as_u64().unwrap()).unwrap(),
                creation_time: status["process"]["creationTime"].as_u64().unwrap(),
            };
            let program = PathBuf::from(status["process"]["program"].as_str().unwrap());
            assert!(
                process_is_live(identity, &program),
                "the successor model-free controller is not live: {status}"
            );
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "the successor did not start its own model-free controller; log growth: {}",
            fs::read(&log_path)
                .unwrap_or_default()
                .len()
                .saturating_sub(before_successor)
        );
        std::thread::sleep(Duration::from_millis(250));
    };
    // A full host poll must not replay the retained Uncertain stage.
    std::thread::sleep(Duration::from_secs(5));
    if let Some((_, status, log)) = host_status(&home, &thread) {
        assert_eq!(
            status["process"]["pid"], successor_status["process"]["pid"],
            "the successor controller changed while the retained stage was idle: {status}"
        );
        assert_eq!(
            log.len(),
            before_successor,
            "the successor replayed or advanced the retained Uncertain stage; log:\n{}",
            log.get(before_successor..).unwrap_or_default()
        );
    } else {
        panic!("the successor model-free controller status disappeared");
    }
    let state: ReflectionState = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    let record = state.records.get(&acceptance).unwrap();
    assert_eq!(
        record.executor.state,
        RecordedStageState::Unknown,
        "{record:?}"
    );
    assert_eq!(record.executor.usage.attempts, 1, "{record:?}");
    assert_eq!(record.lead.state, RecordedStageState::Pending, "{record:?}");
    assert_eq!(
        record.delivery,
        RecordedDeliveryState::Pending,
        "{record:?}"
    );
    assert_eq!(
        reflection_requests(&evidence),
        0,
        "succession must not rerun the retained executor stage"
    );
    let issue = board_cli::show_issue(Path::new("bd.exe"), &board, &task).unwrap();
    assert_eq!(issue["status"], "closed", "{issue}");
    assert_eq!(issue["close_reason"], ACCEPTED_CLOSE_REASON, "{issue}");

    let successor_outcome = quit_session(successor, &entry);
    assert_eq!(
        successor_outcome.outcome.reason,
        StopReason::Exited,
        "{}",
        successor_outcome.transcript
    );
    assert_eq!(
        successor_outcome.outcome.exit_code, 0,
        "{}",
        successor_outcome.transcript
    );
    assert!(!endpoint_path.exists());
    wait_process_gone(
        successor_identity,
        &successor_program,
        "the successor app-server",
    );
    assert!(endpoint_records(&registry).is_empty());
    eprintln!(
        "pending reflection probe: task {task} acceptance {acceptance}; old pid {} successor pid {}; evidence {}",
        old_identity.pid,
        successor_identity.pid,
        evidence.display()
    );
}
