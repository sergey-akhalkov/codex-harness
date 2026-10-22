//! Minimal `codex app-server` child adapter for the ordinary control-backed
//! dispatch path.
//!
//! The ordinary route reserves a loopback port and spawns the installed
//! launcher as `app-server --listen ws://127.0.0.1:PORT --ws-auth
//! capability-token --ws-token-file FILE`. This module reuses the kit's owned
//! canned endpoint (`tests/fixtures/control_endpoint.rs`: real RFC6455 framing,
//! capability bearer, canned answers, notification push and request capture)
//! and serves exactly that port, answers one ordinary conversation to a
//! completed turn, and - when the caller asks for it through the documented
//! mode switches - leaves the slot's committed solution and the native rollout
//! a real client would have produced. It is a test double for the *child*
//! process only: the host, the relay, the frontend, the receipt and the
//! lifecycle are the real production path.
//!
//! Selection: `HARNESS_EXECUTOR_FIXTURE_MODE=control-app-server` on an
//! `app-server` invocation. The host forwards that explicit child-only mode
//! from `HARNESS_EXECUTOR_CHILD_FIXTURE_MODE`, so every other fixture mode
//! keeps its documented behavior for the same argv.
//!
//! Mode switches (all optional, all explicit):
//! - `HARNESS_IMPROVEMENT_FIXTURE_SESSION`: the thread identity to serve;
//! - `HARNESS_IMPROVEMENT_FIXTURE_FORK_SESSION`: when present, answer
//!   `thread/fork` for the existing native fork entrypoint with this exact
//!   selected identity; absent keeps the ordinary route fork-free;
//! - `HARNESS_IMPROVEMENT_FIXTURE_CONTROL`: what the controlled agent does
//!   once the host submitted its assignment: `complete` (default) finishes the
//!   turn, `protocol-defect` records an item without a type before the normal
//!   completion so the finite host must refuse it, `hold` keeps the turn open
//!   without ever completing it so the real stop owner can interrupt and end
//!   this run, and `fail` ends this child with a nonzero exit code before any
//!   completion. `recovery` fails the first submitted turn as an overloaded
//!   server with a blocked goal, then answers the host's status-only automatic
//!   resume and completes one continuation turn. With
//!   `HARNESS_IMPROVEMENT_FIXTURE_CONTROL_MARKER` the child records the
//!   assignment it observed before `hold` or `fail` takes effect;
//! - `HARNESS_IMPROVEMENT_FIXTURE_RECOVERY_RECORD`: append every recovery
//!   request, notification and goal state as JSON lines for a check;
//!   `HARNESS_IMPROVEMENT_FIXTURE_RECOVERY_ACK=drop-first` leaves the first
//!   automatic goal set unanswered without applying it, exercising the
//!   unconfirmed-acknowledgement path;
//!   `HARNESS_IMPROVEMENT_FIXTURE_RECOVERY_OBSERVE=fail` makes the goal read
//!   after the overload return a native error;
//! - `HARNESS_IMPROVEMENT_FIXTURE_MODEL` / `..._EFFORT`: override the model and
//!   reasoning effort recorded in the native rollout (the wrong-observation
//!   counterexamples); the default echoes the arm's installed configuration;
//! - `HARNESS_IMPROVEMENT_FIXTURE_SOLUTION_FILE` / `..._SOLUTION_TEXT`: the
//!   committed solution the controlled agent leaves in its bound slot
//!   (default `solution.txt` = `solved`); an empty text skips the commit.
//! - `HARNESS_IMPROVEMENT_FIXTURE_SCENARIO=measured-workflow` with
//!   `HARNESS_IMPROVEMENT_FIXTURE_ROLE=lead-frame|executor|lead-review|correction|lead-acceptance`:
//!   one explicit turn of a synthetic measured workflow. Only `executor` and
//!   `correction` commit the bound solution; every lead turn performs no
//!   executor commit. Each completed turn records its own native rollout and
//!   an append-only role receipt under `CODEX_HOME`, so repeated turns on one
//!   lead session retain distinct identities. Without the scenario both the
//!   message and the unconditional commit keep the legacy fixture behavior.
//! - `tool-probes/<arm>.json` beside the comparison run, when present: the
//!   controlled agent resolves each named unqualified command through the
//!   host-supplied environment and executes it only when the hit is under the
//!   request's allow prefix. It also runs the owner shell. Completion is
//!   pushed after that work so the host cannot end the child first.

#[path = "../../../tests/fixtures/control_endpoint.rs"]
mod control_endpoint;

use control_endpoint::{Answer, Bearer, Notifier, Server};
use serde_json::{Value, json};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

/// The thread identity the served conversation uses when the caller does not
/// name one.
const DEFAULT_SESSION: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f5";
const TURN: &str = "fixture-turn-1";
/// Item id and function-call `call_id`. It is not passed to the heavy command.
const HEAVY_CALL_ID: &str = "call_heavy_blocked_1";
const FINAL_MESSAGE: &str = "fixture turn completed; the committed solution is in the bound slot";
const INHERITED_MUTATOR_SERVER: &str = "inherited-mutator";
const INHERITED_DOTTED_SERVER: &str = "odd.name";
const INHERITED_MUTATOR_TOOL: &str = "write_owned_target";
const INHERITED_APP: &str = "inherited-app";
const WORKFLOW_SCENARIO: &str = "measured-workflow";
const WORKFLOW_RECORD_FILE: &str = "fixture-workflow-turns.jsonl";

/// One explicit synthetic turn of the measured workflow. The default fixture
/// is the legacy one-turn executor double; the synthetic mode names a role so
/// a lead turn can never commit executor work by sharing the executor fixture.
#[derive(Clone, Debug)]
struct FixtureTurn {
    role: Option<&'static str>,
    id: String,
    message_id: String,
    response_id: String,
    message: String,
    commits: bool,
}

impl FixtureTurn {
    fn legacy() -> Self {
        Self {
            role: None,
            id: TURN.to_owned(),
            message_id: "message-1".to_owned(),
            response_id: "response-1".to_owned(),
            message: FINAL_MESSAGE.to_owned(),
            commits: true,
        }
    }
}

fn synthetic_turn() -> io::Result<Option<FixtureTurn>> {
    let scenario = env::var("HARNESS_IMPROVEMENT_FIXTURE_SCENARIO").ok();
    let Some(scenario) = scenario.as_deref() else {
        return Ok(None);
    };
    if scenario != WORKFLOW_SCENARIO {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "unknown HARNESS_IMPROVEMENT_FIXTURE_SCENARIO {scenario:?}; the only synthetic scenario is {WORKFLOW_SCENARIO}"
            ),
        ));
    }
    let role = env::var("HARNESS_IMPROVEMENT_FIXTURE_ROLE").map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "HARNESS_IMPROVEMENT_FIXTURE_ROLE is required with the measured-workflow scenario",
        )
    })?;
    let mut turn = match role.as_str() {
        "lead-frame" => FixtureTurn {
            role: Some("lead-frame"),
            id: "fixture-turn-lead-frame".to_owned(),
            message_id: "message-lead-frame".to_owned(),
            response_id: "response-lead-frame".to_owned(),
            message:
                "lead-frame: framed the measured task and participants; no executor work committed"
                    .to_owned(),
            commits: false,
        },
        "executor" => FixtureTurn {
            role: Some("executor"),
            id: "fixture-turn-executor".to_owned(),
            message_id: "message-executor".to_owned(),
            response_id: "response-executor".to_owned(),
            message: "executor: completed the measured task and committed its solution".to_owned(),
            commits: true,
        },
        "lead-review" => FixtureTurn {
            role: Some("lead-review"),
            id: "fixture-turn-lead-review".to_owned(),
            message_id: "message-lead-review".to_owned(),
            response_id: "response-lead-review".to_owned(),
            message: "lead-review: reviewed the executor artifact; no executor work committed"
                .to_owned(),
            commits: false,
        },
        "correction" => FixtureTurn {
            role: Some("correction"),
            id: "fixture-turn-correction".to_owned(),
            message_id: "message-correction".to_owned(),
            response_id: "response-correction".to_owned(),
            message: "correction: applied the bounded correction requested by the lead".to_owned(),
            commits: true,
        },
        "lead-acceptance" => FixtureTurn {
            role: Some("lead-acceptance"),
            id: "fixture-turn-lead-acceptance".to_owned(),
            message_id: "message-lead-acceptance".to_owned(),
            response_id: "response-lead-acceptance".to_owned(),
            message: "lead-acceptance: accepted the reviewed result; no executor work committed"
                .to_owned(),
            commits: false,
        },
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "unknown HARNESS_IMPROVEMENT_FIXTURE_ROLE {role:?}; expected lead-frame, executor, lead-review, correction or lead-acceptance"
                ),
            ));
        }
    };
    if let Ok(result) = env::var("HARNESS_IMPROVEMENT_FIXTURE_RESULT") {
        if result.len() > 256 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "synthetic result exceeds its bound",
            ));
        }
        turn.message = result;
    }
    if let Ok(suffix) = env::var("HARNESS_IMPROVEMENT_FIXTURE_TURN_SUFFIX") {
        if suffix.is_empty()
            || suffix.len() > 64
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "synthetic turn suffix must be a bounded identity",
            ));
        }
        for id in [&mut turn.id, &mut turn.message_id, &mut turn.response_id] {
            id.push('-');
            id.push_str(&suffix);
        }
    }
    Ok(Some(turn))
}

