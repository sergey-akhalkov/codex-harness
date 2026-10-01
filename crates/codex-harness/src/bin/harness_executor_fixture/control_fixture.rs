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
//! - `HARNESS_IMPROVEMENT_FIXTURE_MODEL` / `..._EFFORT`: override the model and
//!   reasoning effort recorded in the native rollout (the wrong-observation
//!   counterexamples); the default echoes the arm's installed configuration;
//! - `HARNESS_IMPROVEMENT_FIXTURE_SOLUTION_FILE` / `..._SOLUTION_TEXT`: the
//!   committed solution the controlled agent leaves in its bound slot
//!   (default `solution.txt` = `solved`); an empty text skips the commit.
//! - `tool-probes/<arm>.json` beside the comparison run, when present: the
//!   controlled agent resolves each named unqualified command through the
//!   host-supplied environment and executes it only when the hit is under the
//!   request's allow prefix. It also runs the owner shell. Completion is
//!   pushed after that work so the host cannot end the child first.

#[path = "../../../tests/fixtures/control_endpoint.rs"]
mod control_endpoint;

use control_endpoint::{Answer, Bearer, Server};
use serde_json::json;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

/// The thread identity the served conversation uses when the caller does not
/// name one.
const DEFAULT_SESSION: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f5";
const TURN: &str = "fixture-turn-1";
const FINAL_MESSAGE: &str = "fixture turn completed; the committed solution is in the bound slot";

/// Serves one `app-server` invocation. Returns the process exit code; the
/// caller's Job owns termination, exactly as it owns the real child.
pub fn run_app_server(args: &[std::ffi::OsString]) -> io::Result<i32> {
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
    let route = installed_route()?;
    let rollout_model =
        env::var("HARNESS_IMPROVEMENT_FIXTURE_MODEL").unwrap_or_else(|_| route.model.clone());
    let rollout_effort =
        env::var("HARNESS_IMPROVEMENT_FIXTURE_EFFORT").unwrap_or_else(|_| route.effort.clone());

    let session_for_thread = session.clone();
    let resumed_model = route.model.clone();
    let resumed_provider = route.provider.clone();
    let resumed_effort = route.effort.clone();
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
            server.answer("thread/name/set", Answer::Result(json!({})));
            server.answer(
                "turn/start",
                Answer::Result(json!({"turn": {"id": TURN, "status": "inProgress"}})),
            );
            // The first read must see an empty thread (the host refuses a
            // conversation whose fresh thread already has turns); the second read
            // serves the completed turn's final assistant message.
            server.answer_sequence(
            "thread/read",
            vec![
                Answer::Result(json!({"thread": {"id": session_for_thread.clone(), "turns": []}})),
                Answer::Result(json!({"thread": {
                    "id": session_for_thread.clone(),
                    "turns": [{
                        "id": TURN,
                        "status": "completed",
                        "items": [{"id": "message-1", "type": "agentMessage", "text": FINAL_MESSAGE}],
                    }],
                }})),
            ],
        );
            server.push(json!({"method": "thread/started", "params": {"thread": {"id": session_for_thread.clone()}}}));
            server.push(json!({
            "method": "turn/started",
            "params": {"threadId": session_for_thread.clone(), "turn": {"id": TURN, "status": "inProgress"}},
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
                    if env::var("HARNESS_IMPROVEMENT_FIXTURE_HEAVY").is_ok() {
                        run_owned_heavy(&server, &session);
                    }
                    record_selected_tools();
                    let _ = commit_solution();
                    let heavy = env::var("HARNESS_IMPROVEMENT_FIXTURE_HEAVY").is_ok();
                    let _ = write_rollout(
                        &session,
                        &rollout_model,
                        &rollout_effort,
                        !heavy,
                        heavy.then_some("heavy-cmd-1"),
                    );
                    server.push(json!({
                        "method": "item/started",
                        "params": {
                            "threadId": session.clone(),
                            "item": {"id": "message-1", "type": "agentMessage", "text": FINAL_MESSAGE},
                        },
                    }));
                    server.push(json!({
                        "method": "item/completed",
                        "params": {
                            "threadId": session.clone(),
                            "item": {"id": "message-1", "type": "agentMessage", "text": FINAL_MESSAGE},
                        },
                    }));
                    server.push(json!({
                        "method": "turn/completed",
                        "params": {"threadId": session.clone(), "turn": {"id": TURN, "status": "completed"}},
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

fn option(args: &[std::ffi::OsString], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.to_str())
        .map(str::to_owned)
}

/// The installed client route of this arm home: the explicit configuration the
/// installation owner wrote, read exactly as the served thread must report it.
struct InstalledRoute {
    model: String,
    provider: String,
    effort: String,
}

fn installed_route() -> io::Result<InstalledRoute> {
    let home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("the fixture app-server requires CODEX_HOME"))?;
    let bytes = fs::read(home.join("config.toml"))?;
    let document: toml::Table =
        toml::from_str(std::str::from_utf8(&bytes).map_err(io::Error::other)?)
            .map_err(io::Error::other)?;
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

/// Runs one real heavy command under the controller-supplied account and
/// evidence environment, and emits the matching command item so the host can
/// correlate it. The command id is the item id. This is not a model call.
fn run_owned_heavy(server: &Server, session: &str) {
    const COMMAND_ID: &str = "heavy-cmd-1";
    server.push(json!({
        "method": "item/started",
        "params": {
            "threadId": session,
            "item": {
                "id": COMMAND_ID,
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
    let output = Command::new(cli)
        .args([
            "heavy",
            "--tool-call",
            COMMAND_ID,
            "--command-id",
            COMMAND_ID,
            "--",
            &program,
        ])
        .output();
    let exit = output.as_ref().ok().and_then(|output| output.status.code());
    server.push(json!({
        "method": "item/completed",
        "params": {
            "threadId": session,
            "item": {
                "id": COMMAND_ID,
                "type": "commandExecution",
                "command": "codex-harness heavy",
                "status": if exit == Some(0) { "completed" } else { "failed" },
                "exitCode": exit,
            },
        },
    }));
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
/// `wait_call` names a single tool call that is the whole response. It is not
/// assumed idle: the accounting owner still has to match it to one blocked
/// admission. Publication time is not written as a request interval.
fn write_rollout(
    session: &str,
    model: &str,
    effort: &str,
    include_usage: bool,
    wait_call: Option<&str>,
) -> io::Result<()> {
    let home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("the fixture rollout requires CODEX_HOME"))?;
    let sessions = home.join("sessions").join("2026").join("10").join("01");
    fs::create_dir_all(&sessions)?;
    let mut lines = vec![
        json!({
            "type": "session_meta",
            "payload": {"id": session, "base_instructions": "fixture"},
        }),
        json!({
            "type": "turn_context",
            "payload": {"model": model, "effort": effort, "turn_id": TURN},
        }),
    ];
    if let Some(call) = wait_call {
        lines.push(json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "call_id": call,
                "command_id": call,
                "name": "exec_command",
                "response_id": "response-heavy",
            },
        }));
        lines.push(json!({
            "type": "token_usage_record",
            "payload": {
                "response_id": "response-heavy",
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
                "response_id": "response-1",
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
    let text = lines
        .iter()
        .map(|line| serde_json::to_string(line).map_err(io::Error::other))
        .collect::<io::Result<Vec<_>>>()?
        .join("\n");
    fs::write(
        sessions.join(format!("rollout-{session}.jsonl")),
        format!("{text}\n"),
    )
}
