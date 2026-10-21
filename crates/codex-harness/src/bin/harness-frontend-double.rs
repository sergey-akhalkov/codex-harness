//! Owned frontend double for the executor observation checks.
//!
//! By default it is not a renderer and does not speak the app-server protocol.
//! It records the arguments the host actually passed, sets the console caption
//! the host uses as attachment evidence, and waits until the test releases it.
//! The production path launches the native Codex TUI instead.
//!
//! `HARNESS_FRONTEND_PROTOCOL_MODE=new|resume|fork|transfer` opts into one native
//! frontend flow against the endpoint named by the actual `--remote` argument.
//! It authenticates with the capability token named by
//! `--remote-auth-token-env`, initializes the connection, selects the thread
//! with `thread/start`, `thread/resume` or `thread/fork`, records the exact
//! returned `result.thread.id`, checks whether the host registered that thread
//! before sending one synthetic `turn/start`, and then retains the ordinary
//! release lifetime. `HARNESS_FRONTEND_THREAD` supplies the explicit source id
//! for resume and fork. The mode never lists or opens unrelated sessions and
//! never contacts a model: checks point it at their own canned endpoint.
//! `transfer` selects the source, completes its first turn, repeats the same
//! selection, waits for the check to inspect the retained lifecycle, forks the
//! explicit `HARNESS_FRONTEND_FORK_THREAD`, and waits again before that new
//! thread's first turn. `HARNESS_FRONTEND_EXIT_ON_FILE` makes the double exit
//! after that marker appears, so the host's late-selection cleanup is observed.

use harness_core::task_control::ControlConnection;
use serde_json::{Value, json};
use std::{
    env, fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const PROTOCOL_MODE_ENV: &str = "HARNESS_FRONTEND_PROTOCOL_MODE";
const PROTOCOL_THREAD_ENV: &str = "HARNESS_FRONTEND_THREAD";
const PROTOCOL_FORK_THREAD_ENV: &str = "HARNESS_FRONTEND_FORK_THREAD";
const EXIT_ON_FILE_ENV: &str = "HARNESS_FRONTEND_EXIT_ON_FILE";
const REMOTE_ARG: &str = "--remote";
const TOKEN_ENV_ARG: &str = "--remote-auth-token-env";
const SELECTED_FILE: &str = "frontend-selected-thread.txt";
const REGISTRY_FILE: &str = "frontend-registry-before-turn.txt";
const TRANSFER_REPEAT_FILE: &str = "frontend-transfer-repeat.txt";
const TRANSFER_BEFORE_FORK_FILE: &str = "frontend-transfer-before-fork";
const TRANSFER_TURN_GATE_FILE: &str = "frontend-transfer-turn-gate";
const TRANSFER_TURN_FILE: &str = "frontend-transfer-turn-started.txt";
const EXIT_MARKER_FILE: &str = "frontend-exit-triggered.txt";
/// The one synthetic assignment this fixture submits after selecting its
/// thread. It contains no user data and no task identity.
const SYNTHETIC_INPUT: &str = "harness frontend double: synthetic protocol turn";
/// Bound on the capability connect and on one send.
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Bound on one request/answer exchange after the connection exists.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const RELEASE_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    New,
    Resume,
    Fork,
    Transfer,
}

fn main() {
    let home = env::var_os("CODEX_HOME").map(PathBuf::from);
    if let Some(home) = &home {
        let _ = fs::write(
            home.join("frontend-argv.txt"),
            env::args().skip(1).fold(String::new(), |mut acc, arg| {
                if !acc.is_empty() {
                    acc.push('\n');
                }
                acc.push_str(&arg);
                acc
            }),
        );
    }
    let title = home
        .as_ref()
        .and_then(|path| fs::read_to_string(path.join("frontend-title.txt")).ok())
        .unwrap_or_default();
    let title = title.trim();
    if title.is_empty() {
        eprintln!("frontend double: frontend-title.txt is missing");
        std::process::exit(2);
    }
    if let Err(error) = harness_core::task_control::set_console_caption(&format!("{title} | ready"))
    {
        eprintln!("frontend double caption: {error}");
        std::process::exit(3);
    }
    let Some(home) = home else {
        std::process::exit(2);
    };
    install_exit_trigger(&home);

    let _protocol = match protocol_mode() {
        Ok(None) => None,
        Ok(Some(mode)) => match run_protocol(&home, mode) {
            Ok(connection) => Some(connection),
            Err(error) => {
                eprintln!("frontend double protocol: {error}");
                std::process::exit(4);
            }
        },
        Err(error) => {
            eprintln!("frontend double protocol: {error}");
            std::process::exit(4);
        }
    };

    let release = home.join("frontend-release");
    let until = Instant::now() + RELEASE_TIMEOUT;
    while Instant::now() < until && !release.is_file() {
        thread::sleep(Duration::from_millis(40));
    }
}