/// Serves one `app-server` invocation. Returns the process exit code; the
/// caller's Job owns termination, exactly as it owns the real child.
pub fn run_app_server(args: &[std::ffi::OsString]) -> io::Result<i32> {
    if env::var_os("HARNESS_REFLECTION_SESSION").is_some() {
        return run_reflection_app_server(args);
    }
    // The dispatching side may direct this child to record the exact
    // invocation and role-marking environment it received, so the effective
    // role context is asserted from the child instead of inferred.
    if let Some(path) = env::var_os("HARNESS_EXECUTOR_FIXTURE_RECORD") {
        record_child(PathBuf::from(path))?;
    }
    let listen = option(args, "--listen").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the fixture app-server requires --listen ws://127.0.0.1:PORT",
        )
    })?;
    let port = listen
        .strip_prefix("ws://127.0.0.1:")
        .and_then(|text| text.parse::<u16>().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("the fixture app-server cannot serve {listen}"),
            )
        })?;
    let token_file = option(args, "--ws-token-file").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the fixture app-server requires --ws-token-file",
        )
    })?;
    let token_file = PathBuf::from(token_file);
    let session =
        env::var("HARNESS_IMPROVEMENT_FIXTURE_SESSION").unwrap_or_else(|_| DEFAULT_SESSION.into());
    let synthetic = synthetic_turn()?;
    let turn = synthetic.clone().unwrap_or_else(FixtureTurn::legacy);
    let route = installed_route(args)?;
    if control_mode() == "recovery" {
        return run_recovery_app_server(port, token_file, session, route);
    }
    let rollout_model =
        env::var("HARNESS_IMPROVEMENT_FIXTURE_MODEL").unwrap_or_else(|_| route.model.clone());
    let rollout_effort =
        env::var("HARNESS_IMPROVEMENT_FIXTURE_EFFORT").unwrap_or_else(|_| route.effort.clone());

    let session_for_thread = session.clone();
    let resumed_model = route.model.clone();
    let resumed_provider = route.provider.clone();
    let resumed_effort = route.effort.clone();
    let fork_session = env::var("HARNESS_IMPROVEMENT_FIXTURE_FORK_SESSION").ok();
    let server = Arc::new(Server::start_on_with(
        port,
        Bearer::File(token_file),
        |server| {
            server.answer("initialize", Answer::Result(json!({})));
            server.answer(
            "thread/start",
            Answer::Result(json!({
                "thread": {"id": session_for_thread.clone(), "cwd": env::current_dir().ok(), "turns": []},
                "model": route.model,
                "modelProvider": route.provider,
                "reasoningEffort": route.effort,
            })),
        );
            // The ordinary route materializes the named thread through
            // `thread/resume` before the native frontend attaches and refuses an
            // answer for another thread or another routing, so the double echoes
            // the identity it just started, exactly like a resumed real thread.
            server.answer(
            "thread/resume",
            Answer::Result(json!({
                "thread": {"id": session_for_thread.clone(), "cwd": env::current_dir().ok(), "turns": []},
                "model": resumed_model,
                "modelProvider": resumed_provider,
                "reasoningEffort": resumed_effort,
            })),
        );
            // The ordinary host observes the native frontend's own fork and
            // switches the same app-server only after its lifecycle transfer.
            // This canned answer exists solely for that already agreed native
            // selection path; without the explicit identity the route stays
            // fork-free.
            if let Some(fork_session) = fork_session {
                server.answer(
                    "thread/fork",
                    Answer::Result(json!({
                        "thread": {"id": fork_session.clone(), "cwd": env::current_dir().ok(), "turns": []},
                    })),
                );
            }
            server.answer("thread/name/set", Answer::Result(json!({})));
            server.answer(
                "turn/start",
                Answer::Result(json!({"turn": {"id": turn.id, "status": "inProgress"}})),
            );
            // Until the assignment is submitted this is the fresh empty thread
            // a pre-submission check must see (the host refuses a conversation
            // whose fresh thread already has turns). The agent thread below
            // replaces this answer with the completed turn as soon as
            // `turn/start` arrives, so every read after submission - in any
            // read order or count the attaching route uses - observes the
            // turn's final assistant message.
            server.answer(
                "thread/read",
                Answer::Result(json!({"thread": {"id": session_for_thread.clone(), "turns": []}})),
            );
            server.push(json!({"method": "thread/started", "params": {"thread": {"id": session_for_thread.clone()}}}));
            server.push(json!({
            "method": "turn/started",
            "params": {"threadId": session_for_thread.clone(), "turn": {"id": turn.id, "status": "inProgress"}},
        }));
            // Completion is pushed by the agent thread after its owned work,
            // including command-resolution evidence. Pushing it here lets the
            // host end the child before that work runs.
        },
    ));

    // The controlled agent's own work: once the host submitted its assignment
    // through `turn/start`, leave the committed solution and the native rollout
    // in the arm's own owned state, exactly where the observation owner reads
    // them. Nothing is written to the visible surface.
    let agent = thread::spawn({
        let server = Arc::clone(&server);
        let session = session.clone();
        move || {
            let until = Instant::now() + Duration::from_secs(120);
            while Instant::now() < until {
                if !server.requests_for("turn/start").is_empty() {
                    let control = control_mode();
                    if control != "complete" {
                        record_control_marker(&session, &control, &turn.id);
                    }
                    if control == "hold" {
                        // The submitted turn stays open: the thread record
                        // names it as the active turn, so the real stop owner
                        // addresses its interruption through it, and no
                        // completion is ever pushed - the run stays live until
                        // its own owner ends it.
                        server.answer(
                            "thread/read",
                            Answer::Result(json!({"thread": {
                                "id": session.clone(),
                                "turns": [{"id": turn.id, "status": "inProgress", "items": []}],
                            }})),
                        );
                        server.answer("turn/interrupt", Answer::Result(json!({})));
                        return;
                    }
                    if control == "fail" {
                        // A failing child: the host observes the closed
                        // connection and records the failure outcome while the
                        // slot, the binding and the partial work stay.
                        thread::sleep(Duration::from_millis(250));
                        std::process::exit(9);
                    }
                    // The thread is no longer fresh: the compatibility route
                    // reads it only here, while a native frontend read the
                    // empty thread before this assignment was submitted.
                    server.answer(
                        "thread/read",
                        Answer::Result(json!({"thread": {
                            "id": session.clone(),
                            "turns": [{
                                "id": turn.id,
                                "status": "completed",
                                "items": [{"id": turn.message_id, "type": "agentMessage", "text": turn.message}],
                            }],
                        }})),
                    );
                    record_selected_tools();
                    if control == "protocol-defect" {
                        // A real protocol deviation that does not itself change
                        // the lifecycle: the exact completion below must not
                        // turn this finite assignment into success.
                        server.push(json!({
                            "method": "item/completed",
                            "params": {
                                "threadId": session.clone(),
                                "item": {"id": "fixture-protocol-defect"},
                            },
                        }));
                    }
                    if turn.commits {
                        let _ = commit_solution();
                    }
                    let heavy = env::var("HARNESS_IMPROVEMENT_FIXTURE_HEAVY").is_ok();
                    let command_bound = heavy.then(|| run_owned_heavy(&server, &session));
                    let message_ms = unix_ms();
                    let _ = write_rollout(
                        &session,
                        &rollout_model,
                        &rollout_effort,
                        !heavy,
                        command_bound,
                        message_ms,
                        &turn,
                    );
                    if turn.role.is_some() {
                        let _ = record_workflow_turn(&session, &turn, message_ms);
                    }
                    if turn.role == Some("executor") {
                        // One known visible tool action for the synthetic
                        // executor turn. It carries a stable id, the exact
                        // command, status, exit code and bounded output so a
                        // reviewer packet can bind the real retained action
                        // rather than a lifecycle label.
                        server.push(json!({
                            "method": "item/started",
                            "params": {
                                "threadId": session.clone(),
                                "turnId": turn.id,
                                "startedAtMs": message_ms,
                                "item": {
                                    "id": "fixture-command-executor-1",
                                    "type": "commandExecution",
                                    "command": "git status --short",
                                    "status": "inProgress",
                                },
                            },
                        }));
                        server.push(json!({
                            "method": "item/completed",
                            "params": {
                                "threadId": session.clone(),
                                "turnId": turn.id,
                                "startedAtMs": message_ms,
                                "completedAtMs": message_ms,
                                "item": {
                                    "id": "fixture-command-executor-1",
                                    "type": "commandExecution",
                                    "command": "git status --short",
                                    "status": "completed",
                                    "exitCode": 0,
                                    "aggregatedOutput": " M solution.txt\n",
                                },
                            },
                        }));
                    }
                    server.push(json!({
                        "method": "item/started",
                        "params": {
                            "threadId": session.clone(),
                            "startedAtMs": message_ms,
                            "item": {"id": turn.message_id, "type": "agentMessage", "text": turn.message},
                        },
                    }));
                    server.push(json!({
                        "method": "item/completed",
                        "params": {
                            "threadId": session.clone(),
                            "startedAtMs": message_ms,
                            "completedAtMs": message_ms,
                            "item": {"id": turn.message_id, "type": "agentMessage", "text": turn.message},
                        },
                    }));
                    server.push(json!({
                        "method": "turn/completed",
                        "params": {"threadId": session.clone(), "turn": {"id": turn.id, "status": "completed"}},
                    }));
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
    });

    // The real app-server serves until its owning host ends it; this double
    // does the same.
    agent.join().ok();
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

const RECOVERY_TURN: &str = "fixture-turn-overloaded";
const RECOVERY_CONTINUATION_TURN: &str = "fixture-turn-recovered";
const RECOVERY_CREATED_AT: i64 = 1_760_000_000_000;
const RECOVERY_BUDGET: i64 = 10_000;

struct RecoveryState {
    thread: String,
    status: &'static str,
    tokens_used: i64,
    automatic_sets: u32,
    user_sets: u32,
    turn_starts: u32,
    failed: bool,
    recovered_turn: bool,
    fail_observation: bool,
    record: Option<PathBuf>,
}

impl RecoveryState {
    fn goal(&self) -> Value {
        json!({
            "threadId": self.thread,
            "objective": "ordinary recovery fixture objective",
            "status": self.status,
            "createdAt": RECOVERY_CREATED_AT,
            "tokenBudget": RECOVERY_BUDGET,
            "tokensUsed": self.tokens_used,
        })
    }

    fn record(&self, value: Value) {
        let Some(path) = self.record.as_ref() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{value}");
        }
    }
}

/// One model-free ordinary conversation with an overloaded first turn and a
/// native goal continuation. All state remains in this fake app-server; the
/// host, relay and recovery state machine are the real production path.
fn run_recovery_app_server(
    port: u16,
    token_file: PathBuf,
    thread: String,
    route: InstalledRoute,
) -> io::Result<i32> {
    let drop_ack = env::var("HARNESS_IMPROVEMENT_FIXTURE_RECOVERY_ACK")
        .is_ok_and(|value| value == "drop-first");
    let fail_observation =
        env::var("HARNESS_IMPROVEMENT_FIXTURE_RECOVERY_OBSERVE").is_ok_and(|value| value == "fail");
    let record = env::var_os("HARNESS_IMPROVEMENT_FIXTURE_RECOVERY_RECORD").map(PathBuf::from);
    let state = Arc::new(Mutex::new(RecoveryState {
        thread: thread.clone(),
        status: "blocked",
        tokens_used: 20,
        automatic_sets: 0,
        user_sets: 0,
        turn_starts: 0,
        failed: false,
        recovered_turn: false,
        fail_observation,
        record,
    }));
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .record(json!({"kind": "fixture-start", "pid": std::process::id()}));
    let _server = Server::start_on_with(port, Bearer::File(token_file), |server| {
        let state = Arc::clone(&state);
        let notifier = server.notifier();
        server.answer_fn(move |request| {
            recovery_answer(&state, &notifier, &thread, &route, drop_ack, request)
        });
    });
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn recovery_answer(
    state: &Arc<Mutex<RecoveryState>>,
    notifier: &Notifier,
    thread: &str,
    route: &InstalledRoute,
    drop_ack: bool,
    request: &Value,
) -> Answer {
    let mut state = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let method = request["method"].as_str().unwrap_or_default();
    let params = &request["params"];
    state.record(json!({
        "kind": "request",
        "atMs": unix_ms(),
        "method": method,
        "params": params,
    }));
    match method {
        "initialize" | "initialized" => Answer::Result(json!({})),
        "thread/start" => {
            notifier.push(json!({
                "method": "thread/started",
                "params": {"thread": {"id": thread}},
            }));
            Answer::Result(json!({
                "thread": {"id": thread, "cwd": env::current_dir().ok(), "turns": []},
                "model": route.model,
                "modelProvider": route.provider,
                "reasoningEffort": route.effort,
            }))
        }
        "thread/resume" => Answer::Result(json!({
            "thread": {"id": thread, "cwd": env::current_dir().ok(), "turns": []},
            "model": route.model,
            "modelProvider": route.provider,
            "reasoningEffort": route.effort,
        })),
        "thread/read" => {
            let mut turns = Vec::new();
            if state.failed {
                turns.push(json!({
                    "id": RECOVERY_TURN,
                    "status": "failed",
                    "items": [],
                    "error": {
                        "message": "synthetic server overload",
                        "codexErrorInfo": "serverOverloaded",
                    },
                }));
            }
            if state.recovered_turn {
                turns.push(json!({
                    "id": RECOVERY_CONTINUATION_TURN,
                    "status": "completed",
                    "items": [{
                        "id": "fixture-recovered-message",
                        "type": "agentMessage",
                        "text": "recovered continuation completed",
                    }],
                }));
            }
            Answer::Result(json!({"thread": {
                "id": thread,
                "cwd": env::current_dir().ok(),
                "turns": turns,
            }}))
        }
        "thread/name/set" => Answer::Result(json!({})),
        "turn/start" => {
            state.turn_starts += 1;
            if state.turn_starts != 1 {
                return Answer::Error(json!({
                    "message": "the recovery fixture refuses a resent prompt or turn",
                }));
            }
            notifier.push(json!({
                "method": "turn/started",
                "params": {
                    "threadId": thread,
                    "turn": {"id": RECOVERY_TURN, "status": "inProgress"},
                },
            }));
            let failed = json!({
                "method": "turn/completed",
                "params": {
                    "threadId": thread,
                    "turn": {
                        "id": RECOVERY_TURN,
                        "status": "failed",
                        "error": {
                            "message": "synthetic server overload",
                            "codexErrorInfo": "serverOverloaded",
                        },
                    },
                },
            });
            state.failed = true;
            notifier.push(failed.clone());
            state.record(json!({
                "kind": "notification",
                "atMs": unix_ms(),
                "notification": failed,
            }));
            Answer::Result(json!({
                "turn": {"id": RECOVERY_TURN, "status": "inProgress"},
            }))
        }
        "thread/goal/get" => {
            if state.fail_observation {
                state.record(json!({
                    "kind": "observation-failure",
                    "atMs": unix_ms(),
                }));
                return Answer::Error(json!({
                    "message": "synthetic native goal observation failure",
                }));
            }
            let goal = state.goal();
            state.record(json!({
                "kind": "goal",
                "atMs": unix_ms(),
                "origin": "read",
                "goal": goal,
            }));
            Answer::Result(json!({"goal": goal}))
        }
        "thread/goal/set" => {
            if params["threadId"].as_str() != Some(thread) {
                return Answer::Error(json!({"message": "the goal names another thread"}));
            }
            match params["origin"].as_str() {
                Some("automatic") => {
                    if params["status"] != "active" {
                        return Answer::Error(json!({
                            "message": "automatic recovery must set only active status",
                        }));
                    }
                    if ["objective", "tokenBudget", "model", "provider"]
                        .iter()
                        .any(|key| params.get(key).is_some())
                    {
                        return Answer::Error(json!({
                            "message": "automatic recovery carried more than its status",
                        }));
                    }
                    state.automatic_sets += 1;
                    if drop_ack && state.automatic_sets == 1 {
                        state.record(json!({
                            "kind": "automatic-set",
                            "atMs": unix_ms(),
                            "acknowledged": false,
                        }));
                        return Answer::Silence;
                    }
                    state.status = "active";
                    let active = state.goal();
                    let updated = json!({
                        "method": "thread/goal/updated",
                        "params": {"threadId": thread, "goal": active},
                    });
                    notifier.push(updated.clone());
                    state.record(json!({
                        "kind": "automatic-set",
                        "atMs": unix_ms(),
                        "acknowledged": true,
                    }));
                    state.record(json!({
                        "kind": "notification",
                        "atMs": unix_ms(),
                        "notification": updated,
                    }));
                    let started = json!({
                        "method": "turn/started",
                        "params": {
                            "threadId": thread,
                            "turn": {"id": RECOVERY_CONTINUATION_TURN, "status": "inProgress"},
                        },
                    });
                    notifier.push(started.clone());
                    state.record(json!({
                        "kind": "notification",
                        "atMs": unix_ms(),
                        "notification": started,
                    }));
                    state.recovered_turn = true;
                    state.tokens_used = 40;
                    state.status = "complete";
                    let complete = state.goal();
                    let updated = json!({
                        "method": "thread/goal/updated",
                        "params": {"threadId": thread, "goal": complete},
                    });
                    notifier.push(updated.clone());
                    state.record(json!({
                        "kind": "notification",
                        "atMs": unix_ms(),
                        "notification": updated,
                    }));
                    state.record(json!({
                        "kind": "goal",
                        "atMs": unix_ms(),
                        "origin": "automatic",
                        "goal": complete,
                    }));
                    let completed = json!({
                        "method": "turn/completed",
                        "params": {
                            "threadId": thread,
                            "turn": {"id": RECOVERY_CONTINUATION_TURN, "status": "completed"},
                        },
                    });
                    notifier.push(completed.clone());
                    state.record(json!({
                        "kind": "notification",
                        "atMs": unix_ms(),
                        "notification": completed,
                    }));
                    // The acknowledgement reports the status this request
                    // applied; the continuation's later notifications follow.
                    Answer::Result(json!({"goal": active}))
                }
                Some("user") => {
                    if params["status"] != "paused" {
                        return Answer::Error(json!({
                            "message": "the fixture user mutation pauses the goal",
                        }));
                    }
                    state.user_sets += 1;
                    state.status = "paused";
                    let goal = state.goal();
                    let updated = json!({
                        "method": "thread/goal/updated",
                        "params": {"threadId": thread, "goal": goal},
                    });
                    notifier.push(updated.clone());
                    state.record(json!({
                        "kind": "notification",
                        "atMs": unix_ms(),
                        "notification": updated,
                    }));
                    state.record(json!({
                        "kind": "goal",
                        "atMs": unix_ms(),
                        "origin": "user",
                        "goal": goal,
                    }));
                    Answer::Result(json!({"goal": goal}))
                }
                _ => Answer::Error(json!({
                    "message": "the recovery fixture supports automatic or user goal mutations",
                })),
            }
        }
        _ => Answer::Result(json!({})),
    }
}

/// State of one model-free self-reflection app-server. It answers the exact
/// source, fork, goal and turn contract the native runner uses, without a
/// model or installed CLI, and leaves the identities and rollout accounting a
/// check can inspect.
struct ReflectionState {
    stage: String,
    fixture_dir: PathBuf,
    home: PathBuf,
    executor_source: String,
    executor_source_turn: String,
    lead_source: String,
    lead_source_turn: String,
    fork_thread: String,
    fork_turn: String,
    investigations: u64,
    outcome: String,
    result: String,
    model: String,
    provider: String,
    effort: String,
    fork_cwd: Option<String>,
    fork_model: Option<String>,
    fork_provider: Option<String>,
    fork_effort: Option<String>,
    fork_config: Option<Value>,
    source_goal: Value,
    forked: bool,
    goal_cleared: bool,
    turn_started: bool,
}

impl ReflectionState {
    fn source(&self) -> (&str, &str) {
        if self.stage == "executor-self-reflection" {
            (&self.executor_source, &self.executor_source_turn)
        } else {
            (&self.lead_source, &self.lead_source_turn)
        }
    }

    /// Restores the exact fork slot and submitted-turn marker a previous
    /// fixture process wrote. Reconciliation after a restart must see the same
    /// conversation, not a fresh in-memory one.
    fn restore_markers(&mut self) {
        let fork_path = self.fixture_dir.join(format!("fork-{}.json", self.stage));
        if let Ok(bytes) = fs::read(&fork_path)
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            && value["forkThread"].as_str() == Some(self.fork_thread.as_str())
        {
            self.forked = true;
            self.fork_cwd = value["cwd"].as_str().map(str::to_owned);
            self.fork_model = value["model"].as_str().map(str::to_owned);
            self.fork_provider = value["modelProvider"].as_str().map(str::to_owned);
            self.fork_effort = value["reasoningEffort"].as_str().map(str::to_owned);
            self.fork_config = value.get("config").cloned();
        }
        let turn_path = self.fixture_dir.join(format!("turn-{}.json", self.stage));
        if let Ok(bytes) = fs::read(&turn_path)
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            && value["forkThread"].as_str() == Some(self.fork_thread.as_str())
            && value["turnId"].as_str() == Some(self.fork_turn.as_str())
            && value["started"] == json!(true)
        {
            self.turn_started = true;
        }
    }

    fn record_fork(&mut self, request: &Value) {
        self.fork_cwd = request["cwd"].as_str().map(str::to_owned);
        self.fork_model = request["model"].as_str().map(str::to_owned);
        self.fork_provider = request["modelProvider"].as_str().map(str::to_owned);
        self.fork_effort = request["config"]["model_reasoning_effort"]
            .as_str()
            .map(str::to_owned);
        self.fork_config = request.get("config").cloned();
        let value = json!({
            "forkThread": self.fork_thread,
            "cwd": self.fork_cwd,
            "model": self.fork_model,
            "modelProvider": self.fork_provider,
            "reasoningEffort": self.fork_effort,
            "config": self.fork_config,
        });
        let _ = fs::write(
            self.fixture_dir.join(format!("fork-{}.json", self.stage)),
            serde_json::to_vec_pretty(&value).unwrap_or_default(),
        );
    }

    fn record_turn(&self) {
        let value = json!({
            "forkThread": self.fork_thread,
            "turnId": self.fork_turn,
            "started": true,
            "outcome": self.outcome,
        });
        let _ = fs::write(
            self.fixture_dir.join(format!("turn-{}.json", self.stage)),
            serde_json::to_vec_pretty(&value).unwrap_or_default(),
        );
    }

    /// Records the final-looking assistant item this fixture exposed while the
    /// native turn stayed in progress. The receipt is fixture evidence only:
    /// the consumer still has to establish native completion before treating
    /// the text as a retained outcome.
    fn record_visible_text(&self) {
        let item = json!({
            "id": format!("{}-message", self.fork_turn),
            "type": "agentMessage",
            "text": self.result,
        });
        let value = json!({
            "schema": 1,
            "stage": self.stage,
            "thread": self.fork_thread,
            "turn": self.fork_turn,
            "turnStatus": "inProgress",
            "events": ["item/started", "item/completed"],
            "item": item,
            "nativeCompletion": false,
        });
        let _ = fs::write(
            self.fixture_dir
                .join(format!("visible-{}.json", self.stage)),
            serde_json::to_vec_pretty(&value).unwrap_or_default(),
        );
    }

    fn source_record(&self) -> Value {
        let (thread, turn) = self.source();
        let later = matches!(self.outcome.as_str(), "source-drift" | "source-later-work");
        let last = if later {
            format!("{turn}-moved")
        } else {
            turn.to_owned()
        };
        let checkpoint_status = if self.outcome == "source-checkpoint-changed" && self.forked {
            "inProgress"
        } else {
            "completed"
        };
        let checkpoint_text = if self.outcome == "source-drift" && self.forked {
            "source checkpoint rewritten after the fork".to_owned()
        } else {
            format!("copied source work for {thread}")
        };
        let mut turns = vec![reflection_turn(
            turn,
            checkpoint_status,
            &checkpoint_text,
            None,
        )];
        if later {
            turns.push(reflection_turn(
                &last,
                "completed",
                "source conversation moved after the declared boundary",
                None,
            ));
        }
        json!({
            "id": thread,
            "status": {"type": "idle"},
            "turns": turns,
        })
    }

    fn fork_record(&self) -> Value {
        let (_, source_turn) = self.source();
        let mut turns = vec![reflection_turn(
            source_turn,
            "completed",
            "copied source work retained by the fork",
            None,
        )];
        if self.turn_started {
            let status = match self.outcome.as_str() {
                "complete"
                | "missing-usage"
                | "mutator-window"
                | "dotted-mutator-window"
                | "source-later-work" => "completed",
                "interrupted" => "interrupted",
                "failed" => "failed",
                _ => "inProgress",
            };
            let text = if status == "completed" {
                Some(self.result.as_str())
            } else {
                None
            };
            let error = (status == "failed").then(
                || json!({"message": "the synthetic reflection turn failed before a result"}),
            );
            let mut turn = reflection_turn(&self.fork_turn, status, text.unwrap_or(""), error);
            if status == "inProgress" && self.outcome == "unknown-hold" {
                turn["items"] = json!([{
                    "id": format!("{}-message", self.fork_turn),
                    "type": "agentMessage",
                    "text": self.result,
                }]);
            }
            turns.push(turn);
        }
        json!({
            "id": self.fork_thread,
            "cwd": self.fork_cwd.clone().unwrap_or_else(|| self.home.to_string_lossy().into_owned()),
            "status": {"type": "idle"},
            "turns": turns,
        })
    }

    fn route(&self, params: &Value) -> Value {
        let model = if self.outcome == "model-drift" {
            json!("wrong-reflection-model")
        } else {
            params["model"].clone()
        };
        let sandbox = if self.outcome == "sandbox-drift" {
            json!("workspace-write")
        } else {
            params["sandbox"].clone()
        };
        json!({
            "model": model,
            "modelProvider": params["modelProvider"].clone(),
            "reasoningEffort": params["config"]["model_reasoning_effort"].clone(),
            "cwd": params["cwd"].clone(),
            "sandbox": sandbox,
        })
    }

    /// Whether the recorded fork config disabled the inherited mutator, as the
    /// real app-server would apply a fork-local override.
    fn fork_mcp_disabled(&self, name: &str) -> bool {
        self.fork_config
            .as_ref()
            .and_then(|config| config.get("mcp_servers"))
            .and_then(|servers| servers.get(name))
            .and_then(|server| server.get("enabled"))
            .and_then(Value::as_bool)
            == Some(false)
    }

    /// Whether the recorded fork config disabled the inherited App.
    fn fork_app_disabled(&self) -> bool {
        self.fork_config
            .as_ref()
            .and_then(|config| config.get("apps"))
            .and_then(|apps| apps.get(INHERITED_APP))
            .and_then(|app| app.get("enabled"))
            .and_then(Value::as_bool)
            == Some(false)
    }

    /// The source MCP catalogue always advertises the inherited mutator. A
    /// fork receives the same server record only while its config leaves it
    /// enabled; the default fork record is a disabled server with empty
    /// inventories.
    fn mcp_catalogue(&self, thread: Option<&str>) -> Answer {
        let (source, _) = self.source();
        if thread.is_some_and(|thread| thread != source && thread != self.fork_thread) {
            return Answer::Error(json!({
                "message": format!("synthetic reflection has no MCP owner {}", thread.unwrap_or_default())
            }));
        }
        if thread == Some(self.fork_thread.as_str()) && self.outcome == "mcp-catalogue-unavailable"
        {
            return Answer::Error(json!({
                "message": "the synthetic fork MCP catalogue is unavailable"
            }));
        }
        // The dotted scenario advertises a second server whose name contains a
        // legal dot. The fork-local override must suppress it by its literal
        // name; the source catalogue keeps it enabled.
        let names: &[&str] = if self.outcome == "dotted-mutator-window" {
            &[INHERITED_MUTATOR_SERVER, INHERITED_DOTTED_SERVER]
        } else {
            &[INHERITED_MUTATOR_SERVER]
        };
        let mut servers = Vec::with_capacity(names.len());
        for name in names {
            let exposed = thread.is_none()
                || thread == Some(source)
                || self.outcome == "mcp-exposed"
                || (thread == Some(self.fork_thread.as_str()) && !self.fork_mcp_disabled(name));
            let mut server = json!({
                "name": name,
                "authStatus": "unsupported",
                "runtimeStatus": if exposed { "connected" } else { "disabled" },
                "resources": [],
                "resourceTemplates": [],
                "tools": {},
            });
            if exposed {
                server["tools"] = json!({
                    INHERITED_MUTATOR_TOOL: {
                        "name": INHERITED_MUTATOR_TOOL,
                        "description": "writes an owned target, dispatches, and mutates the board",
                        "inputSchema": {"type": "object"},
                    }
                });
            }
            servers.push(server);
        }
        Answer::Result(json!({"data": servers}))
    }

    /// The source Apps catalogue always advertises one enabled App. The fork
    /// sees it disabled only when its config names the per-App override.
    fn app_catalogue(&self, thread: Option<&str>) -> Answer {
        let (source, _) = self.source();
        if thread.is_some_and(|thread| thread != source && thread != self.fork_thread) {
            return Answer::Error(json!({
                "message": format!("synthetic reflection has no Apps owner {}", thread.unwrap_or_default())
            }));
        }
        let exposed = thread.is_none()
            || thread == Some(source)
            || self.outcome == "app-exposed"
            || (thread == Some(self.fork_thread.as_str()) && !self.fork_app_disabled());
        Answer::Result(json!({
            "data": [{
                "id": INHERITED_APP,
                "name": "Inherited mutating app",
                "isEnabled": exposed,
                "isAccessible": exposed,
            }]
        }))
    }

    /// Records one attempt to invoke the inherited mutator and refuses before
    /// any owned target, dispatch or board mutation exists. The durable record
    /// is the evidence a check reads; the source conversation is untouched.
    fn refuse_mutator_call(&self, params: &Value) -> Answer {
        let server = params["server"].as_str().unwrap_or_default();
        let attempt = json!({
            "stage": self.stage,
            "threadId": params["threadId"],
            "server": server,
            "tool": params["tool"],
            "arguments": params["arguments"],
            "disposition": "refused: inherited mutator is disabled for the reflection fork",
        });
        fs::create_dir_all(&self.fixture_dir).ok();
        if let Ok(mut file) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.fixture_dir.join("mutator-attempts.jsonl"))
        {
            let _ = writeln!(file, "{attempt}");
        }
        Answer::Error(json!({
            "code": -32000,
            "message": format!("MCP server {server} is disabled for this reflection thread; no owned target, dispatch or board mutation was performed")
        }))
    }
}

fn reflection_turn(id: &str, status: &str, text: &str, error: Option<Value>) -> Value {
    let items = if status == "completed" {
        json!([{"id": format!("{id}-message"), "type": "agentMessage", "text": text}])
    } else {
        json!([])
    };
    let mut turn = json!({"id": id, "status": status, "items": items});
    if let Some(error) = error {
        turn["error"] = error;
    }
    turn
}

fn reflection_env(name: &str, fallback: &str) -> String {
    env::var(name).unwrap_or_else(|_| fallback.to_owned())
}

fn reflection_state() -> ReflectionState {
    let stage = reflection_env("HARNESS_REFLECTION_STAGE", "reflection-fixture");
    let home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let fixture_dir = env::var_os("HARNESS_REFLECTION_FIXTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("reflection-fixture"));
    let executor_source = reflection_env(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_SOURCE_THREAD",
        "fixture-executor-source",
    );
    let executor_source_turn = reflection_env(
        "HARNESS_REFLECTION_FIXTURE_EXECUTOR_SOURCE_TURN",
        "fixture-executor-source-turn",
    );
    let lead_source = reflection_env(
        "HARNESS_REFLECTION_FIXTURE_LEAD_SOURCE_THREAD",
        "fixture-lead-source",
    );
    let lead_source_turn = reflection_env(
        "HARNESS_REFLECTION_FIXTURE_LEAD_SOURCE_TURN",
        "fixture-lead-source-turn",
    );
    let (fork_thread, fork_turn) = if stage == "executor-self-reflection" {
        (
            reflection_env(
                "HARNESS_REFLECTION_FIXTURE_EXECUTOR_FORK_THREAD",
                "fixture-executor-reflection-fork",
            ),
            reflection_env(
                "HARNESS_REFLECTION_FIXTURE_EXECUTOR_FORK_TURN",
                "fixture-executor-reflection-turn",
            ),
        )
    } else {
        (
            reflection_env(
                "HARNESS_REFLECTION_FIXTURE_LEAD_FORK_THREAD",
                "fixture-lead-reflection-fork",
            ),
            reflection_env(
                "HARNESS_REFLECTION_FIXTURE_LEAD_FORK_TURN",
                "fixture-lead-reflection-turn",
            ),
        )
    };
    let investigations = if stage == "executor-self-reflection" {
        reflection_env("HARNESS_REFLECTION_FIXTURE_EXECUTOR_INVESTIGATIONS", "0")
            .parse()
            .unwrap_or(0)
    } else {
        reflection_env("HARNESS_REFLECTION_FIXTURE_LEAD_INVESTIGATIONS", "0")
            .parse()
            .unwrap_or(0)
    };
    let default_outcome = reflection_env("HARNESS_REFLECTION_FIXTURE_OUTCOME", "complete");
    let outcome = if stage == "executor-self-reflection" {
        reflection_env(
            "HARNESS_REFLECTION_FIXTURE_EXECUTOR_OUTCOME",
            &default_outcome,
        )
    } else {
        reflection_env("HARNESS_REFLECTION_FIXTURE_LEAD_OUTCOME", &default_outcome)
    };
    let result = if stage == "executor-self-reflection" {
        reflection_env(
            "HARNESS_REFLECTION_FIXTURE_EXECUTOR_RESULT",
            "synthetic executor reflection finding",
        )
    } else {
        reflection_env(
            "HARNESS_REFLECTION_FIXTURE_LEAD_RESULT",
            "synthetic lead reflection synthesis",
        )
    };
    let mut state = ReflectionState {
        stage,
        fixture_dir,
        home,
        executor_source,
        executor_source_turn,
        lead_source,
        lead_source_turn,
        fork_thread,
        fork_turn,
        investigations,
        outcome,
        result,
        model: reflection_env(
            "HARNESS_REFLECTION_FIXTURE_MODEL",
            "synthetic-reflection-model",
        ),
        provider: reflection_env(
            "HARNESS_REFLECTION_FIXTURE_PROVIDER",
            "synthetic-reflection-provider",
        ),
        effort: reflection_env("HARNESS_REFLECTION_FIXTURE_EFFORT", "max"),
        fork_cwd: None,
        fork_model: None,
        fork_provider: None,
        fork_effort: None,
        fork_config: None,
        source_goal: json!({
            "objective": "the accepted source goal must remain frozen",
            "status": "active",
            "source": "synthetic-reflection-fixture",
        }),
        forked: false,
        goal_cleared: false,
        turn_started: false,
    };
    state.restore_markers();
    state
}

fn record_reflection_request(state: &ReflectionState, request: &Value) {
    let _ = fs::create_dir_all(&state.fixture_dir);
    let path = state.fixture_dir.join("requests.jsonl");
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let line = json!({
            "stage": state.stage,
            "method": request["method"],
            "request": request,
        });
        let _ = writeln!(file, "{line}");
    }
}

fn reflection_answer(state: &mut ReflectionState, notifier: &Notifier, request: &Value) -> Answer {
    record_reflection_request(state, request);
    let method = request["method"].as_str().unwrap_or_default();
    let params = &request["params"];
    match method {
        "initialize" => Answer::Result(json!({
            "serverInfo": {"name": "synthetic-self-reflection", "version": "1"}
        })),
        "thread/read" => {
            let thread = params["threadId"].as_str().unwrap_or_default();
            let (source, _) = state.source();
            if thread == source {
                Answer::Result(json!({"thread": state.source_record()}))
            } else if thread == state.fork_thread {
                Answer::Result(json!({"thread": state.fork_record()}))
            } else {
                Answer::Error(json!({
                    "message": format!("synthetic reflection has no thread {thread}")
                }))
            }
        }
        "thread/goal/get" => {
            let thread = params["threadId"].as_str().unwrap_or_default();
            let (source, _) = state.source();
            let goal = if thread == source {
                state.source_goal.clone()
            } else if thread == state.fork_thread {
                if state.goal_cleared {
                    Value::Null
                } else {
                    state.source_goal.clone()
                }
            } else {
                return Answer::Error(json!({
                    "message": format!("synthetic reflection has no goal owner {thread}")
                }));
            };
            Answer::Result(json!({"goal": goal}))
        }
        "thread/goal/clear" => {
            if params["threadId"].as_str() != Some(state.fork_thread.as_str()) {
                return Answer::Error(json!({"message": "the source goal is not mutable"}));
            }
            if state.outcome != "goal-sticky" {
                state.goal_cleared = true;
            }
            Answer::Result(json!({}))
        }
        "thread/fork" => {
            let (source, source_turn) = state.source();
            let source = source.to_owned();
            let source_turn = source_turn.to_owned();
            if state.forked {
                return Answer::Error(json!({"message": "duplicate reflection fork refused"}));
            }
            if params["threadId"].as_str() != Some(source.as_str())
                || params["lastTurnId"].as_str() != Some(source_turn.as_str())
                || params["deferGoalContinuation"] != json!(true)
            {
                return Answer::Error(json!({
                    "message": "the reflection fork does not name the exact source boundary"
                }));
            }
            if state.outcome == "fork-rejected" {
                // The exact acknowledged refusal the retained live artifact
                // carries: the server rejected the request before creating a
                // fork, so no fork or turn identity exists.
                return Answer::Error(json!({
                    "code": -32600,
                    "message": "failed to load configuration: Model provider `deepseek` not found"
                }));
            }
            state.forked = true;
            state.record_fork(params);
            let mut route = state.route(params);
            route["thread"] = json!({
                "id": state.fork_thread,
                "cwd": params["cwd"].clone(),
                "turns": [reflection_turn(
                    &source_turn,
                    "completed",
                    "copied source work retained by the fork",
                    None,
                )],
            });
            Answer::Result(route)
        }
        "mcpServerStatus/list" => {
            let thread = params["threadId"].as_str().map(str::to_owned);
            state.mcp_catalogue(thread.as_deref())
        }
        "app/list" => {
            let thread = params["threadId"].as_str().map(str::to_owned);
            state.app_catalogue(thread.as_deref())
        }
        "mcpServer/tool/call" => state.refuse_mutator_call(params),
        "thread/name/set" => {
            if params["threadId"].as_str() != Some(state.fork_thread.as_str()) {
                return Answer::Error(json!({"message": "only the reflection fork may be named"}));
            }
            let title = params["name"].as_str().unwrap_or_default();
            if title.trim().is_empty() {
                return Answer::Error(json!({"message": "the reflection title is empty"}));
            }
            let _ = fs::write(state.home.join("frontend-title.txt"), title);
            Answer::Result(json!({}))
        }
        "thread/resume" => {
            if params["threadId"].as_str() != Some(state.fork_thread.as_str()) {
                return Answer::Error(
                    json!({"message": "only the retained reflection fork may resume"}),
                );
            }
            let mut route = json!({
                "model": state.fork_model.clone().unwrap_or_else(|| state.model.clone()),
                "modelProvider": state.fork_provider.clone().unwrap_or_else(|| state.provider.clone()),
                "reasoningEffort": state.fork_effort.clone().unwrap_or_else(|| state.effort.clone()),
                "thread": {"id": state.fork_thread, "cwd": state.fork_cwd.clone().unwrap_or_else(|| state.home.to_string_lossy().into_owned()), "turns": []},
            });
            route["thread"] = state.fork_record();
            Answer::Result(route)
        }
        "turn/start" => {
            if params["threadId"].as_str() != Some(state.fork_thread.as_str()) {
                return Answer::Error(json!({"message": "the source turn is read-only"}));
            }
            if state.turn_started {
                return Answer::Error(json!({"message": "duplicate reflection turn refused"}));
            }
            state.turn_started = true;
            state.record_turn();
            if state.outcome == "visibility-lost" {
                let _ = fs::write(
                    state.home.join("frontend-release"),
                    "fixture visibility loss",
                );
            }
            if matches!(
                state.outcome.as_str(),
                "complete"
                    | "missing-usage"
                    | "mutator-window"
                    | "dotted-mutator-window"
                    | "source-later-work"
            ) {
                let _ = write_reflection_rollout(state);
            }
            if state.outcome.ends_with("mutator-window") {
                // Keep the fork endpoint live until the owning test has
                // addressed it directly. The release marker is bounded so a
                // lost test cannot hold the native turn open indefinitely.
                let notifier = notifier.clone();
                let fork_thread = state.fork_thread.clone();
                let fork_turn = state.fork_turn.clone();
                let result = state.result.clone();
                let release = state
                    .fixture_dir
                    .join(format!("release-{}.marker", state.stage));
                thread::spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(15);
                    while !release.exists() && Instant::now() < deadline {
                        thread::sleep(Duration::from_millis(25));
                    }
                    notifier.push(json!({
                        "method": "item/started",
                        "params": {
                            "threadId": fork_thread,
                            "turnId": fork_turn,
                            "item": {
                                "id": format!("{fork_turn}-message"),
                                "type": "agentMessage",
                                "text": result,
                            },
                        },
                    }));
                    notifier.push(json!({
                        "method": "item/completed",
                        "params": {
                            "threadId": fork_thread,
                            "turnId": fork_turn,
                            "item": {
                                "id": format!("{fork_turn}-message"),
                                "type": "agentMessage",
                                "text": result,
                            },
                        },
                    }));
                    notifier.push(json!({
                        "method": "turn/completed",
                        "params": {
                            "threadId": fork_thread,
                            "turn": {"id": fork_turn, "status": "completed"},
                        },
                    }));
                });
            }
            notifier.push(json!({
                "method": "turn/started",
                "params": {
                    "threadId": state.fork_thread,
                    "turn": {"id": state.fork_turn, "status": "inProgress"},
                },
            }));
            for index in 0..state.investigations {
                let item = json!({
                    "id": format!("{}-investigation-{index}", state.fork_turn),
                    "type": "commandExecution",
                    "command": "synthetic read-only investigation",
                });
                notifier.push(json!({
                    "method": "item/started",
                    "params": {
                        "threadId": state.fork_thread,
                        "turnId": state.fork_turn,
                        "item": item.clone(),
                    },
                }));
                notifier.push(json!({
                    "method": "item/completed",
                    "params": {
                        "threadId": state.fork_thread,
                        "turnId": state.fork_turn,
                        "item": item,
                    },
                }));
            }
            match state.outcome.as_str() {
                "complete" | "missing-usage" | "source-later-work" => {
                    notifier.push(json!({
                        "method": "item/started",
                        "params": {
                            "threadId": state.fork_thread,
                            "turnId": state.fork_turn,
                            "item": {
                                "id": format!("{}-message", state.fork_turn),
                                "type": "agentMessage",
                                "text": state.result,
                            },
                        },
                    }));
                    notifier.push(json!({
                        "method": "item/completed",
                        "params": {
                            "threadId": state.fork_thread,
                            "turnId": state.fork_turn,
                            "item": {
                                "id": format!("{}-message", state.fork_turn),
                                "type": "agentMessage",
                                "text": state.result,
                            },
                        },
                    }));
                    notifier.push(json!({
                        "method": "turn/completed",
                        "params": {
                            "threadId": state.fork_thread,
                            "turn": {"id": state.fork_turn, "status": "completed"},
                        },
                    }));
                }
                "interrupted" | "failed" => {
                    let mut turn = json!({
                        "id": state.fork_turn,
                        "status": state.outcome,
                    });
                    if state.outcome == "failed" {
                        turn["error"] = json!({"message": "synthetic reflection failure"});
                    }
                    notifier.push(json!({
                        "method": "turn/completed",
                        "params": {"threadId": state.fork_thread, "turn": turn},
                    }));
                }
                "unknown-hold" => {
                    // A submitted turn exposes a final-looking assistant item
                    // but never establishes native completion. This is the
                    // visible-only boundary: downstream work must not treat
                    // the text as the stage outcome.
                    let item = json!({
                        "id": format!("{}-message", state.fork_turn),
                        "type": "agentMessage",
                        "text": state.result,
                    });
                    notifier.push(json!({
                        "method": "item/started",
                        "params": {
                            "threadId": state.fork_thread,
                            "turnId": state.fork_turn,
                            "item": item.clone(),
                        },
                    }));
                    notifier.push(json!({
                        "method": "item/completed",
                        "params": {
                            "threadId": state.fork_thread,
                            "turnId": state.fork_turn,
                            "item": item,
                        },
                    }));
                    state.record_visible_text();
                }
                "visibility-lost" => {}
                _ => {}
            }
            Answer::Result(json!({
                "turn": {"id": state.fork_turn, "status": "inProgress"}
            }))
        }
        _ => Answer::Result(json!({})),
    }
}

