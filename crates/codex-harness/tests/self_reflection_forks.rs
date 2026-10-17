#![cfg(windows)]

//! Native, model-free checks of the two-stage self-reflection runner. The
//! manager, app-server child, native frontend double, durable state and lead
//! delivery endpoint are real; the two conversations and their rollout usage
//! are synthetic fixture responses. No model, subscription or installed CLI is
//! used.

#[path = "fixtures/control_endpoint.rs"]
mod control_endpoint;

use control_endpoint::{Answer, Bearer, Server, Wire};
use harness_core::console::{ConsoleOutcome, ConsoleSession, ConsoleSpec};
use harness_core::process::{Cancellation, CommandSpec, Deadline};
use harness_core::role_context::{self, Role};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

const EXECUTOR_SOURCE: &str = "fixture-executor-source";
const EXECUTOR_SOURCE_TURN: &str = "fixture-executor-source-turn";
const LEAD_SOURCE: &str = "fixture-lead-source";
const LEAD_SOURCE_TURN: &str = "fixture-lead-source-turn";
const EXECUTOR_FORK: &str = "fixture-executor-reflection-fork";
const EXECUTOR_FORK_TURN: &str = "fixture-executor-reflection-turn";
const LEAD_FORK: &str = "fixture-lead-reflection-fork";
const LEAD_FORK_TURN: &str = "fixture-lead-reflection-turn";
const LEAD_THREAD: &str = "fixture-real-lead";
const LEAD_TURN: &str = "fixture-live-lead-turn";
const MODEL: &str = "deepseek-flash";
const PROVIDER: &str = "deepseek";
const EFFORT: &str = "max";
const PROFILE: &str = "ds";
const TASK: &str = "review the accepted executor work without changing it";
const ACCEPTANCE: &str = "the reflection is retained and the synthesis reaches the real lead";
const TOKEN: &str = "9f0c1d2e3a4b5c6d7e8f90123456789abcdef0123456789abcdef0123456789a";
const EXECUTOR_RESULT: &str = "executor reflection: one evidenced remedy";
const LEAD_RESULT: &str = "lead reflection: synthesized remedy";
/// Every native feature channel that could expose an inherited mutation
/// surface outside the read-only filesystem sandbox. The reflection fork must
/// switch each one off in its own config.
const BLOCKED_FEATURES: [&str; 26] = [
    "features.apps",
    "features.enable_mcp_apps",
    "features.codex_apps_mcp_2026_07_28",
    "features.plugins",
    "features.remote_plugin",
    "features.browser_use",
    "features.browser_use_external",
    "features.browser_use_full_cdp_access",
    "features.browser_annotation_api",
    "features.in_app_browser",
    "features.computer_use",
    "features.image_generation",
    "features.artifact",
    "features.code_mode",
    "features.hooks",
    "features.goals",
    "features.multi_agent",
    "features.multi_agent_v2",
    "features.agent_message_board",
    "features.deferred_executor",
    "features.request_permissions_tool",
    "features.guardian_approval",
    "features.use_agent_identity",
    "features.send_message_to_user_async",
    "features.workspace_dependencies",
    "features.skill_mcp_dependency_install",
];

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"))
}

fn frontend_double() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"))
}

fn kit_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .canonicalize()
        .expect("canonical workspace root")
}

struct RoleFacts {
    executor_revision: String,
    lead_revision: String,
    executor_instructions: String,
    lead_instructions: String,
}

fn role_facts() -> &'static RoleFacts {
    static FACTS: OnceLock<RoleFacts> = OnceLock::new();
    FACTS.get_or_init(|| {
        let root = kit_root();
        let executor = role_context::resolve(&root, Role::ExecutorSelfReflection).unwrap();
        let lead = role_context::resolve(&root, Role::LeadSelfReflection).unwrap();
        RoleFacts {
            executor_revision: executor.revision,
            lead_revision: lead.revision,
            executor_instructions: executor.developer_instructions,
            lead_instructions: lead.developer_instructions,
        }
    })
}

