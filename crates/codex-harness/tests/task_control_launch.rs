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
#[ignore = "requires native CLI and parent/child desktop observations; synthetic Responses only"]
fn ordinary_launcher_opens_executor_before_its_first_request() {
    native_entry(EntryCase::Child);
}

#[test]
#[ignore = "requires native CLI, model catalog and three desktop observations; synthetic Responses only"]
fn ordinary_launcher_opens_two_distinct_executor_conversations() {
    native_entry(EntryCase::TwoChildren);
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

#[test]
#[ignore = "requires native CLI and helper pane observations; synthetic Responses only"]
fn ordinary_launcher_opens_helper_before_its_first_request() {
    native_entry(EntryCase::Helper);
}

#[test]
#[ignore = "requires native CLI and closed-window restore; synthetic Responses only"]
fn ordinary_launcher_restores_closed_executor_conversation() {
    native_entry(EntryCase::ViewClose);
}

#[test]
#[ignore = "requires native CLI; explicit user stop through the ordinary entry point; synthetic Responses only"]
fn ordinary_launcher_stops_on_explicit_user_stop() {
    native_entry(EntryCase::UserStop);
}

#[derive(Clone, Copy)]
enum EntryCase {
    Normal,
    Child,
    TwoChildren,
    ViewLoss,
    QuotaRefusal,
    QuotaHandoff,
    Helper,
    ViewClose,
    UserStop,
}

fn native_entry(case: EntryCase) {
    let pair = matches!(case, EntryCase::TwoChildren);
    let helper = matches!(case, EntryCase::Helper);
    let view_close = matches!(case, EntryCase::ViewClose);
    let child = matches!(
        case,
        EntryCase::Child | EntryCase::TwoChildren | EntryCase::Helper | EntryCase::ViewClose
    );
    let view_loss = matches!(case, EntryCase::ViewLoss);
    let user_stop = matches!(case, EntryCase::UserStop);
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
    fs::write(
        root.join("require-initial-view"),
        "verify before first provider response",
    )
    .unwrap();
    eprintln!("ordinary control evidence: {}", root.display());
    if pair {
        fs::write(root.join("two-executors"), "owned concurrent assignments").unwrap();
    }
    if helper {
        fs::write(root.join("helper-window"), "owned nested helper").unwrap();
    }
    if view_close {
        fs::write(root.join("close-view"), "owned closed conversation restore").unwrap();
    }
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
    if quota_handoff {
        fs::write(
            source.join("global/orchestration.toml"),
            "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"zai\"\nexecutor_profiles = [\"zai\"]\nmax_concurrent_executors = 2\n",
        )
        .unwrap();
        fs::write(
            home.join("zai.config.toml"),
            "model = \"zai/glm-5.3\"\nmodel_provider = \"control_fixture\"\n",
        )
        .unwrap();
    }
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
        EntryCase::ViewLoss | EntryCase::UserStop => {
            control_responses::Responses::with_view_loss(root.into())
        }
        EntryCase::QuotaRefusal => control_responses::Responses::with_quota_refusal(root.into()),
        EntryCase::QuotaHandoff => control_responses::Responses::with_quota_handoff(root.into()),
        EntryCase::Normal => control_responses::Responses::start(root.into(), true),
        EntryCase::Child | EntryCase::TwoChildren | EntryCase::Helper | EntryCase::ViewClose => {
            control_responses::Responses::start(root.into(), false)
        }
    };
    let trusted = serde_json::to_string(&workspace.to_string_lossy()).unwrap();
    let catalog_config = if quota_handoff || pair {
        let source = PathBuf::from(
            std::env::var_os("HARNESS_CONTROL_MODEL_CATALOG")
                .expect("explicit model metadata, without credentials"),
        );
        let catalog = home.join("catalog.json");
        fs::write(&catalog, pair_model_catalog(&source)).unwrap();
        format!(
            "model_catalog_json = {}",
            serde_json::to_string(&catalog.to_string_lossy()).unwrap()
        )
    } else {
        String::new()
    };
    let agents_config = if helper || pair {
        "[agents]\nmax_depth = 2\n"
    } else {
        ""
    };
    let multi_agent_v2 = if pair { "true" } else { "false" };
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
multi_agent = true
multi_agent_v2 = {multi_agent_v2}
{agents_config}
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
        if pair {
            "Delegate two independent owned proof tasks to Z.AI and Grok.".into()
        } else if child {
            "Delegate the owned proof to one native child.".into()
        } else {
            "Perform the owned proof command and return its consumed result.".into()
        },
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
    reveal_conversation(&state, &upstream);
    if view_loss {
        exercise_view_loss(root, &state, &workspace);
    }
    if user_stop {
        exercise_user_stop(root, &state, &workspace);
    }
    if child {
        pause_synthetic_parent_goal(&state);
    }
    if view_close {
        exercise_view_close(root, &state, &workspace, &upstream);
    }
    let marker = if pair {
        control_responses::PARENT_FINAL.to_owned()
    } else if quota_refusal {
        quota_refusal_result(root, &state, &workspace)
    } else if user_stop {
        "explicit stop".to_owned()
    } else {
        control_responses::FINAL.to_owned()
    };
    let until = Instant::now()
        + if helper || view_close || pair {
            Duration::from_secs(90)
        } else {
            WAIT
        };
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
        if user_stop && state.join("closed.json").is_file() {
            break;
        }
        if !user_stop
            && state.join("view.json").is_file()
            && fs::read_to_string(state.join("task.json")).is_ok_and(|saved| {
                saved.contains(&marker)
                    && (!pair
                        || saved.contains("CONTROL_TOOL_RESULT_CONSUMED_ONE")
                            && saved.contains("CONTROL_TOOL_RESULT_CONSUMED_TWO"))
            })
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if user_stop {
        let closed: Value =
            serde_json::from_slice(&fs::read(state.join("closed.json")).unwrap()).unwrap();
        assert_eq!(closed["reason"], "explicit stop");
        assert_eq!(closed["stopped"], true);
        assert!(!root.join("provider-2.json").exists());
    } else {
        assert!(
            fs::read_to_string(state.join("task.json"))
                .unwrap()
                .contains(&marker),
            "controller must consume the result in the visible native session"
        );
    }
    if !quota_refusal {
        assert_eq!(
            fs::read_to_string(workspace.join("proof.txt")).unwrap(),
            "one"
        );
    }
    if pair {
        let saved = fs::read_to_string(state.join("task.json")).unwrap();
        assert!(
            saved.contains("CONTROL_TOOL_RESULT_CONSUMED_ONE")
                && saved.contains("CONTROL_TOOL_RESULT_CONSUMED_TWO")
        );
        assert_eq!(
            fs::read_to_string(workspace.join("proof-two.txt")).unwrap(),
            "one"
        );
    }
    let initial_view: Value =
        serde_json::from_slice(&fs::read(state.join("view.json")).unwrap()).unwrap();
    if pair {
        let saved: Value =
            serde_json::from_slice(&fs::read(state.join("task.json")).unwrap()).unwrap();
        assert!(
            saved["threads"][initial_view["threadId"].as_str().unwrap()]["turns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|turn| turn["items"].as_array().is_some_and(|items| items
                    .iter()
                    .any(|item| item["type"] == "agentMessage"
                        && item["text"] == control_responses::PARENT_FINAL))),
            "the leader's own native final must consume both children"
        );
    }
    let view = if child {
        let views: Value =
            serde_json::from_slice(&fs::read(state.join("additional-views.json")).unwrap())
                .unwrap();
        let children = views["threads"].as_object().unwrap();
        assert_eq!(children.len(), if pair || helper { 2 } else { 1 });
        if helper {
            let named: Value =
                serde_json::from_slice(&fs::read(state.join("child-view-requests.json")).unwrap())
                    .unwrap();
            let titles: Vec<_> = named
                .as_object()
                .unwrap()
                .values()
                .map(|value| value["title"].as_str().unwrap().to_owned())
                .collect();
            assert!(titles.iter().any(|title| title == "Executor 1"));
            assert!(titles.iter().any(|title| title == "Helper 1"));
        }
        let (id, window) = children.iter().next().unwrap();
        assert_ne!(id, initial_view["threadId"].as_str().unwrap());
        json!({"schema":1,"threadId":id,"window":window,"allExecutors":children})
    } else if quota_handoff {
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
        serde_json::to_vec_pretty(&json!({"state":state,"view":view,"previousView":(quota_handoff || child).then_some(initial_view),"marker":marker})).unwrap(),
    )
    .unwrap();
    let snapshot: harness_core::task_view::Snapshot =
        serde_json::from_value(view["window"].clone()).unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    if !user_stop {
        assert!(
            snapshot.is_visible(&upstream, &user).unwrap(),
            "native conversation must remain visible for the final result"
        );
    }
    fs::write(
        root.join("native-entry-result-observed.json"),
        serde_json::to_vec_pretty(&json!({"finalVisible":!user_stop,"stopped":user_stop})).unwrap(),
    )
    .unwrap();
    let observed: Value =
        serde_json::from_slice(&fs::read(root.join("native-entry-result-observed.json")).unwrap())
            .unwrap();
    if user_stop {
        assert_eq!(observed["stopped"], true);
    } else {
        assert_eq!(observed["finalVisible"], true);
    }
    session.send("/quit").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    session.send("\r").unwrap();
    close_owned_views(&state);
    let result = session
        .wait(
            Deadline::after(WAIT).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(result.outcome.reason, StopReason::Exited);
    if !user_stop {
        assert_eq!(result.outcome.exit_code, 0);
    }
    let until = Instant::now() + Duration::from_secs(5);
    while !state.join("closed.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        state.join("closed.json").is_file(),
        "idle controller must close after native TUI exit"
    );
    if !user_stop {
        let checkpoint = fs::read_to_string(state.join("task.json")).unwrap();
        assert!(
            checkpoint.contains(&marker),
            "controller must consume the final result"
        );
    }
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
        EntryCase::Child => 4,
        EntryCase::TwoChildren => 9,
        EntryCase::ViewLoss => 3,
        EntryCase::UserStop => 1,
        EntryCase::QuotaRefusal => 1,
        EntryCase::QuotaHandoff => 3,
        EntryCase::Helper => 7,
        EntryCase::ViewClose => 4,
    };
    for sequence in 1..=expected_requests {
        let exchange: Value = serde_json::from_slice(
            &fs::read(state.join(format!("gateway-exchange-{sequence}.json")))
                .expect("every native provider request must pass the installed task ingress"),
        )
        .unwrap();
        assert_eq!(exchange["completed"], true);
        assert_eq!(exchange["responseStarted"], true);
        assert!(exchange["errorKind"].is_null());
    }
    assert!(
        !state
            .join(format!("gateway-exchange-{}.json", expected_requests + 1))
            .exists()
    );
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

