//! Owned native `codex exec --json` event fixture.
//!
//! The executor observation path consumes the installed CLI's JSONL event
//! stream and its `--output-last-message` file. This fixture emits that
//! control-plane contract (CLI 0.155.1 shape) with no model, network or
//! subscription use so the host, the receipt lifecycle and `executor watch`
//! can be exercised end to end, including their failure shapes.
//!
//! Modes come from `HARNESS_EXECUTOR_FIXTURE_MODE`: `complete` (default),
//! `empty`, `nofinal`, `error`, `nonzero`, `malformed`, `truncated`, `slow`
//! (delay from `HARNESS_EXECUTOR_FIXTURE_DELAY_MS`), `hang` and `descendant`
//! (bounded by `HARNESS_EXECUTOR_FIXTURE_RELEASE`, a file to create) plus the
//! `child-hold` helper the descendant spawns. `HARNESS_EXECUTOR_FIXTURE_STARTED`
//! records this process's identity immediately, and
//! `HARNESS_EXECUTOR_FIXTURE_CHILD_MARKER` records the descendant's.

use serde_json::json;
use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
    process::Stdio,
    thread,
    time::{Duration, Instant},
};

const DEFAULT_SESSION: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
const FINAL_MESSAGE: &str =
    "FIXTURE_OUTCOME_DONE\nremaining: none\nchecks: fixture event stream verified";

fn main() -> io::Result<()> {
    // The installed-launcher shell preflight is a model-free diagnostic; the
    // fixture answers it so receipts without a recorded shell still exercise
    // the real preparation path.
    if env::args().skip(1).any(|arg| arg == "prompt-input") {
        let block = "<permissions instructions>\n`sandbox_mode` is `danger-full-access`\n</permissions instructions>";
        println!(
            "{}",
            json!([{
                "type": "message",
                "role": "developer",
                "content": [{"type": "input_text", "text": block}]
            }])
        );
        return Ok(());
    }
    if let Some(path) = env::var_os("HARNESS_EXECUTOR_FIXTURE_STARTED") {
        write_identity(PathBuf::from(path))?;
    }
    let mode = env::var("HARNESS_EXECUTOR_FIXTURE_MODE").unwrap_or_else(|_| "complete".into());
    let session =
        env::var("HARNESS_EXECUTOR_FIXTURE_SESSION").unwrap_or_else(|_| DEFAULT_SESSION.into());
    let result = last_message_path(env::args_os().skip(1).collect());
    match mode.as_str() {
        "child-hold" => {
            if let Some(marker) = env::var_os("HARNESS_EXECUTOR_FIXTURE_CHILD_MARKER") {
                write_identity(PathBuf::from(marker))?;
            }
            wait_for_release();
            return Ok(());
        }
        "descendant" => {
            // The descendant exists before the first event, so an interruption
            // or observer failure must reap a real grandchild process. The hold
            // keeps producing events so a failing observer notices immediately.
            let _descendant = std::process::Command::new(env::current_exe()?)
                .env("HARNESS_EXECUTOR_FIXTURE_MODE", "child-hold")
                // The descendant records its own identity marker only.
                .env_remove("HARNESS_EXECUTOR_FIXTURE_STARTED")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            event(json!({"type": "thread.started", "thread_id": session}));
            event(json!({"type": "turn.started"}));
            event(json!({"type": "item.completed", "item": {
                "id": "message-1", "type": "agent_message", "text": "descendant fixture is holding"
            }}));
            let release = env::var_os("HARNESS_EXECUTOR_FIXTURE_RELEASE").map(PathBuf::from);
            let until = Instant::now() + Duration::from_secs(60);
            let mut round = 0;
            while Instant::now() < until {
                if release.as_deref().is_some_and(|path| path.is_file()) {
                    break;
                }
                round += 1;
                event(json!({"type": "item.completed", "item": {
                    "id": format!("hold-{round}"), "type": "agent_message",
                    "text": format!("descendant hold {round}")
                }}));
                thread::sleep(Duration::from_millis(400));
            }
            return exit(3);
        }
        "stderr-noise" => {
            eprintln!("FIXTURE_STDERR_SENTINEL: owned failure detail");
            event(json!({"type": "thread.started", "thread_id": session}));
            event(json!({"type": "turn.started"}));
            return exit(19);
        }
        "nonzero" => {
            eprintln!("fixture launcher fails before any native event");
            return exit(19);
        }
        "error" => {
            event(json!({"type": "thread.started", "thread_id": session}));
            event(json!({"type": "turn.started"}));
            event(json!({"type": "error", "message": "fixture provider error: synthetic failure"}));
            event(json!({"type": "turn.failed", "error": {"message": "fixture turn failed"}}));
            if let Some(path) = &result {
                write_message(path, Some("partial output before failure"))?;
            }
            return exit(1);
        }
        "malformed" => {
            event(json!({"type": "thread.started", "thread_id": session}));
            println!("this line is not a native event");
            event(json!({"type": "turn.started"}));
            event(json!({"type": "turn.completed", "usage": usage()}));
            if let Some(path) = &result {
                write_message(path, Some(FINAL_MESSAGE))?;
            }
            return Ok(());
        }
        "truncated" => {
            event(json!({"type": "thread.started", "thread_id": session}));
            event(json!({"type": "turn.started"}));
            print!("{{\"type\":\"item.started\",\"item\":{{\"id\":\"truncated");
            io::stdout().flush()?;
            return exit(5);
        }
        "empty" => {
            complete_stream(&session, None)?;
            if let Some(path) = &result {
                write_message(path, Some(""))?;
            }
            return Ok(());
        }
        "nofinal" => {
            complete_stream(&session, None)?;
            // No final-message file at all: the CLI "completed" without
            // writing its recorded result.
            return Ok(());
        }
        "slow" => {
            let delay_ms = env::var("HARNESS_EXECUTOR_FIXTURE_DELAY_MS")
                .ok()
                .and_then(|text| text.parse::<u64>().ok())
                .unwrap_or(1500);
            event(json!({"type": "thread.started", "thread_id": session}));
            event(json!({"type": "turn.started"}));
            event(json!({"type": "item.completed", "item": {
                "id": "message-1", "type": "agent_message", "text": "working on the assignment"
            }}));
            thread::sleep(Duration::from_millis(delay_ms));
            event(json!({"type": "item.completed", "item": {
                "id": "message-2", "type": "agent_message", "text": FINAL_MESSAGE
            }}));
            event(json!({"type": "turn.completed", "usage": usage()}));
            if let Some(path) = &result {
                write_message(path, Some(FINAL_MESSAGE))?;
            }
            return Ok(());
        }
        "hang" => {
            event(json!({"type": "thread.started", "thread_id": session}));
            event(json!({"type": "turn.started"}));
            event(json!({"type": "item.started", "item": {
                "id": "command-1", "type": "command_execution", "command": "fixture long command"
            }}));
            wait_for_release();
            return exit(7);
        }
        _ => {}
    }
    complete_stream(&session, Some(FINAL_MESSAGE))?;
    if let Some(path) = &result {
        write_message(path, Some(FINAL_MESSAGE))?;
    }
    Ok(())
}