/// The live identity of this test process, in the shape a managed lead
/// endpoint record carries for the app-server that serves it.
fn live_endpoint_process() -> Value {
    let program = std::env::current_exe().unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    let identity = harness_core::process_service::ServiceProcess::observe(
        std::process::id(),
        &program,
        0,
        &user,
    )
    .unwrap()
    .identity();
    json!({
        "pid": identity.pid,
        "creationTime": identity.creation_time,
        "program": program,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LeadMode {
    Delivered,
    Queued,
    Indeterminate,
    Unavailable,
}

#[derive(Default)]
struct LeadState {
    accepted: bool,
    client_id: Option<String>,
    text: Option<String>,
}

struct Workspace {
    _root: tempfile::TempDir,
    home: PathBuf,
    checkout: PathBuf,
    state: PathBuf,
    fixture_dir: PathBuf,
    request: PathBuf,
    endpoint: PathBuf,
    lead: Server,
    lead_state: Arc<Mutex<LeadState>>,
    lead_mode: LeadMode,
}

impl Workspace {
    fn new(name: &str, lead_mode: LeadMode) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join(format!("home-{name}"));
        let checkout = root.path().join(format!("checkout-{name}"));
        let state = root.path().join(format!("state-{name}"));
        let fixture_dir = root.path().join(format!("fixture-{name}"));
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&checkout).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&fixture_dir).unwrap();
        fs::write(
            home.join(format!("{PROFILE}.config.toml")),
            format!(
                "model = \"{MODEL}\"\nmodel_provider = \"{PROVIDER}\"\nmodel_reasoning_effort = \"{EFFORT}\"\n"
            ),
        )
        .unwrap();
        let launch = home.join("harness");
        fs::create_dir_all(&launch).unwrap();
        fs::write(
            launch.join("native-launch.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 2,
                "upstream": {"executable": frontend_double()}
            }))
            .unwrap(),
        )
        .unwrap();
        let checkout = checkout.canonicalize().unwrap();
        let state = state.canonicalize().unwrap();
        let token_path = root.path().join(format!("lead-{name}.token"));
        fs::write(&token_path, TOKEN).unwrap();
        let lead = Server::start(Bearer::File(token_path));
        let lead_state = Arc::new(Mutex::new(LeadState::default()));
        configure_lead(&lead, lead_mode, Arc::clone(&lead_state));
        let endpoint = root.path().join(format!("lead-{name}-endpoint.json"));
        if lead_mode != LeadMode::Unavailable {
            fs::write(
                &endpoint,
                serde_json::to_vec_pretty(&json!({
                    "schema": 1,
                    "port": lead.port,
                    "token": TOKEN,
                    "threadId": LEAD_THREAD,
                    "process": live_endpoint_process()
                }))
                .unwrap(),
            )
            .unwrap();
        }
        let request = root.path().join(format!("request-{name}.json"));
        Self {
            _root: root,
            home,
            checkout,
            state,
            fixture_dir,
            request,
            endpoint,
            lead,
            lead_state,
            lead_mode,
        }
    }

    fn request(&self, run: &str) -> Value {
        let facts = role_facts();
        json!({
            "schema": 1,
            "task": TASK,
            "run": run,
            "acceptance": ACCEPTANCE,
            "source": kit_root(),
            "codexHome": self.home,
            "launcher": fixture(),
            "stateDir": self.state,
            "executorSource": {"thread": EXECUTOR_SOURCE, "lastTurn": EXECUTOR_SOURCE_TURN},
            "leadSource": {"thread": LEAD_SOURCE, "lastTurn": LEAD_SOURCE_TURN},
            "executorStage": {
                "role": "executor-self-reflection",
                "revision": facts.executor_revision,
                "profile": PROFILE,
                "model": MODEL,
                "provider": PROVIDER,
                "effort": EFFORT,
                "checkout": self.checkout,
                "title": "executor reflection check"
            },
            "leadStage": {
                "role": "lead-self-reflection",
                "revision": facts.lead_revision,
                "profile": PROFILE,
                "model": MODEL,
                "provider": PROVIDER,
                "effort": EFFORT,
                "checkout": self.checkout,
                "title": "lead reflection check"
            },
            "leadReturn": {
                "endpoint": self.endpoint,
                "thread": LEAD_THREAD
            },
            "boundSeconds": 20
        })
    }
}

fn configure_lead(server: &Server, mode: LeadMode, state: Arc<Mutex<LeadState>>) {
    server.answer_fn(move |request| {
        let method = request["method"].as_str().unwrap_or_default();
        let params = &request["params"];
        match method {
            "initialize" => Answer::Result(json!({})),
            "thread/read" => {
                let state = state.lock().unwrap();
                let turns = if state.accepted && mode == LeadMode::Delivered {
                    json!([{
                        "id": LEAD_TURN,
                        "status": "completed",
                        "items": [{
                            "id": "lead-user-message",
                            "type": "userMessage",
                            "clientId": state.client_id.clone(),
                            "content": [{"type": "text", "text": state.text.clone()}]
                        }]
                    }])
                } else {
                    json!([])
                };
                Answer::Result(json!({
                    "thread": {"id": LEAD_THREAD, "status": {"type": "idle"}, "turns": turns}
                }))
            }
            "turn/start" | "turn/steer" => {
                let mut state = state.lock().unwrap();
                state.accepted = true;
                state.client_id = params["clientUserMessageId"].as_str().map(str::to_owned);
                state.text = params["input"][0]["text"].as_str().map(str::to_owned);
                if mode == LeadMode::Indeterminate {
                    Answer::Result(json!({}))
                } else if method == "turn/steer" {
                    Answer::Result(json!({"turnId": LEAD_TURN}))
                } else {
                    Answer::Result(json!({"turn": {"id": LEAD_TURN, "status": "inProgress"}}))
                }
            }
            _ => Answer::Result(json!({})),
        }
    });
}