fn reflection_usage(input: u64, cached: u64, output: u64, reasoning: u64, total: u64) -> Value {
    json!({
        "input_tokens": input,
        "cached_input_tokens": cached,
        "output_tokens": output,
        "reasoning_output_tokens": reasoning,
        "total_tokens": total,
    })
}

fn write_reflection_rollout(state: &ReflectionState) -> io::Result<()> {
    let sessions = state.home.join("sessions");
    fs::create_dir_all(&sessions)?;
    let (source_turn, source_usage, cumulative) = if state.stage == "executor-self-reflection" {
        (
            state.executor_source_turn.as_str(),
            reflection_usage(400, 120, 50, 10, 450),
            reflection_usage(500, 160, 70, 15, 570),
        )
    } else {
        (
            state.lead_source_turn.as_str(),
            reflection_usage(600, 180, 80, 20, 680),
            reflection_usage(800, 260, 120, 30, 920),
        )
    };
    let mut new_usage = if state.stage == "executor-self-reflection" {
        reflection_usage(100, 40, 20, 5, 120)
    } else {
        reflection_usage(200, 80, 40, 10, 240)
    };
    if state.outcome == "missing-usage" {
        new_usage.as_object_mut().unwrap().remove("total_tokens");
    }
    let mut lines = vec![
        json!({"type": "session_meta", "payload": {"id": state.fork_thread}}),
        json!({
            "type": "token_usage_record",
            "payload": {
                "turn_id": source_turn,
                "response_id": format!("{}-copied-source", state.stage),
                "usage": source_usage.clone(),
                "turn_token_usage": source_usage.clone(),
                "thread_token_usage": source_usage,
            },
        }),
        json!({
            "type": "token_usage_record",
            "payload": {
                "turn_id": state.fork_turn,
                "response_id": format!("{}-new-turn", state.stage),
                "usage": new_usage.clone(),
                "turn_token_usage": new_usage,
                "thread_token_usage": cumulative,
            },
        }),
    ];
    for index in 0..state.investigations {
        lines.push(json!({
            "type": "event_msg",
            "payload": {
                "type": "item_completed",
                "turn_id": state.fork_turn,
                "item": {
                    "type": "commandExecution",
                    "id": format!("{}-investigation-{index}", state.fork_turn),
                    "command": "synthetic read-only investigation",
                },
            },
        }));
    }
    let text = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let safe = |value: &str| {
        value
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '-' {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>()
    };
    fs::write(
        sessions.join(format!(
            "{}-{}-{}.jsonl",
            safe(&state.stage),
            safe(&state.fork_thread),
            safe(&state.fork_turn)
        )),
        format!("{text}\n"),
    )
}

/// Serves the self-reflection child contract. The runner still owns process
/// creation, the read-only plan, frontend attachment and every request; this
/// branch only answers those requests with inspectable synthetic identities.
fn run_reflection_app_server(args: &[std::ffi::OsString]) -> io::Result<i32> {
    let listen = option(args, "--listen").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the reflection fixture app-server requires --listen ws://127.0.0.1:PORT",
        )
    })?;
    let port = listen
        .strip_prefix("ws://127.0.0.1:")
        .and_then(|text| text.parse::<u16>().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("the reflection fixture app-server cannot serve {listen}"),
            )
        })?;
    let token_file = option(args, "--ws-token-file")
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "the reflection fixture app-server requires --ws-token-file",
            )
        })?;
    fs::create_dir_all(
        env::var_os("HARNESS_REFLECTION_FIXTURE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
    )?;
    let state = Arc::new(Mutex::new(reflection_state()));
    {
        let state = state.lock().unwrap();
        let meta = json!({
            "reflectionSession": env::var("HARNESS_REFLECTION_SESSION").unwrap_or_default(),
            "stage": state.stage,
            "cwd": env::current_dir().ok(),
            "codexHome": state.home,
        });
        fs::create_dir_all(&state.fixture_dir)?;
        fs::write(
            state.fixture_dir.join(format!("meta-{}.json", state.stage)),
            serde_json::to_vec_pretty(&meta)?,
        )?;
    }
    let server = Server::start_on_with(port, Bearer::File(token_file), |server| {
        let notifier = server.notifier();
        server.answer_fn(move |request| {
            let mut state = state.lock().unwrap();
            reflection_answer(&mut state, &notifier, request)
        });
    });
    // The real app-server serves until its owning Job ends it. Keep this
    // fixture's endpoint alive for the same lifecycle.
    let _server = server;
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn option(args: &[std::ffi::OsString], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.to_str())
        .map(str::to_owned)
}