/// The ordinary successful turn: identity, one tool item, one assistant
/// message and a completed turn.
fn complete_stream(session: &str, message: Option<&str>) -> io::Result<()> {
    event(json!({"type": "thread.started", "thread_id": session}));
    event(json!({"type": "turn.started"}));
    event(json!({"type": "item.started", "item": {
        "id": "tool-1", "type": "command_execution", "command": "fixture check"
    }}));
    event(json!({"type": "item.completed", "item": {
        "id": "tool-1", "type": "command_execution", "command": "fixture check",
        "aggregated_output": "fixture output", "exit_code": 0, "status": "completed"
    }}));
    if let Some(text) = message {
        event(json!({"type": "item.completed", "item": {
            "id": "message-1", "type": "agent_message", "text": text
        }}));
    }
    event(json!({"type": "turn.completed", "usage": usage()}));
    Ok(())
}

fn usage() -> serde_json::Value {
    json!({
        "input_tokens": 100,
        "cached_input_tokens": 20,
        "cache_write_input_tokens": 0,
        "output_tokens": 50,
        "reasoning_output_tokens": 10
    })
}

fn event(value: serde_json::Value) {
    println!("{value}");
    let _ = io::stdout().flush();
}

/// Records this process's full identity (pid, creation time) so a check can
/// verify containment with a real identity instead of a bare pid.
fn write_identity(path: PathBuf) -> io::Result<()> {
    let program = env::current_exe()?;
    let user = harness_core::process_service::current_user()?;
    let identity = harness_core::process_service::ServiceProcess::observe(
        std::process::id(),
        &program,
        0,
        &user,
    )?
    .identity();
    fs::write(
        path,
        serde_json::to_vec(&identity).map_err(io::Error::other)?,
    )
}

/// Bounded hold used by `hang`/`descendant`: a release file lets the check end
/// it deterministically, and the 60-second bound prevents a leaked fixture.
fn wait_for_release() {
    let release = env::var_os("HARNESS_EXECUTOR_FIXTURE_RELEASE").map(PathBuf::from);
    let until = Instant::now() + Duration::from_secs(60);
    while Instant::now() < until {
        if release.as_deref().is_some_and(|path| path.is_file()) {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn last_message_path(args: Vec<std::ffi::OsString>) -> Option<PathBuf> {
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let text = arg.to_string_lossy().into_owned();
        if text == "--output-last-message" || text == "-o" {
            return iter.next().map(PathBuf::from);
        }
        if let Some(path) = text.strip_prefix("--output-last-message=") {
            return Some(PathBuf::from(path));
        }
    }
    None
}

fn write_message(path: &std::path::Path, text: Option<&str>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text.unwrap_or_default())
}

/// The fixture is a launcher double, so its own exit code must be settable.
fn exit(code: i32) -> io::Result<()> {
    std::process::exit(code)
}