/// Absence or an empty value keeps the original double. A named but unknown
/// mode is a fixture contract error, not a silent fallback.
fn protocol_mode() -> Result<Option<Mode>, String> {
    let Some(value) = env::var_os(PROTOCOL_MODE_ENV) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .ok_or_else(|| format!("{PROTOCOL_MODE_ENV} is not valid UTF-8"))?;
    if value.is_empty() {
        return Ok(None);
    }
    match value {
        "new" => Ok(Some(Mode::New)),
        "resume" => Ok(Some(Mode::Resume)),
        "fork" => Ok(Some(Mode::Fork)),
        "transfer" => Ok(Some(Mode::Transfer)),
        other => Err(format!(
            "unsupported {PROTOCOL_MODE_ENV} value {other:?}; expected new, resume, fork or transfer"
        )),
    }
}

/// Test-only exit path: when the check names a marker, this double exits as
/// soon as that file appears. The marker the double writes first lets the check
/// distinguish "frontend has begun exiting" from a slow protocol step.
fn install_exit_trigger(home: &Path) {
    let Some(trigger) = env::var_os(EXIT_ON_FILE_ENV).map(PathBuf::from) else {
        return;
    };
    let marker = home.join(EXIT_MARKER_FILE);
    thread::spawn(move || {
        loop {
            if trigger.is_file() {
                let _ = fs::write(marker, b"exiting");
                std::process::exit(0);
            }
            thread::sleep(Duration::from_millis(20));
        }
    });
}

/// Performs the one opted-in frontend flow and returns the live connection so
/// it stays attached for the ordinary release lifetime.
fn run_protocol(home: &Path, mode: Mode) -> Result<ControlConnection, String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let remote = argument(&args, REMOTE_ARG)
        .ok_or_else(|| format!("{REMOTE_ARG} is missing from the frontend arguments"))?;
    let token_env = argument(&args, TOKEN_ENV_ARG)
        .ok_or_else(|| format!("{TOKEN_ENV_ARG} is missing from the frontend arguments"))?;
    let port = remote_port(remote)?;
    let token = env::var(token_env).map_err(|error| {
        format!("the capability environment {token_env} is unavailable: {error}")
    })?;
    let source = match mode {
        Mode::New | Mode::Transfer => None,
        Mode::Resume | Mode::Fork => Some(
            env::var(PROTOCOL_THREAD_ENV)
                .map_err(|error| format!("{PROTOCOL_THREAD_ENV} is unavailable: {error}"))?,
        ),
    };
    if let Some(source) = source.as_deref() {
        validate_thread_id(source, PROTOCOL_THREAD_ENV)?;
    }

    let mut connection = ControlConnection::connect(port, &token, IO_TIMEOUT)
        .map_err(|error| format!("capability connection to {remote} failed: {error}"))?;
    call(
        &mut connection,
        1,
        "initialize",
        json!({
            "clientInfo": {
                "name": "harness-frontend-double",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {"experimentalApi": true},
        }),
    )?;
    connection
        .send(&json!({"method": "initialized"}), IO_TIMEOUT)
        .map_err(|error| format!("initialized notification failed: {error}"))?;

    if mode == Mode::Transfer {
        return run_transfer(home, connection);
    }

    let (method, params) = match mode {
        Mode::New => {
            let cwd = env::current_dir().map_err(|error| {
                format!("the frontend working directory is unavailable: {error}")
            })?;
            ("thread/start", json!({"cwd": cwd.to_string_lossy()}))
        }
        Mode::Resume => (
            "thread/resume",
            json!({"threadId": source.as_deref().unwrap_or_default()}),
        ),
        Mode::Fork => (
            "thread/fork",
            json!({"threadId": source.as_deref().unwrap_or_default()}),
        ),
        Mode::Transfer => unreachable!("transfer returns before the single-selection flow"),
    };
    let selected = call(&mut connection, 2, method, params)?;
    let selected = selected
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{method} result has no thread.id"))?;
    validate_thread_id(selected, &format!("{method} result thread.id"))?;
    fs::write(home.join(SELECTED_FILE), selected)
        .map_err(|error| format!("the selected thread could not be recorded: {error}"))?;

    // This is intentionally a point-in-time existence check. Waiting here
    // would hide the production ordering this fixture exists to observe.
    let registry = home
        .join("harness")
        .join("lead-endpoints")
        .join(format!("{selected}.json"));
    let registered = registry.is_file();
    fs::write(
        home.join(REGISTRY_FILE),
        if registered { "true" } else { "false" },
    )
    .map_err(|error| format!("the registry readiness could not be recorded: {error}"))?;

    let started = call(
        &mut connection,
        3,
        "turn/start",
        json!({
            "threadId": selected,
            "input": [{"type": "text", "text": SYNTHETIC_INPUT}],
        }),
    )?;
    if started
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("turn/start result has no turn.id".to_owned());
    }
    Ok(connection)
}