fn wait_session(session: ConsoleSession) -> ConsoleOutcome {
    session
        .wait(
            Deadline::after(Duration::from_secs(70)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap()
}

fn wait_for_path(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} was not created within {timeout:?}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn spawn_chain(
    workspace: &Workspace,
    request: &Value,
    executor_outcome: &str,
    lead_outcome: &str,
    executor_result: &str,
    lead_result: &str,
) -> ConsoleSession {
    fs::write(
        &workspace.request,
        serde_json::to_vec_pretty(request).unwrap(),
    )
    .unwrap();
    let mut spec = CommandSpec::new(manager());
    spec.args = vec![
        "self-reflection".into(),
        "--request".into(),
        workspace.request.as_os_str().to_owned(),
    ];
    spec.env.insert(
        "CODEX_HOME".into(),
        Some(workspace.home.as_os_str().to_owned()),
    );
    spec.env.insert(
        "HARNESS_EXECUTOR_FIXTURE_MODE".into(),
        Some("control-app-server".into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_DIR".into(),
        Some(workspace.fixture_dir.as_os_str().to_owned()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_SOURCE_THREAD".into(),
        Some(EXECUTOR_SOURCE.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_SOURCE_TURN".into(),
        Some(EXECUTOR_SOURCE_TURN.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_LEAD_SOURCE_THREAD".into(),
        Some(LEAD_SOURCE.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_LEAD_SOURCE_TURN".into(),
        Some(LEAD_SOURCE_TURN.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_FORK_THREAD".into(),
        Some(EXECUTOR_FORK.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_FORK_TURN".into(),
        Some(EXECUTOR_FORK_TURN.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_LEAD_FORK_THREAD".into(),
        Some(LEAD_FORK.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_LEAD_FORK_TURN".into(),
        Some(LEAD_FORK_TURN.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_OUTCOME".into(),
        Some(executor_outcome.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_LEAD_OUTCOME".into(),
        Some(lead_outcome.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_RESULT".into(),
        Some(executor_result.into()),
    );
    spec.env.insert(
        "HARNESS_REFLECTION_FIXTURE_LEAD_RESULT".into(),
        Some(lead_result.into()),
    );
    spec.env.insert("HARNESS_REFLECTION_SESSION".into(), None);
    spec.env.insert("HARNESS_EXECUTOR_SESSION".into(), None);
    spec.env.insert("WT_SESSION".into(), None);
    ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap()
}

fn run_chain(
    workspace: &Workspace,
    request: &Value,
    executor_outcome: &str,
    lead_outcome: &str,
    executor_result: &str,
    lead_result: &str,
) -> ConsoleOutcome {
    wait_session(spawn_chain(
        workspace,
        request,
        executor_outcome,
        lead_outcome,
        executor_result,
        lead_result,
    ))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn stage(workspace: &Workspace, role: &str) -> Value {
    read_json(&workspace.state.join(format!("{role}.json")))
}

fn fixture_requests(workspace: &Workspace, stage: &str, method: &str) -> Vec<Value> {
    fs::read_to_string(workspace.fixture_dir.join("requests.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry["stage"] == stage && entry["method"] == method)
        .map(|entry| entry["request"].clone())
        .collect()
}

fn request_param(request: &Value, key: &str) -> Value {
    request["params"][key].clone()
}

/// The exact stage/method/thread sequence the fixture observed, in arrival
/// order. It distinguishes the pre-fork source catalogue read from the
/// post-fork verification and proves the fork was named before its first turn.
fn request_sequence(workspace: &Workspace) -> Vec<(String, String, String)> {
    fs::read_to_string(workspace.fixture_dir.join("requests.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .map(|entry| {
            (
                entry["stage"].as_str().unwrap_or_default().to_owned(),
                entry["method"].as_str().unwrap_or_default().to_owned(),
                entry["request"]["params"]["threadId"]
                    .as_str()
                    .unwrap_or("<process>")
                    .to_owned(),
            )
        })
        .collect()
}

fn assert_usage(stage: &Value, total: u64, cumulative: u64, response: &str, copied: &str) {
    assert_eq!(stage["usageKnown"], json!(true), "{stage}");
    assert_eq!(stage["usage"]["total_tokens"], json!(total), "{stage}");
    assert_eq!(
        stage["cumulativeThread"]["total_tokens"],
        json!(cumulative),
        "{stage}"
    );
    assert_eq!(stage["matchedResponseIds"], json!([response]), "{stage}");
    assert!(
        stage["copiedSourceTurns"]
            .as_array()
            .is_some_and(|turns| turns.iter().any(|turn| turn == copied)),
        "{stage}"
    );
}

#[test]
fn native_reflection_forks_preserve_sources_retain_results_and_charge_only_new_turns() {
    let workspace = Workspace::new("happy", LeadMode::Delivered);
    let request = workspace.request("run-happy");
    let finished = run_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert!(
        finished
            .transcript
            .contains("visible identity before model work"),
        "{}",
        finished.transcript
    );
    let executor = stage(&workspace, "executor-stage");
    let lead = stage(&workspace, "lead-stage");
    assert_eq!(executor["status"], "completed", "{executor}");
    assert_eq!(lead["status"], "completed", "{lead}");
    assert_eq!(executor["result"], EXECUTOR_RESULT, "{executor}");
    assert_eq!(lead["result"], LEAD_RESULT, "{lead}");
    assert_eq!(executor["turnId"], EXECUTOR_FORK_TURN, "{executor}");
    assert_eq!(lead["turnId"], LEAD_FORK_TURN, "{lead}");
    assert_eq!(executor["sourceThread"], EXECUTOR_SOURCE, "{executor}");
    assert_eq!(lead["sourceThread"], LEAD_SOURCE, "{lead}");
    assert_eq!(executor["fork"]["forkThread"], EXECUTOR_FORK, "{executor}");
    assert_eq!(lead["fork"]["forkThread"], LEAD_FORK, "{lead}");
    assert_eq!(executor["fork"]["goalCleared"], true, "{executor}");
    assert_eq!(executor["fork"]["sourcePreserved"], true, "{executor}");
    assert_eq!(lead["fork"]["goalCleared"], true, "{lead}");
    assert_eq!(lead["fork"]["sourcePreserved"], true, "{lead}");
    for stage in [&executor, &lead] {
        assert_eq!(
            stage["fork"]["suppressedMcpServers"],
            json!(["inherited-mutator"]),
            "{stage}"
        );
        assert_eq!(
            stage["fork"]["suppressedApps"],
            json!(["inherited-app"]),
            "{stage}"
        );
        assert_eq!(
            stage["fork"]["toolSurface"]["verified"],
            json!(true),
            "{stage}"
        );
        assert_eq!(
            stage["fork"]["toolSurface"]["mcpServers"],
            json!([{
                "name": "inherited-mutator",
                "runtimeStatus": "disabled",
                "tools": 0,
                "resources": 0,
                "resourceTemplates": 0,
            }]),
            "{stage}"
        );
        assert_eq!(
            stage["fork"]["toolSurface"]["apps"],
            json!([{
                "id": "inherited-app",
                "enabled": false,
                "accessible": false,
            }]),
            "{stage}"
        );
    }
    assert_eq!(executor["fork"]["sandbox"], "read-only", "{executor}");
    assert_eq!(
        executor["fork"]["cwd"],
        json!(workspace.checkout.to_string_lossy()),
        "{executor}"
    );
    assert_eq!(executor["fork"]["model"], MODEL, "{executor}");
    assert_eq!(executor["fork"]["modelProvider"], PROVIDER, "{executor}");
    assert_eq!(executor["fork"]["reasoningEffort"], EFFORT, "{executor}");
    assert_usage(
        &executor,
        120,
        570,
        "executor-self-reflection-new-turn",
        EXECUTOR_SOURCE_TURN,
    );
    assert_usage(
        &lead,
        240,
        920,
        "lead-self-reflection-new-turn",
        LEAD_SOURCE_TURN,
    );

    let executor_forks = fixture_requests(&workspace, "executor-self-reflection", "thread/fork");
    assert_eq!(executor_forks.len(), 1, "{executor_forks:?}");
    let executor_fork = request_param(&executor_forks[0], "threadId");
    assert_eq!(executor_fork, EXECUTOR_SOURCE, "{executor_forks:?}");
    let params = &executor_forks[0]["params"];
    assert_eq!(params["lastTurnId"], EXECUTOR_SOURCE_TURN);
    assert_eq!(params["deferGoalContinuation"], true);
    assert_eq!(params["cwd"], json!(workspace.checkout.to_string_lossy()));
    assert_eq!(params["sandbox"], "read-only");
    assert_eq!(params["approvalPolicy"], "never");
    assert_eq!(params["model"], MODEL);
    assert_eq!(params["modelProvider"], PROVIDER);
    assert_eq!(params["config"]["model_reasoning_effort"], EFFORT);
    assert_eq!(params["config"]["agents.enabled"], false);
    assert_eq!(params["config"]["web_search"], "disabled");
    assert_eq!(
        params["config"]["mcp_servers"],
        json!({"inherited-mutator": {"enabled": false}})
    );
    assert!(
        params["config"]
            .get("mcp_servers.inherited-mutator.enabled")
            .is_none(),
        "the fork override must use a literal table, not a dotted scalar: {params:?}"
    );
    assert_eq!(
        params["config"]["apps"],
        json!({"inherited-app": {"enabled": false}})
    );
    assert!(
        params["config"].get("apps.inherited-app.enabled").is_none(),
        "the fork override must use a literal table, not a dotted scalar: {params:?}"
    );
    for feature in BLOCKED_FEATURES {
        assert_eq!(
            params["config"][feature],
            json!(false),
            "the reflection fork left {feature} enabled: {params:?}"
        );
    }
    assert!(
        params["developerInstructions"]
            .as_str()
            .is_some_and(|text| text.contains("bounded, read-only retrospective")),
        "{params:?}"
    );
    let executor_instructions = params["developerInstructions"].as_str().unwrap();
    assert!(
        executor_instructions.contains(role_facts().executor_instructions.as_str()),
        "{params:?}"
    );
    assert!(
        !executor_instructions.contains(role_facts().lead_instructions.as_str()),
        "{params:?}"
    );
    assert!(
        params["config"]["skills.config"]
            .as_array()
            .is_some_and(|entries| entries.iter().any(|entry| {
                entry["enabled"] == false
                    && entry["path"].as_str().is_some_and(|path| {
                        path.contains("team-lead") && path.ends_with("SKILL.md")
                    })
            })),
        "{params:?}"
    );
    assert!(
        !params["config"]["skills.config"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path"].as_str().is_some_and(|path| path
                .contains("project-verification")
                && path.ends_with("SKILL.md"))),
        "{params:?}"
    );

    let lead_forks = fixture_requests(&workspace, "lead-self-reflection", "thread/fork");
    assert_eq!(lead_forks.len(), 1, "{lead_forks:?}");
    let lead_params = &lead_forks[0]["params"];
    assert_eq!(lead_params["threadId"], LEAD_SOURCE);
    assert_eq!(lead_params["lastTurnId"], LEAD_SOURCE_TURN);
    assert_eq!(lead_params["deferGoalContinuation"], true);
    assert_eq!(lead_params["sandbox"], "read-only");
    assert!(
        lead_params["developerInstructions"]
            .as_str()
            .is_some_and(|text| text.contains("bounded, read-only synthesis")),
        "{lead_params:?}"
    );
    assert!(
        lead_params["developerInstructions"]
            .as_str()
            .is_some_and(|text| text.contains("retained from a completed task")),
        "{lead_params:?}"
    );
    let lead_instructions = lead_params["developerInstructions"].as_str().unwrap();
    assert!(
        lead_instructions.contains(role_facts().lead_instructions.as_str()),
        "{lead_params:?}"
    );
    assert!(
        !lead_instructions.contains(role_facts().executor_instructions.as_str()),
        "{lead_params:?}"
    );

    for method in ["mcpServerStatus/list", "app/list"] {
        let reads = fixture_requests(&workspace, "executor-self-reflection", method);
        assert_eq!(reads.len(), 3, "{method}: {reads:?}");
        assert_eq!(
            request_param(&reads[0], "threadId"),
            Value::Null,
            "the pre-fork read uses the fresh process catalogue: {method}"
        );
        assert_eq!(
            request_param(&reads[1], "threadId"),
            EXECUTOR_FORK,
            "{method}"
        );
        assert_eq!(
            request_param(&reads[2], "threadId"),
            Value::Null,
            "the post-fork source read uses the fresh process catalogue: {method}"
        );
    }
    let sequence = request_sequence(&workspace);
    let position = |stage: &str, method: &str, thread: &str| {
        sequence
            .iter()
            .position(|(seen_stage, seen_method, seen_thread)| {
                seen_stage == stage && seen_method == method && seen_thread == thread
            })
            .unwrap_or_else(|| panic!("{stage} {method} {thread} was not observed: {sequence:?}"))
    };
    let source_catalogue = position(
        "executor-self-reflection",
        "mcpServerStatus/list",
        "<process>",
    );
    let fork_verification = position(
        "executor-self-reflection",
        "mcpServerStatus/list",
        EXECUTOR_FORK,
    );
    let fork_title = position("executor-self-reflection", "thread/name/set", EXECUTOR_FORK);
    let fork_turn = position("executor-self-reflection", "turn/start", EXECUTOR_FORK);
    assert!(
        source_catalogue < fork_verification
            && fork_verification < fork_title
            && fork_title < fork_turn,
        "source catalogue, fork verification, title and turn were not ordered: {sequence:?}"
    );

    let goal_clears = fixture_requests(&workspace, "executor-self-reflection", "thread/goal/clear");
    assert_eq!(goal_clears.len(), 1, "{goal_clears:?}");
    assert_eq!(goal_clears[0]["params"]["threadId"], EXECUTOR_FORK);
    let lead_turns = fixture_requests(&workspace, "lead-self-reflection", "turn/start");
    assert_eq!(lead_turns.len(), 1, "{lead_turns:?}");
    let lead_input = lead_turns[0]["params"]["input"][0]["text"]
        .as_str()
        .unwrap();
    assert!(lead_input.contains(EXECUTOR_RESULT), "{lead_input}");
    assert!(lead_input.contains(LEAD_SOURCE), "{lead_input}");
    assert!(lead_input.contains("EXECUTOR REFLECTION"), "{lead_input}");

    let delivery = read_json(&workspace.state.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "delivered", "{delivery}");
    assert_eq!(delivery["by"], "clientId", "{delivery}");
    assert!(delivery["envelopeSha256"].as_str().is_some(), "{delivery}");
    assert_eq!(workspace.lead_mode, LeadMode::Delivered);
    let lead_requests = workspace.lead.requests_for("turn/start");
    assert_eq!(lead_requests.len(), 1, "{lead_requests:?}");
    assert_eq!(
        lead_requests[0]["params"]["clientUserMessageId"],
        delivery["clientMessageId"]
    );
    assert!(
        lead_requests[0]["params"]["input"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains(EXECUTOR_RESULT) && text.contains(LEAD_RESULT)),
        "{lead_requests:?}"
    );
    assert!(
        workspace
            .lead_state
            .lock()
            .unwrap()
            .text
            .as_deref()
            .is_some_and(|text| text.contains(EXECUTOR_RESULT)),
        "lead thread did not own the delivered synthesis"
    );

    let title_requests =
        fixture_requests(&workspace, "executor-self-reflection", "thread/name/set");
    assert_eq!(title_requests.len(), 1, "{title_requests:?}");
    assert!(
        title_requests[0]["params"]["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("ERefl ds/")),
        "{title_requests:?}"
    );
    let lead_titles = fixture_requests(&workspace, "lead-self-reflection", "thread/name/set");
    assert_eq!(lead_titles.len(), 1, "{lead_titles:?}");
    assert!(
        lead_titles[0]["params"]["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("LRefl ds/")),
        "{lead_titles:?}"
    );
    assert!(
        executor["visibleTitle"].as_str().is_some_and(|title| title
            .contains(&format!("profile={PROFILE}"))
            && title.contains(&format!("provider={PROVIDER}"))
            && title.contains(&format!("model={MODEL}"))
            && title.contains(&format!("effort={EFFORT}"))
            && title.contains("task=")
            && title.contains("run=run-happy")),
        "{executor}"
    );
}

#[test]
fn reflection_marker_blocks_nested_harness_commands() {
    let output = Command::new(manager())
        .args(["self-reflection", "--help"])
        .env("HARNESS_REFLECTION_SESSION", "1")
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_ne!(output.status.code(), Some(0), "{text}");
    assert!(text.contains("HARNESS_REFLECTION_SESSION"), "{text}");
    assert!(text.contains("read-only"), "{text}");
}

fn request_counts(workspace: &Workspace) -> (usize, usize, usize, usize) {
    (
        fixture_requests(workspace, "executor-self-reflection", "thread/fork").len(),
        fixture_requests(workspace, "executor-self-reflection", "turn/start").len(),
        fixture_requests(workspace, "lead-self-reflection", "thread/fork").len(),
        fixture_requests(workspace, "lead-self-reflection", "turn/start").len(),
    )
}

fn assert_refused(finished: &ConsoleOutcome, needle: &str) {
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert!(
        finished.transcript.contains(needle),
        "{}",
        finished.transcript
    );
}

#[test]
fn no_finding_completion_is_retained_and_still_synthesized() {
    let workspace = Workspace::new("no-finding", LeadMode::Delivered);
    let request = workspace.request("run-no-finding");
    let finished = run_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        "NO FINDING",
        LEAD_RESULT,
    );
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let executor = stage(&workspace, "executor-stage");
    let lead = stage(&workspace, "lead-stage");
    assert_eq!(executor["status"], "completed", "{executor}");
    assert_eq!(executor["result"], "NO FINDING", "{executor}");
    assert_eq!(lead["status"], "completed", "{lead}");
    assert_eq!(lead["result"], LEAD_RESULT, "{lead}");
    let lead_turns = fixture_requests(&workspace, "lead-self-reflection", "turn/start");
    assert_eq!(lead_turns.len(), 1, "{lead_turns:?}");
    assert!(
        lead_turns[0]["params"]["input"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("NO FINDING")),
        "{lead_turns:?}"
    );
    let delivery = read_json(&workspace.state.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "delivered", "{delivery}");
}

#[test]
fn source_run_and_effective_model_drift_refuse_before_any_fork() {
    let source = Workspace::new("drift-source", LeadMode::Delivered);
    let source_request = source.request("run-drift-source");
    let finished = run_chain(
        &source,
        &source_request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let before = request_counts(&source);
    let mut drifted = source_request.clone();
    drifted["executorSource"]["lastTurn"] = json!("moved-source-turn");
    let refused = run_chain(
        &source,
        &drifted,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&refused, "request drifts from the retained chain");
    assert_eq!(request_counts(&source), before);

    let run = Workspace::new("drift-run", LeadMode::Delivered);
    let run_request = run.request("run-drift-run");
    let finished = run_chain(
        &run,
        &run_request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let before = request_counts(&run);
    let mut drifted = run_request.clone();
    drifted["run"] = json!("run-drift-run-2");
    let refused = run_chain(
        &run,
        &drifted,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&refused, "request drifts from the retained chain");
    assert_eq!(request_counts(&run), before);

    let model = Workspace::new("drift-model", LeadMode::Delivered);
    let model_request = model.request("run-drift-model");
    let finished = run_chain(
        &model,
        &model_request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let before = request_counts(&model);
    fs::write(
        model.home.join(format!("{PROFILE}.config.toml")),
        format!(
            "model = \"other-model\"\nmodel_provider = \"{PROVIDER}\"\nmodel_reasoning_effort = \"{EFFORT}\"\n"
        ),
    )
    .unwrap();
    let refused = run_chain(
        &model,
        &model_request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(
        &refused,
        "resolves model=other-model, not the declared deepseek-flash",
    );
    assert_eq!(request_counts(&model), before);
}

#[test]
fn later_source_work_preserves_the_selected_checkpoint() {
    let workspace = Workspace::new("source-later", LeadMode::Delivered);
    let request = workspace.request("run-source-later");
    let finished = run_chain(
        &workspace,
        &request,
        "source-later-work",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let executor = stage(&workspace, "executor-stage");
    assert_eq!(executor["status"], "completed", "{executor}");
    assert_eq!(executor["fork"]["sourcePreserved"], true, "{executor}");
    let later = format!("{EXECUTOR_SOURCE_TURN}-moved");
    for key in ["sourceTurnsBefore", "sourceTurnsAfter"] {
        assert_eq!(
            executor["fork"][key],
            json!([EXECUTOR_SOURCE_TURN, later.as_str()]),
            "{key}: {executor}"
        );
    }
    assert_eq!(stage(&workspace, "lead-stage")["status"], "completed");
}

#[test]
fn selected_source_checkpoint_drift_refuses_before_turn() {
    let workspace = Workspace::new("source-checkpoint-drift", LeadMode::Delivered);
    let request = workspace.request("run-source-checkpoint-drift");
    let finished = run_chain(
        &workspace,
        &request,
        "source-drift",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert!(
        finished.transcript.contains("selected source checkpoint")
            && finished.transcript.contains("changed during the fork"),
        "{}",
        finished.transcript
    );
    let executor = stage(&workspace, "executor-stage");
    assert_eq!(executor["status"], "failed", "{executor}");
    assert!(
        executor["error"].as_str().is_some_and(|error| {
            error.contains("selected source checkpoint")
                && error.contains("changed during the fork")
        }),
        "{executor}"
    );
    assert_eq!(request_counts(&workspace), (1, 0, 0, 0));
    assert!(
        !workspace.state.join("lead-stage.json").exists(),
        "the lead stage must not start after the selected checkpoint drifted"
    );
}

#[test]
fn dotted_mcp_server_name_is_disabled_without_touching_source_configuration() {
    let workspace = Workspace::new("dotted-mcp", LeadMode::Delivered);
    let config_path = workspace.home.join(format!("{PROFILE}.config.toml"));
    let source_config = format!(
        "model = \"{MODEL}\"\nmodel_provider = \"{PROVIDER}\"\nmodel_reasoning_effort = \"{EFFORT}\"\n\n[mcp_servers.inherited-mutator]\nenabled = true\ncommand = \"synthetic-mutator\"\n\n[mcp_servers.\"odd.name\"]\nenabled = true\ncommand = \"synthetic-dotted-mutator\"\n"
    );
    fs::write(&config_path, &source_config).unwrap();
    let before_config = fs::read(&config_path).unwrap();
    let request = workspace.request("run-dotted-mcp");
    let session = spawn_chain(
        &workspace,
        &request,
        "dotted-mutator-window",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    let endpoint_path = workspace
        .state
        .join("executor-self-reflection-native")
        .join("endpoint.json");
    let turn_path = workspace
        .fixture_dir
        .join("turn-executor-self-reflection.json");
    wait_for_path(&endpoint_path, Duration::from_secs(20));
    wait_for_path(&turn_path, Duration::from_secs(20));
    let endpoint = read_json(&endpoint_path);
    let port = u16::try_from(endpoint["port"].as_u64().unwrap()).unwrap();
    let token = endpoint["token"].as_str().unwrap();
    let mut wire = Wire::connect(port, token).unwrap();
    wire.call(
        1,
        "initialize",
        json!({"clientInfo": {"name": "dotted-native-check", "version": "1"}}),
    )
    .unwrap();
    let refusal = wire
        .call(
            2,
            "mcpServer/tool/call",
            json!({
                "threadId": EXECUTOR_FORK,
                "server": "odd.name",
                "tool": "write_owned_target",
                "arguments": {
                    "ownedTarget": workspace.checkout.join("owned-target.txt").to_string_lossy(),
                    "dispatch": workspace.checkout.join("dispatch.json").to_string_lossy(),
                    "board": workspace.checkout.join(".beads").to_string_lossy(),
                },
            }),
        )
        .unwrap();
    assert_eq!(refusal["error"]["code"], json!(-32000), "{refusal}");
    assert!(
        refusal["error"]["message"].as_str().is_some_and(|message| {
            message.contains("odd.name") && message.contains("no owned target")
        }),
        "{refusal}"
    );
    let source = wire
        .call(
            3,
            "mcpServerStatus/list",
            json!({"threadId": EXECUTOR_SOURCE, "detail": "toolsAndAuthOnly", "limit": 100}),
        )
        .unwrap();
    let source_servers = source["result"]["data"].as_array().unwrap();
    let dotted = source_servers
        .iter()
        .find(|server| server["name"] == "odd.name")
        .expect("the source keeps advertising odd.name");
    assert_eq!(dotted["runtimeStatus"], "connected", "{source}");
    assert!(
        dotted["tools"].get("write_owned_target").is_some(),
        "{source}"
    );
    fs::write(
        workspace
            .fixture_dir
            .join("release-executor-self-reflection.marker"),
        "release",
    )
    .unwrap();
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let attempts: Vec<Value> =
        fs::read_to_string(workspace.fixture_dir.join("mutator-attempts.jsonl"))
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
    assert_eq!(attempts.len(), 1, "{attempts:?}");
    assert_eq!(
        attempts[0]["stage"], "executor-self-reflection",
        "{attempts:?}"
    );
    assert_eq!(attempts[0]["threadId"], EXECUTOR_FORK, "{attempts:?}");
    assert_eq!(attempts[0]["server"], "odd.name", "{attempts:?}");
    assert_eq!(attempts[0]["tool"], "write_owned_target", "{attempts:?}");
    assert!(
        attempts[0]["disposition"]
            .as_str()
            .is_some_and(|value| value.contains("refused")),
        "{attempts:?}"
    );
    assert!(!workspace.checkout.join("owned-target.txt").exists());
    assert!(!workspace.checkout.join("dispatch.json").exists());
    assert!(!workspace.checkout.join(".beads").exists());
    assert_eq!(fs::read(&config_path).unwrap(), before_config);
    let forks = fixture_requests(&workspace, "executor-self-reflection", "thread/fork");
    assert_eq!(forks.len(), 1, "{forks:?}");
    let config = &forks[0]["params"]["config"];
    assert_eq!(
        config["mcp_servers"],
        json!({
            "inherited-mutator": {"enabled": false},
            "odd.name": {"enabled": false},
        }),
        "{config}"
    );
    assert!(
        config.get("mcp_servers.odd.name.enabled").is_none(),
        "a legal dotted server name must stay one literal table key: {config:?}"
    );
    let executor = stage(&workspace, "executor-stage");
    assert_eq!(executor["status"], "completed", "{executor}");
    assert_eq!(
        executor["fork"]["suppressedMcpServers"],
        json!(["inherited-mutator", "odd.name"]),
        "{executor}"
    );
    assert_eq!(executor["fork"]["sourcePreserved"], true, "{executor}");
    assert_eq!(
        executor["fork"]["toolSurface"]["mcpServers"],
        json!([
            {
                "name": "inherited-mutator",
                "runtimeStatus": "disabled",
                "tools": 0,
                "resources": 0,
                "resourceTemplates": 0,
            },
            {
                "name": "odd.name",
                "runtimeStatus": "disabled",
                "tools": 0,
                "resources": 0,
                "resourceTemplates": 0,
            }
        ]),
        "{executor}"
    );
}

#[test]
fn inherited_goal_failure_refuses_turn_and_does_not_replay() {
    let workspace = Workspace::new("goal-sticky", LeadMode::Delivered);
    let request = workspace.request("run-goal-sticky");
    let finished = run_chain(
        &workspace,
        &request,
        "goal-sticky",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert!(
        finished
            .transcript
            .contains("still reports a goal after thread/goal/clear"),
        "{}",
        finished.transcript
    );
    let executor = stage(&workspace, "executor-stage");
    assert_eq!(executor["status"], "failed", "{executor}");
    assert!(
        executor["error"]
            .as_str()
            .is_some_and(|error| error.contains("still reports a goal after thread/goal/clear")),
        "{executor}"
    );
    let before = request_counts(&workspace);
    assert_eq!(before.0, 1, "{before:?}");
    assert_eq!(before.1, 0, "{before:?}");
    assert_eq!(before.2, 0, "{before:?}");
    assert_eq!(
        fixture_requests(&workspace, "executor-self-reflection", "thread/goal/clear").len(),
        1
    );
    let replay = run_chain(
        &workspace,
        &request,
        "goal-sticky",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&replay, "already ended as failed");
    assert_eq!(request_counts(&workspace), before);
}

#[test]
fn interrupted_unknown_and_visibility_loss_outcomes_are_not_replayed() {
    let interrupted = Workspace::new("interrupted", LeadMode::Delivered);
    let request = interrupted.request("run-interrupted");
    let finished = run_chain(
        &interrupted,
        &request,
        "interrupted",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let executor = stage(&interrupted, "executor-stage");
    assert_eq!(executor["status"], "interrupted", "{executor}");
    assert!(
        executor["error"]
            .as_str()
            .is_some_and(|error| error.contains("interrupted")),
        "{executor}"
    );
    let before = request_counts(&interrupted);
    assert_eq!(before.1, 1, "{before:?}");
    let replay = run_chain(
        &interrupted,
        &request,
        "interrupted",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&replay, "already ended as interrupted");
    assert_eq!(request_counts(&interrupted), before);

    let unknown = Workspace::new("unknown", LeadMode::Delivered);
    let mut request = unknown.request("run-unknown");
    request["boundSeconds"] = json!(1);
    let finished = run_chain(
        &unknown,
        &request,
        "unknown-hold",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let executor = stage(&unknown, "executor-stage");
    assert_eq!(executor["status"], "unknown", "{executor}");
    assert!(
        executor["error"]
            .as_str()
            .is_some_and(|error| error.contains("no terminal native state")),
        "{executor}"
    );
    let before = request_counts(&unknown);
    assert_eq!(before.1, 1, "{before:?}");
    let replay = run_chain(
        &unknown,
        &request,
        "unknown-hold",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&replay, "already ended as unknown");
    assert_eq!(request_counts(&unknown), before);

    let visibility = Workspace::new("visibility", LeadMode::Delivered);
    let request = visibility.request("run-visibility");
    let finished = run_chain(
        &visibility,
        &request,
        "visibility-lost",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let executor = stage(&visibility, "executor-stage");
    assert_eq!(executor["status"], "visibility-lost", "{executor}");
    assert!(
        executor["error"].as_str().is_some_and(|error| {
            error.contains("reflection surface closed while the turn was in flight")
        }),
        "{executor}"
    );
    let before = request_counts(&visibility);
    assert_eq!(before.1, 1, "{before:?}");
    let replay = run_chain(
        &visibility,
        &request,
        "visibility-lost",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_refused(&replay, "already ended as visibility-lost");
    assert_eq!(request_counts(&visibility), before);
}

#[test]
fn completed_stages_are_reused_without_duplicate_forks_or_turns() {
    let workspace = Workspace::new("restart", LeadMode::Delivered);
    let request = workspace.request("run-restart");
    let first = run_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(first.outcome.exit_code, 0, "{}", first.transcript);
    let executor_before = stage(&workspace, "executor-stage");
    let lead_before = stage(&workspace, "lead-stage");
    let before = request_counts(&workspace);
    assert_eq!(before, (1, 1, 1, 1), "{before:?}");
    let second = run_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(second.outcome.exit_code, 0, "{}", second.transcript);
    assert_eq!(stage(&workspace, "executor-stage"), executor_before);
    assert_eq!(stage(&workspace, "lead-stage"), lead_before);
    assert_eq!(request_counts(&workspace), before);
    let delivery = read_json(&workspace.state.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "delivered", "{delivery}");
}

#[test]
fn queued_and_indeterminate_lead_returns_are_not_delivered_or_resent() {
    let queued = Workspace::new("queued", LeadMode::Queued);
    let request = queued.request("run-queued");
    let first = run_chain(
        &queued,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(first.outcome.exit_code, 3, "{}", first.transcript);
    let delivery = read_json(&queued.state.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "queued", "{delivery}");
    assert_eq!(
        fixture_requests(&queued, "lead-self-reflection", "turn/start").len(),
        1
    );
    let before = request_counts(&queued);
    let second = run_chain(
        &queued,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(second.outcome.exit_code, 3, "{}", second.transcript);
    let delivery = read_json(&queued.state.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "queued", "{delivery}");
    assert!(
        delivery["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("not resent")),
        "{delivery}"
    );
    assert_eq!(request_counts(&queued), before);

    let indeterminate = Workspace::new("indeterminate", LeadMode::Indeterminate);
    let request = indeterminate.request("run-indeterminate");
    let first = run_chain(
        &indeterminate,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(first.outcome.exit_code, 3, "{}", first.transcript);
    let delivery = read_json(&indeterminate.state.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "indeterminate", "{delivery}");
    assert_eq!(
        fixture_requests(&indeterminate, "lead-self-reflection", "turn/start").len(),
        1
    );
    let before = request_counts(&indeterminate);
    let second = run_chain(
        &indeterminate,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(second.outcome.exit_code, 3, "{}", second.transcript);
    let delivery = read_json(&indeterminate.state.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "indeterminate", "{delivery}");
    assert_eq!(request_counts(&indeterminate), before);
}

#[test]
fn missing_lead_return_endpoint_refuses_before_stages_and_retries_once() {
    let workspace = Workspace::new("lead-failed", LeadMode::Delivered);
    fs::remove_file(&workspace.endpoint).unwrap();
    let request = workspace.request("run-lead-failed");
    let first = run_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_ne!(first.outcome.exit_code, 0, "{}", first.transcript);
    assert!(
        first
            .transcript
            .contains("is unavailable before model work"),
        "{}",
        first.transcript
    );
    assert!(
        first
            .transcript
            .contains("no reflection fork or turn was started"),
        "{}",
        first.transcript
    );
    assert!(
        first
            .transcript
            .contains("codex-harness lead start --session THREAD"),
        "{}",
        first.transcript
    );
    assert_eq!(request_counts(&workspace), (0, 0, 0, 0));
    assert!(
        !workspace.state.join("executor-stage.json").exists(),
        "the missing lead endpoint must fail before the executor stage"
    );
    assert!(
        !workspace.state.join("lead-stage.json").exists(),
        "the missing lead endpoint must fail before the lead stage"
    );
    fs::write(
        &workspace.endpoint,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "port": workspace.lead.port,
            "token": TOKEN,
            "threadId": LEAD_THREAD,
            "process": live_endpoint_process()
        }))
        .unwrap(),
    )
    .unwrap();
    let second = run_chain(
        &workspace,
        &request,
        "complete",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(second.outcome.exit_code, 0, "{}", second.transcript);
    assert_eq!(request_counts(&workspace), (1, 1, 1, 1));
    let delivery = read_json(&workspace.state.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "delivered", "{delivery}");
    assert_eq!(workspace.lead.requests_for("turn/start").len(), 1);
}

#[test]
fn missing_new_turn_usage_stays_unknown_and_copied_source_is_not_charged() {
    let workspace = Workspace::new("missing-usage", LeadMode::Delivered);
    let request = workspace.request("run-missing-usage");
    let finished = run_chain(
        &workspace,
        &request,
        "missing-usage",
        "complete",
        EXECUTOR_RESULT,
        LEAD_RESULT,
    );
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let executor = stage(&workspace, "executor-stage");
    assert_eq!(executor["usageKnown"], false, "{executor}");
    assert_eq!(executor["usage"]["total_tokens"], Value::Null, "{executor}");
    assert_eq!(executor["usage"]["input_tokens"], json!(100), "{executor}");
    assert_eq!(
        executor["cumulativeThread"]["total_tokens"],
        json!(570),
        "{executor}"
    );
    assert_eq!(
        executor["copiedSourceTurns"],
        json!([EXECUTOR_SOURCE_TURN]),
        "{executor}"
    );
    assert!(
        executor["usageUnknownReason"]
            .as_str()
            .is_some_and(|reason| reason.contains("total_tokens")),
        "{executor}"
    );
    let lead = stage(&workspace, "lead-stage");
    assert_eq!(lead["usageKnown"], true, "{lead}");
    assert_eq!(lead["usage"]["total_tokens"], json!(240), "{lead}");
    assert_eq!(
        lead["cumulativeThread"]["total_tokens"],
        json!(920),
        "{lead}"
    );
    let delivery = read_json(&workspace.state.join("lead-delivery.json"));
    assert_eq!(delivery["state"], "delivered", "{delivery}");
}