/// Records this child's own invocation and role-marking environment when the
/// host names an output path, so a model-free check asserts the effective
/// role context and routing argv the app-server actually received instead of
/// inferring them from the dispatching process. Every ordinary run records
/// nothing.
fn record_child(path: PathBuf) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let argv = env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let marker = |name: &str| match env::var(name) {
        Ok(value) => json!(value),
        Err(_) => json!(null),
    };
    let record = json!({
        "schema": 1,
        "argv": argv,
        "executorSession": marker("HARNESS_EXECUTOR_SESSION"),
        "boundedLeadSession": marker("HARNESS_BOUNDED_LEAD_SESSION"),
        "contextRole": marker("HARNESS_CONTEXT_ROLE"),
        "contextRevision": marker("HARNESS_CONTEXT_REVISION"),
        "currentDir": env::current_dir().ok().map(|path| path.display().to_string()),
    });
    fs::write(
        &path,
        serde_json::to_vec_pretty(&record).map_err(io::Error::other)?,
    )?;
    Ok(())
}

/// The controlled behavior of this child once the host submitted its
/// assignment; the default completes the turn exactly as before.
fn control_mode() -> String {
    env::var("HARNESS_IMPROVEMENT_FIXTURE_CONTROL").unwrap_or_else(|_| "complete".into())
}