/// Exercises the ordinary host's current-selection transfer with the same
/// native operations a frontend uses: resume old, repeat the same selection,
/// then fork a new thread for the next turn. The two gates keep the check able
/// to observe the retained lifecycle before the next native selection begins.
fn run_transfer(
    home: &Path,
    mut connection: ControlConnection,
) -> Result<ControlConnection, String> {
    let source = env::var(PROTOCOL_THREAD_ENV)
        .map_err(|error| format!("{PROTOCOL_THREAD_ENV} is unavailable: {error}"))?;
    let fork = env::var(PROTOCOL_FORK_THREAD_ENV)
        .map_err(|error| format!("{PROTOCOL_FORK_THREAD_ENV} is unavailable: {error}"))?;
    validate_thread_id(&source, PROTOCOL_THREAD_ENV)?;
    validate_thread_id(&fork, PROTOCOL_FORK_THREAD_ENV)?;

    let selected = select(
        &mut connection,
        2,
        "thread/resume",
        json!({"threadId": source}),
        home,
    )?;
    start_turn(&mut connection, 3, &selected)?;

    // Repeating the same selection must be an idempotent host no-op rather
    // than a second registry write or lifecycle transfer.
    let repeated = select(
        &mut connection,
        4,
        "thread/resume",
        json!({"threadId": source}),
        home,
    )?;
    if repeated != selected {
        return Err(format!(
            "repeating the same resume selected {repeated} instead of {selected}"
        ));
    }
    fs::write(home.join(TRANSFER_REPEAT_FILE), &repeated)
        .map_err(|error| format!("the repeated selection could not be recorded: {error}"))?;
    wait_for_file(&home.join(TRANSFER_BEFORE_FORK_FILE), "before-fork gate")?;

    let forked = select(
        &mut connection,
        5,
        "thread/fork",
        json!({"threadId": source}),
        home,
    )?;
    if forked != fork {
        return Err(format!("thread/fork selected {forked} instead of {fork}"));
    }
    wait_for_file(&home.join(TRANSFER_TURN_GATE_FILE), "new-turn gate")?;
    start_turn(&mut connection, 6, &forked)?;
    fs::write(home.join(TRANSFER_TURN_FILE), &forked)
        .map_err(|error| format!("the new turn could not be recorded: {error}"))?;
    Ok(connection)
}