fn pair_model_catalog(source: &Path) -> Vec<u8> {
    let bytes = fs::read(source).expect("explicit model metadata, without credentials");
    let mut catalog = if bytes.is_empty() {
        json!({"models": []})
    } else {
        serde_json::from_slice::<Value>(&bytes)
            .expect("model catalog must be JSON metadata without credentials")
    };
    if !catalog["models"].is_array() {
        catalog = json!({"models": []});
    }
    let models = catalog["models"].as_array_mut().unwrap();
    for (slug, display, effort) in [
        ("gpt-6-astra", "GPT-6 Astra", "low"),
        ("glm-5.3", "Z.AI GLM-5.3", "high"),
        ("zai/glm-5.3", "Z.AI GLM-5.3", "high"),
        ("grok-4.6", "Grok 4.6", "high"),
        ("xai/grok-4.6", "Grok 4.6", "high"),
    ] {
        if !models.iter().any(|model| model["slug"] == slug) {
            models.push(json!({
                "slug": slug,
                "display_name": display,
                "description": display,
                "default_reasoning_level": effort,
                "supported_reasoning_levels": [
                    {"effort": "low", "description": "Light reasoning"},
                    {"effort": "high", "description": "Enhanced reasoning"},
                    {"effort": "max", "description": "Deep reasoning"}
                ],
                "shell_type": "shell_command",
                "visibility": "list",
                "supported_in_api": true,
                "priority": 0,
                "base_instructions": "",
                "supports_reasoning_summaries": true,
                "default_reasoning_summary": "none",
                "support_verbosity": false,
                "apply_patch_tool_type": "freeform",
                "truncation_policy": {"mode": "bytes", "limit": 10000},
                "context_window": 1048576,
                "max_context_window": 1048576,
                "effective_context_window_percent": 95,
                "supports_parallel_tool_calls": true,
                "experimental_supported_tools": [],
                "input_modalities": ["text"],
                "multi_agent_version": "v2"
            }));
        }
    }
    serde_json::to_vec(&catalog).unwrap()
}
fn pause_synthetic_parent_goal(state: &Path) {
    // The canned final cannot complete a native goal. Disable that fixture-only
    // scheduling loop, as in the native child/reconnect contract, while the
    // actual child continues through its own tool/result path.
    let until = Instant::now() + WAIT;
    while !state.join("child-view-requests.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        state.join("child-view-requests.json").is_file(),
        "native child must request its own window"
    );
    let endpoint: Value =
        serde_json::from_slice(&fs::read(state.join("endpoint.json")).unwrap()).unwrap();
    let mut connection = harness_core::task_control::ControlConnection::connect(
        endpoint["port"].as_u64().unwrap() as u16,
        endpoint["token"].as_str().unwrap(),
        WAIT,
    )
    .unwrap();
    for (id, method, params) in [
        (
            1,
            "initialize",
            json!({"clientInfo":{"name":"owned-child-fixture","version":"1"},"capabilities":{"experimentalApi":true}}),
        ),
        (
            2,
            "thread/resume",
            json!({"threadId":endpoint["thread_id"]}),
        ),
        (
            3,
            "thread/goal/set",
            json!({"threadId":endpoint["thread_id"],"objective":"Preserve the owned proof and consume the child result","status":"paused"}),
        ),
    ] {
        connection
            .send(&json!({"id":id,"method":method,"params":params}), WAIT)
            .unwrap();
        let until = Instant::now() + WAIT;
        loop {
            assert!(
                Instant::now() < until,
                "native fixture control reply deadline"
            );
            if let Some(value) = connection.receive(Duration::from_millis(100)).unwrap()
                && value["id"] == id
            {
                assert!(value.get("error").is_none(), "{method}: {value}");
                break;
            }
        }
        if id == 1 {
            connection
                .send(&json!({"method":"initialized"}), WAIT)
                .unwrap();
        }
    }
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

fn exercise_user_stop(root: &Path, state: &Path, workspace: &Path) {
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
    harness_core::task_runtime::request_stop(state).unwrap();
    let until = Instant::now() + WAIT;
    while !state.join("closed.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    let closed: Value = serde_json::from_slice(
        &fs::read(state.join("closed.json")).expect("explicit stop must close the controller"),
    )
    .unwrap();
    assert_eq!(closed["reason"], "explicit stop");
    assert_eq!(closed["stopped"], true);
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !root.join("provider-2.json").exists(),
        "explicit stop must not start another model request"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt")).unwrap(),
        "one"
    );
    fs::write(
        root.join("native-entry-stopped.json"),
        serde_json::to_vec(&json!({"state":state,"stopped":true,"requests":1})).unwrap(),
    )
    .unwrap();
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

fn exercise_view_close(root: &Path, state: &Path, workspace: &Path, executable: &Path) {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if state.join("additional-views.json").is_file() && workspace.join("proof.txt").is_file() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt")).unwrap(),
        "one"
    );
    let views: Value =
        serde_json::from_slice(&fs::read(state.join("additional-views.json")).unwrap()).unwrap();
    let children = views["threads"].as_object().unwrap();
    assert_eq!(children.len(), 1, "close-restore uses one executor pane");
    let (thread_id, window) = children.iter().next().unwrap();
    let thread_id = thread_id.clone();
    let snapshot: harness_core::task_view::Snapshot =
        serde_json::from_value(window.clone()).unwrap();
    let before = snapshot.process;
    let providers = (1..=16)
        .filter(|n| root.join(format!("provider-{n}.json")).is_file())
        .count();
    close_conversation(&snapshot);
    let until = Instant::now() + WAIT;
    loop {
        let visibility: Value = fs::read(state.join("visibility.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        let hidden = visibility["conversations"]
            .as_object()
            .is_some_and(|map| map.get(&thread_id) == Some(&json!(false)));
        if hidden
            && visibility["pendingRequests"]
                .as_object()
                .is_some_and(|pending| pending.is_empty())
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "closed view must suspend that conversation; inspect visibility.json"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !root
            .join(format!("provider-{}.json", providers + 1))
            .exists(),
        "no new model request while the required view is closed"
    );
    let until = Instant::now() + Duration::from_secs(90);
    let restored = loop {
        assert!(
            Instant::now() < until,
            "closed conversation must restore a new visible window for the same thread"
        );
        if let Ok(bytes) = fs::read(state.join("additional-views.json")) {
            let views: Value = serde_json::from_slice(&bytes).unwrap();
            if let Ok(snapshot) = serde_json::from_value::<harness_core::task_view::Snapshot>(
                views["threads"][&thread_id].clone(),
            ) {
                if snapshot.process != before {
                    let user = harness_core::process_service::current_user().unwrap();
                    if snapshot.is_visible(executable, &user).unwrap_or(false) {
                        break snapshot;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        state.join("view-restore.json").is_file(),
        "controller must report the closed conversation"
    );
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !root
            .join(format!("provider-{}.json", providers + 1))
            .exists(),
        "restoring the view must not resume the model while its previous tool is still running"
    );
    fs::write(
        root.join("native-entry-view-restored.json"),
        serde_json::to_vec(&json!({
            "threadId": thread_id,
            "previous": before,
            "restored": restored.process
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(workspace.join("finish-tool"), "release owned command").unwrap();
}

fn reveal_conversation(state: &Path, executable: &Path) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };
    let until = Instant::now() + WAIT;
    while !state.join("view.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    let view: Value = serde_json::from_slice(&fs::read(state.join("view.json")).unwrap()).unwrap();
    let snapshot: harness_core::task_view::Snapshot =
        serde_json::from_value(view["window"].clone()).unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    unsafe {
        SetWindowPos(
            snapshot.window as _,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if snapshot.is_visible(executable, &user).unwrap_or(false) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn close_owned_views(state: &Path) {
    let mut windows = Vec::new();
    if let Ok(bytes) = fs::read(state.join("view.json")) {
        let view: Value = serde_json::from_slice(&bytes).unwrap();
        windows.push(view["window"].clone());
    }
    if let Ok(bytes) = fs::read(state.join("additional-views.json")) {
        let views: Value = serde_json::from_slice(&bytes).unwrap();
        if let Some(threads) = views["threads"].as_object() {
            windows.extend(threads.values().cloned());
        }
    }
    for window in windows {
        if let Ok(snapshot) = serde_json::from_value::<harness_core::task_view::Snapshot>(window) {
            let _ = close_conversation(&snapshot);
        }
    }
}

fn close_conversation(snapshot: &harness_core::task_view::Snapshot) {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        TerminateProcess,
    };
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            snapshot.process.pid,
        );
        if handle.is_null() {
            return;
        }
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut created = zero;
        let mut exit = zero;
        let mut kernel = zero;
        let mut user = zero;
        if GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user) == 0 {
            CloseHandle(handle);
            return;
        }
        let ticks = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
        if ticks != snapshot.process.creation_time {
            CloseHandle(handle);
            return;
        }
        let _ = TerminateProcess(handle, 0);
        CloseHandle(handle);
    }
}
