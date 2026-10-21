#![cfg(windows)]
//! Contract checks for the opt-in protocol mode of the owned frontend double.
//!
//! Every endpoint here is a canned loopback WebSocket owned by the test. No
//! check starts a model, a subscription or the installed Codex CLI. The double
//! is still the real binary the host launches; only its counterpart is canned.

#[path = "fixtures/control_endpoint.rs"]
mod control_endpoint;

use control_endpoint::{Answer, Bearer, Server};
use harness_core::{
    console::{ConsoleOutcome, ConsoleSession, ConsoleSpec},
    process::{Cancellation, CommandSpec, Deadline},
};
use serde_json::json;
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

const DOUBLE: &str = env!("CARGO_BIN_EXE_harness-frontend-double");
const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TOKEN_ENV: &str = "HARNESS_FRONTEND_FIXTURE_TOKEN";
const MODE_ENV: &str = "HARNESS_FRONTEND_PROTOCOL_MODE";
const THREAD_ENV: &str = "HARNESS_FRONTEND_THREAD";
const TITLE: &str = "CEx (fixture) - protocol frontend";
const NEW_THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
const TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f2401";
const RESUME_SOURCE: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f5";
const FORK_SOURCE: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f6";
const FORK_CHILD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f7";
const SYNTHETIC_INPUT: &str = "harness frontend double: synthetic protocol turn";
const WAIT: Duration = Duration::from_secs(20);
const CLEANUP: Duration = Duration::from_secs(5);

struct Fixture {
    home: tempfile::TempDir,
    server: Server,
}

impl Fixture {
    fn new(configure: impl FnOnce(&Server)) -> Self {
        let server = Server::start_on_with(0, Bearer::Value(TOKEN.to_owned()), configure);
        Self {
            home: tempfile::tempdir().unwrap(),
            server,
        }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn remote(&self) -> String {
        format!("ws://127.0.0.1:{}", self.server.port)
    }

    fn prepare_title(&self) {
        fs::write(self.home().join("frontend-title.txt"), TITLE).unwrap();
    }

    fn selected(&self) -> std::path::PathBuf {
        self.home().join("frontend-selected-thread.txt")
    }

    fn readiness(&self) -> std::path::PathBuf {
        self.home().join("frontend-registry-before-turn.txt")
    }

    fn release(&self, session: ConsoleSession) -> ConsoleOutcome {
        fs::write(self.home().join("frontend-release"), b"release").unwrap();
        wait_session(session)
    }
}

fn spawn(fixture: &Fixture, mode: Option<&str>, source: Option<&str>) -> ConsoleSession {
    spawn_with_token(fixture, mode, source, TOKEN)
}

fn spawn_with_token(
    fixture: &Fixture,
    mode: Option<&str>,
    source: Option<&str>,
    token: &str,
) -> ConsoleSession {
    fixture.prepare_title();
    let mut spec = CommandSpec::new(DOUBLE);
    spec.args = vec![
        "--remote".into(),
        fixture.remote().into(),
        "--remote-auth-token-env".into(),
        TOKEN_ENV.into(),
        "-c".into(),
        "features.worktrees=false".into(),
        "resume".into(),
        "argv-thread-is-not-the-protocol-selection".into(),
    ];
    spec.env.insert(
        "CODEX_HOME".into(),
        Some(fixture.home().as_os_str().to_owned()),
    );
    spec.env.insert(TOKEN_ENV.into(), Some(token.into()));
    spec.env
        .insert(MODE_ENV.into(), mode.map(std::ffi::OsString::from));
    spec.env
        .insert(THREAD_ENV.into(), source.map(std::ffi::OsString::from));
    ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap()
}

fn wait_session(session: ConsoleSession) -> ConsoleOutcome {
    session
        .wait(
            Deadline::after(Duration::from_secs(30)).unwrap(),
            &Cancellation::default(),
            CLEANUP,
        )
        .unwrap()
}

fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if condition() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    condition()
}

fn text(outcome: &ConsoleOutcome) -> String {
    outcome.transcript.replace("\r\n", "\n")
}

fn methods(server: &Server) -> Vec<String> {
    server
        .requests()
        .into_iter()
        .filter_map(|request| request["method"].as_str().map(str::to_owned))
        .collect()
}

fn initialize(server: &Server) {
    server.answer("initialize", Answer::Result(json!({})));
}