/// Selects one thread through the actual request and records the exact result
/// plus the point-in-time host registry readiness used by the existing checks.
fn select(
    connection: &mut ControlConnection,
    id: u64,
    method: &str,
    params: Value,
    home: &Path,
) -> Result<String, String> {
    let selected = call(connection, id, method, params)?;
    let selected = selected
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{method} result has no thread.id"))?;
    validate_thread_id(selected, &format!("{method} result thread.id"))?;
    fs::write(home.join(SELECTED_FILE), selected)
        .map_err(|error| format!("the selected thread could not be recorded: {error}"))?;
    let registry = home
        .join("harness")
        .join("lead-endpoints")
        .join(format!("{selected}.json"));
    fs::write(
        home.join(REGISTRY_FILE),
        if registry.is_file() { "true" } else { "false" },
    )
    .map_err(|error| format!("the registry readiness could not be recorded: {error}"))?;
    Ok(selected.to_owned())
}

fn start_turn(connection: &mut ControlConnection, id: u64, thread: &str) -> Result<(), String> {
    let started = call(
        connection,
        id,
        "turn/start",
        json!({
            "threadId": thread,
            "input": [{"type": "text", "text": SYNTHETIC_INPUT}],
        }),
    )?;
    if started
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("turn/start result has no turn.id".to_owned());
    }
    Ok(())
}

fn wait_for_file(path: &Path, name: &str) -> Result<(), String> {
    let until = Instant::now() + REQUEST_TIMEOUT;
    while Instant::now() < until {
        if path.is_file() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
    Err(format!("the {name} {} did not appear", path.display()))
}

/// Reads one option from the actual argv, accepting both the separated and
/// `--name=value` spellings the launcher may pass.
fn argument<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().enumerate().find_map(|(index, argument)| {
        if argument == name {
            args.get(index + 1)
                .map(String::as_str)
                .filter(|value| !value.is_empty())
        } else {
            argument
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix('='))
                .filter(|value| !value.is_empty())
        }
    })
}

/// Only the exact loopback shape the ordinary host emits is accepted. The
/// fixture never discovers or connects to another address.
fn remote_port(remote: &str) -> Result<u16, String> {
    let address = remote
        .strip_prefix("ws://")
        .ok_or_else(|| format!("{REMOTE_ARG} must be a ws:// loopback URL, got {remote:?}"))?;
    let (host, port) = address.rsplit_once(':').ok_or_else(|| {
        format!("{REMOTE_ARG} must name a loopback host and port, got {remote:?}")
    })?;
    if host != "127.0.0.1" {
        return Err(format!(
            "{REMOTE_ARG} must name 127.0.0.1, got {host:?}; refusing to connect elsewhere"
        ));
    }
    let port = port
        .parse::<u16>()
        .map_err(|error| format!("{REMOTE_ARG} has no valid port in {remote:?}: {error}"))?;
    if port == 0 {
        return Err(format!("{REMOTE_ARG} must name a nonzero port"));
    }
    Ok(port)
}

/// Thread ids become registry file names, so only the native token shape is
/// accepted. This also rejects separators and traversal before any write.
fn validate_thread_id(thread: &str, label: &str) -> Result<(), String> {
    let valid = !thread.is_empty()
        && thread.len() <= 128
        && thread
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err(format!("{label} is not a native thread id: {thread:?}"))
    }
}

/// Sends one request and returns its own result, ignoring notifications and
/// answers to other requests until the bounded deadline.
fn call(
    connection: &mut ControlConnection,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    connection
        .send(
            &json!({"id": id, "method": method, "params": params}),
            IO_TIMEOUT,
        )
        .map_err(|error| format!("{method} request failed: {error}"))?;
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!(
                "no answer to {method} within {}s",
                REQUEST_TIMEOUT.as_secs()
            ));
        }
        let Some(answer) = connection
            .receive(remaining)
            .map_err(|error| format!("{method} answer failed: {error}"))?
        else {
            return Err(format!(
                "no answer to {method} within {}s",
                REQUEST_TIMEOUT.as_secs()
            ));
        };
        if answer.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(error) = answer.get("error") {
            return Err(format!("{method} was refused: {error}"));
        }
        return Ok(answer.get("result").cloned().unwrap_or(Value::Null));
    }
}
