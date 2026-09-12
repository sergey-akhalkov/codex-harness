//! Ordinary native entry point with an automatically owned controller/server.
//! Explicit native CLI input, synthetic provider; no subscription requests.
#![cfg(windows)]
#[path = "fixtures/control_responses.rs"]
mod control_responses;
use harness_core::{
    broker_state::BrokerRoot,
    build_identity::{self, BuildRecord},
    console::{ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline, StopReason},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(30);

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        harness_core::task_runtime::request_stop(&self.0).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !self.0.join("closed.json").is_file() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned native entry point, no model use"]
fn ordinary_launcher_starts_control_and_delivers_tool_result() {
    native_entry(EntryCase::Normal);
}

#[test]
#[ignore = "requires explicit native CLI and scoped minimize/restore/final desktop observations; synthetic responses only"]
fn ordinary_launcher_suspends_and_recovers_visible_conversation() {
    native_entry(EntryCase::ViewLoss);
}

#[test]
#[ignore = "requires explicit native CLI and scoped final desktop observation; synthetic quota refusal only"]
fn ordinary_launcher_records_quota_refusal_without_another_gpt_request() {
    native_entry(EntryCase::QuotaRefusal);
}

#[test]
#[ignore = "requires native CLI, explicit model catalog and both native chat observations; synthetic quota handoff"]
fn ordinary_launcher_hands_quota_refusal_to_visible_zai_lead() {
    native_entry(EntryCase::QuotaHandoff);
}

#[derive(Clone, Copy)]
enum EntryCase {
    Normal,
    ViewLoss,
    QuotaRefusal,
    QuotaHandoff,
}

fn native_entry(case: EntryCase) {
    let view_loss = matches!(case, EntryCase::ViewLoss);
    let quota_refusal = matches!(case, EntryCase::QuotaRefusal);
    let quota_handoff = matches!(case, EntryCase::QuotaHandoff);
    assert_eq!(
        Path::new(env!("CARGO_BIN_EXE_codex"))
            .parent()
            .unwrap()
            .file_name()
            .unwrap(),
        "release",
        "run this installed-entry test with --release"
    );
    let upstream = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    let owned = BrokerRoot::prepare().unwrap().keep();
    let root = owned.path();
    eprintln!("ordinary control evidence: {}", root.display());
    let home = root.join("home");
    let workspace = root.join("workspace");
    let build = root.join("build");
    let source = root.join("source");
    for directory in [
        home.join("harness"),
        workspace.clone(),
        build.clone(),
        source.join("global"),
        source.join("crates/fixture/src"),
        source.join("tools/rtk-adapter/src"),
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
    fs::write(source.join("global/kit.json"), serde_json::to_vec(&json!({"schema":1,"profile_name":"harness","profile":"global/profile.toml","instructions":"global/instructions.md","skills":"skills","agents":"global/agents","hooks":"global/hooks.json","token_hooks":"global/token-hooks.json"})).unwrap()).unwrap();
    // Real Cargo outputs; the owned registration only substitutes install paths.
    for (name, binary) in [
        ("codex.exe", env!("CARGO_BIN_EXE_codex")),
        ("codex-harness.exe", env!("CARGO_BIN_EXE_codex-harness")),
        ("harness-inspect.exe", env!("CARGO_BIN_EXE_harness-inspect")),
        ("harness-observe.exe", env!("CARGO_BIN_EXE_harness-observe")),
    ] {
        fs::copy(binary, build.join(name)).unwrap();
    }
    let rtk = Path::new(env!("CARGO_BIN_EXE_codex")).with_file_name("harness-rtk.exe");
    fs::copy(rtk, build.join("harness-rtk.exe"))
        .expect("build the workspace binaries with --release before this entry-point test");
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
    fs::write(home.join("harness/native-launch.json"), serde_json::to_vec(&json!({"schema":2,"build":build,"task_control":true,"upstream":{"executable":upstream,"sha256":build_identity::hash_file(&upstream).unwrap(),"package":null}})).unwrap()).unwrap();
    let responses = match case {
        EntryCase::ViewLoss => control_responses::Responses::with_view_loss(root.into()),
        EntryCase::QuotaRefusal => control_responses::Responses::with_quota_refusal(root.into()),
        EntryCase::QuotaHandoff => control_responses::Responses::with_quota_handoff(root.into()),
        EntryCase::Normal => control_responses::Responses::start(root.into(), true),
    };
    let trusted = serde_json::to_string(&workspace.to_string_lossy()).unwrap();
    let catalog_config = if quota_handoff {
        let source = PathBuf::from(
            std::env::var_os("HARNESS_CONTROL_MODEL_CATALOG")
                .expect("explicit model metadata, without credentials"),
        );
        let catalog = home.join("catalog.json");
        fs::copy(source, &catalog).unwrap();
        format!(
            "model_catalog_json = {}",
            serde_json::to_string(&catalog.to_string_lossy()).unwrap()
        )
    } else {
        String::new()
    };
    fs::write(
        home.join("config.toml"),
        format!(
            r#"
model = "gpt-6-astra"
model_reasoning_effort = "low"
model_provider = "control_fixture"
{catalog_config}
approval_policy = "never"
sandbox_mode = "danger-full-access"
cli_auth_credentials_store = "file"
[model_providers.control_fixture]
name = "Owned native controller fixture"
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
    let mut command = CommandSpec::new(build.join("codex.exe"));
    command.current_dir = Some(workspace.clone());
    command.env.insert("CODEX_HOME".into(), Some(home.into()));
    command.env.insert(
        "HARNESS_CONTROL_FIXTURE_KEY".into(),
        Some("synthetic-owned-fixture".into()),
    );
    for key in ["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_ACCESS_TOKEN"] {
        command.env.insert(key.into(), None);
    }
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
    command.args = vec![
        "--no-alt-screen".into(),
        "Perform the owned proof command and return its consumed result.".into(),
    ];
    let mut console = ConsoleSpec::new(command);
    console.size.columns = 180;
    let session = ConsoleSession::spawn(console).unwrap();
    let escapes = regex::Regex::new(r"\x1b\][^\x07]*(?:\x07)|\x1b\[[0-?]*[ -/]*[@-~]").unwrap();
    let until = Instant::now() + WAIT;
    let state = loop {
        let transcript = session.transcript().replace("\x1b[1C", " ");
        let plain = escapes.replace_all(&transcript, "");
        let state = plain
            .split("codex-harness: task control state:")
            .nth(1)
            .and_then(|tail| tail.split_once('\n'))
            .map(|(path, _)| PathBuf::from(path.trim()))
            .filter(|path| path.is_absolute() && path.join("launch.json").is_file());
        if state.is_some() || Instant::now() >= until {
            break state;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    fs::write(root.join("terminal.txt"), session.transcript()).unwrap();
    let _cleanup = state.clone().map(Cleanup);
    let state = state.expect("controller state location must be visible");
    fs::write(
        root.join("controller-state.json"),
        serde_json::to_vec(&json!({"root":state})).unwrap(),
    )
    .unwrap();
    if view_loss {
        exercise_view_loss(root, &state, &workspace);
    }
    let marker = if quota_refusal {
        quota_refusal_result(root, &state, &workspace)
    } else {
        control_responses::FINAL.to_owned()
    };
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if quota_handoff && let Ok(bytes) = fs::read(state.join("handoff.json")) {
            let transfer: Value = serde_json::from_slice(&bytes).unwrap();
            assert_ne!(
                transfer["transfer"]["phase"], "blocked",
                "handoff blocked: {}",
                transfer["transfer"]["error"]
            );
        }
        assert!(
            !state.join("client-closed.json").is_file(),
            "native conversation closed before its result; inspect client-closed.json"
        );
        if state.join("view.json").is_file()
            && fs::read_to_string(state.join("task.json"))
                .is_ok_and(|saved| saved.contains(&marker))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        fs::read_to_string(state.join("task.json"))
            .unwrap()
            .contains(&marker),
        "controller must consume the result in the visible native session"
    );
    if !quota_refusal {
        assert_eq!(
            fs::read_to_string(workspace.join("proof.txt")).unwrap(),
            "one"
        );
    }
    let initial_view: Value =
        serde_json::from_slice(&fs::read(state.join("view.json")).unwrap()).unwrap();
    let view = if quota_handoff {
        let leader: Value =
            serde_json::from_slice(&fs::read(state.join("leader.json")).unwrap()).unwrap();
        let views: Value =
            serde_json::from_slice(&fs::read(state.join("additional-views.json")).unwrap())
                .unwrap();
        let id = leader["threadId"].as_str().unwrap();
        assert_ne!(id, initial_view["threadId"].as_str().unwrap());
        assert!(root.join("successor-visible-at-request.json").is_file());
        json!({"schema":1,"threadId":id,"window":views["threads"][id]})
    } else {
        initial_view.clone()
    };
    fs::write(
        root.join("native-entry-result-ready.json"),
        serde_json::to_vec_pretty(&json!({"state":state,"view":view,"previousView":quota_handoff.then_some(initial_view),"marker":marker})).unwrap(),
    )
    .unwrap();
    eprintln!(
        "native entry window awaits scoped desktop final observation and /quit: {}",
        root.display()
    );
    let until = Instant::now() + Duration::from_secs(90);
    while !root.join("native-entry-result-observed.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
    let observed: Value = serde_json::from_slice(
        &fs::read(root.join("native-entry-result-observed.json"))
            .expect("native entry desktop observation receipt"),
    )
    .unwrap();
    assert_eq!(observed["finalVisible"], true);
    let result = session
        .wait(
            Deadline::after(WAIT).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(result.outcome.reason, StopReason::Exited);
    assert_eq!(result.outcome.exit_code, 0);
    let until = Instant::now() + Duration::from_secs(5);
    while !state.join("closed.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        state.join("closed.json").is_file(),
        "idle controller must close after native TUI exit"
    );
    let checkpoint = fs::read_to_string(state.join("task.json")).unwrap();
    assert!(
        checkpoint.contains(&marker),
        "controller must consume the final result"
    );
    let runtime: Value =
        serde_json::from_slice(&fs::read(state.join("runtime.json")).unwrap()).unwrap();
    let identity = harness_core::process::ProcessIdentity {
        pid: runtime["process"]["pid"]
            .as_u64()
            .unwrap()
            .try_into()
            .unwrap(),
        creation_time: runtime["process"]["creationTime"].as_u64().unwrap(),
    };
    let user = harness_core::process_service::current_user().unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while harness_core::process_service::ServiceProcess::inspect(
        identity,
        &build.join("codex-harness.exe"),
        &user,
    )
    .unwrap()
    .is_some()
    {
        assert!(
            Instant::now() < until,
            "owned controller must exit after its completion receipt"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let provider: Value =
        serde_json::from_slice(&fs::read(root.join("provider-1.json")).unwrap()).unwrap();
    assert_eq!(provider["model"], "gpt-6-astra");
    assert_eq!(provider["reasoning"]["effort"], "low");
    let expected_requests = match case {
        EntryCase::Normal => 2,
        EntryCase::ViewLoss => 3,
        EntryCase::QuotaRefusal => 1,
        EntryCase::QuotaHandoff => 3,
    };
    assert!(
        root.join(format!("provider-{expected_requests}.json"))
            .is_file()
    );
    assert!(
        !root
            .join(format!("provider-{}.json", expected_requests + 1))
            .exists(),
        "named conversation must not issue an auxiliary model request"
    );
    fs::write(root.join("launch-result.json"), serde_json::to_vec(&json!({"schema":1,"upstreamSha256":build_identity::hash_file(&upstream).unwrap(),"nativeEntrySha256":record.binaries["codex.exe"],"managerSha256":record.binaries["codex-harness.exe"],"initialModel":"gpt-6-astra","model":if quota_handoff {"zai/glm-5.3"} else {"gpt-6-astra"},"outcome":if quota_handoff {"quotaHandoff"} else if quota_refusal {"quotaRefused"} else {"toolResult"},"toolSideEffectExactlyOnce":(!quota_refusal).then_some(true),"requests":expected_requests,"finalVisible":true,"manualSecondaryStartup":false})).unwrap()).unwrap();
}

fn quota_refusal_result(root: &Path, state: &Path, workspace: &Path) -> String {
    let until = Instant::now() + WAIT;
    while !state.join("failures.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    let saved: Value = serde_json::from_slice(
        &fs::read(state.join("failures.json")).expect("native terminal refusal must be retained"),
    )
    .unwrap();
    let threads = saved["threads"].as_object().unwrap();
    assert_eq!(threads.len(), 1);
    let failure = &threads.values().next().unwrap()["failure"];
    assert_eq!(
        failure["cause"], "quota",
        "preserve the native cause instead of inferring quota from HTTP 429"
    );
    assert_eq!(failure["error"]["codexErrorInfo"], "usageLimitExceeded");
    assert!(!workspace.join("proof.txt").exists());
    std::thread::sleep(Duration::from_secs(2));
    assert!(!root.join("provider-2.json").exists());
    failure["error"]["message"].as_str().unwrap().to_owned()
}

fn exercise_view_loss(root: &Path, state: &Path, workspace: &Path) {
    let until = Instant::now() + WAIT;
    while !(state.join("view.json").is_file() && workspace.join("proof.txt").is_file())
        && Instant::now() < until
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt")).unwrap(),
        "one"
    );
    let view: Value = serde_json::from_slice(&fs::read(state.join("view.json")).unwrap()).unwrap();
    fs::write(
        root.join("native-entry-partial-ready.json"),
        serde_json::to_vec(&json!({"state":state,"view":view})).unwrap(),
    )
    .unwrap();
    eprintln!(
        "native partial effect ready; minimize owned conversation: {}",
        root.display()
    );
    let until = Instant::now() + WAIT;
    loop {
        let visibility: Value = fs::read(state.join("visibility.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        if visibility["visible"] == false
            && visibility["activeTurns"]
                .as_object()
                .is_some_and(|turns| turns.is_empty())
            && visibility["pendingRequests"]
                .as_object()
                .is_some_and(|pending| pending.is_empty())
            && visibility["recoveryPending"]
                .as_array()
                .is_some_and(|threads| threads.len() == 1)
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "view loss must interrupt and settle the native turn; inspect visibility.json"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // Observe quiescence after native interruption, not only at the minimize instant.
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !root.join("provider-2.json").exists(),
        "no new model request while required view is unavailable"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt")).unwrap(),
        "one"
    );
    fs::write(
        root.join("native-entry-suspended-ready.json"),
        serde_json::to_vec(
            &json!({"state":state,"view":view,"requests":1,"partialEffectPreserved":true}),
        )
        .unwrap(),
    )
    .unwrap();
    eprintln!(
        "native interruption settled; restore owned conversation: {}",
        root.display()
    );
    let until = Instant::now() + Duration::from_secs(90);
    while !root.join("native-entry-view-restored.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        root.join("native-entry-view-restored.json").is_file(),
        "scoped restoration observation required"
    );
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !root.join("provider-2.json").exists(),
        "restoring the view must not resume the model while its previous tool is still running"
    );
    let visibility: Value =
        serde_json::from_slice(&fs::read(state.join("visibility.json")).unwrap()).unwrap();
    assert!(
        visibility["activeOperations"]
            .as_object()
            .is_some_and(|operations| !operations.is_empty()),
        "the unfinished native tool must remain owned after turn interruption"
    );
    fs::write(workspace.join("finish-tool"), "release owned command").unwrap();
}