/// Records the assignment this child observed, when the caller named a marker
/// path, so a model-free check waits for the child's own observation of the
/// `hold` or `fail` mode instead of guessing from process timing.
fn record_control_marker(session: &str, control: &str, turn_id: &str) {
    let Some(path) = env::var_os("HARNESS_IMPROVEMENT_FIXTURE_CONTROL_MARKER") else {
        return;
    };
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let record = json!({
        "schema": 1,
        "control": control,
        "session": session,
        "turn": turn_id,
    });
    let _ = fs::write(path, record.to_string());
}

/// Appends one synthetic workflow turn receipt under the arm's own
/// `CODEX_HOME`. The receipt is not a controller protocol: it lets a
/// model-free check prove which role actually ran, whether that role could
/// commit, and whether the later lead turns observed the earlier participant
/// artifacts. Nothing is written into the measured checkout.
fn record_workflow_turn(session: &str, turn: &FixtureTurn, message_ms: i64) -> io::Result<()> {
    let home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("the workflow receipt requires CODEX_HOME"))?;
    let path = home.join(WORKFLOW_RECORD_FILE);
    let prior = fs::read_to_string(&path).unwrap_or_default();
    let prior_roles = prior
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|record| record["role"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let checkout = env::current_dir()?;
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&checkout)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let solution = fs::read_to_string(checkout.join("solution.txt")).ok();
    let role = turn.role.unwrap_or("legacy");
    let review_observed = prior_roles.iter().any(|role| role == "lead-review");
    let correction_observed = prior_roles.iter().any(|role| role == "correction");
    let lead_frame_observed = prior_roles.iter().any(|role| role == "lead-frame");
    let record = json!({
        "schema": 1,
        "scenario": WORKFLOW_SCENARIO,
        "role": role,
        "turnId": turn.id,
        "messageId": turn.message_id,
        "responseId": turn.response_id,
        "session": session,
        "completedAtMs": message_ms,
        "committed": turn.commits,
        "head": head,
        "solution": solution,
        "leadFrameObserved": lead_frame_observed,
        "reviewObserved": review_observed,
        "correctionObserved": correction_observed,
        "priorRoles": prior_roles,
    });
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    use std::io::Write as _;
    writeln!(file, "{record}")?;
    Ok(())
}

