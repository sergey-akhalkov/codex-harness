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
    os::windows::fs::OpenOptionsExt,
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
const EXECUTOR_PROMPT_MARKER: &str = "Bounded read-only executor self-reflection";
const LEAD_PROMPT_MARKER: &str = "Bounded read-only lead self-reflection";
const TITLE_PROMPT: &str = "Generate a concise, single-line task title";
const WAIT: Duration = Duration::from_secs(120);
const COMPLETE_WAIT: Duration = Duration::from_secs(300);
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

/// Counts provider requests that carry one exact role marker. The two stage
/// prompts use distinct bounded lines, so the lead request's embedded executor
/// result cannot be mistaken for a second executor request.
fn reflection_request_count(evidence: &Path, marker: &str) -> usize {
    let mut found = 0;
    if let Ok(entries) = fs::read_dir(evidence.join("provider")) {
        for entry in entries.flatten() {
            if let Ok(body) = fs::read_to_string(entry.path())
                && body.contains(marker)
            {
                found += 1;
            }
        }
    }
    found
}

/// Counts the lead conversation's own user-message items carrying one exact
/// client message id. A replayed delivery would add a second item.
fn delivered_user_message_items(
    port: u16,
    token: &str,
    thread: &str,
    client_message_id: &str,
) -> usize {
    let mut connection = NativeClient::connect(port, token);
    let mut cursor: Option<String> = None;
    let mut found = 0;
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
        for turn in page["data"].as_array().cloned().unwrap_or_default() {
            for item in turn["items"].as_array().cloned().unwrap_or_default() {
                if item["type"] == "userMessage"
                    && item["clientId"].as_str() == Some(client_message_id)
                {
                    found += 1;
                }
            }
        }
        cursor = page["nextCursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
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

/// The managed lead host's existing model-free `_serve` consumer runs the two
/// pending reflection stages in their own visibly titled foreground surfaces,
/// retains the real executor and lead results, and delivers the synthesis
/// exactly once. The ordinary source home config uses `features.goals = false`;
/// the reflection app-server's process-scope admin override still supports the
/// source goal reads and clears the production fork preflight needs, while the
/// fork keeps its own production `features.goals = false`.
#[test]
#[ignore = "requires the installed native upstream and the checkout-built launcher; owned canned Responses; no live model"]
fn native_managed_host_runs_reflections_in_distinct_surfaces_and_delivers_once() {
    let evidence = tempfile::Builder::new()
        .prefix("lead-recovery-managed-host-")
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
            return message("Managed host fixture");
        }
        let prompt = request["input"].to_string();
        // The lead prompt embeds the retained executor result, so role
        // routing keys on each stage's own bounded prompt line.
        if prompt.contains(LEAD_PROMPT_MARKER) {
            if !prompt.contains(EXECUTOR_RESULT_TEXT) {
                return message("LEAD-RECOVERY-MISSING-EXECUTOR-RESULT");
            }
            return message(LEAD_RESULT_TEXT);
        }
        if prompt.contains(EXECUTOR_PROMPT_MARKER) {
            return message(EXECUTOR_RESULT_TEXT);
        }
        if prompt.contains("LEAD-RECOVERY-OWNED-EXECUTOR-SOURCE") {
            return message(EXECUTOR_SOURCE_REPLY);
        }
        message(FIRST_REPLY)
    });
    // The ordinary source home keeps `features.goals = false`. The production
    // goal fix serves the fork preflight's source goal reads and clears through
    // the reflection app-server's process-scope admin override, while the fork
    // itself keeps the production `features.goals = false` suppression.
    write_config(&home, &checkout, responses.port, false);
    let entry = manager();
    let registry = home.join("harness/lead-endpoints");
    fs::create_dir_all(&registry).unwrap();
    println!("managed reflection evidence: {}", evidence.display());

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
    let (host_identity, host_program) = endpoint_process(&endpoint);
    assert!(process_is_live(host_identity, &host_program));
    assert_eq!(endpoint_records(&registry).len(), 1);
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
    let binding_deadline = Instant::now() + WAIT;
    let accepted_binding = loop {
        if let Some(record) = fs::read(&state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ReflectionState>(&bytes).ok())
            .and_then(|state| state.records.get(&acceptance).cloned())
        {
            break record.binding;
        }
        assert!(
            Instant::now() < binding_deadline,
            "the accepted record's binding disappeared after the board closure"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    eprintln!(
        "accepted board: task {task} acceptance {acceptance} executor {executor_thread}/{executor_turn} lead {thread}/{first_turn}"
    );

    // Wait for both reflections and the single delivery while keeping the
    // model-free host log as failure evidence.
    let deadline = Instant::now() + COMPLETE_WAIT;
    let mut observed_status = Value::Null;
    let mut observed_log = String::new();
    let record = loop {
        let state = fs::read(&state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ReflectionState>(&bytes).ok())
            .and_then(|state| state.records.get(&acceptance).cloned());
        if let Some((_, status, log)) = host_status(&home, &thread) {
            if status["state"] == "failed" {
                panic!("the managed host failed before delivery: {status}\nlog:\n{log}");
            }
            observed_status = status;
            observed_log = log;
        }
        let complete = state.as_ref().is_some_and(|record| {
            record.executor.state == RecordedStageState::Retained
                && record.lead.state == RecordedStageState::Retained
                && record.delivery == RecordedDeliveryState::Consumed
        });
        if complete {
            break state.expect("the completed acceptance retains its record");
        }
        assert!(
            Instant::now() < deadline,
            "the managed host did not complete both reflections and delivery; record={state:?} status={observed_status}\nlog:\n{observed_log}"
        );
        std::thread::sleep(Duration::from_millis(250));
    };
    eprintln!("managed host status: {observed_status}\nhost log:\n{observed_log}");

    assert_eq!(
        record.executor.state,
        RecordedStageState::Retained,
        "{record:?}"
    );
    assert_eq!(record.executor.usage.attempts, 1, "{record:?}");
    assert_eq!(
        record
            .executor
            .outcome
            .as_ref()
            .map(|outcome| outcome.summary.as_str()),
        Some(EXECUTOR_RESULT_TEXT),
        "{record:?}"
    );
    assert_eq!(
        record.lead.state,
        RecordedStageState::Retained,
        "{record:?}"
    );
    assert_eq!(record.lead.usage.attempts, 1, "{record:?}");
    assert_eq!(
        record
            .lead
            .outcome
            .as_ref()
            .map(|outcome| outcome.summary.as_str()),
        Some(LEAD_RESULT_TEXT),
        "{record:?}"
    );
    assert_eq!(
        record.delivery,
        RecordedDeliveryState::Consumed,
        "{record:?}"
    );
    assert_eq!(
        record.binding, accepted_binding,
        "the accepted task/acceptance binding changed during reflection"
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

    // One native provider request per role proves the lead consumed the
    // retained executor result instead of starting another executor stage.
    assert_eq!(
        reflection_request_count(&evidence, EXECUTOR_PROMPT_MARKER),
        1,
        "the executor reflection must run exactly once"
    );
    assert_eq!(
        reflection_request_count(&evidence, LEAD_PROMPT_MARKER),
        1,
        "the lead reflection must run exactly once"
    );

    let chain_dir = state_path.with_extension("").join(&acceptance);
    let executor_stage = chain_dir.join("executor-stage.json");
    let lead_stage = chain_dir.join("lead-stage.json");
    let delivery_path = chain_dir.join("lead-delivery.json");
    let executor_surface_path = executor_stage.with_extension("surface.json");
    let lead_surface_path = lead_stage.with_extension("surface.json");
    for path in [
        &executor_stage,
        &lead_stage,
        &delivery_path,
        &executor_surface_path,
        &lead_surface_path,
    ] {
        assert!(
            path.is_file(),
            "the managed host retained no {}",
            path.display()
        );
    }
    for path in [
        chain_dir.join("executor-self-reflection-surface-request.json"),
        chain_dir.join("executor-self-reflection-surface-result.json"),
        chain_dir.join("lead-self-reflection-surface-request.json"),
        chain_dir.join("lead-self-reflection-surface-result.json"),
    ] {
        assert!(
            !path.exists(),
            "the transient surface artifact {} was not cleaned",
            path.display()
        );
    }
    let names: Vec<String> = fs::read_dir(&chain_dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !names.iter().any(|name| name.contains("-attempt-")),
        "a reflection stage was archived as a retry: {names:?}"
    );

    let executor_native: Value =
        serde_json::from_slice(&fs::read(&executor_stage).unwrap()).unwrap();
    let lead_native: Value = serde_json::from_slice(&fs::read(&lead_stage).unwrap()).unwrap();
    assert_eq!(executor_native["status"], "completed", "{executor_native}");
    assert_eq!(
        executor_native["result"], EXECUTOR_RESULT_TEXT,
        "{executor_native}"
    );
    assert_eq!(executor_native["stage"], "executor-self-reflection");
    assert_eq!(executor_native["task"], task);
    assert_eq!(executor_native["run"], "replacement-run");
    assert_eq!(executor_native["acceptance"], acceptance);
    assert_eq!(executor_native["sourceThread"], executor_thread);
    assert_eq!(executor_native["sourceLastTurn"], executor_turn);
    assert_eq!(executor_native["usageKnown"], true, "{executor_native}");
    assert_eq!(lead_native["status"], "completed", "{lead_native}");
    assert_eq!(lead_native["result"], LEAD_RESULT_TEXT, "{lead_native}");
    assert_eq!(lead_native["stage"], "lead-self-reflection");
    assert_eq!(lead_native["sourceThread"], thread);
    assert_eq!(lead_native["sourceLastTurn"], first_turn);
    assert_eq!(lead_native["usageKnown"], true, "{lead_native}");
    for native in [&executor_native, &lead_native] {
        assert_eq!(native["fork"]["sourcePreserved"], true, "{native}");
        assert!(
            native["fork"]["forkThread"]
                .as_str()
                .is_some_and(|fork| !fork.is_empty()),
            "{native}"
        );
    }

    // Both stages ran in distinct visibly titled consoles, not in the
    // consumer's redirected pipe, and each console belonged to the exact live
    // model-free controller.
    let executor_surface: Value =
        serde_json::from_slice(&fs::read(&executor_surface_path).unwrap()).unwrap();
    let lead_surface: Value =
        serde_json::from_slice(&fs::read(&lead_surface_path).unwrap()).unwrap();
    let (_, status, _) = host_status(&home, &thread).expect("the managed host status");
    assert_eq!(status["state"], "running", "{status}");
    let controller_pid = status["process"]["pid"].as_u64().unwrap();
    let controller_created = status["process"]["creationTime"].as_u64().unwrap();
    let controller_program = PathBuf::from(status["process"]["program"].as_str().unwrap());
    assert!(
        process_is_live(
            ProcessIdentity {
                pid: u32::try_from(controller_pid).unwrap(),
                creation_time: controller_created,
            },
            &controller_program
        ),
        "the model-free controller is not live: {status}"
    );
    for (role, surface, prefix) in [
        ("executor-self-reflection", &executor_surface, "ERefl "),
        ("lead-self-reflection", &lead_surface, "LRefl "),
    ] {
        assert_eq!(surface["schema"], 1, "{surface}");
        assert_eq!(surface["role"], role, "{surface}");
        assert_eq!(surface["consoleAttached"], true, "{surface}");
        assert_eq!(
            surface["captionBefore"], surface["nativeTitle"],
            "{surface}"
        );
        assert!(
            surface["nativeTitle"]
                .as_str()
                .is_some_and(|title| title.starts_with(prefix)),
            "{surface}"
        );
        assert_eq!(surface["coordinator"]["pid"], controller_pid, "{surface}");
        assert_eq!(
            surface["coordinator"]["creation_time"], controller_created,
            "{surface}"
        );
        assert!(
            same_program(
                Path::new(surface["coordinatorProgram"].as_str().unwrap()),
                &controller_program
            ),
            "{surface}"
        );
        assert!(
            same_program(
                Path::new(surface["program"].as_str().unwrap()),
                &controller_program
            ),
            "the foreground trampoline must be the exact controller binary: {surface}"
        );
    }
    let executor_pid = executor_surface["identity"]["pid"].as_u64().unwrap();
    let lead_pid = lead_surface["identity"]["pid"].as_u64().unwrap();
    assert_ne!(
        executor_pid, lead_pid,
        "both stages must retain their own foreground process"
    );
    assert_ne!(
        executor_surface["nativeTitle"], lead_surface["nativeTitle"],
        "the two native surfaces must carry distinct role titles"
    );
    for (what, surface) in [("executor", &executor_surface), ("lead", &lead_surface)] {
        let identity = ProcessIdentity {
            pid: u32::try_from(surface["identity"]["pid"].as_u64().unwrap()).unwrap(),
            creation_time: surface["identity"]["creation_time"].as_u64().unwrap(),
        };
        let program = PathBuf::from(surface["program"].as_str().unwrap());
        wait_process_gone(
            identity,
            &program,
            &format!("the {what} foreground surface"),
        );
    }
    assert_eq!(
        observed_log.matches("foreground surface pid").count(),
        2,
        "the model-free host did not launch exactly two foreground surfaces:\n{observed_log}"
    );
    assert!(
        observed_log.contains("executor-self-reflection: foreground surface pid")
            && observed_log.contains("lead-self-reflection: foreground surface pid"),
        "the host log names the wrong stages:\n{observed_log}"
    );
    assert!(
        !observed_log.contains("no live terminal surface"),
        "the managed host still refuses its stage:\n{observed_log}"
    );

    // One delivered synthesis: the exact client message id appears once in
    // the lead's own conversation, and the host does not replay it.
    let delivery: Value = serde_json::from_slice(&fs::read(&delivery_path).unwrap()).unwrap();
    assert_eq!(delivery["state"], "delivered", "{delivery}");
    let client_message_id = delivery["clientMessageId"].as_str().unwrap().to_owned();
    assert!(!client_message_id.is_empty());
    assert!(
        delivery["turn"]
            .as_str()
            .is_some_and(|turn| !turn.is_empty()),
        "{delivery}"
    );
    assert!(
        delivery["item"]
            .as_str()
            .is_some_and(|item| !item.is_empty()),
        "{delivery}"
    );
    assert_eq!(delivery["by"], "clientId", "{delivery}");
    assert_eq!(
        record.delivery_receipt.as_deref(),
        Some(client_message_id.as_str()),
        "{record:?}"
    );
    assert!(
        wait_until(Duration::from_secs(30), || {
            delivered_user_message_items(port, &token, &thread, &client_message_id) == 1
        }),
        "the lead conversation did not show exactly one delivered synthesis input"
    );

    // A full extra host poll must not start a second stage or delivery.
    std::thread::sleep(Duration::from_secs(6));
    let after: ReflectionState = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    let after_record = after.records.get(&acceptance).unwrap();
    assert_eq!(after_record.executor.usage.attempts, 1, "{after_record:?}");
    assert_eq!(after_record.lead.usage.attempts, 1, "{after_record:?}");
    assert_eq!(
        after_record.delivery,
        RecordedDeliveryState::Consumed,
        "{after_record:?}"
    );
    assert_eq!(
        reflection_request_count(&evidence, EXECUTOR_PROMPT_MARKER),
        1
    );
    assert_eq!(reflection_request_count(&evidence, LEAD_PROMPT_MARKER), 1);
    assert_eq!(
        delivered_user_message_items(port, &token, &thread, &client_message_id),
        1,
        "the retained lead delivery was replayed"
    );
    let issue = board_cli::show_issue(Path::new("bd.exe"), &board, &task).unwrap();
    assert_eq!(issue["status"], "closed", "{issue}");
    assert_eq!(issue["close_reason"], ACCEPTED_CLOSE_REASON, "{issue}");

    let outcome = quit_session(host, &entry);
    assert_eq!(
        outcome.outcome.reason,
        StopReason::Exited,
        "{}",
        outcome.transcript
    );
    assert_eq!(outcome.outcome.exit_code, 0, "{}", outcome.transcript);
    assert!(
        !endpoint_path.exists(),
        "the ended host must retire its endpoint record"
    );
    wait_process_gone(host_identity, &host_program, "the managed host app-server");
    assert!(endpoint_records(&registry).is_empty());
    eprintln!(
        "managed reflection probe: task {task} acceptance {acceptance}; host pid {}; executor surface pid {}; lead surface pid {}; evidence {}",
        host_identity.pid,
        executor_pid,
        lead_pid,
        evidence.display()
    );
}

/// Reads one durable stage state until it names the expected status. The
/// owning runner rewrites this file several times around a native turn.
fn wait_for_stage_status(state_path: &Path, status: &str) -> Value {
    let deadline = Instant::now() + WAIT;
    loop {
        let current = fs::read(state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        if current
            .as_ref()
            .is_some_and(|value| value["status"] == status)
        {
            return current.unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "{} did not reach status {status}: {current:?}",
            state_path.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// TurnSubmitted reconciliation attaches a frontend, so it cannot run in the
/// model-free consumer's redirected pipe. The provider response is held until
/// the exact fork and turn are retained; the state file then refuses the
/// terminal replace so the first host fails with `turn-submitted` still
/// durable. The supported same-thread successor must reconcile that exact turn
/// in a new visibly titled foreground surface, with no second fork or provider
/// turn and one delivered synthesis.
#[test]
#[ignore = "requires the installed native upstream and the checkout-built launcher; owned canned Responses; no live model"]
fn native_managed_host_reconciles_retained_submitted_turn_in_a_new_surface() {
    // Read sharing stays open so the accepted owner can still classify the
    // retained turn; write and delete sharing stay closed so the staged
    // terminal replace cannot overwrite the submitted state.
    const SHARE_READ_ONLY: u32 = 0x1;
    let evidence = tempfile::Builder::new()
        .prefix("lead-recovery-managed-retained-")
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
    let release = evidence.join("executor-release");
    let provider_release = release.clone();
    let responses =
        control_responses::Responses::routed(evidence.join("provider"), move |request| {
            if title_generation_request(request) {
                return message("Managed retained fixture");
            }
            let prompt = request["input"].to_string();
            if prompt.contains(LEAD_PROMPT_MARKER) {
                if !prompt.contains(EXECUTOR_RESULT_TEXT) {
                    return message("LEAD-RECOVERY-MISSING-EXECUTOR-RESULT");
                }
                return message(LEAD_RESULT_TEXT);
            }
            if prompt.contains(EXECUTOR_PROMPT_MARKER) {
                let until = Instant::now() + WAIT;
                while !provider_release.is_file() {
                    assert!(
                        Instant::now() < until,
                        "the test never released the held executor reflection response"
                    );
                    std::thread::sleep(Duration::from_millis(25));
                }
                return message(EXECUTOR_RESULT_TEXT);
            }
            if prompt.contains("LEAD-RECOVERY-OWNED-EXECUTOR-SOURCE") {
                return message(EXECUTOR_SOURCE_REPLY);
            }
            message(FIRST_REPLY)
        });
    // The ordinary source home keeps `features.goals = false`; the reflection
    // app-server's process-scope admin override serves the source goal reads
    // and clears while the fork keeps its own goal suppression.
    write_config(&home, &checkout, responses.port, false);
    let entry = manager();
    let registry = home.join("harness/lead-endpoints");
    fs::create_dir_all(&registry).unwrap();
    println!("managed retained recovery evidence: {}", evidence.display());

    let (executor_thread, executor_turn) = native_executor_source(&home, &build, &evidence);

    let host = spawn_lead(
        &entry,
        &home,
        &checkout,
        None,
        Some(FIRST_PROMPT),
        &evidence,
    );
    let (_, endpoint) = wait_for_endpoint(&registry, WAIT);
    let thread = endpoint["threadId"].as_str().unwrap().to_owned();
    let port = u16::try_from(endpoint["port"].as_u64().unwrap()).unwrap();
    let token = endpoint["token"].as_str().unwrap().to_owned();
    let (host_identity, host_program) = endpoint_process(&endpoint);
    assert!(process_is_live(host_identity, &host_program));
    assert_eq!(endpoint_records(&registry).len(), 1);
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
    let accepted_binding = {
        let state: ReflectionState =
            serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        state
            .records
            .get(&acceptance)
            .expect("the accepted record's binding")
            .binding
            .clone()
    };
    eprintln!(
        "accepted board: task {task} acceptance {acceptance} executor {executor_thread}/{executor_turn} lead {thread}/{first_turn}"
    );

    let chain_dir = state_path.with_extension("").join(&acceptance);
    let executor_stage = chain_dir.join("executor-stage.json");
    let lead_stage = chain_dir.join("lead-stage.json");
    let delivery_path = chain_dir.join("lead-delivery.json");
    let executor_surface_path = executor_stage.with_extension("surface.json");
    let lead_surface_path = lead_stage.with_extension("surface.json");

    let submitted = wait_for_stage_status(&executor_stage, "turn-submitted");
    assert!(
        wait_until(WAIT, || {
            reflection_request_count(&evidence, EXECUTOR_PROMPT_MARKER) == 1
        }),
        "the executor reflection never reached the held canned provider response"
    );
    let fork_thread = submitted["fork"]["forkThread"]
        .as_str()
        .expect("the submitted stage retains its fork")
        .to_owned();
    let turn_id = submitted["turnId"]
        .as_str()
        .expect("the submitted stage retains its turn")
        .to_owned();
    assert!(
        !fork_thread.is_empty() && !turn_id.is_empty(),
        "{submitted}"
    );
    let (_, running_status, _) = host_status(&home, &thread).expect("the managed host status");
    assert_eq!(running_status["state"], "running", "{running_status}");
    let controller_pid = running_status["process"]["pid"].as_u64().unwrap();
    let controller_created = running_status["process"]["creationTime"].as_u64().unwrap();
    let controller_program = PathBuf::from(running_status["process"]["program"].as_str().unwrap());
    let controller_identity = ProcessIdentity {
        pid: u32::try_from(controller_pid).unwrap(),
        creation_time: controller_created,
    };
    assert!(process_is_live(controller_identity, &controller_program));

    let guard = fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_ONLY)
        .open(&executor_stage)
        .expect("guard the submitted stage state against the terminal replace");
    fs::write(&release, "release the held executor response").unwrap();
    assert!(
        wait_until(WAIT, || {
            host_status(&home, &thread).is_some_and(|(_, status, _)| status["state"] == "failed")
        }),
        "the model-free host did not stop after the terminal stage write was refused"
    );
    let (_, failed_status, failed_log) = host_status(&home, &thread).unwrap();
    assert!(
        failed_log.contains("foreground surface"),
        "the failing first host kept no foreground evidence:\n{failed_status}\n{failed_log}"
    );
    let failed_surface: Value =
        serde_json::from_slice(&fs::read(&executor_surface_path).unwrap()).unwrap();
    assert_eq!(failed_surface["consoleAttached"], true, "{failed_surface}");
    assert_eq!(
        failed_surface["captionBefore"], failed_surface["nativeTitle"],
        "{failed_surface}"
    );
    let failed_surface_identity = (
        failed_surface["identity"]["pid"].as_u64().unwrap(),
        failed_surface["identity"]["creation_time"]
            .as_u64()
            .unwrap(),
    );

    let outcome = quit_session(host, &entry);
    assert_eq!(
        outcome.outcome.reason,
        StopReason::Exited,
        "{}",
        outcome.transcript
    );
    assert_eq!(outcome.outcome.exit_code, 0, "{}", outcome.transcript);
    wait_process_gone(
        host_identity,
        &host_program,
        "the first managed host app-server",
    );
    wait_process_gone(
        controller_identity,
        &controller_program,
        "the failed journal consumer",
    );
    drop(guard);

    let after_failure = read_json(&executor_stage);
    assert_eq!(after_failure["status"], "turn-submitted", "{after_failure}");
    assert_eq!(after_failure["fork"]["forkThread"], fork_thread);
    assert_eq!(after_failure["turnId"], turn_id);
    let in_flight: ReflectionState =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    let in_flight = in_flight
        .records
        .get(&acceptance)
        .expect("the accepted record stays retained");
    assert_eq!(
        in_flight.executor.state,
        RecordedStageState::InFlight,
        "{in_flight:?}"
    );
    assert_eq!(in_flight.executor.usage.attempts, 1, "{in_flight:?}");
    assert_eq!(in_flight.executor.usage.turns, 0, "{in_flight:?}");

    // The first host's failed receipt stays on disk under the same lead
    // thread. Only a status written after this successor started describes
    // the successor's own controller.
    let successor_started = std::time::SystemTime::now();
    let successor = spawn_lead(&entry, &home, &checkout, Some(&thread), None, &evidence);
    let (_, successor_endpoint) = wait_for_replacement_endpoint(&registry, host_identity.pid, WAIT);
    let successor_port = u16::try_from(successor_endpoint["port"].as_u64().unwrap()).unwrap();
    let successor_token = successor_endpoint["token"].as_str().unwrap().to_owned();
    let (successor_identity, successor_program) = endpoint_process(&successor_endpoint);
    assert!(process_is_live(successor_identity, &successor_program));
    assert_eq!(
        successor_endpoint["threadId"].as_str(),
        Some(thread.as_str()),
        "the successor must resume the exact same lead thread"
    );

    let deadline = Instant::now() + COMPLETE_WAIT;
    let mut observed_status = Value::Null;
    let mut observed_log = String::new();
    let record = loop {
        let state = fs::read(&state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ReflectionState>(&bytes).ok())
            .and_then(|state| state.records.get(&acceptance).cloned());
        if let Some((path, status, log)) = host_status(&home, &thread) {
            let fresh = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified >= successor_started);
            if fresh && status["state"] == "failed" {
                panic!("the successor host failed before delivery: {status}\nlog:\n{log}");
            }
            observed_status = status;
            observed_log = log;
        }
        let complete = state.as_ref().is_some_and(|record| {
            record.executor.state == RecordedStageState::Retained
                && record.lead.state == RecordedStageState::Retained
                && record.delivery == RecordedDeliveryState::Consumed
        });
        if complete {
            break state.expect("the completed acceptance retains its record");
        }
        assert!(
            Instant::now() < deadline,
            "the successor did not reconcile the retained turn and deliver; record={state:?} status={observed_status}\nlog:\n{observed_log}"
        );
        std::thread::sleep(Duration::from_millis(250));
    };
    eprintln!("successor host status: {observed_status}\nhost log:\n{observed_log}");

    assert_eq!(
        record.executor.state,
        RecordedStageState::Retained,
        "{record:?}"
    );
    assert_eq!(
        record.executor.usage.attempts, 1,
        "reconciliation began a second accepted attempt: {record:?}"
    );
    assert_eq!(
        record.executor.usage.turns, 1,
        "reconciliation did not retain exactly the one submitted turn: {record:?}"
    );
    assert!(record.executor.usage.tokens > 0, "{record:?}");
    assert_eq!(
        record
            .executor
            .outcome
            .as_ref()
            .map(|outcome| outcome.summary.as_str()),
        Some(EXECUTOR_RESULT_TEXT),
        "{record:?}"
    );
    assert_eq!(
        record.lead.state,
        RecordedStageState::Retained,
        "{record:?}"
    );
    assert_eq!(record.lead.usage.attempts, 1, "{record:?}");
    assert_eq!(
        record.delivery,
        RecordedDeliveryState::Consumed,
        "{record:?}"
    );
    assert_eq!(
        record.binding, accepted_binding,
        "the accepted task/acceptance binding changed during retained recovery"
    );
    assert_eq!(record.binding.task, task);
    assert_eq!(record.acceptance_id, acceptance);
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

    let executor_native: Value = read_json(&executor_stage);
    assert_eq!(executor_native["status"], "completed", "{executor_native}");
    assert_eq!(
        executor_native["fork"]["forkThread"], fork_thread,
        "reconciliation forked again: {executor_native}"
    );
    assert_eq!(
        executor_native["turnId"], turn_id,
        "reconciliation submitted another turn: {executor_native}"
    );
    assert_eq!(
        executor_native["result"], EXECUTOR_RESULT_TEXT,
        "{executor_native}"
    );
    assert_eq!(executor_native["usageKnown"], true, "{executor_native}");
    let lead_native: Value = read_json(&lead_stage);
    assert_eq!(lead_native["status"], "completed", "{lead_native}");
    assert_eq!(lead_native["result"], LEAD_RESULT_TEXT, "{lead_native}");
    assert_eq!(
        reflection_request_count(&evidence, EXECUTOR_PROMPT_MARKER),
        1,
        "the retained executor turn was submitted to the provider twice"
    );
    assert_eq!(
        reflection_request_count(&evidence, LEAD_PROMPT_MARKER),
        1,
        "the lead reflection did not run exactly once"
    );

    let (_, final_status, _) = host_status(&home, &thread).expect("the successor host status");
    assert_eq!(final_status["state"], "running", "{final_status}");
    let successor_controller_pid = final_status["process"]["pid"].as_u64().unwrap();
    let successor_controller_created = final_status["process"]["creationTime"].as_u64().unwrap();
    let successor_controller_program =
        PathBuf::from(final_status["process"]["program"].as_str().unwrap());
    assert_ne!(
        successor_controller_pid, controller_pid,
        "the successor kept the failed consumer"
    );
    let successor_controller_identity = ProcessIdentity {
        pid: u32::try_from(successor_controller_pid).unwrap(),
        creation_time: successor_controller_created,
    };
    assert!(process_is_live(
        successor_controller_identity,
        &successor_controller_program
    ));
    let reconciled_surface: Value = read_json(&executor_surface_path);
    assert_eq!(
        reconciled_surface["role"], "executor-self-reflection",
        "{reconciled_surface}"
    );
    assert_eq!(
        reconciled_surface["consoleAttached"], true,
        "{reconciled_surface}"
    );
    assert_eq!(
        reconciled_surface["captionBefore"], reconciled_surface["nativeTitle"],
        "{reconciled_surface}"
    );
    assert!(
        reconciled_surface["nativeTitle"]
            .as_str()
            .is_some_and(|title| title.starts_with("ERefl ")),
        "{reconciled_surface}"
    );
    assert_eq!(
        reconciled_surface["coordinator"]["pid"], successor_controller_pid,
        "{reconciled_surface}"
    );
    assert_eq!(
        reconciled_surface["coordinator"]["creation_time"], successor_controller_created,
        "{reconciled_surface}"
    );
    assert!(
        same_program(
            Path::new(reconciled_surface["coordinatorProgram"].as_str().unwrap()),
            &successor_controller_program
        ),
        "the reconciling surface belongs to another controller: {reconciled_surface}"
    );
    assert!(
        same_program(
            Path::new(reconciled_surface["program"].as_str().unwrap()),
            &successor_controller_program
        ),
        "the reconciling trampoline is not the exact controller binary: {reconciled_surface}"
    );
    let reconciled_surface_identity = (
        reconciled_surface["identity"]["pid"].as_u64().unwrap(),
        reconciled_surface["identity"]["creation_time"]
            .as_u64()
            .unwrap(),
    );
    assert_ne!(
        reconciled_surface_identity, failed_surface_identity,
        "reconciliation reused the failed foreground process"
    );
    let lead_surface: Value = read_json(&lead_surface_path);
    assert_eq!(lead_surface["consoleAttached"], true, "{lead_surface}");
    assert!(
        lead_surface["nativeTitle"]
            .as_str()
            .is_some_and(|title| title.starts_with("LRefl ")),
        "{lead_surface}"
    );

    let delivery: Value = serde_json::from_slice(&fs::read(&delivery_path).unwrap()).unwrap();
    assert_eq!(delivery["state"], "delivered", "{delivery}");
    assert_eq!(delivery["by"], "clientId", "{delivery}");
    let client_message_id = delivery["clientMessageId"].as_str().unwrap().to_owned();
    assert!(!client_message_id.is_empty());
    assert_eq!(
        record.delivery_receipt.as_deref(),
        Some(client_message_id.as_str()),
        "{record:?}"
    );
    assert!(
        wait_until(Duration::from_secs(30), || {
            delivered_user_message_items(
                successor_port,
                &successor_token,
                &thread,
                &client_message_id,
            ) == 1
        }),
        "the resumed lead conversation did not show exactly one delivered synthesis"
    );

    std::thread::sleep(Duration::from_secs(6));
    let after: ReflectionState = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    let after_record = after.records.get(&acceptance).unwrap();
    assert_eq!(after_record.executor.usage.attempts, 1, "{after_record:?}");
    assert_eq!(
        reflection_request_count(&evidence, EXECUTOR_PROMPT_MARKER),
        1,
        "the settled successor submitted the retained turn again"
    );
    assert_eq!(
        delivered_user_message_items(
            successor_port,
            &successor_token,
            &thread,
            &client_message_id,
        ),
        1,
        "the retained delivery was replayed"
    );
    let issue = board_cli::show_issue(Path::new("bd.exe"), &board, &task).unwrap();
    assert_eq!(issue["status"], "closed", "{issue}");

    let outcome = quit_session(successor, &entry);
    assert_eq!(
        outcome.outcome.reason,
        StopReason::Exited,
        "{}",
        outcome.transcript
    );
    assert_eq!(outcome.outcome.exit_code, 0, "{}", outcome.transcript);
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
        "managed retained recovery: task {task} acceptance {acceptance} fork {fork_thread} turn {turn_id}; old host pid {} successor pid {}; evidence {}",
        host_identity.pid,
        successor_identity.pid,
        evidence.display()
    );
}

/// Asserts that one retained surface artifact proves a distinct titled console
/// owned by the invoking accepted process, with the executor role title, and
/// returns the foreground process identity so cleanup can be checked.
fn assert_foreground_surface(
    surface: &Value,
    coordinator: ProcessIdentity,
    prefix: &str,
) -> ProcessIdentity {
    assert_eq!(surface["schema"], 1, "{surface}");
    assert_eq!(surface["role"], "executor-self-reflection", "{surface}");
    assert_eq!(surface["consoleAttached"], true, "{surface}");
    assert_eq!(
        surface["captionBefore"], surface["nativeTitle"],
        "{surface}"
    );
    assert!(
        surface["nativeTitle"].as_str().is_some_and(|title| {
            title.starts_with(prefix) && title.contains("/deepseek/") && title.contains("/max#")
        }),
        "{surface}"
    );
    assert_eq!(surface["coordinator"]["pid"], coordinator.pid, "{surface}");
    assert_eq!(
        surface["coordinator"]["creation_time"], coordinator.creation_time,
        "{surface}"
    );
    assert!(
        same_program(
            Path::new(surface["coordinatorProgram"].as_str().unwrap()),
            &manager()
        ),
        "{surface}"
    );
    assert!(
        same_program(Path::new(surface["program"].as_str().unwrap()), &manager()),
        "{surface}"
    );
    let identity = ProcessIdentity {
        pid: u32::try_from(surface["identity"]["pid"].as_u64().unwrap()).unwrap(),
        creation_time: surface["identity"]["creation_time"].as_u64().unwrap(),
    };
    assert_ne!(
        identity.pid, coordinator.pid,
        "the retained stage must own a new foreground process: {surface}"
    );
    identity
}

/// A pre-submission surface loss retains the exact fork, measured work and no
/// turn. The accepted owner must recover that fork through the same foreground
/// boundary as a fresh stage: an immediate retry must refuse without visibility
/// and stay retryable, and the later attempt must resume the one fork, submit
/// its initial turn once and deliver once.
#[test]
fn native_accepted_pre_submission_loss_recovers_retained_fork_in_a_titled_surface() {
    let workspace = Workspace::new("lead-recovery-pre-submission", LeadMode::Delivered);
    let (controller, request, acceptance) = accepted::accepted(&workspace);
    let state_dir = workspace.state.clone();
    let executor_stage = state_dir.join("executor-stage.json");
    let executor_surface = executor_stage.with_extension("surface.json");
    let release = workspace.home.join("frontend-release");

    // The owned frontend exits as soon as this marker exists. The first pass
    // still creates and measures the exact fork, then loses its surface before
    // any turn is submitted.
    fs::write(&release, "pre-submission surface loss").unwrap();
    let first = ConsoleSession::spawn(ConsoleSpec::new(chain_spec(
        &workspace,
        &request,
        "mutator-window",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
        Some(&workspace.checkout),
    )))
    .unwrap();
    let first_identity = first.identity();
    let lost = wait_for_stage_status(&executor_stage, "visibility-lost");
    assert_eq!(lost["fork"]["forkThread"], EXECUTOR_FORK, "{lost}");
    assert!(lost["turnId"].is_null(), "{lost}");
    assert!(
        lost["error"].as_str().is_some_and(|error| {
            error.contains("no turn was submitted") && error.contains("retryable")
        }),
        "{lost}"
    );
    let accepted_work_before = read_json(&state_dir.join("chain.json"))["acceptedWork"].clone();
    let work_before = lost["work"].clone();
    let source_context_before = lost["sourceContextBytes"].clone();
    assert_eq!(
        request_counts(&workspace),
        (1, 0, 0, 0),
        "the first pass submitted model work"
    );
    assert!(
        fixture_requests(&workspace, "executor-self-reflection", "thread/start").is_empty(),
        "the first pass started a new thread instead of forking the source"
    );
    let in_flight = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(in_flight.executor.state, RecordedStageState::InFlight);
    assert_eq!(in_flight.executor.usage.attempts, 1);
    assert_eq!(in_flight.executor.usage.turns, 0);
    let finished = wait_session(first);
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let first_surface: Value = read_json(&executor_surface);
    let first_surface_identity =
        assert_foreground_surface(&first_surface, first_identity, "ERefl ");
    wait_process_gone(
        first_surface_identity,
        Path::new(first_surface["program"].as_str().unwrap()),
        "the first executor foreground surface",
    );
    assert_eq!(read_json(&executor_stage), lost);
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap(),
        in_flight,
        "the pre-submission loss changed the accepted ledger"
    );

    // The retry still cannot restore visibility. It must resume the retained
    // fork in a new titled foreground surface, refuse without submitting a
    // turn, and keep the exact in-flight ledger for another attempt.
    let second = ConsoleSession::spawn(ConsoleSpec::new(chain_spec(
        &workspace,
        &request,
        "mutator-window",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
        Some(&workspace.checkout),
    )))
    .unwrap();
    let second_identity = second.identity();
    let refused = wait_session(second);
    assert_ne!(refused.outcome.exit_code, 0, "{}", refused.transcript);
    assert!(
        refused
            .transcript
            .contains("pre-submission reflection recovery")
            && refused.transcript.contains("No turn was submitted"),
        "{}",
        refused.transcript
    );
    let still_lost = read_json(&executor_stage);
    assert_eq!(still_lost["status"], "visibility-lost", "{still_lost}");
    assert_eq!(
        still_lost["fork"]["forkThread"], EXECUTOR_FORK,
        "{still_lost}"
    );
    assert!(still_lost["turnId"].is_null(), "{still_lost}");
    assert_eq!(still_lost["work"], work_before, "{still_lost}");
    assert_eq!(
        still_lost["sourceContextBytes"], source_context_before,
        "{still_lost}"
    );
    assert_eq!(
        request_counts(&workspace),
        (1, 0, 0, 0),
        "the refused retry submitted model work"
    );
    let retry_resumes = fixture_requests(&workspace, "executor-self-reflection", "thread/resume");
    assert_eq!(retry_resumes.len(), 1, "{retry_resumes:?}");
    assert_eq!(
        retry_resumes[0]["params"]["threadId"], EXECUTOR_FORK,
        "{retry_resumes:?}"
    );
    assert_eq!(
        controller.record(&acceptance).unwrap().unwrap(),
        in_flight,
        "the refused retry changed the accepted ledger"
    );
    let retry_surface: Value = read_json(&executor_surface);
    let retry_surface_identity =
        assert_foreground_surface(&retry_surface, second_identity, "ERefl ");
    wait_process_gone(
        retry_surface_identity,
        Path::new(retry_surface["program"].as_str().unwrap()),
        "the refused executor retry surface",
    );

    // Visibility returns. The same retained fork must now become visible,
    // submit its initial turn once, complete the stages and deliver once.
    fs::remove_file(&release).unwrap();
    let recovering = ConsoleSession::spawn(ConsoleSpec::new(chain_spec(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
        Some(&workspace.checkout),
    )))
    .unwrap();
    let recovering_identity = recovering.identity();
    let recovered = wait_session(recovering);
    assert_eq!(recovered.outcome.exit_code, 0, "{}", recovered.transcript);

    let executor = read_json(&executor_stage);
    assert_eq!(executor["status"], "completed", "{executor}");
    assert_eq!(executor["fork"]["forkThread"], EXECUTOR_FORK, "{executor}");
    assert_eq!(executor["turnId"], EXECUTOR_FORK_TURN, "{executor}");
    assert_eq!(executor["result"], EXECUTOR_RESULT, "{executor}");
    assert_eq!(executor["work"], work_before, "{executor}");
    assert_eq!(
        executor["sourceContextBytes"], source_context_before,
        "{executor}"
    );
    assert_usage(
        &executor,
        120,
        570,
        "executor-self-reflection-new-turn",
        EXECUTOR_SOURCE_TURN,
    );
    assert_eq!(
        request_counts(&workspace),
        (1, 1, 1, 1),
        "recovery forked or submitted a stage twice"
    );
    let executor_turns = fixture_requests(&workspace, "executor-self-reflection", "turn/start");
    assert_eq!(executor_turns.len(), 1, "{executor_turns:?}");
    assert_eq!(
        executor_turns[0]["params"]["threadId"], EXECUTOR_FORK,
        "{executor_turns:?}"
    );
    let resumes = fixture_requests(&workspace, "executor-self-reflection", "thread/resume");
    assert_eq!(resumes.len(), 2, "{resumes:?}");
    for resume in &resumes {
        assert_eq!(resume["params"]["threadId"], EXECUTOR_FORK, "{resumes:?}");
    }
    assert_eq!(
        fixture_requests(&workspace, "executor-self-reflection", "thread/name/set").len(),
        1,
        "the retained fork was relabelled during recovery"
    );
    assert_eq!(
        read_json(&state_dir.join("chain.json"))["acceptedWork"],
        accepted_work_before,
        "recovery changed the accepted context"
    );
    let recovered_surface: Value = read_json(&executor_surface);
    let recovered_surface_identity =
        assert_foreground_surface(&recovered_surface, recovering_identity, "ERefl ");
    wait_process_gone(
        recovered_surface_identity,
        Path::new(recovered_surface["program"].as_str().unwrap()),
        "the recovered executor foreground surface",
    );

    let lead = read_json(&state_dir.join("lead-stage.json"));
    assert_eq!(lead["status"], "completed", "{lead}");
    assert_eq!(lead["fork"]["forkThread"], LEAD_FORK, "{lead}");
    assert_eq!(lead["turnId"], LEAD_FORK_TURN, "{lead}");
    assert_eq!(lead["result"], LEAD_RESULT, "{lead}");
    let delivery = read_json(&state_dir.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "delivered", "{delivery}");
    assert_eq!(delivery["by"], "clientId", "{delivery}");
    let lead_requests = workspace.lead.requests_for("turn/start");
    assert_eq!(lead_requests.len(), 1, "{lead_requests:?}");
    assert_eq!(
        lead_requests[0]["params"]["clientUserMessageId"], delivery["clientMessageId"],
        "{lead_requests:?}"
    );
    assert_eq!(
        workspace.lead_state.lock().unwrap().client_id.as_deref(),
        delivery["clientMessageId"].as_str(),
        "the delivered client-id message was replaced or repeated"
    );

    let record = controller.record(&acceptance).unwrap().unwrap();
    assert_eq!(record.executor.state, RecordedStageState::Retained);
    assert_eq!(
        record.executor.usage.attempts, 1,
        "pre-submission recovery began a second stage attempt"
    );
    assert_eq!(record.executor.usage.turns, 1);
    assert_eq!(record.executor.usage.tokens, 120);
    assert_eq!(record.executor.usage.investigations, 0);
    assert_eq!(
        record.executor.usage.context_bytes,
        executor["work"]["context_bytes"].as_u64().unwrap()
    );
    assert_eq!(record.lead.state, RecordedStageState::Retained);
    assert_eq!(record.lead.usage.attempts, 1);
    assert_eq!(record.lead.usage.turns, 1);
    assert_eq!(record.lead.usage.tokens, 240);
    assert_eq!(record.usage_totals().tokens, 360);
    assert_eq!(record.delivery, RecordedDeliveryState::Consumed);
    assert_eq!(controller.stored_state().unwrap().unwrap().records.len(), 1);

    eprintln!(
        "pre-submission retained recovery evidence: {}",
        workspace.root().display()
    );
    std::mem::forget(workspace);
}
