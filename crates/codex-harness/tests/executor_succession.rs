//! OFAP 4.x instruction-refresh succession acceptance with owned synthetic
//! sessions: one exact managed session, a changed instruction and skill
//! revision, a durable handover, predecessor process stop and a verified
//! `codex exec resume` successor. No real model or subscription is used.
#![cfg(windows)]
#[path = "fixtures/succession_responses.rs"]
mod succession_responses;

use harness_core::{
    broker_state::BrokerRoot,
    build_identity::{self, BuildRecord},
    console::{ConsoleSession, ConsoleSpec},
    process::CommandSpec,
    process_service::{ServiceProcess, current_user},
    task_store::{Assignment, TaskRecord},
    task_succession,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(30);
const AGENTS_SEED: &str = "SUCCESSION_AGENTS_MARKER=seed-1f2a";
const AGENTS_RELOAD: &str = "SUCCESSION_AGENTS_MARKER=reload-9c4d";
const SKILL_SEED: &str = "SUCCESSION_SKILL_MARKER=seed-1f2a";
const SKILL_RELOAD: &str = "SUCCESSION_SKILL_MARKER=reload-9c4d";

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned native entry point and synthetic Responses"]
fn instruction_refresh_succession_preserves_work_and_reports_not_established() {
    let OwnedSession {
        home,
        workspace,
        build,
        evidence,
        upstream,
        responses: _responses,
        session,
        state,
        console,
        skill_dir,
        seed_revision,
    } = owned_session(false);
    let pointer = task_succession::session_pointer_path(&home, &session);
    assert!(
        pointer.is_file(),
        "managed sessions must be discoverable by exact session id"
    );
    assert_eq!(
        task_succession::read_session_pointer(&home, &session)
            .unwrap()
            .unwrap(),
        state
    );
    let view_identity = read_view_process(&state);
    let user = current_user().unwrap();
    assert!(
        ServiceProcess::inspect(view_identity, &upstream, &user)
            .unwrap()
            .is_some(),
        "the predecessor session process must be running before succession"
    );
    let binding = task_succession::binding_from_leader(&state)
        .unwrap()
        .expect("recorded session binding");
    write_task_record(&home, &workspace, &session);
    // Accepted instruction and skill change: the live revision and content move.
    write_agents(&workspace, AGENTS_RELOAD);
    write_skill(&skill_dir, SKILL_RELOAD);
    let reload_package = skill_evolution::package::load(&skill_dir).unwrap();
    let base = request_base(
        &home,
        &workspace,
        &evidence,
        &session,
        &build.join("codex.exe"),
    );
    // 1. A stale published revision is refused before anything is replaced.
    let mut stale = base.clone();
    stale["revision"] = json!({"name":"succession-probe","path":skill_dir,
        "revision":seed_revision,"operation":"update"});
    let (code, receipt, stderr) = run_succession(&stale);
    assert_eq!(code, 1, "{receipt}");
    assert_eq!(receipt["status"], "notEstablished");
    assert!(
        stderr.contains(task_succession::NOT_ESTABLISHED),
        "the failure must be named on stderr: {stderr}"
    );
    assert!(
        receipt["message"]
            .as_str()
            .unwrap()
            .contains(task_succession::NOT_ESTABLISHED),
        "{receipt}"
    );
    assert!(!evidence.join("provider-2.json").exists());
    assert!(
        ServiceProcess::inspect(view_identity, &upstream, &user)
            .unwrap()
            .is_some(),
        "a refused succession must leave the predecessor running"
    );
    // 2. The verified succession replaces the process at a safe boundary.
    let mut valid = base.clone();
    valid["revision"] = json!({"name":reload_package.name,"path":reload_package.root,"revision":reload_package.revision,"operation":"update"});
    let (code, receipt, _) = run_succession(&valid);
    assert_eq!(code, 0, "{receipt}");
    assert_eq!(receipt["status"], "established", "{receipt}");
    assert_eq!(
        receipt["predecessor"]["stop"], "confirmed",
        "the predecessor process stop must be confirmed: {receipt}"
    );
    let argv = receipt["successor"]["argv"].as_array().unwrap();
    let argv: Vec<&str> = argv.iter().filter_map(|value| value.as_str()).collect();
    let resume = argv.iter().position(|arg| *arg == "resume").unwrap();
    assert_eq!(argv[resume + 1], session);
    assert!(!argv.contains(&"--last"));
    let sandbox = argv.iter().position(|arg| *arg == "--sandbox").unwrap();
    assert_eq!(Some(argv[sandbox + 1]), binding.sandbox.as_deref());
    assert!(
        argv.contains(
            &format!("approval_policy={}", binding.approval.as_deref().unwrap()).as_str()
        )
    );
    assert_eq!(receipt["successor"]["threadStarted"], session);
    assert_eq!(receipt["successor"]["exitCode"], 0);
    assert!(
        ServiceProcess::inspect(view_identity, &upstream, &user)
            .unwrap()
            .is_none(),
        "the predecessor session process must be stopped"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt")).unwrap(),
        "one"
    );
    let handover: Value = serde_json::from_slice(
        &fs::read(home.join("harness/orchestration/succession-task.succession.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        handover["requirements"],
        "complete the owned succession probe"
    );
    assert_eq!(handover["predecessor"]["stop"], "confirmed");
    assert_eq!(handover["revision"]["revision"], reload_package.revision);
    assert_eq!(handover["taskRecord"]["authorization"], "full");
    assert!(evidence.join("provider-3.json").exists());
    assert!(!evidence.join("provider-4.json").exists());
    // 3. A resumed session that does not show the expected instructions is
    // reported as not established instead of being relied on.
    let decoy = home.join("decoy-instructions.md");
    fs::write(
        &decoy,
        "# Decoy instructions\n\nNEVER_LOADED_MARKER=deadbeef\n",
    )
    .unwrap();
    let mut decoy_request = base.clone();
    decoy_request["revision"] = json!({"name":reload_package.name,"path":reload_package.root,"revision":reload_package.revision,"operation":"update"});
    decoy_request["instructions"] = json!(decoy);
    let (code, receipt, stderr) = run_succession(&decoy_request);
    assert_eq!(code, 1, "{receipt}");
    assert_eq!(receipt["status"], "notEstablished", "{receipt}");
    assert!(
        stderr.contains(task_succession::NOT_ESTABLISHED),
        "the failure must be named on stderr: {stderr}"
    );
    assert!(
        receipt["message"]
            .as_str()
            .unwrap()
            .contains(task_succession::NOT_ESTABLISHED),
        "{receipt}"
    );
    assert!(evidence.join("provider-5.json").exists());
    let rollout = rollout_text(&home, &session);
    assert!(
        rollout.contains(AGENTS_RELOAD),
        "the rollout must carry the reloaded instructions"
    );
    assert!(
        rollout.contains(SKILL_RELOAD),
        "the rollout must carry the reloaded skill revision"
    );
    assert!(rollout.contains(succession_responses::CONTINUATION_MARKER));
    // The predecessor frontend stops; the owned console is released.
    let outcome = console
        .wait(
            harness_core::process::Deadline::after(Duration::from_secs(20)).unwrap(),
            &harness_core::process::Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(
        outcome.outcome.reason,
        harness_core::process::StopReason::Exited,
        "the predecessor launcher must exit after succession"
    );
    let _ = harness_core::task_runtime::request_stop(&state);
}

/// One owned managed session with a synthetic provider, a changed skill and
/// instruction source, and no live subscription or model call.
struct OwnedSession {
    home: PathBuf,
    workspace: PathBuf,
    build: PathBuf,
    evidence: PathBuf,
    upstream: PathBuf,
    responses: succession_responses::Responses,
    session: String,
    state: PathBuf,
    console: ConsoleSession,
    skill_dir: PathBuf,
    seed_revision: String,
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned native entry point and synthetic Responses"]
fn succession_defers_while_the_predecessor_turn_is_in_flight() {
    let owned = owned_session(true);
    let user = current_user().unwrap();
    let view_identity = read_view_process(&owned.state);
    wait_for_active(&owned.state);
    write_task_record(&owned.home, &owned.workspace, &owned.session);
    let package = skill_evolution::package::load(&owned.skill_dir).unwrap();
    let mut request = request_base(
        &owned.home,
        &owned.workspace,
        &owned.evidence,
        &owned.session,
        &owned.build.join("codex.exe"),
    );
    request["revision"] = json!({"name":package.name,"path":package.root,
        "revision":package.revision,"operation":"update"});
    request["timeout_seconds"] = json!(2);
    let (code, receipt, stderr) = run_succession(&request);
    assert_eq!(code, 2, "{receipt}");
    assert_eq!(receipt["status"], "deferred", "{receipt}");
    assert!(
        stderr.contains("succession deferred"),
        "the deferral must be named on stderr: {stderr}"
    );
    assert!(
        receipt["message"]
            .as_str()
            .unwrap()
            .contains("succession deferred"),
        "{receipt}"
    );
    assert!(
        !owned.state.join("stop.json").exists(),
        "a deferred succession must not stop the predecessor"
    );
    assert!(
        !owned.state.join("succession.json").exists(),
        "a deferred succession must not hand over the session"
    );
    assert!(
        !owned.evidence.join("provider-2.json").exists(),
        "a deferred succession must not spawn the successor"
    );
    assert!(
        ServiceProcess::inspect(view_identity, &owned.upstream, &user)
            .unwrap()
            .is_some(),
        "the predecessor must keep running while the boundary is deferred"
    );
    // Release the held turn and settle before ending this owned session.
    fs::write(owned.evidence.join("release-seed"), "release").unwrap();
    wait_for_seed(&owned.evidence);
    wait_for_idle(&owned.state);
    harness_core::task_runtime::request_stop(&owned.state).unwrap();
    if let Some(process) = ServiceProcess::inspect(view_identity, &owned.upstream, &user).unwrap() {
        process.terminate(130).unwrap();
        assert!(
            process
                .wait_for_exit(
                    harness_core::process::Deadline::after(Duration::from_secs(10)).unwrap()
                )
                .unwrap()
        );
    }
    let outcome = owned
        .console
        .wait(
            harness_core::process::Deadline::after(Duration::from_secs(20)).unwrap(),
            &harness_core::process::Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(
        outcome.outcome.reason,
        harness_core::process::StopReason::Exited,
        "the owned launcher must exit with its session"
    );
}

fn owned_session(hold_seed: bool) -> OwnedSession {
    let binaries = if let Some(candidate) = std::env::var_os("HARNESS_ACCEPTANCE_BUILD") {
        let candidate = PathBuf::from(candidate);
        assert!(
            candidate.is_absolute(),
            "use an absolute immutable build path"
        );
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let check = build_identity::check(&candidate, Some(source));
        assert_eq!(check.status, build_identity::Health::Healthy, "{check:?}");
        eprintln!(
            "succession verified release candidate: {}",
            candidate.display()
        );
        candidate
    } else {
        let binaries = Path::new(env!("CARGO_BIN_EXE_codex")).parent().unwrap();
        assert_eq!(
            binaries.file_name().unwrap(),
            "release",
            "run with --release or supply HARNESS_ACCEPTANCE_BUILD"
        );
        binaries.to_path_buf()
    };
    let upstream = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    assert!(upstream.is_absolute() && upstream.is_file());
    let owned = BrokerRoot::prepare().unwrap().keep();
    let root = owned.path();
    eprintln!("succession acceptance evidence: {}", root.display());
    let home = root.join("home");
    let workspace = root.join("workspace");
    let build = root.join("build");
    let source = root.join("source");
    let evidence = root.join("evidence");
    for directory in [
        home.join("harness"),
        home.join("skills/succession-probe"),
        workspace.join(".git"),
        build.clone(),
        source.join("global"),
        source.join("crates/fixture/src"),
        source.join("tools/rtk-adapter/src"),
        evidence.clone(),
        source
            .join(build_identity::INSPECTION_SCHEMA)
            .parent()
            .unwrap()
            .into(),
    ] {
        fs::create_dir_all(directory).unwrap();
    }
    for file in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/fixture/src/lib.rs",
        build_identity::INSPECTION_SCHEMA,
    ] {
        fs::write(source.join(file), "owned fixture source identity").unwrap();
    }
    fs::write(
        source.join("global/profile.toml"),
        "approval_policy = 'never'\nsandbox_mode = 'danger-full-access'\nmodel = 'gpt-6-astra'\n",
    )
    .unwrap();
    fs::write(
        source.join("global/kit.json"),
        serde_json::to_vec(&json!({"schema":1,"profile_name":"harness","profile":"global/profile.toml","instructions":"global/instructions.md","skills":"skills","agents":"global/agents","hooks":"global/hooks.json","token_hooks":"global/token-hooks.json"})).unwrap(),
    )
    .unwrap();
    for name in build_identity::BINARIES {
        fs::copy(binaries.join(name), build.join(name))
            .expect("prepare every workspace binary before this entry-point test");
    }
    let record = BuildRecord {
        schema: build_identity::SCHEMA,
        source_root: source.clone(),
        source: build_identity::source_identity(&source).unwrap(),
        rustc: "fixture registration of Cargo outputs".into(),
        cargo: "fixture registration of Cargo outputs".into(),
        target: "x86_64-pc-windows-msvc".into(),
        profile: "release".into(),
        binaries: build_identity::BINARIES
            .iter()
            .map(|name| {
                (
                    name.to_string(),
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
    fs::write(
        home.join("harness/native-launch.json"),
        serde_json::to_vec(&json!({"schema":2,"build":build,"task_control":true,"upstream":{"executable":upstream,"sha256":build_identity::hash_file(&upstream).unwrap(),"package":null}})).unwrap(),
    )
    .unwrap();
    let responses = succession_responses::Responses::start(evidence.clone());
    let trusted = serde_json::to_string(&workspace.to_string_lossy()).unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            r#"
model = "gpt-6-astra"
model_reasoning_effort = "low"
model_provider = "control_fixture"
approval_policy = "never"
sandbox_mode = "danger-full-access"
cli_auth_credentials_store = "file"
[model_providers.control_fixture]
name = "Owned native succession fixture"
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
    fs::write(
        home.join("ds.config.toml"),
        "model = \"gpt-6-astra\"\nmodel_provider = \"control_fixture\"\nmodel_reasoning_effort = \"low\"\n",
    )
    .unwrap();
    let skill_dir = home.join("skills/succession-probe");
    write_skill(&skill_dir, SKILL_SEED);
    write_agents(&workspace, AGENTS_SEED);
    let seed_package = skill_evolution::package::load(&skill_dir).unwrap();
    // Partial work exists before succession and must never be repeated.
    fs::write(workspace.join("proof.txt"), "one").unwrap();
    if hold_seed {
        fs::write(evidence.join("hold-seed"), "hold the seed turn").unwrap();
    }
    let (session, state, console) = launch_predecessor(&home, &workspace, &build, &evidence);
    wait_for_view(&state);
    if !hold_seed {
        wait_for_seed(&evidence);
        wait_for_idle(&state);
    }
    OwnedSession {
        home,
        workspace,
        build,
        evidence,
        upstream,
        responses,
        session,
        state,
        console,
        skill_dir,
        seed_revision: seed_package.revision,
    }
}

fn write_skill(directory: &Path, marker: &str) {
    fs::write(
        directory.join("SKILL.md"),
        format!(
            "---\nname: succession-probe\ndescription: Owned probe marker {marker}. Use only for succession acceptance.\n---\n\n{marker}\n"
        ),
    )
    .unwrap();
}

fn write_agents(workspace: &Path, marker: &str) {
    fs::write(
        workspace.join("AGENTS.md"),
        format!("# Owned succession probe instructions\n\n{marker}\n"),
    )
    .unwrap();
}

fn launch_predecessor(
    home: &Path,
    workspace: &Path,
    build: &Path,
    evidence: &Path,
) -> (String, PathBuf, ConsoleSession) {
    let mut command = CommandSpec::new(build.join("codex.exe"));
    command.current_dir = Some(workspace.to_path_buf());
    command.env.insert("CODEX_HOME".into(), Some(home.into()));
    command.env.insert(
        "HARNESS_CONTROL_FIXTURE_KEY".into(),
        Some("synthetic-owned-fixture".into()),
    );
    for key in ["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_ACCESS_TOKEN"] {
        command.env.insert(key.into(), None);
    }
    // The owned fixture is unsandboxed. Keep the owner's installed PowerShell
    // discoverable just as real executor dispatch does; removing WindowsApps
    // can hide the only PowerShell 7 installation and select the legacy shell.
    command.args = vec![
        "--no-alt-screen".into(),
        "Reply with the owned seed acknowledgement and stop.".into(),
    ];
    let mut spec = ConsoleSpec::new(command);
    spec.size.columns = 180;
    let console = ConsoleSession::spawn(spec).unwrap();
    let escapes = regex::Regex::new(r"\x1b\][^\x07]*(?:\x07)|\x1b\[[0-?]*[ -/]*[@-~]").unwrap();
    let until = Instant::now() + WAIT;
    let state = loop {
        let transcript = console.transcript().replace("\x1b[1C", " ");
        let plain = escapes.replace_all(&transcript, "");
        let state = plain
            .split("codex-harness: task control state:")
            .nth(1)
            .and_then(|tail| tail.split_once('\n'))
            .map(|(path, _)| PathBuf::from(path.trim()))
            .filter(|path| path.is_absolute() && path.join("launch.json").is_file());
        if state.is_some() || Instant::now() >= until {
            if state.is_none() {
                let _ = fs::write(
                    evidence.join("predecessor-launch-terminal.txt"),
                    console.transcript(),
                );
            }
            break state.expect("controller state location must be visible");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let session = loop {
        if let Ok(bytes) = fs::read(state.join("endpoint.json"))
            && let Ok(endpoint) = serde_json::from_slice::<Value>(&bytes)
            && let Some(session) = endpoint["thread_id"].as_str()
            && !session.is_empty()
        {
            break session.to_owned();
        }
        assert!(Instant::now() < until, "managed session id must appear");
        std::thread::sleep(Duration::from_millis(100));
    };
    (session, state, console)
}

fn wait_for_idle(state: &Path) {
    let until = Instant::now() + WAIT;
    loop {
        let settled = fs::read(state.join("task.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| {
                value["threads"]
                    .as_object()
                    .and_then(|threads| threads.values().next())
                    .map(|thread| thread["status"]["type"] == "idle")
            })
            .unwrap_or(false);
        let visibility = fs::read(state.join("visibility.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        let quiet = visibility.as_ref().is_none_or(|value| {
            value["activeTurns"]
                .as_object()
                .is_none_or(|map| map.is_empty())
                && value["activeOperations"]
                    .as_object()
                    .is_none_or(|map| map.is_empty())
                && value["pendingRequests"]
                    .as_object()
                    .is_none_or(|map| map.is_empty())
        });
        if settled && quiet {
            return;
        }
        assert!(Instant::now() < until, "predecessor session must settle");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_for_view(state: &Path) {
    let until = Instant::now() + WAIT;
    while !state.join("view.json").is_file() {
        assert!(
            Instant::now() < until,
            "the managed conversation view must be recorded"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_for_active(state: &Path) {
    let until = Instant::now() + WAIT;
    loop {
        let active = fs::read(state.join("visibility.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| {
                value["activeTurns"]
                    .as_object()
                    .map(|turns| !turns.is_empty())
            })
            .unwrap_or(false);
        if active {
            return;
        }
        assert!(
            Instant::now() < until,
            "the predecessor turn must become active"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_seed(evidence: &Path) {
    let until = Instant::now() + WAIT;
    while !evidence.join("provider-1.json").is_file() {
        assert!(
            Instant::now() < until,
            "the predecessor seed turn must reach the owned provider"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn read_view_process(state: &Path) -> harness_core::process::ProcessIdentity {
    let view: Value = serde_json::from_slice(&fs::read(state.join("view.json")).unwrap()).unwrap();
    let pid = view["window"]["process"]["pid"].as_u64().unwrap() as u32;
    let creation_time = view["window"]["process"]["creation_time"].as_u64().unwrap();
    harness_core::process::ProcessIdentity { pid, creation_time }
}

fn write_task_record(home: &Path, workspace: &Path, session: &str) {
    let mut record = TaskRecord {
        schema: 1,
        id: "succession-task".into(),
        workspace: workspace.to_path_buf(),
        worktree: Some(workspace.to_path_buf()),
        lead_profile: "default".into(),
        authorization: "full".into(),
        requirements: "complete the owned succession probe".into(),
        stopped: false,
        active_lead: "default".into(),
        assignments: vec![Assignment {
            id: "a1".into(),
            profile: "ds".into(),
            owner_thread: Some(session.into()),
            worktree: Some(workspace.to_path_buf()),
            attempt: 1,
            accepted: None,
            defect: None,
        }],
    };
    harness_core::task_store::save(home, &record).unwrap();
    record.stopped = false;
}

fn request_base(
    home: &Path,
    workspace: &Path,
    evidence: &Path,
    session: &str,
    launcher: &Path,
) -> Value {
    json!({
        "schema": 1,
        "session": session,
        "profile": "ds",
        "codex_home": home,
        "workspace": workspace,
        "task": "succession-task",
        "assignment": "a1",
        "executable": launcher,
        "requirements": "complete the owned succession probe",
        "partial_work": "proof.txt already contains the single owned effect; do not repeat it",
        "evidence": evidence,
        "timeout_seconds": 60,
    })
}

fn run_succession(request: &Value) -> (i32, Value, String) {
    let path = request["evidence"]
        .as_str()
        .map(PathBuf::from)
        .unwrap()
        .join(format!(
            "succession-request-{}.json",
            request["instructions"]
                .as_str()
                .map(|value| value.len())
                .unwrap_or(0)
        ));
    fs::write(&path, serde_json::to_vec_pretty(request).unwrap()).unwrap();
    let manager =
        Path::new(request["executable"].as_str().unwrap()).with_file_name("codex-harness.exe");
    let output = Command::new(manager)
        .args(["executor", "succeed", "--request"])
        .arg(&path)
        .env("HARNESS_CONTROL_FIXTURE_KEY", "synthetic-owned-fixture")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let receipt: Value = serde_json::from_str(&stdout).unwrap_or_else(|_| {
        panic!(
            "succession receipt must be JSON: {stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (
        output.status.code().unwrap_or(-1),
        receipt,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn rollout_text(home: &Path, session: &str) -> String {
    let marker = task_succession::marker_for(session);
    let path = task_succession::find_rollout(home, session, &marker)
        .unwrap()
        .expect("successor rollout evidence");
    fs::read_to_string(path).unwrap()
}