/// The installed client route of this arm home: the explicit configuration the
/// installation owner wrote, read exactly as the served thread must report it.
struct InstalledRoute {
    model: String,
    provider: String,
    effort: String,
}

fn installed_route(args: &[std::ffi::OsString]) -> io::Result<InstalledRoute> {
    let home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("the fixture app-server requires CODEX_HOME"))?;
    let bytes = fs::read(home.join("config.toml"))?;
    let mut document: toml::Table =
        toml::from_str(std::str::from_utf8(&bytes).map_err(io::Error::other)?)
            .map_err(io::Error::other)?;
    // Native app-server receives the selected profile through explicit -c
    // overrides. Honor the actual command, rather than answering every role
    // with the base home's model.
    for pair in args
        .windows(2)
        .filter(|pair| pair[0] == "-c" || pair[0] == "--config")
    {
        let argument = pair[1].to_string_lossy();
        let Some((key, _)) = argument.split_once('=') else {
            continue;
        };
        if !matches!(
            key.trim(),
            "model" | "model_provider" | "model_reasoning_effort"
        ) {
            continue;
        }
        let value: toml::Table = toml::from_str(&argument).map_err(io::Error::other)?;
        document.extend(value);
    }
    let text = |key: &str| {
        document
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Ok(InstalledRoute {
        model: text("model"),
        provider: text("model_provider"),
        effort: text("model_reasoning_effort"),
    })
}