#[test]
fn new_mode_selects_the_canned_thread_and_records_true_before_the_turn() {
    let fixture = Fixture::new(|server| {
        initialize(server);
        server.answer(
            "thread/start",
            Answer::Result(json!({"thread": {"id": NEW_THREAD}})),
        );
        server.answer(
            "turn/start",
            Answer::Result(json!({"turn": {"id": TURN, "status": "inProgress"}})),
        );
    });
    let registry = fixture
        .home()
        .join(format!("harness/lead-endpoints/{NEW_THREAD}.json"));
    fs::create_dir_all(registry.parent().unwrap()).unwrap();
    fs::write(&registry, b"{}").unwrap();

    let session = spawn(&fixture, Some("new"), None);
    if !wait_until(|| !fixture.server.requests_for("turn/start").is_empty()) {
        let finished = fixture.release(session);
        panic!(
            "the fixture never submitted its one synthetic turn: {}",
            text(&finished)
        );
    }

    assert_eq!(fs::read_to_string(fixture.selected()).unwrap(), NEW_THREAD);
    assert_eq!(fs::read_to_string(fixture.readiness()).unwrap(), "true");
    assert_eq!(
        methods(&fixture.server),
        vec!["initialize", "initialized", "thread/start", "turn/start"]
    );

    let initialized = &fixture.server.requests_for("initialize")[0];
    assert_eq!(
        initialized["params"]["clientInfo"]["name"],
        "harness-frontend-double"
    );
    assert!(
        initialized["params"]["clientInfo"]["version"]
            .as_str()
            .is_some_and(|version| !version.is_empty()),
        "{initialized}"
    );
    let started = &fixture.server.requests_for("thread/start")[0];
    assert!(
        started["params"]["cwd"]
            .as_str()
            .is_some_and(|cwd| !cwd.is_empty()),
        "{started}"
    );
    let turned = &fixture.server.requests_for("turn/start")[0];
    assert_eq!(turned["params"]["threadId"], NEW_THREAD);
    assert_eq!(
        turned["params"]["input"],
        json!([{"type": "text", "text": SYNTHETIC_INPUT}])
    );

    let argv = fs::read_to_string(fixture.home().join("frontend-argv.txt")).unwrap();
    assert!(argv.contains("--remote"), "{argv}");
    assert!(argv.contains(&fixture.remote()), "{argv}");
    assert!(!argv.contains(TOKEN), "the capability token leaked: {argv}");
    let finished = fixture.release(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", text(&finished));
}

#[test]
fn absent_registration_is_recorded_false_without_blocking_the_turn() {
    let fixture = Fixture::new(|server| {
        initialize(server);
        server.answer(
            "thread/start",
            Answer::Result(json!({"thread": {"id": NEW_THREAD}})),
        );
        server.answer("turn/start", Answer::Result(json!({"turn": {"id": TURN}})));
    });

    let session = spawn(&fixture, Some("new"), None);
    assert!(
        wait_until(|| fixture.readiness().is_file()),
        "the fixture never recorded registry readiness"
    );
    assert_eq!(fs::read_to_string(fixture.readiness()).unwrap(), "false");
    assert!(
        wait_until(|| !fixture.server.requests_for("turn/start").is_empty()),
        "the fixture waited for registration instead of submitting the turn"
    );
    assert!(
        !fixture
            .home()
            .join(format!("harness/lead-endpoints/{NEW_THREAD}.json"))
            .exists()
    );
    let finished = fixture.release(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", text(&finished));
}

#[test]
fn resume_and_fork_use_the_explicit_source_and_the_returned_identity() {
    for (mode, method, source, returned) in [
        ("resume", "thread/resume", RESUME_SOURCE, RESUME_SOURCE),
        ("fork", "thread/fork", FORK_SOURCE, FORK_CHILD),
    ] {
        let fixture = Fixture::new(|server| {
            initialize(server);
            server.answer(method, Answer::Result(json!({"thread": {"id": returned}})));
            server.answer("turn/start", Answer::Result(json!({"turn": {"id": TURN}})));
        });
        let registry = fixture
            .home()
            .join(format!("harness/lead-endpoints/{returned}.json"));
        fs::create_dir_all(registry.parent().unwrap()).unwrap();
        fs::write(&registry, b"{}").unwrap();

        let session = spawn(&fixture, Some(mode), Some(source));
        assert!(
            wait_until(|| !fixture.server.requests_for("turn/start").is_empty()),
            "{mode} mode submitted no turn"
        );
        assert_eq!(fs::read_to_string(fixture.selected()).unwrap(), returned);
        assert_eq!(fs::read_to_string(fixture.readiness()).unwrap(), "true");

        let selection = fixture.server.requests_for(method);
        assert_eq!(selection.len(), 1, "{mode}: {selection:?}");
        assert_eq!(selection[0]["params"]["threadId"], source);
        assert!(
            fixture.server.requests_for("thread/start").is_empty(),
            "{mode} mode started a different thread"
        );
        assert!(
            fixture.server.requests_for("thread/list").is_empty(),
            "{mode} mode discovered historical sessions"
        );
        let turned = &fixture.server.requests_for("turn/start")[0];
        assert_eq!(turned["params"]["threadId"], returned);
        assert_eq!(
            turned["params"]["input"],
            json!([{"type": "text", "text": SYNTHETIC_INPUT}])
        );
        let finished = fixture.release(session);
        assert_eq!(finished.outcome.exit_code, 0, "{mode}: {}", text(&finished));
    }
}

#[test]
fn selection_refusal_is_observable_and_submits_no_turn() {
    let fixture = Fixture::new(|server| {
        initialize(server);
        server.answer(
            "thread/start",
            Answer::Error(json!({"code": -32600, "message": "selection refused"})),
        );
    });
    let session = spawn(&fixture, Some("new"), None);
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_eq!(finished.outcome.exit_code, 4, "{transcript}");
    assert!(
        transcript.contains("thread/start was refused"),
        "{transcript}"
    );
    assert!(transcript.contains("selection refused"), "{transcript}");
    assert!(fixture.server.requests_for("turn/start").is_empty());
    assert!(!fixture.selected().exists());
    assert!(!fixture.readiness().exists());
}

#[test]
fn malformed_selection_identity_is_rejected_before_any_write_or_turn() {
    for (label, result) in [
        ("missing id", json!({"thread": {}})),
        ("path traversal", json!({"thread": {"id": "../escape"}})),
    ] {
        let fixture = Fixture::new(move |server| {
            initialize(server);
            server.answer("thread/start", Answer::Result(result));
        });
        let session = spawn(&fixture, Some("new"), None);
        let finished = wait_session(session);
        let transcript = text(&finished);
        assert_eq!(finished.outcome.exit_code, 4, "{label}: {transcript}");
        assert!(transcript.contains("thread.id"), "{label}: {transcript}");
        assert!(
            fixture.server.requests_for("turn/start").is_empty(),
            "{label} submitted a turn"
        );
        assert!(!fixture.selected().exists(), "{label} recorded a selection");
        assert!(!fixture.readiness().exists(), "{label} recorded readiness");
        assert!(
            !fixture.home().join("harness/escape.json").exists(),
            "{label} escaped the registry directory"
        );
    }
}

#[test]
fn capability_handshake_failure_is_observable() {
    let fixture = Fixture::new(initialize);
    let wrong = "f".repeat(TOKEN.len());
    let session = spawn_with_token(&fixture, Some("new"), None, &wrong);
    let finished = wait_session(session);
    let transcript = text(&finished);
    assert_ne!(finished.outcome.exit_code, 0, "{transcript}");
    assert!(transcript.contains("capability connection"), "{transcript}");
    assert!(fixture.server.requests().is_empty());
    assert!(fixture.server.refusals() > 0);
    assert!(!fixture.selected().exists());
}

#[test]
fn absent_mode_keeps_the_original_double_without_connecting() {
    let fixture = Fixture::new(|_| {});
    let session = spawn(&fixture, None, None);
    assert!(
        wait_until(|| fixture.home().join("frontend-argv.txt").is_file()),
        "the default double did not record its arguments"
    );
    thread::sleep(Duration::from_millis(250));
    assert_eq!(fixture.server.connections(), 0);
    assert!(!fixture.selected().exists());
    assert!(!fixture.readiness().exists());
    let argv = fs::read_to_string(fixture.home().join("frontend-argv.txt")).unwrap();
    assert!(argv.contains("--remote"), "{argv}");
    let finished = fixture.release(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", text(&finished));
}