/// One committed solution in the bound slot (the process working directory),
/// exactly the shape the controller's solution validation requires.
fn commit_solution() -> io::Result<()> {
    let text = env::var("HARNESS_IMPROVEMENT_FIXTURE_SOLUTION_TEXT")
        .unwrap_or_else(|_| "solved".to_owned());
    if text.is_empty() {
        return Ok(());
    }
    let file = env::var("HARNESS_IMPROVEMENT_FIXTURE_SOLUTION_FILE")
        .unwrap_or_else(|_| "solution.txt".to_owned());
    let slot = env::current_dir()?;
    fs::write(slot.join(&file), format!("{text}\n"))?;
    git(&slot, &["add", &file])?;
    git(
        &slot,
        &[
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "user.name=Fixture",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "controlled agent solution",
        ],
    )?;
    Ok(())
}

struct OwnedHeavy {
    started_at_ms: i64,
    completed_at_ms: i64,
    /// Opaque producer process id. This is not the spawned OS pid.
    process_id: Option<String>,
}

/// Runs one real heavy command under the controller-supplied account and
/// evidence environment, and emits the command item the host can correlate.
/// The item id is the function-call id. It is not supplied as a heavy-command
/// label. The item's process id is an opaque producer id, not `child.id()`.
/// The selected app-server route does not put the OS pid in that field, so
/// the control owner records the private OS link for the process it spawned
/// while that process is alive instead of seeding the item with a pid.
fn run_owned_heavy(server: &Server, session: &str) -> OwnedHeavy {
    let started_at_ms = unix_ms();
    server.push(json!({
        "method": "item/started",
        "params": {
            "threadId": session,
            "startedAtMs": started_at_ms,
            "item": {
                "id": HEAVY_CALL_ID,
                "type": "commandExecution",
                "command": "codex-harness heavy",
                "status": "inProgress",
            },
        },
    }));
    // The host polls the control stream. Give it time to record the start
    // before the admission interval begins.
    thread::sleep(Duration::from_millis(800));
    if let Some(marker) = env::var_os("HARNESS_IMPROVEMENT_FIXTURE_HEAVY_MARKER") {
        let _ = fs::write(marker, b"waiting\n");
    }
    let program = env::var("HARNESS_IMPROVEMENT_FIXTURE_HEAVY_PROGRAM")
        .unwrap_or_else(|_| "cmd.exe".to_owned());
    let cli = env::var("HARNESS_IMPROVEMENT_FIXTURE_HEAVY_CLI")
        .unwrap_or_else(|_| "codex-harness.exe".to_owned());
    let spawned = Command::new(cli).args(["heavy", "--", &program]).spawn();
    let (process_id, exit) = match spawned {
        Ok(child) => {
            let os_pid = child.id();
            let opaque = opaque_producer_process_id(os_pid);
            record_control_os_link(&opaque, os_pid);
            let waited = child.wait_with_output();
            (
                Some(opaque),
                waited.ok().and_then(|output| output.status.code()),
            )
        }
        Err(_) => (None, None),
    };
    let completed_at_ms = unix_ms();
    server.push(json!({
        "method": "item/completed",
        "params": {
            "threadId": session,
            "startedAtMs": started_at_ms,
            "completedAtMs": completed_at_ms,
            "item": {
                "id": HEAVY_CALL_ID,
                "type": "commandExecution",
                "command": "codex-harness heavy",
                "status": if exit == Some(0) { "completed" } else { "failed" },
                "exitCode": exit,
                "processId": process_id.clone(),
                "source": "unified_exec_startup",
            },
        },
    }));
    OwnedHeavy {
        started_at_ms,
        completed_at_ms,
        process_id,
    }
}

/// Records the control owner's private OS link for the command it spawned
/// while that process is alive. The route reports an opaque producer id, so
/// only this live association plus the admission's own ancestry can bind the
/// item. `HARNESS_IMPROVEMENT_FIXTURE_HEAVY_NO_OS_LINK` suppresses it for the
/// missing-OS-identity negative control.
fn record_control_os_link(opaque_process_id: &str, os_pid: u32) {
    if env::var_os("HARNESS_IMPROVEMENT_FIXTURE_HEAVY_NO_OS_LINK").is_some() {
        return;
    }
    let Some(directory) = env::var_os(harness_core::heavy_command_trace::EVIDENCE_ENV) else {
        return;
    };
    let _ = harness_core::heavy_command::record_live_os_link(
        Path::new(&directory),
        HEAVY_CALL_ID,
        opaque_process_id,
        os_pid,
    );
}

/// A numeric producer id that is not the spawned OS pid. The selected route
/// reports a unified-exec session id in this field.
fn opaque_producer_process_id(os_pid: u32) -> String {
    let mut opaque = os_pid.wrapping_add(1_000_003);
    if opaque == 0 || opaque == os_pid {
        opaque = 1_000_003;
    }
    opaque.to_string()
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Records unqualified command identity from the host-supplied environment.
/// The request is tool-probes/<arm>.json in the comparison run directory,
/// derived from CODEX_HOME, so it survives the host wrapper. Absent when the
/// caller did not ask for it.
fn record_selected_tools() {
    let Some(home) = env::var_os("CODEX_HOME").map(PathBuf::from) else {
        return;
    };
    let Some(arm_dir) = home.parent() else {
        return;
    };
    let Some(arm) = arm_dir.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let Some(run) = arm_dir.parent().and_then(|comparison| comparison.parent()) else {
        return;
    };
    let request_path = run.join("tool-probes").join(format!("{arm}.json"));
    let Ok(bytes) = fs::read(&request_path) else {
        return;
    };
    let Ok(request) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    let Some(receipt) = request["receipt"].as_str() else {
        return;
    };
    let allow = request["allow"].as_str().unwrap_or("");
    let mut commands = serde_json::Map::new();
    if let Some(names) = request["commands"].as_array() {
        for name in names.iter().filter_map(|value| value.as_str()) {
            commands.insert(name.to_owned(), probe_command(name, allow));
        }
    }
    let record = json!({
        "pid": std::process::id(),
        "commands": commands,
        "shell": probe_shell(),
    });
    if let Some(directory) = Path::new(receipt).parent() {
        let _ = fs::create_dir_all(directory);
    }
    if let Ok(encoded) = serde_json::to_vec_pretty(&record) {
        let _ = fs::write(receipt, encoded);
    }
}

fn probe_command(name: &str, allow: &str) -> serde_json::Value {
    let located = Command::new("where.exe").arg(name).output();
    let (resolved, where_exit) = match &located {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            let first = text.lines().next().unwrap_or("").trim().to_owned();
            (
                if first.is_empty() { None } else { Some(first) },
                output.status.code(),
            )
        }
        Err(error) => {
            return json!({
                "resolved": serde_json::Value::Null,
                "executed": false,
                "error": error.to_string(),
            });
        }
    };
    // A resolved path outside the fixture is already a foreign answer. Do not
    // execute it. A name where.exe cannot see is still invoked, so an explicit
    // suffix that PowerShell would accept cannot hide behind the lookup.
    let outside = resolved.as_deref().is_some_and(|path| {
        allow.is_empty()
            || !path
                .to_ascii_lowercase()
                .starts_with(&allow.to_ascii_lowercase())
    });
    if outside {
        return json!({
            "resolved": resolved,
            "whereExit": where_exit,
            "executed": false,
        });
    }
    match Command::new("pwsh")
        .args(["-NoLogo", "-NoProfile", "-Command", name])
        .output()
    {
        Ok(output) => json!({
            "resolved": resolved,
            "whereExit": where_exit,
            "executed": true,
            "exit": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        }),
        Err(error) => json!({
            "resolved": resolved,
            "whereExit": where_exit,
            "executed": false,
            "error": error.to_string(),
        }),
    }
}

fn probe_shell() -> serde_json::Value {
    match Command::new("pwsh")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-Command",
            "$PSVersionTable.PSVersion.ToString(); (Get-Command pwsh.exe).Source",
        ])
        .output()
    {
        Ok(output) => json!({
            "exit": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout).trim(),
            "stderr": String::from_utf8_lossy(&output.stderr).trim(),
        }),
        Err(error) => json!({"error": error.to_string()}),
    }
}

fn git(cwd: &Path, args: &[&str]) -> io::Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

/// Writes the native rollout the observation owner reads for this session,
/// with the installed route (or the explicitly overridden facts).
///
/// The legacy (no-scenario) fixture keeps its single-turn file exactly: the
/// same identities and one overwrite per invocation. A synthetic measured
/// workflow turn appends its own turn to the same per-session rollout so the
/// accounting owner can see every participant, retry and role on one retained
/// native session; the session metadata line is written once.
///
/// `wait_call` names a single tool call that is the whole response. It is not
/// assumed idle: the accounting owner still has to match it to one blocked
/// admission. Publication time is not written as a request interval.
fn write_rollout(
    session: &str,
    model: &str,
    effort: &str,
    include_usage: bool,
    command_bound: Option<OwnedHeavy>,
    message_ms: i64,
    turn: &FixtureTurn,
) -> io::Result<()> {
    let home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("the fixture rollout requires CODEX_HOME"))?;
    let sessions = home.join("sessions").join("2026").join("10").join("01");
    fs::create_dir_all(&sessions)?;
    let path = sessions.join(format!("rollout-{session}.jsonl"));
    let synthetic = turn.role.is_some();
    // Synthetic turns share one session rollout. Every earlier turn keeps its
    // own bytes and only the first write carries the session metadata line;
    // the legacy fixture keeps its single overwritten turn.
    let mut prefix = String::new();
    let mut lines = Vec::new();
    if synthetic {
        match fs::read_to_string(&path) {
            Ok(prior) if !prior.is_empty() => {
                prefix = prior;
                if !prefix.ends_with('\n') {
                    prefix.push('\n');
                }
            }
            Ok(_) => lines.push(json!({
                "type": "session_meta",
                "payload": {"id": session, "base_instructions": "fixture"},
            })),
            Err(error) if error.kind() == io::ErrorKind::NotFound => lines.push(json!({
                "type": "session_meta",
                "payload": {"id": session, "base_instructions": "fixture"},
            })),
            Err(error) => return Err(error),
        }
    } else {
        lines.push(json!({
            "type": "session_meta",
            "payload": {"id": session, "base_instructions": "fixture"},
        }));
    }
    lines.push(json!({
        "type": "turn_context",
        "payload": {"model": model, "effort": effort, "turn_id": turn.id},
    }));
    if let Some(command) = command_bound {
        let mut item = json!({
            "type": "CommandExecution",
            "id": HEAVY_CALL_ID
        });
        if let Some(process_id) = command.process_id {
            item["process_id"] = json!(process_id);
            item["source"] = json!("unified_exec_startup");
        }
        lines.push(json!({
            "type": "event_msg",
            "payload": {
                "type": "item_completed",
                "turn_id": turn.id,
                "started_at_ms": command.started_at_ms,
                "completed_at_ms": command.completed_at_ms,
                "item": item
            }
        }));
        lines.push(json!({
            "type": "event_msg",
            "payload": {
                "type": "item_completed",
                "turn_id": turn.id,
                "started_at_ms": message_ms,
                "completed_at_ms": message_ms,
                "item": {"type": "AgentMessage", "id": turn.message_id}
            }
        }));
        lines.push(json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "call_id": HEAVY_CALL_ID,
                "name": "exec_command",
            },
        }));
        lines.push(json!({
            "type": "token_usage_record",
            "payload": {
                "response_id": if synthetic {
                    format!("{}-heavy", turn.response_id)
                } else {
                    "response-heavy".to_owned()
                },
                "turn_id": turn.id,
                "usage": {
                    "input_tokens": 12,
                    "cached_input_tokens": 0,
                    "output_tokens": 2,
                    "reasoning_output_tokens": 0,
                    "total_tokens": 14,
                },
            },
        }));
    } else if include_usage {
        lines.push(json!({
            "type": "token_usage_record",
            "payload": {
                "response_id": turn.response_id,
                "usage": {
                    "input_tokens": 100,
                    "cached_input_tokens": 40,
                    "output_tokens": 20,
                    "reasoning_output_tokens": 5,
                    "total_tokens": 120,
                },
            },
        }));
    }
    if synthetic && include_usage {
        let mut cumulative = prefix
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|line| line["type"] == "event_msg" && line["payload"]["type"] == "token_count")
            .map(|line| line["payload"]["info"]["total_token_usage"].clone())
            .next_back()
            .unwrap_or_else(|| json!({}));
        for line in &lines {
            if line["type"] != "token_usage_record" {
                continue;
            }
            for field in harness_core::rollout_reader::TOKEN_FIELDS {
                let prior = cumulative[field].as_u64().unwrap_or(0);
                let next = line["payload"]["usage"][field]
                    .as_u64()
                    .ok_or_else(|| io::Error::other("synthetic response usage must be complete"))?;
                cumulative[field] =
                    json!(prior.checked_add(next).ok_or_else(|| {
                        io::Error::other("synthetic cumulative usage overflow")
                    })?);
            }
        }
        lines.push(json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {"total_token_usage": cumulative}
            }
        }));
    }
    let text = lines
        .iter()
        .map(|line| serde_json::to_string(line).map_err(io::Error::other))
        .collect::<io::Result<Vec<_>>>()?
        .join("\n");
    prefix.push_str(&text);
    prefix.push('\n');
    fs::write(path, prefix)
}
