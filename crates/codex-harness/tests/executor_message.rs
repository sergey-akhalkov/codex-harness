#![cfg(windows)]

//! Native checks of `executor message` through the real CLI entry point: one
//! bound pooled slot whose receipt, lease, host process, app-server child record
//! and kit-local control endpoint are the ones a real dispatch writes, with a
//! canned owned WebSocket endpoint answering the control protocol. The text and
//! file payloads, the active-turn steering, the queued/delivered/error/
//! indeterminate classification, the repeat rules, the refusals and the
//! unsupported-surface results are exercised without a model, a subscription or
//! the installed CLI.

#[path = "fixtures/control_endpoint.rs"]
mod control_endpoint;

use control_endpoint::{Answer, Bearer, Server};
use harness_core::console::{ConsoleSession, ConsoleSpec};
use harness_core::process::{Cancellation, CommandSpec, Deadline, ProcessIdentity};
use harness_core::process_service::{self, ServiceProcess};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const TOKEN: &str = "9f0c1d2e3a4b5c6d7e8f90123456789abcdef0123456789abcdef0123456789a";
const THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
const TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f2401";
const NEXT_TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f2402";
const MODEL: &str = "deepseek-v4-flash";
const PROVIDER: &str = "deepseek-fixture";
const EFFORT: &str = "max";
const OWNER: &str = "exec-message-fixture";
const FINAL: &str = "CONTROL_FIXTURE_FINAL_MESSAGE";
const FIXTURE: &str = env!("CARGO_BIN_EXE_harness-executor-fixture");
const WAIT: Duration = Duration::from_secs(30);

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

fn launcher() -> PathBuf {
    PathBuf::from(FIXTURE)
}

/// A lead invocation: no inherited executor marker, so the kit's nested
/// dispatch refusal does not apply to the calling side.
fn lead_command() -> Command {
    let mut command = Command::new(manager());
    command.env_remove("HARNESS_EXECUTOR_SESSION");
    command.env_remove("WT_SESSION");
    command
}

fn output_text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn orchestration() -> String {
    "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\nworktree_limit = 1\n".to_owned()
}

/// Rewrites one fixture receipt as an owned test step: the address and identity
/// fields stay untouched, so the command still verifies the same run.
fn edit_receipt(path: &Path, edit: impl FnOnce(&mut Value)) {
    let mut receipt: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    edit(&mut receipt);
    fs::write(path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// The content identity `executor message` derives for one text on one thread:
/// SHA-256 over the thread id, a separator and the exact text, first 12 bytes as
/// lowercase hex.
fn content_id(text: &str) -> String {
    content_id_for(THREAD, text)
}

fn content_id_for(thread: &str, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(thread.as_bytes());
    hasher.update([0u8]);
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("msg-{hex}")
}

/// One `thread/read` answer: the addressed thread, running in the addressed
/// slot with the recorded binding, holding the given turns.
fn thread_read(slot: &Path, status: &str, turns: Value) -> Value {
    json!({"thread": {
        "id": THREAD,
        "sessionId": THREAD,
        "cwd": slot,
        "model": MODEL,
        "modelProvider": PROVIDER,
        "reasoningEffort": EFFORT,
        "status": {"type": status},
        "turns": turns
    }})
}

/// The running assignment turn every fixture starts from.
fn active_turns() -> Value {
    json!([{
        "id": TURN,
        "status": "inProgress",
        "items": [{"id": "c1", "type": "commandExecution", "command": "fixture long command"}]
    }])
}

/// The assignment turn as the conversation's record shows it once the delivered
/// input is in the thread: a `userMessage` item correlated by the recorded
/// client message id.
fn turns_with_input(text: &str) -> Value {
    json!([
        {
            "id": TURN,
            "status": "inProgress",
            "items": [
                {"id": "c1", "type": "commandExecution", "command": "fixture long command"},
                {
                    "id": "u1",
                    "type": "userMessage",
                    "clientId": content_id(text),
                    "content": [{"type": "text", "text": text}]
                }
            ]
        },
        {
            "id": NEXT_TURN,
            "status": "completed",
            "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
        }
    ])
}

/// One pooled exec run as a dispatch records it: the checkout with its
/// registered slot, the kit home and state directory, a bound slot record, a
/// live host process with its lease, a receipt with the control route and
/// observation, a live app-server child and the endpoint record that addresses
/// the canned endpoint.
struct Fixture {
    _root: tempfile::TempDir,
    source: PathBuf,
    home: PathBuf,
    slot: PathBuf,
    receipt: PathBuf,
    endpoint: PathBuf,
    lease: PathBuf,
    server: Server,
    host: Child,
    app_server: Child,
    release: PathBuf,
}

impl Fixture {
    fn new(name: &str, state: &str, control: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join(format!("proj-{name}"));
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "-q", "--initial-branch=main"]);
        git(&source, &["config", "user.email", "message@example.test"]);
        git(&source, &["config", "user.name", "Message"]);
        fs::create_dir_all(source.join("global")).unwrap();
        fs::write(source.join("global/orchestration.toml"), orchestration()).unwrap();
        fs::write(source.join("README.md"), "seed\n").unwrap();
        git(&source, &["add", "."]);
        git(&source, &["commit", "-qm", "seed"]);
        let slot = root.path().join(format!("proj-{name}-wt1"));
        git(
            &source,
            &["worktree", "add", "-q", "--detach", slot.to_str().unwrap()],
        );
        let home = root.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let state_dir = harness_core::task_worktree::pool_state_dir(&home, &source).unwrap();
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join("slot-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "source": source,
                "index": 1,
                "path": slot,
                "state": "occupied",
                "owner": OWNER,
                "base": "abc123",
                "disposition": null,
                "reason": null,
            }))
            .unwrap(),
        )
        .unwrap();
        let release = root.path().join("release");
        let (host, host_identity) = fixture_child(&root.path().join("host.json"), &release);
        let (app_server, child_identity) =
            fixture_child(&root.path().join("app-server.json"), &release);
        fs::write(
            state_dir.join("lease-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "owner": OWNER,
                "index": 1,
                "path": slot,
                "pid": host_identity["pid"],
                "created": host_identity["creation_time"],
                "program": launcher(),
            }))
            .unwrap(),
        )
        .unwrap();
        // The canned endpoint reads its bearer from the token file the control
        // driver writes, exactly as the installed app-server reads it through
        // `--ws-token-file`; a host that starts the conversation replaces both
        // the file and the endpoint record with its own values.
        fs::write(state_dir.join("endpoint-1.token"), TOKEN).unwrap();
        let server = Server::start(Bearer::File(state_dir.join("endpoint-1.token")));
        server.answer("initialize", Answer::Result(json!({})));
        server.answer(
            "thread/start",
            Answer::Result(json!({
                "thread": {"id": THREAD},
                "model": MODEL,
                "modelProvider": PROVIDER,
                "reasoningEffort": EFFORT,
            })),
        );
        server.answer("thread/name/set", Answer::Result(json!({})));
        server.answer(
            "thread/read",
            Answer::Result(thread_read(&slot, "active", active_turns())),
        );
        server.answer("turn/steer", Answer::Result(json!({"turnId": TURN})));
        server.answer(
            "turn/start",
            Answer::Result(json!({"turn": {"id": NEXT_TURN, "status": "inProgress"}})),
        );
        let receipt = state_dir.join("spawn-1.json");
        let endpoint = state_dir.join("endpoint-1.json");
        let lease = state_dir.join("lease-1.json");
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "launcher": launcher(),
                "profile": "ds",
                "mode": "exec",
                "args": [],
                "visible": true,
                "host": "owned-console",
                "control": if control {
                    json!({
                        "schema": 1,
                        "assignment": "fixture assignment text",
                        "identity": {
                            "profile": "ds",
                            "model": MODEL,
                            "modelProvider": PROVIDER,
                            "reasoningEffort": EFFORT,
                        },
                        "port": server.port,
                    })
                } else {
                    Value::Null
                },
                "terminal": null,
                "isolation": false,
                "slot": {
                    "index": 1,
                    "path": slot,
                    "source": source,
                    "owner": OWNER,
                    "base": "abc123",
                    "remote": "origin",
                    "branch": "main",
                },
                "model": MODEL,
                "modelProvider": PROVIDER,
                "reasoningEffort": EFFORT,
                "window": null,
                "shell": null,
                "observation": {
                    "schema": 1,
                    "coverage": "native",
                    "state": state,
                    "session": THREAD,
                    "exitCode": null,
                    "events": 3,
                    "messages": 1,
                    "toolCalls": 1,
                    "malformed": 0,
                    "cause": null,
                    "host": {
                        "pid": host_identity["pid"],
                        "created": host_identity["creation_time"],
                        "program": launcher(),
                    },
                    "result": state_dir.join("message-1.txt"),
                    "detail": state_dir.join("stream-1.jsonl"),
                    "updatedMs": now_ms(),
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &endpoint,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "port": server.port,
                "token": TOKEN,
                "threadId": THREAD,
                "process": {
                    "pid": child_identity["pid"],
                    "creationTime": child_identity["creation_time"],
                    "program": launcher(),
                }
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            _root: root,
            source,
            home,
            slot,
            receipt,
            endpoint,
            lease,
            server,
            host,
            app_server,
            release,
        }
    }

    /// Runs the real `executor message` entry point against this run.
    fn message(&self, extra: &[&str]) -> Output {
        lead_command()
            .args([
                "executor",
                "message",
                "--source",
                self.source.to_str().unwrap(),
                "--codex-home",
                self.home.to_str().unwrap(),
                "--slot",
                "1",
                "--owner",
                OWNER,
            ])
            .args(extra)
            .output()
            .unwrap()
    }

    fn receipt(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.receipt).unwrap()).unwrap()
    }

    fn attempts(&self) -> Vec<Value> {
        self.receipt()["messages"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // The fixture's own children hold until this file appears, so ending
        // them is deterministic instead of a kill of an unrelated process.
        let _ = fs::write(&self.release, b"release");
        for child in [&mut self.host, &mut self.app_server] {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Spawns one owned fixture process and returns it with the identity it
/// recorded (pid plus creation time). The child is waited for when its fixture
/// is dropped, and it ends by itself after its own bounded hold, so a failed
/// check leaves no process behind.
#[allow(clippy::zombie_processes)]
fn fixture_child(marker: &Path, release: &Path) -> (Child, Value) {
    let child = Command::new(launcher())
        .env("HARNESS_EXECUTOR_FIXTURE_MODE", "hang")
        .env("HARNESS_EXECUTOR_FIXTURE_STARTED", marker)
        .env("HARNESS_EXECUTOR_FIXTURE_RELEASE", release)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        if let Ok(bytes) = fs::read(marker)
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
        {
            return (child, value);
        }
        assert!(
            Instant::now() < deadline,
            "fixture identity marker {} never appeared",
            marker.display()
        );
        thread::sleep(Duration::from_millis(25));
    }
}

/// Bounded wait for one fixture observation.
fn wait_for(mut condition: impl FnMut() -> bool, reason: &str) {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("{reason}");
}

#[test]
fn message_delivers_literal_text_into_the_same_thread() {
    let fixture = Fixture::new("text", "running", true);
    // Nothing runs yet: the thread is idle, so the input opens a turn on the
    // same thread. The first read shows the idle thread, every later read the
    // conversation's own record of the delivered input.
    let delivered = "\u{43f}\u{440}\u{438}\u{432}\u{435}\u{442} from the lead\nsecond line: $(throw), %PATH%, `whoami`";
    fixture.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(thread_read(
                &fixture.slot,
                "idle",
                json!([{
                    "id": TURN,
                    "status": "completed",
                    "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
                }]),
            )),
            Answer::Result(thread_read(
                &fixture.slot,
                "active",
                turns_with_input(delivered),
            )),
        ],
    );
    let out = fixture.message(&["--text", delivered]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    assert!(
        text.contains("observed as userMessage u1 of turn") && text.contains(THREAD),
        "{text}"
    );

    // The literal text travels once, to the recorded thread, as one user input
    // with the recorded client message id - no new conversation is started.
    assert!(
        fixture.server.requests_for("thread/start").is_empty(),
        "message must not start a conversation"
    );
    assert!(
        fixture.server.requests_for("turn/steer").is_empty(),
        "an idle thread gets a new turn instead of a steer"
    );
    assert!(
        fixture.server.requests_for("turn/interrupt").is_empty(),
        "nothing interrupts the run"
    );
    let started = fixture.server.requests_for("turn/start");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(started[0]["params"]["threadId"], THREAD, "{started:?}");
    assert_eq!(
        started[0]["params"]["input"],
        json!([{"type": "text", "text": delivered}]),
        "the text must arrive verbatim, without shell evaluation: {started:?}"
    );
    assert_eq!(
        started[0]["params"]["clientUserMessageId"],
        content_id(delivered),
        "{started:?}"
    );

    // The receipt records the attempt with its content identity, the method, the
    // turn and the evidence it was classified from.
    let attempts = fixture.attempts();
    assert_eq!(attempts.len(), 1, "{attempts:?}");
    let attempt = &attempts[0];
    assert_eq!(attempt["id"], content_id(delivered), "{attempt}");
    assert_eq!(attempt["status"], "delivered", "{attempt}");
    assert_eq!(attempt["method"], "turn/start", "{attempt}");
    assert_eq!(
        attempt["turnId"], TURN,
        "the turn the conversation's own record places the input in is the recorded one: {attempt}"
    );
    assert_eq!(attempt["attempts"], 1, "{attempt}");
    assert!(
        attempt["evidence"]
            .as_str()
            .unwrap()
            .contains("client message id"),
        "{attempt}"
    );
    // The recorded attempt never carries the message text itself.
    assert!(
        !fixture.receipt().to_string().contains("second line"),
        "the receipt must not copy the message text: {}",
        fixture.receipt()
    );
    // The conversation's own address is untouched: one thread, one session.
    let endpoint: Value = serde_json::from_slice(&fs::read(&fixture.endpoint).unwrap()).unwrap();
    assert_eq!(endpoint["threadId"], THREAD, "{endpoint}");
}

#[test]
fn message_delivers_a_utf8_file_verbatim() {
    let fixture = Fixture::new("file", "running", true);
    let delivered = "path correction:\r\nuse D:\\home\\sample\\crates\\harness-core\r\n\u{00e4}\u{00f6}\u{00fc} \u{2713}\r\n";
    let file = fixture.home.join("correction.txt");
    fs::write(&file, delivered).unwrap();
    fixture.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(thread_read(
                &fixture.slot,
                "idle",
                json!([{
                    "id": TURN,
                    "status": "completed",
                    "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
                }]),
            )),
            Answer::Result(thread_read(
                &fixture.slot,
                "active",
                turns_with_input(delivered),
            )),
        ],
    );
    let out = fixture.message(&["--file", file.to_str().unwrap()]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    let started = fixture.server.requests_for("turn/start");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(
        started[0]["params"]["input"],
        json!([{"type": "text", "text": delivered}]),
        "the file content must arrive with its real line breaks: {started:?}"
    );
    assert_eq!(fixture.attempts()[0]["status"], "delivered");
}

#[test]
fn message_steers_an_active_turn_without_interrupting_it() {
    let fixture = Fixture::new("steer", "running", true);
    // The assignment turn is running and the conversation's items never show
    // the input: the acceptance answer is classified as queued, never as
    // delivered and never as applied.
    let correction =
        "CORRECTION_FIXTURE: keep the running tool call and then use harness-core for the path";
    let out = fixture.message(&["--text", correction]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": queued in"), "{text}");
    assert!(
        text.contains("the tool call in flight was not interrupted"),
        "{text}"
    );
    let steered = fixture.server.requests_for("turn/steer");
    assert_eq!(steered.len(), 1, "{steered:?}");
    assert_eq!(steered[0]["params"]["threadId"], THREAD, "{steered:?}");
    assert_eq!(
        steered[0]["params"]["expectedTurnId"], TURN,
        "the steer must address the active turn: {steered:?}"
    );
    assert_eq!(
        steered[0]["params"]["input"],
        json!([{"type": "text", "text": correction}]),
        "{steered:?}"
    );
    assert_eq!(
        steered[0]["params"]["clientUserMessageId"],
        content_id(correction),
        "{steered:?}"
    );
    assert!(
        fixture.server.requests_for("turn/interrupt").is_empty(),
        "a message never interrupts the run's tool call"
    );
    assert!(
        fixture.server.requests_for("turn/start").is_empty(),
        "an active turn is steered instead of opening another turn"
    );
    let attempt = &fixture.attempts()[0];
    assert_eq!(attempt["status"], "queued", "{attempt}");
    assert_eq!(attempt["method"], "turn/steer", "{attempt}");
    assert_eq!(attempt["turnId"], TURN, "{attempt}");
    assert_eq!(attempt["expectedTurnId"], TURN, "{attempt}");
}

#[test]
fn a_queued_message_is_upgraded_by_evidence_instead_of_being_sent_again() {
    let fixture = Fixture::new("queued-upgrade", "running", true);
    let correction = "UPGRADE_FIXTURE: the queued correction";
    let out = fixture.message(&["--text", correction]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": queued in"), "{text}");
    assert_eq!(fixture.attempts()[0]["status"], "queued");
    let sent = fixture.server.requests_for("turn/steer").len();
    assert_eq!(sent, 1, "{sent}");

    // The conversation now shows the input the earlier invocation accepted:
    // a repeat reports the delivery instead of steering the same text twice.
    fixture.server.answer(
        "thread/read",
        Answer::Result(thread_read(
            &fixture.slot,
            "active",
            turns_with_input(correction),
        )),
    );
    let out = fixture.message(&["--text", correction]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    assert_eq!(
        fixture.server.requests_for("turn/steer").len(),
        1,
        "the repeat must not be submitted again"
    );
    let attempt = &fixture.attempts()[0];
    assert_eq!(attempt["status"], "delivered", "{attempt}");
    assert_eq!(attempt["attempts"], 1, "{attempt}");
}

#[test]
fn message_is_visible_on_the_run_surface_the_host_renders() {
    let fixture = Fixture::new("host-visible", "running", false);
    // The receipt the host consumes carries the control route and a pinned
    // port, so the host drives its own conversation against the canned
    // endpoint exactly as a dispatched tab does.
    let mut document = fixture.receipt();
    document["control"] = json!({
        "schema": 1,
        "assignment": "fixture assignment text",
        "identity": {
            "profile": "ds",
            "model": MODEL,
            "modelProvider": PROVIDER,
            "reasoningEffort": EFFORT,
        },
        "port": fixture.server.port,
    });
    document["shell"] = json!({
        "path": std::env::var_os("PATH").unwrap(),
        "executable": launcher(),
        "version": "fixture",
        "sandbox_mode": "danger-full-access",
    });
    fs::write(
        &fixture.receipt,
        serde_json::to_vec_pretty(&document).unwrap(),
    )
    .unwrap();
    let host = lead_command()
        .args(["executor", "run", "--file"])
        .arg(&fixture.receipt)
        .env("CODEX_HOME", &fixture.home)
        .env("HARNESS_EXECUTOR_FIXTURE_MODE", "hang")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The host records its own lease and identity before the assignment is
    // submitted; the message addresses exactly that live run.
    wait_for(
        || {
            fixture.receipt()["observation"]["host"]["pid"]
                .as_u64()
                .is_some()
                && fixture.server.requests_for("turn/start").len() == 1
        },
        "the host hosts the assignment through the control driver",
    );
    let correction = "VISIBLE_CORRECTION_FIXTURE: continue with the corrected path";
    fixture.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(thread_read(&fixture.slot, "active", active_turns())),
            Answer::Result(thread_read(
                &fixture.slot,
                "active",
                turns_with_input(correction),
            )),
        ],
    );
    let out = fixture.message(&["--text", correction]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    // The conversation itself announces the input; the host renders that record
    // on the run's own surface, once.
    wait_for(
        || !fixture.server.requests_for("turn/steer").is_empty(),
        "the message steers the live conversation",
    );
    fixture.server.push(json!({
        "method": "item/started",
        "params": {"threadId": THREAD, "item": {
            "id": "u1", "type": "userMessage", "clientId": content_id(correction),
            "content": [{"type": "text", "text": correction}]
        }}
    }));
    fixture.server.push(json!({
        "method": "item/completed",
        "params": {"threadId": THREAD, "item": {
            "id": "u1", "type": "userMessage", "clientId": content_id(correction),
            "content": [{"type": "text", "text": correction}]
        }}
    }));
    thread::sleep(Duration::from_millis(500));
    // The host accepted the turn `turn/start` answered, not the historical
    // turn id in the thread record. Completing the other id must not end it.
    fixture.server.push(json!({
        "method": "turn/completed",
        "params": {"threadId": THREAD, "turn": {"id": NEXT_TURN, "status": "completed"}}
    }));
    let mut host = host;
    let deadline = Instant::now() + WAIT;
    let status = loop {
        if let Some(status) = host.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the host did not end");
        thread::sleep(Duration::from_millis(50));
    };
    let rendered = {
        use std::io::Read;
        let mut output = String::new();
        host.stdout
            .take()
            .unwrap()
            .read_to_string(&mut output)
            .unwrap();
        output
    };
    assert_eq!(status.code(), Some(0), "{rendered}");
    let occurrences = rendered.matches(correction).count();
    assert_eq!(
        occurrences, 1,
        "the delivered input must appear once on the run's surface: {rendered}"
    );
    assert!(
        rendered.contains(&format!("input: {correction}")),
        "{rendered}"
    );
}

#[test]
fn message_refuses_a_stale_identity_without_sending_anything() {
    let fixture = Fixture::new("stale", "running", true);
    // The slot was rebound to another session: the address no longer names the
    // run this caller asks for.
    let mut record = fixture.receipt();
    record["slot"]["owner"] = json!("exec-other");
    fs::write(
        &fixture.receipt,
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();
    let out = fixture.message(&["--text", "correction"]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(
        text.contains("instead of") && text.contains("owner"),
        "{text}"
    );
    assert!(
        fixture.server.requests().is_empty(),
        "a stale identity must not reach the endpoint: {:?}",
        fixture.server.requests()
    );
}

#[test]
fn message_refuses_another_recorded_session_without_sending_anything() {
    let fixture = Fixture::new("session", "running", true);
    let out = fixture.message(&[
        "--session",
        "01a0c719-f4d4-7880-a9d2-1a96ee0f23f5",
        "--text",
        "correction",
    ]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(
        text.contains(THREAD) && text.contains("instead of"),
        "{text}"
    );
    assert!(
        fixture.server.requests().is_empty(),
        "a session mismatch must not reach the endpoint"
    );
}

#[test]
fn message_reports_a_finished_or_unaddressable_run_with_the_resume_remedy() {
    for (state, name) in [
        ("completed", "completed"),
        ("stopped", "stopped"),
        ("interrupted", "interrupted"),
    ] {
        let fixture = Fixture::new(name, state, true);
        let out = fixture.message(&["--text", "correction"]);
        let text = output_text(&out);
        assert_eq!(out.status.code(), Some(2), "{state}: {text}");
        assert!(text.contains(&format!("not delivered ({state})")), "{text}");
        assert!(text.contains("message starts no new one"), "{text}");
        assert!(
            text.contains("executor resume --source") && text.contains(THREAD),
            "the exact-session remedy must be named: {text}"
        );
        assert!(
            fixture.server.requests().is_empty(),
            "{state}: a finished run must not be addressed"
        );
        assert!(fixture.attempts().is_empty(), "{state}");
    }
    // A surface without a recorded control endpoint (tui mode, or a receipt
    // written before the control route) is unsupported, not "delivered".
    let fixture = Fixture::new("unsupported", "running", false);
    let out = fixture.message(&["--text", "correction"]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(
        text.contains("no control-backed conversation")
            && text.contains("no addressed input channel"),
        "{text}"
    );
    assert!(
        text.contains("nothing was sent") && text.contains("executor resume --source"),
        "{text}"
    );
    assert!(fixture.server.requests().is_empty());
}

#[test]
fn message_classifies_a_native_refusal_as_error_and_only_an_error_is_retryable() {
    let fixture = Fixture::new("refusal", "running", true);
    let correction = "REFUSAL_FIXTURE: use the corrected path";
    fixture.server.answer_sequence(
        "thread/read",
        vec![Answer::Result(thread_read(
            &fixture.slot,
            "idle",
            json!([{
                "id": TURN,
                "status": "completed",
                "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
            }]),
        ))],
    );
    fixture.server.answer(
        "turn/start",
        Answer::Error(json!({"code": -32000, "message": "no active turn"})),
    );
    let out = fixture.message(&["--text", correction]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains(": error in"), "{text}");
    assert!(text.contains("no active turn"), "{text}");
    assert!(
        text.contains("nothing was delivered") && text.contains("retryable"),
        "{text}"
    );
    let attempt = &fixture.attempts()[0];
    assert_eq!(attempt["status"], "error", "{attempt}");
    assert_eq!(attempt["attempts"], 3, "{attempt}");
    assert!(
        attempt["detail"]
            .as_str()
            .unwrap()
            .contains("no active turn"),
        "{attempt}"
    );

    // The refusal applied nothing, so the same text may be sent again; the
    // conversation's own record then shows the delivery.
    fixture.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(thread_read(
                &fixture.slot,
                "idle",
                json!([{
                    "id": TURN,
                    "status": "completed",
                    "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
                }]),
            )),
            Answer::Result(thread_read(
                &fixture.slot,
                "active",
                turns_with_input(correction),
            )),
        ],
    );
    fixture.server.answer(
        "turn/start",
        Answer::Result(json!({"turn": {"id": NEXT_TURN, "status": "inProgress"}})),
    );
    let out = fixture.message(&["--text", correction]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    let attempt = &fixture.attempts()[0];
    assert_eq!(attempt["status"], "delivered", "{attempt}");
    assert_eq!(attempt["attempts"], 4, "{attempt}");
}

#[test]
fn an_indeterminate_message_is_never_delivered_twice() {
    let fixture = Fixture::new("indeterminate", "running", true);
    let correction = "INDETERMINATE_FIXTURE: the answer never arrives";
    fixture.server.answer_sequence(
        "thread/read",
        vec![Answer::Result(thread_read(
            &fixture.slot,
            "idle",
            json!([{
                "id": TURN,
                "status": "completed",
                "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
            }]),
        ))],
    );
    // The answer carries another request identity: whether the input was
    // applied is unknown, so nothing may be reported as delivered.
    fixture.server.answer(
        "turn/start",
        Answer::Record(json!({"id": 424242, "result": {}})),
    );
    let out = fixture.message(&["--text", correction]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains(": indeterminate in"), "{text}");
    assert!(text.contains("nothing is reported as delivered"), "{text}");
    assert!(
        text.contains("cannot be delivered twice") && text.contains("next action"),
        "{text}"
    );
    let attempt = &fixture.attempts()[0];
    assert_eq!(attempt["status"], "indeterminate", "{attempt}");
    assert_eq!(attempt["attempts"], 1, "{attempt}");
    let sent = fixture.server.requests_for("turn/start").len();
    assert_eq!(sent, 1, "{attempt}");

    // A repeat of the same content cannot be sent again while the attempt is
    // indeterminate, so the same text cannot reach the conversation twice.
    let out = fixture.message(&["--text", correction]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains(": indeterminate in"), "{text}");
    assert!(text.contains("stays refused"), "{text}");
    assert_eq!(
        fixture.server.requests_for("turn/start").len(),
        1,
        "the repeat must not reach the endpoint"
    );
    let attempt = &fixture.attempts()[0];
    assert_eq!(attempt["status"], "indeterminate", "{attempt}");
    assert_eq!(attempt["attempts"], 1, "{attempt}");
}

#[test]
fn a_stopped_run_marks_a_queued_message_undelivered() {
    let fixture = Fixture::new("undelivered", "running", true);
    let correction = "QUEUED_FIXTURE: correction while the turn runs";
    let lease: Value = serde_json::from_slice(&fs::read(&fixture.lease).unwrap()).unwrap();
    assert_eq!(lease["owner"], OWNER, "{lease}");
    let out = fixture.message(&["--text", correction]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": queued in"), "{text}");
    assert_eq!(fixture.attempts()[0]["status"], "queued");
    // Stopping the run stops further delivery and records the queued input as
    // undelivered, exactly as the stop path's own contract says.
    let out = lead_command()
        .args([
            "executor",
            "stop",
            "--source",
            fixture.source.to_str().unwrap(),
            "--codex-home",
            fixture.home.to_str().unwrap(),
            "--slot",
            "1",
            "--owner",
            OWNER,
        ])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let attempt = &fixture.attempts()[0];
    assert_eq!(attempt["status"], "undelivered", "{attempt}");
    assert!(attempt["undeliveredMs"].as_u64().is_some(), "{attempt}");
    assert_eq!(attempt["id"], content_id(correction), "{attempt}");
}

#[test]
fn a_full_receipt_refuses_a_new_message_instead_of_evicting_unresolved_attempts() {
    let fixture = Fixture::new("capacity", "running", true);
    let recorded: Vec<Value> = (0..64)
        .map(|index| {
            json!({
                "schema": 1,
                "id": format!("msg-{index:04}"),
                "clientMessageId": format!("msg-{index:04}"),
                "status": "delivered",
                "attempts": 1,
            })
        })
        .collect();
    edit_receipt(&fixture.receipt, |receipt| {
        receipt["messages"] = json!(recorded);
    });
    // The receipt is full of records that each prevent a repeat of their
    // content: none may be silently evicted for one more message.
    let out = fixture.message(&["--text", "one more correction"]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(text.contains("refusing to evict"), "{text}");
    assert!(text.contains("nothing was sent"), "{text}");
    assert!(
        fixture.server.requests_for("turn/start").is_empty()
            && fixture.server.requests_for("turn/steer").is_empty(),
        "the refused attempt must not reach the endpoint: {text}"
    );
    assert_eq!(
        fixture.attempts().len(),
        64,
        "no recorded attempt was evicted"
    );

    // A definite refusal applied nothing, so it is the disposable record: the
    // same bound then admits the new message instead of blocking it forever.
    edit_receipt(&fixture.receipt, |receipt| {
        receipt["messages"][0]["status"] = json!("error");
        receipt["messages"][0]["id"] = json!("msg-error");
        receipt["messages"][0]["clientMessageId"] = json!("msg-error");
    });
    let out = fixture.message(&["--text", "one more correction"]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": queued in"), "{text}");
    let attempts = fixture.attempts();
    assert_eq!(attempts.len(), 64, "{attempts:?}");
    assert!(
        !attempts.iter().any(|attempt| attempt["id"] == "msg-error"),
        "the refused attempt is the one that was disposed of: {attempts:?}"
    );
}

const BOUNDARY_TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f24bb";
const TOOL_TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f24cc";
const NEIGHBOR_THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f25aa";
const ATTACHED_OWNER: &str = "exec-message-frontend";
const NEIGHBOR_OWNER: &str = "exec-message-neighbor";
const PARTIAL_TEXT: &str = "partial work \u{043f}\u{0443}\u{0442}\u{044c}\n";

/// One managed run whose native frontend is attached and whose app-server
/// child still owns a live tool. Message addresses this run, not a second
/// controller.
struct AttachedRun {
    owner: String,
    thread: String,
    source: PathBuf,
    home: PathBuf,
    slot: PathBuf,
    state: PathBuf,
    receipt: PathBuf,
    release: PathBuf,
    tool_marker: PathBuf,
    partial: PathBuf,
    server: Server,
    session: Option<ConsoleSession>,
    _root: tempfile::TempDir,
}

impl AttachedRun {
    fn new(name: &str, owner: &str, thread: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join(format!("proj-{name}"));
        fs::create_dir_all(source.join("global")).unwrap();
        git(&source, &["init", "-q", "--initial-branch=main"]);
        fs::write(source.join("global/orchestration.toml"), orchestration()).unwrap();
        let source = source.canonicalize().unwrap();
        let home = root.path().join("home");
        let slot = root.path().join("slot");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&slot).unwrap();
        let home = home.canonicalize().unwrap();
        let slot = slot.canonicalize().unwrap();
        let state = harness_core::task_worktree::pool_state_dir(&home, &source).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("slot-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "source": source,
                "index": 1,
                "path": slot,
                "state": "occupied",
                "owner": owner,
                "base": "abc123",
                "disposition": null,
                "reason": null
            }))
            .unwrap(),
        )
        .unwrap();
        let server = Server::start(Bearer::File(state.join("endpoint-1.token")));
        server.answer("initialize", Answer::Result(json!({})));
        server.answer("thread/name/set", Answer::Result(json!({})));
        server.answer(
            "thread/start",
            Answer::Result(json!({
                "thread": {"id": thread, "cwd": slot},
                "model": MODEL,
                "modelProvider": PROVIDER,
                "reasoningEffort": EFFORT
            })),
        );
        server.answer(
            "thread/resume",
            Answer::Result(json!({
                "thread": {"id": thread, "cwd": slot},
                "model": MODEL,
                "modelProvider": PROVIDER,
                "reasoningEffort": EFFORT
            })),
        );
        server.answer(
            "turn/start",
            Answer::Result(json!({"turn": {"id": TOOL_TURN, "status": "inProgress"}})),
        );
        server.answer("turn/steer", Answer::Result(json!({"turnId": TOOL_TURN})));
        server.answer_sequence(
            "thread/read",
            vec![
                Answer::Result(json!({"thread": {"id": thread, "cwd": slot, "turns": []}})),
                Answer::Result(json!({"thread": {"id": thread, "cwd": slot, "turns": []}})),
            ],
        );
        let receipt = state.join("spawn-1.json");
        let release = root.path().join("release");
        let tool_marker = root.path().join("tool.json");
        let partial = slot.join("partial.txt");
        fs::write(&partial, PARTIAL_TEXT).unwrap();
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "launcher": launcher(),
                "profile": "deepseek",
                "mode": "tui",
                "args": [],
                "visible": true,
                "host": "owned-console",
                "control": {
                    "schema": 1,
                    "assignment": "fixture assignment text",
                    "identity": {
                        "profile": "deepseek",
                        "model": MODEL,
                        "modelProvider": PROVIDER,
                        "reasoningEffort": EFFORT
                    },
                    "presentation": "native-tui",
                    "port": server.port
                },
                "terminal": null,
                "isolation": false,
                "slot": {
                    "index": 1,
                    "path": slot,
                    "source": source,
                    "owner": owner,
                    "base": "abc123",
                    "remote": "origin",
                    "branch": "main"
                },
                "model": MODEL,
                "modelProvider": PROVIDER,
                "reasoningEffort": EFFORT,
                "window": null,
                "shell": {
                    "path": std::env::var_os("PATH").unwrap(),
                    "executable": launcher(),
                    "version": "fixture",
                    "sandbox_mode": "danger-full-access"
                },
                "observation": {
                    "schema": 1,
                    "coverage": "native",
                    "state": "dispatch-accepted",
                    "result": state.join("message-1.txt"),
                    "detail": state.join("stream-1.jsonl")
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
        let launch = home.join("harness");
        fs::create_dir_all(&launch).unwrap();
        fs::write(
            launch.join("native-launch.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 2,
                "upstream": {"executable": double}
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            home.join("frontend-title.txt"),
            format!("CEx (deepseek) - {owner}"),
        )
        .unwrap();
        let mut spec = CommandSpec::new(manager());
        spec.args = vec![
            "executor".into(),
            "run".into(),
            "--file".into(),
            receipt.as_os_str().to_owned(),
        ];
        spec.env
            .insert("CODEX_HOME".into(), Some(home.as_os_str().to_owned()));
        spec.env
            .insert("HARNESS_EXECUTOR_FIXTURE_MODE".into(), None);
        spec.env.insert("HARNESS_EXECUTOR_SESSION".into(), None);
        spec.env.insert("WT_SESSION".into(), None);
        spec.env.insert(
            "HARNESS_EXECUTOR_CHILD_FIXTURE_MODE".into(),
            Some("descendant".into()),
        );
        spec.env.insert(
            "HARNESS_EXECUTOR_FIXTURE_CHILD_MARKER".into(),
            Some(tool_marker.as_os_str().to_owned()),
        );
        spec.env.insert(
            "HARNESS_EXECUTOR_FIXTURE_RELEASE".into(),
            Some(release.as_os_str().to_owned()),
        );
        let session = ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap();
        Self {
            owner: owner.to_owned(),
            thread: thread.to_owned(),
            source,
            home,
            slot,
            state,
            receipt,
            release,
            tool_marker,
            partial,
            server,
            session: Some(session),
            _root: root,
        }
    }

    fn wait_attached(&self) {
        let deadline = Instant::now() + WAIT;
        loop {
            let attached = self.frontend_phases().iter().any(|phase| {
                phase["phase"] == "attached"
                    && phase["threadId"] == self.thread
                    && phase["alive"] == true
            });
            let session_recorded = self.receipt()["observation"]["session"] == self.thread;
            let assigned = self.server.requests_for("turn/start").len() == 1;
            let tool_ready = self.tool_marker.is_file();
            if attached && session_recorded && assigned && tool_ready {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "frontend did not attach for {}: phases={:?} receipt={} requests={:?} tool={} log={}",
                self.owner,
                self.frontend_phases(),
                self.receipt(),
                self.server.requests(),
                tool_ready,
                self.log_tail()
            );
            thread::sleep(Duration::from_millis(40));
        }
    }

    fn message(&self, extra: &[&str]) -> Output {
        lead_command()
            .args([
                "executor",
                "message",
                "--source",
                self.source.to_str().unwrap(),
                "--codex-home",
                self.home.to_str().unwrap(),
                "--slot",
                "1",
                "--owner",
                &self.owner,
                "--session",
                &self.thread,
            ])
            .args(extra)
            .output()
            .unwrap()
    }

    fn receipt(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.receipt).unwrap()).unwrap()
    }

    fn frontend_phases(&self) -> Vec<Value> {
        fs::read_to_string(self.state.join("frontend-1.json"))
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn tool_identity(&self) -> ProcessIdentity {
        let value: Value = serde_json::from_slice(&fs::read(&self.tool_marker).unwrap()).unwrap();
        ProcessIdentity {
            pid: value["pid"].as_u64().unwrap() as u32,
            creation_time: value["creation_time"].as_u64().unwrap(),
        }
    }

    fn process_alive(&self, identity: ProcessIdentity, program: &Path) -> bool {
        let user = process_service::current_user().unwrap();
        matches!(
            ServiceProcess::inspect(identity, program, &user),
            Ok(Some(_))
        )
    }

    fn tool_alive(&self) -> bool {
        self.process_alive(self.tool_identity(), &launcher())
    }

    fn frontend_alive(&self) -> bool {
        let phase = self
            .frontend_phases()
            .into_iter()
            .find(|phase| phase["phase"] == "attached")
            .unwrap();
        let identity = ProcessIdentity {
            pid: phase["pid"].as_u64().unwrap() as u32,
            creation_time: phase["creationTime"].as_u64().unwrap(),
        };
        let program = PathBuf::from(phase["program"].as_str().unwrap());
        self.process_alive(identity, &program)
    }

    fn log_tail(&self) -> String {
        let text = fs::read_to_string(self.state.join("endpoint-1.log")).unwrap_or_default();
        let start = text.len().saturating_sub(600);
        text[start..].to_owned()
    }
}

impl Drop for AttachedRun {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, b"release");
        let _ = fs::write(self.home.join("frontend-release"), b"release");
        if let Some(session) = self.session.take() {
            let cancel = Cancellation::default();
            cancel.cancel();
            if let Ok(deadline) = Deadline::after(Duration::from_secs(8)) {
                let _ = session.wait(deadline, &cancel, Duration::from_secs(3));
            }
        }
    }
}

fn bound_thread(slot: &Path, thread: &str, turns: Value) -> Value {
    json!({"thread": {
        "id": thread,
        "cwd": slot,
        "model": MODEL,
        "modelProvider": PROVIDER,
        "reasoningEffort": EFFORT,
        "turns": turns
    }})
}

#[test]
fn message_delivers_to_the_exact_thread_at_a_turn_boundary_while_the_frontend_is_attached() {
    let run = AttachedRun::new("boundary", ATTACHED_OWNER, THREAD);
    let neighbor = AttachedRun::new("boundary-neighbor", NEIGHBOR_OWNER, NEIGHBOR_THREAD);
    run.wait_attached();
    neighbor.wait_attached();
    let correction = "boundary correction: keep partial.txt and do not touch the neighbor";
    run.server.answer(
        "turn/start",
        Answer::Result(json!({"turn": {"id": BOUNDARY_TURN, "status": "inProgress"}})),
    );
    run.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(bound_thread(
                &run.slot,
                THREAD,
                json!([{
                    "id": TOOL_TURN,
                    "status": "completed",
                    "items": [{"id": "c1", "type": "commandExecution", "command": "fixture long command"}]
                }]),
            )),
            Answer::Result(bound_thread(
                &run.slot,
                THREAD,
                json!([{
                    "id": BOUNDARY_TURN,
                    "status": "inProgress",
                    "items": [{
                        "id": "u-boundary",
                        "type": "userMessage",
                        "clientId": content_id_for(THREAD, correction),
                        "content": [{"type": "text", "text": correction}]
                    }]
                }]),
            )),
        ],
    );

    let out = run.message(&["--text", correction]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    assert!(text.contains(THREAD), "{text}");
    let started = run.server.requests_for("turn/start");
    assert_eq!(started.len(), 2, "{started:?}");
    assert_eq!(started[1]["params"]["threadId"], THREAD, "{started:?}");
    assert_eq!(
        started[1]["params"]["input"],
        json!([{"type": "text", "text": correction}]),
        "{started:?}"
    );
    assert!(
        run.server.requests_for("thread/start").len() == 1,
        "message started another conversation: {:?}",
        run.server.requests_for("thread/start")
    );
    assert!(run.server.requests_for("turn/steer").is_empty());
    assert!(run.server.requests_for("turn/interrupt").is_empty());
    assert!(
        run.frontend_alive(),
        "the frontend closed when the message was delivered"
    );
    assert!(run.tool_alive(), "the message interrupted the owned tool");
    assert_eq!(fs::read_to_string(&run.partial).unwrap(), PARTIAL_TEXT);
    assert_eq!(run.receipt()["observation"]["session"], THREAD);
    assert!(
        neighbor.server.requests_for("turn/start").len() == 1,
        "the neighbor received a turn: {:?}",
        neighbor.server.requests()
    );
    assert!(neighbor.server.requests_for("turn/steer").is_empty());
    assert!(neighbor.frontend_alive());
    assert!(neighbor.tool_alive());
    assert_eq!(fs::read_to_string(&neighbor.partial).unwrap(), PARTIAL_TEXT);
    assert_ne!(neighbor.receipt()["observation"]["state"], "stopped");
}

#[test]
fn message_steers_an_active_tool_without_touching_the_neighbor_or_partial_files() {
    let run = AttachedRun::new("steer-attached", ATTACHED_OWNER, THREAD);
    let neighbor = AttachedRun::new("steer-neighbor", NEIGHBOR_OWNER, NEIGHBOR_THREAD);
    run.wait_attached();
    neighbor.wait_attached();
    let correction = "steer the active tool; leave partial.txt and the neighbor alone";
    run.server.answer(
        "thread/read",
        Answer::Result(bound_thread(
            &run.slot,
            THREAD,
            json!([{
                "id": TOOL_TURN,
                "status": "inProgress",
                "items": [{
                    "id": "c1",
                    "type": "commandExecution",
                    "command": "fixture long command",
                    "status": "inProgress"
                }]
            }]),
        )),
    );

    let out = run.message(&["--text", correction]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": queued in"), "{text}");
    assert!(text.contains("was not interrupted"), "{text}");
    let steered = run.server.requests_for("turn/steer");
    assert_eq!(steered.len(), 1, "{steered:?}");
    assert_eq!(steered[0]["params"]["threadId"], THREAD, "{steered:?}");
    assert_eq!(
        steered[0]["params"]["expectedTurnId"], TOOL_TURN,
        "{steered:?}"
    );
    assert_eq!(
        steered[0]["params"]["input"],
        json!([{"type": "text", "text": correction}]),
        "{steered:?}"
    );
    assert_eq!(
        run.server.requests_for("turn/start").len(),
        1,
        "a steer opened another turn"
    );
    assert!(run.server.requests_for("turn/interrupt").is_empty());
    assert!(run.frontend_alive());
    assert!(
        run.tool_alive(),
        "steering interrupted the active tool process"
    );
    assert_eq!(fs::read_to_string(&run.partial).unwrap(), PARTIAL_TEXT);
    assert!(neighbor.server.requests_for("turn/steer").is_empty());
    assert_eq!(neighbor.server.requests_for("turn/start").len(), 1);
    assert!(neighbor.frontend_alive());
    assert!(neighbor.tool_alive());
    assert_eq!(fs::read_to_string(&neighbor.partial).unwrap(), PARTIAL_TEXT);
}

const LEAD_THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
const OTHER_LEAD_THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f5";
const EXECUTOR_SESSION: &str = "executor-session-synthetic";

struct LeadFixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    state: PathBuf,
    checkout: PathBuf,
    worktree: PathBuf,
    generation: String,
    lead_thread: String,
    server: Server,
    endpoint_child: Child,
    dispatcher: Child,
    release: PathBuf,
}

impl LeadFixture {
    fn new(name: &str, lead_thread: &str, host_is_caller_parent: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let state = home.join("harness/executor-pool").join(name);
        fs::create_dir_all(&state).unwrap();
        let checkout = root.path().join("checkout");
        let worktree = root.path().join("worktree");
        fs::create_dir_all(&checkout).unwrap();
        fs::create_dir_all(&worktree).unwrap();
        let release = root.path().join("release");
        let (endpoint_child, endpoint_identity) =
            fixture_child(&root.path().join("endpoint-process.json"), &release);
        let dispatcher = Command::new(r"C:\Windows\System32\ping.exe")
            .args(["-n", "120", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let program = PathBuf::from(r"C:\Windows\System32\ping.exe");
        let user = harness_core::process_service::current_user().unwrap();
        let dispatcher_identity = ServiceProcess::observe(dispatcher.id(), &program, 0, &user)
            .unwrap()
            .identity();
        let host_program = std::env::current_exe().unwrap();
        let host_identity = ServiceProcess::observe(std::process::id(), &host_program, 0, &user)
            .unwrap()
            .identity();
        let host = if host_is_caller_parent {
            json!({
                "pid": host_identity.pid,
                "created": host_identity.creation_time,
                "program": host_program,
            })
        } else {
            json!({
                "pid": endpoint_identity["pid"],
                "created": endpoint_identity["creation_time"],
                "program": launcher(),
            })
        };
        let generation = format!("generation-{name}");
        let server = Server::start(Bearer::Value(TOKEN.to_owned()));
        server.answer("initialize", Answer::Result(json!({})));
        server.answer("turn/steer", Answer::Result(json!({"turnId": TURN})));
        server.answer(
            "turn/start",
            Answer::Result(json!({"turn": {"id": NEXT_TURN, "status": "inProgress"}})),
        );
        server.answer(
            "thread/read",
            Answer::Result(lead_thread_read(lead_thread, "idle", json!([]))),
        );
        fs::write(
            state.join("spawn-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "originatingLead": {
                    "schema": 1,
                    "threadId": lead_thread,
                    "runGeneration": generation,
                    "dispatcher": {
                        "pid": dispatcher_identity.pid,
                        "creationTime": dispatcher_identity.creation_time,
                        "program": program,
                    }
                },
                "slot": {
                    "index": 1,
                    "path": worktree,
                    "source": checkout,
                    "owner": OWNER,
                },
                "observation": {
                    "schema": 1,
                    "state": "running",
                    "session": EXECUTOR_SESSION,
                    "host": host,
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            state.join("lead-endpoint-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "port": server.port,
                "token": TOKEN,
                "threadId": lead_thread,
                "process": {
                    "pid": endpoint_identity["pid"],
                    "creationTime": endpoint_identity["creation_time"],
                    "program": launcher(),
                }
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            _root: root,
            home,
            state,
            checkout,
            worktree,
            generation,
            lead_thread: lead_thread.to_owned(),
            server,
            endpoint_child,
            dispatcher,
            release,
        }
    }

    fn sender(&self, args: &[&str], cwd: Option<&Path>) -> Output {
        let mut command = Command::new(manager());
        command.args(["lead", "message"]).args(args);
        command.env("CODEX_HOME", &self.home);
        command.env("HARNESS_EXECUTOR_RUN", &self.generation);
        command.env("HARNESS_EXECUTOR_SESSION", "1");
        command.env("CODEX_THREAD_ID", "most-recent-session-is-not-authority");
        command.env_remove("HARNESS_ORIGINATING_LEAD");
        command.env_remove("HARNESS_LEAD_THREAD");
        command.env_remove("HARNESS_LEAD_RECIPIENT");
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        command.output().unwrap()
    }
}

impl Drop for LeadFixture {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, b"release");
        let _ = self.endpoint_child.kill();
        let _ = self.endpoint_child.wait();
        let _ = self.dispatcher.kill();
        let _ = self.dispatcher.wait();
    }
}

fn lead_thread_read(thread: &str, status: &str, turns: Value) -> Value {
    json!({"thread": {
        "id": thread,
        "cwd": "C:/lead-checkout",
        "status": {"type": status},
        "turns": turns
    }})
}

fn lead_message_id(generation: &str, kind: &str, nonce: u64, payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(generation.as_bytes());
    hasher.update([0]);
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(nonce.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(payload.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("lead-{hex}")
}

fn lead_envelope(fixture: &LeadFixture, kind: &str, payload: &str, id: &str) -> String {
    let reply = if kind == "notification" {
        "not-requested".to_owned()
    } else {
        format!("codex-harness executor message --reply-to {id} --text TEXT")
    };
    let header = json!({
        "id": id,
        "kind": kind,
        "owner": OWNER,
        "runGeneration": fixture.generation,
        "session": EXECUTOR_SESSION,
        "checkout": fixture.checkout,
        "worktree": fixture.worktree,
        "slot": 1,
        "leadThreadId": fixture.lead_thread,
        "assignment": "unavailable",
        "bd": "unavailable",
        "reply": reply,
    });
    format!(
        "{}\n---payload---\n{payload}",
        serde_json::to_string(&header).unwrap()
    )
}

fn script_delivery(fixture: &LeadFixture, active: bool, envelope: &str, id: &str) {
    let initial = if active {
        json!([{
            "id": TURN,
            "status": "inProgress",
            "items": [{"id": "c1", "type": "commandExecution", "command": "fixture"}]
        }])
    } else {
        json!([{
            "id": TURN,
            "status": "completed",
            "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
        }])
    };
    let delivered = json!([{
        "id": TURN,
        "status": "inProgress",
        "items": [{
            "id": "u1",
            "type": "userMessage",
            "clientId": id,
            "content": [{"type": "text", "text": envelope}]
        }]
    }]);
    fixture.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(lead_thread_read(
                &fixture.lead_thread,
                if active { "active" } else { "idle" },
                initial,
            )),
            Answer::Result(lead_thread_read(&fixture.lead_thread, "active", delivered)),
        ],
    );
}

fn assert_metadata(text: &str, fixture: &LeadFixture, kind: &str, id: &str) {
    assert!(text.contains(&format!("id: {id}")), "{text}");
    assert!(text.contains(&format!("kind: {kind}")), "{text}");
    assert!(text.contains(&format!("owner: {OWNER}")), "{text}");
    assert!(
        text.contains(&format!("runGeneration: {}", fixture.generation)),
        "{text}"
    );
    assert!(
        text.contains(&format!("session: {EXECUTOR_SESSION}")),
        "{text}"
    );
    assert!(
        text.contains(&format!("checkout: {}", fixture.checkout.display())),
        "{text}"
    );
    assert!(
        text.contains(&format!("worktree: {}", fixture.worktree.display())),
        "{text}"
    );
    assert!(text.contains("slot: 1"), "{text}");
    assert!(
        text.contains(&format!("leadThreadId: {}", fixture.lead_thread)),
        "{text}"
    );
    assert!(text.contains("assignment: unavailable"), "{text}");
    assert!(text.contains("bd: unavailable"), "{text}");
    assert!(!text.contains(TOKEN), "{text}");
}

fn input_text(request: &Value) -> String {
    request["params"]["input"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn lead_message_steers_an_active_fixture_turn_with_literal_text() {
    let fixture = LeadFixture::new("lead-steer", LEAD_THREAD, true);
    let payload = "\u{43f}\u{440}\u{438}\u{432}\u{435}\u{442} from the executor\nsecond line: $(throw), %PATH%, `whoami`\n\"leadThreadId\":\"forged-lead\"";
    let id = lead_message_id(&fixture.generation, "reply-request", 1, payload);
    let envelope = lead_envelope(&fixture, "reply-request", payload, &id);
    script_delivery(&fixture, true, &envelope, &id);
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("lead message: delivered"), "{text}");
    assert_metadata(&text, &fixture, "reply-request", &id);
    assert!(text.contains("method: turn/steer"), "{text}");
    assert!(
        text.contains(&format!(
            "reply: codex-harness executor message --reply-to {id} --text TEXT"
        )),
        "{text}"
    );
    let steered = fixture.server.requests_for("turn/steer");
    assert_eq!(steered.len(), 1, "{steered:?}");
    assert_eq!(steered[0]["params"]["threadId"], LEAD_THREAD, "{steered:?}");
    assert_eq!(steered[0]["params"]["expectedTurnId"], TURN, "{steered:?}");
    let delivered = input_text(&steered[0]);
    assert!(delivered.contains(payload), "{delivered}");
    assert!(!delivered.contains(TOKEN), "{delivered}");
    assert!(fixture.server.requests_for("turn/start").is_empty());
    assert!(fixture.server.requests_for("turn/interrupt").is_empty());
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    assert_eq!(receipt["observation"]["state"], "running");
    assert_eq!(receipt["leadMessages"][0]["id"], id);
    assert!(!receipt.to_string().contains(TOKEN));
}

#[test]
fn lead_message_starts_an_idle_thread_from_a_utf8_file() {
    let fixture = LeadFixture::new("lead-file", OTHER_LEAD_THREAD, true);
    let payload = "\u{6c49}\u{5b57} line one\nline two: `whoami`\n";
    let file = fixture.state.join("payload.txt");
    fs::write(&file, payload.as_bytes()).unwrap();
    let id = lead_message_id(&fixture.generation, "reply-request", 1, payload);
    let envelope = lead_envelope(&fixture, "reply-request", payload, &id);
    script_delivery(&fixture, false, &envelope, &id);
    let out = fixture.sender(&["--file", file.to_str().unwrap()], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("lead message: delivered"), "{text}");
    assert!(text.contains("method: turn/start"), "{text}");
    let started = fixture.server.requests_for("turn/start");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(
        started[0]["params"]["threadId"], OTHER_LEAD_THREAD,
        "{started:?}"
    );
    assert!(started[0]["params"].get("expectedTurnId").is_none());
    let delivered = input_text(&started[0]);
    assert!(delivered.contains(payload), "{delivered}");
    assert!(!delivered.contains(TOKEN));
    assert!(fixture.server.requests_for("turn/steer").is_empty());
}

#[test]
fn lead_message_notify_does_not_request_a_reply() {
    let fixture = LeadFixture::new("lead-notify", LEAD_THREAD, true);
    let payload = "notice only\n\u{2603}";
    let id = lead_message_id(&fixture.generation, "notification", 1, payload);
    let envelope = lead_envelope(&fixture, "notification", payload, &id);
    script_delivery(&fixture, true, &envelope, &id);
    let out = fixture.sender(&["--notify", "--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert_metadata(&text, &fixture, "notification", &id);
    assert!(text.contains("reply: not-requested"), "{text}");
    assert!(!text.contains("executor message --reply-to"), "{text}");
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    assert_eq!(receipt["observation"]["state"], "running");
    assert_ne!(receipt["observation"]["state"], "waiting-for-reply");
}

#[test]
fn lead_message_keeps_the_recorded_lead_after_a_cwd_change() {
    let fixture = LeadFixture::new("lead-cwd", LEAD_THREAD, true);
    let other = fixture._root.path().join("other-cwd");
    fs::create_dir_all(&other).unwrap();
    let payload = "same lead after cwd change";
    let id = lead_message_id(&fixture.generation, "reply-request", 1, payload);
    let envelope = lead_envelope(&fixture, "reply-request", payload, &id);
    script_delivery(&fixture, false, &envelope, &id);
    let out = fixture.sender(&["--text", payload], Some(&other));
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let started = fixture.server.requests_for("turn/start");
    assert_eq!(started.len(), 1, "{text} {started:?}");
    assert_eq!(started[0]["params"]["threadId"], LEAD_THREAD);
    assert!(!input_text(&started[0]).contains("most-recent-session-is-not-authority"));
}

#[test]
fn lead_message_refuses_an_unrelated_copied_marker_before_send() {
    let fixture = LeadFixture::new("lead-copied", LEAD_THREAD, false);
    let out = fixture.sender(&["--text", "should not arrive"], None);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("refusing before any send"), "{text}");
    assert!(
        text.contains("copied marker") || text.contains("not in the spawned run"),
        "{text}"
    );
    assert_eq!(fixture.server.connections(), 0, "{text}");
    assert!(fixture.server.requests_for("turn/steer").is_empty());
    assert!(fixture.server.requests_for("turn/start").is_empty());
}

#[test]
fn lead_message_survives_a_dead_dispatcher_while_its_host_lineage_is_live() {
    let mut fixture = LeadFixture::new("dead-dispatcher", LEAD_THREAD, true);
    fixture.dispatcher.kill().unwrap();
    fixture.dispatcher.wait().unwrap();
    let payload = "question after the spawn command exited";
    let id = lead_message_id(&fixture.generation, "reply-request", 1, payload);
    let envelope = lead_envelope(&fixture, "reply-request", payload, &id);
    script_delivery(&fixture, false, &envelope, &id);
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("lead message: delivered"), "{text}");
    let started = fixture.server.requests_for("turn/start");
    assert_eq!(started.len(), 1, "{text} {started:?}");
    assert_eq!(started[0]["params"]["threadId"], LEAD_THREAD, "{started:?}");
}

#[test]
fn lead_message_refuses_recipient_overrides_before_send() {
    let fixture = LeadFixture::new("lead-override", LEAD_THREAD, true);
    for flag in [
        "--slot",
        "--session",
        "--checkout",
        "--source",
        "--lead",
        "--recipient",
        "--thread-id",
        "--endpoint",
    ] {
        let out = fixture.sender(&[flag, "forged", "--text", "no"], None);
        let text = output_text(&out);
        assert!(!out.status.success(), "{flag}: {text}");
        assert!(text.contains("not accepted"), "{flag}: {text}");
        assert!(text.contains("refusing before any send"), "{flag}: {text}");
        assert!(!text.contains("forged"), "{flag}: {text}");
    }
    assert_eq!(fixture.server.connections(), 0);
    assert!(fixture.server.requests_for("turn/steer").is_empty());
}

#[test]
fn lead_message_refuses_a_missing_endpoint_without_starting_a_listener() {
    let fixture = LeadFixture::new("lead-missing-endpoint", LEAD_THREAD, true);
    fs::remove_file(fixture.state.join("lead-endpoint-1.json")).unwrap();
    let out = fixture.sender(&["--text", "no endpoint"], None);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("no verified lead endpoint"), "{text}");
    assert!(text.contains("fresh-launch"), "{text}");
    assert!(text.contains("Nothing was sent"), "{text}");
    assert!(text.contains("does not start a daemon"), "{text}");
    assert!(!fixture.state.join("lead-endpoint-1.json").exists());
    assert_eq!(fixture.server.connections(), 0, "{text}");
}

#[test]
fn lead_message_help_has_no_address_flags() {
    let out = Command::new(manager())
        .args(["lead", "message", "--help"])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("--text"), "{text}");
    assert!(text.contains("--file"), "{text}");
    assert!(text.contains("--notify"), "{text}");
    for flag in [
        "--slot",
        "--session",
        "--recipient",
        "--checkout",
        "--lead",
        "--endpoint",
    ] {
        assert!(!text.contains(flag), "{flag} in {text}");
    }
}

#[test]
fn lead_message_classifies_a_queued_send_and_never_sends_the_same_question_twice() {
    let fixture = LeadFixture::new("lead-queued", LEAD_THREAD, true);
    let payload = "queued question: the lead has not applied it yet";
    let id = lead_message_id(&fixture.generation, "reply-request", 1, payload);
    // The native transport accepts the input, but the conversation's own items
    // never show it: queued, never delivered.
    fixture.server.answer_sequence(
        "thread/read",
        vec![Answer::Result(lead_thread_read(
            &fixture.lead_thread,
            "idle",
            json!([]),
        ))],
    );
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("lead message: queued"), "{text}");
    assert_metadata(&text, &fixture, "reply-request", &id);
    assert_eq!(fixture.server.requests_for("turn/start").len(), 1, "{text}");
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    assert_eq!(receipt["leadMessages"][0]["status"], "queued", "{receipt}");
    assert_eq!(receipt["leadMessages"][0]["id"], id, "{receipt}");
    assert!(!receipt.to_string().contains(payload), "{receipt}");

    // A repeat of the same words cannot deliver them twice: the recorded
    // attempt is reconciled with the conversation's own items, and nothing is
    // sent while its identity is still unresolved.
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("lead message: queued"), "{text}");
    assert!(text.contains(&format!("id: {id}")), "{text}");
    assert!(text.contains("cannot be delivered twice"), "{text}");
    assert_eq!(
        fixture.server.requests_for("turn/start").len(),
        1,
        "the repeat must not reach the lead endpoint: {text}"
    );
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    assert_eq!(
        receipt["leadMessages"].as_array().unwrap().len(),
        1,
        "{receipt}"
    );
    assert_eq!(receipt["leadMessages"][0]["status"], "queued", "{receipt}");
}

#[test]
fn lead_message_records_a_refusal_as_error_without_a_hold_and_allows_a_later_try() {
    let fixture = LeadFixture::new("lead-refused", LEAD_THREAD, true);
    let payload = "refused question";
    let id = lead_message_id(&fixture.generation, "reply-request", 1, payload);
    fixture.server.answer_sequence(
        "thread/read",
        vec![Answer::Result(lead_thread_read(
            &fixture.lead_thread,
            "idle",
            json!([]),
        ))],
    );
    // The native endpoint refuses the request in every delivery round: nothing
    // was delivered, and the refusal is definite.
    fixture.server.answer(
        "turn/start",
        Answer::Error(json!({"code": -32000, "message": "no active turn"})),
    );
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("lead message: error"), "{text}");
    assert!(text.contains("no active turn"), "{text}");
    assert!(text.contains(&format!("id: {id}")), "{text}");
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    // `error` is the definite-refusal status the lifecycle hold reader
    // excludes, so a question that never left cannot keep the run waiting.
    assert_eq!(receipt["leadMessages"][0]["status"], "error", "{receipt}");
    assert_eq!(fixture.server.requests_for("turn/start").len(), 3, "{text}");

    // A definite refusal applied nothing and is retryable: the same words may
    // be sent again as an attempt with its own identity, and the
    // conversation's own items then show the delivery.
    let next = lead_message_id(&fixture.generation, "reply-request", 2, payload);
    let envelope = lead_envelope(&fixture, "reply-request", payload, &next);
    script_delivery(&fixture, false, &envelope, &next);
    fixture.server.answer(
        "turn/start",
        Answer::Result(json!({"turn": {"id": NEXT_TURN, "status": "inProgress"}})),
    );
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("lead message: delivered"), "{text}");
    assert!(text.contains(&format!("id: {next}")), "{text}");
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    let messages = receipt["leadMessages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "{receipt}");
    assert_eq!(messages[0]["status"], "error", "{receipt}");
    assert_eq!(messages[1]["status"], "delivered", "{receipt}");
    assert_eq!(messages[1]["id"], next, "{receipt}");
}

#[test]
fn lead_message_reconciles_an_indeterminate_send_before_any_repeat() {
    let fixture = LeadFixture::new("lead-uncertain", LEAD_THREAD, true);
    let payload = "uncertain question";
    let id = lead_message_id(&fixture.generation, "reply-request", 1, payload);
    let envelope = lead_envelope(&fixture, "reply-request", payload, &id);
    fixture.server.answer_sequence(
        "thread/read",
        vec![Answer::Result(lead_thread_read(
            &fixture.lead_thread,
            "idle",
            json!([]),
        ))],
    );
    // An acceptance without a turn identity leaves the fate of the input to the
    // conversation's own items, and they do not show it: indeterminate, never
    // delivered.
    fixture.server.answer(
        "turn/start",
        Answer::Record(json!({"id": "@request", "result": {}})),
    );
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("lead message: indeterminate"), "{text}");
    assert!(text.contains("without a turn identity"), "{text}");
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    assert_eq!(
        receipt["leadMessages"][0]["status"], "indeterminate",
        "{receipt}"
    );
    assert_eq!(fixture.server.requests_for("turn/start").len(), 1, "{text}");

    // The repeat reconciles the recorded identity with the lead conversation
    // before anything could be sent: with no evidence the same words stay
    // refused instead of being delivered twice, under the same id and with the
    // same unresolved hold.
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("lead message: indeterminate"), "{text}");
    assert!(text.contains(&format!("id: {id}")), "{text}");
    assert!(
        text.contains("could deliver it twice") && text.contains("Next action"),
        "{text}"
    );
    assert_eq!(
        fixture.server.requests_for("turn/start").len(),
        1,
        "the repeat must not reach the lead endpoint: {text}"
    );
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    assert_eq!(
        receipt["leadMessages"].as_array().unwrap().len(),
        1,
        "{receipt}"
    );
    assert_eq!(receipt["leadMessages"][0]["id"], id, "{receipt}");

    // The conversation's own record settles it: the input is there, so the
    // recorded attempt is upgraded to delivered without any second send.
    fixture.server.answer_sequence(
        "thread/read",
        vec![Answer::Result(lead_thread_read(
            &fixture.lead_thread,
            "active",
            json!([{
                "id": TURN,
                "status": "inProgress",
                "items": [{
                    "id": "u1",
                    "type": "userMessage",
                    "clientId": id,
                    "content": [{"type": "text", "text": envelope}]
                }]
            }]),
        ))],
    );
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("lead message: delivered"), "{text}");
    assert!(text.contains("upgraded from indeterminate"), "{text}");
    assert_eq!(fixture.server.requests_for("turn/start").len(), 1, "{text}");
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    assert_eq!(
        receipt["leadMessages"][0]["status"], "delivered",
        "{receipt}"
    );
    assert_eq!(receipt["leadMessages"][0]["id"], id, "{receipt}");
}

#[test]
fn lead_message_sends_the_same_words_again_in_a_later_resolved_exchange() {
    let fixture = LeadFixture::new("lead-resolved", LEAD_THREAD, true);
    let payload = "same words for another question";
    let first = lead_message_id(&fixture.generation, "reply-request", 1, payload);
    let envelope = lead_envelope(&fixture, "reply-request", payload, &first);
    script_delivery(&fixture, false, &envelope, &first);
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(&format!("id: {first}")), "{text}");

    // The lead answered: the exchange is resolved and its content holds nothing.
    edit_receipt(&fixture.state.join("spawn-1.json"), |receipt| {
        receipt["leadMessages"][0]["status"] = json!("resolved");
    });

    // The same words for another question must be a new message with its own
    // identity, not a suppressed repeat.
    let second = lead_message_id(&fixture.generation, "reply-request", 2, payload);
    let envelope = lead_envelope(&fixture, "reply-request", payload, &second);
    script_delivery(&fixture, false, &envelope, &second);
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("lead message: delivered"), "{text}");
    assert!(text.contains(&format!("id: {second}")), "{text}");
    assert_ne!(first, second, "{text}");
    assert_eq!(fixture.server.requests_for("turn/start").len(), 2, "{text}");
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    let messages = receipt["leadMessages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "{receipt}");
    assert_eq!(messages[0]["status"], "resolved", "{receipt}");
    assert_eq!(messages[1]["status"], "delivered", "{receipt}");
    assert_eq!(messages[1]["id"], second, "{receipt}");
}

/// One canned executor run plus the lead endpoint `lead message` needs, with
/// the test process recorded as the originating lead so a child CLI is in that
/// lead's lineage.
struct ReplyRoundTrip {
    fixture: Fixture,
    generation: String,
    lead_server: Server,
    lead_child: Child,
    lead_release: PathBuf,
}

impl ReplyRoundTrip {
    fn new(name: &str) -> Self {
        let fixture = Fixture::new(name, "running", true);
        let generation = format!("generation-{name}");
        let program = std::env::current_exe().unwrap();
        let user = harness_core::process_service::current_user().unwrap();
        let identity = ServiceProcess::observe(std::process::id(), &program, 0, &user)
            .unwrap()
            .identity();
        let mut receipt = fixture.receipt();
        receipt["originatingLead"] = json!({
            "schema": 1,
            "threadId": LEAD_THREAD,
            "runGeneration": generation,
            "dispatcher": {
                "pid": identity.pid,
                "creationTime": identity.creation_time,
                "program": program,
            }
        });
        receipt["observation"]["host"] = json!({
            "pid": identity.pid,
            "created": identity.creation_time,
            "program": program,
        });
        fs::write(
            &fixture.receipt,
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        let mut lease: Value = serde_json::from_slice(&fs::read(&fixture.lease).unwrap()).unwrap();
        lease["pid"] = json!(identity.pid);
        lease["created"] = json!(identity.creation_time);
        lease["program"] = json!(program);
        fs::write(&fixture.lease, serde_json::to_vec_pretty(&lease).unwrap()).unwrap();

        let lead_release = fixture._root.path().join("lead-release");
        let (lead_child, lead_identity) = fixture_child(
            &fixture._root.path().join("lead-process.json"),
            &lead_release,
        );
        let lead_server = Server::start(Bearer::Value(TOKEN.to_owned()));
        lead_server.answer("initialize", Answer::Result(json!({})));
        lead_server.answer("turn/steer", Answer::Result(json!({"turnId": TURN})));
        lead_server.answer(
            "turn/start",
            Answer::Result(json!({"turn": {"id": NEXT_TURN, "status": "inProgress"}})),
        );
        fs::write(
            fixture
                .receipt
                .parent()
                .unwrap()
                .join("lead-endpoint-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "port": lead_server.port,
                "token": TOKEN,
                "threadId": LEAD_THREAD,
                "process": {
                    "pid": lead_identity["pid"],
                    "creationTime": lead_identity["creation_time"],
                    "program": launcher(),
                }
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            fixture,
            generation,
            lead_server,
            lead_child,
            lead_release,
        }
    }

    /// Sends one real lead message and returns the id the command published.
    fn issue(&self, payload: &str) -> String {
        let id = lead_message_id(&self.generation, "reply-request", 1, payload);
        self.lead_server.answer_sequence(
            "thread/read",
            vec![
                Answer::Result(lead_thread_read(LEAD_THREAD, "idle", json!([]))),
                Answer::Result(lead_thread_read(
                    LEAD_THREAD,
                    "active",
                    json!([{
                        "id": TURN,
                        "status": "inProgress",
                        "items": [{
                            "id": "u1",
                            "type": "userMessage",
                            "clientId": id,
                            "content": [{"type": "text", "text": payload}]
                        }]
                    }]),
                )),
            ],
        );
        let mut command = Command::new(manager());
        command.args(["lead", "message", "--text", payload]);
        command.env("CODEX_HOME", &self.fixture.home);
        command.env("HARNESS_EXECUTOR_RUN", &self.generation);
        command.env("HARNESS_EXECUTOR_SESSION", "1");
        command.env("CODEX_THREAD_ID", "most-recent-session-is-not-authority");
        command.env_remove("HARNESS_ORIGINATING_LEAD");
        command.env_remove("HARNESS_LEAD_THREAD");
        command.env_remove("HARNESS_LEAD_RECIPIENT");
        command.current_dir(&self.fixture.slot);
        let out = command.output().unwrap();
        let text = output_text(&out);
        assert_eq!(out.status.code(), Some(0), "{text}");
        assert!(text.contains(&format!("id: {id}")), "{text}");
        let index = self
            .fixture
            .home
            .join("harness/executor-pool/message-index")
            .join(format!("{id}.json"));
        let pointer: Value = serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
        assert_eq!(pointer["schema"], 1, "{pointer}");
        assert_eq!(pointer["id"], id, "{pointer}");
        assert_eq!(
            pointer["receipt"],
            self.fixture.receipt.to_str().unwrap(),
            "{pointer}"
        );
        assert!(pointer.get("owner").is_none(), "{pointer}");
        assert!(
            !pointer.to_string().contains(payload),
            "the lookup must not store the payload: {pointer}"
        );
        assert!(self.fixture.server.requests().is_empty());
        id
    }

    fn reply(&self, lead_thread: &str, args: &[&str]) -> Output {
        let cwd = self.fixture.home.join("lead-cwd");
        fs::create_dir_all(&cwd).unwrap();
        let mut command = lead_command();
        command
            .args(["executor", "message"])
            .args(args)
            .env("CODEX_HOME", &self.fixture.home)
            .env("CODEX_THREAD_ID", lead_thread)
            .env_remove("HARNESS_EXECUTOR_RUN")
            .env_remove("HARNESS_ORIGINATING_LEAD")
            .env_remove("HARNESS_LEAD_THREAD")
            .env_remove("HARNESS_LEAD_RECIPIENT")
            .current_dir(&cwd);
        command.output().unwrap()
    }

    fn rewrite_receipt(&self, edit: impl FnOnce(&mut Value)) {
        let mut receipt = self.fixture.receipt();
        edit(&mut receipt);
        fs::write(
            &self.fixture.receipt,
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
    }
}

impl Drop for ReplyRoundTrip {
    fn drop(&mut self) {
        let _ = fs::write(&self.lead_release, b"release");
        let _ = self.lead_child.kill();
        let _ = self.lead_child.wait();
    }
}

fn script_executor_reply(fixture: &Fixture, active: bool, text: &str) {
    let initial = if active {
        active_turns()
    } else {
        json!([{
            "id": TURN,
            "status": "completed",
            "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
        }])
    };
    fixture.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(thread_read(
                &fixture.slot,
                if active { "active" } else { "idle" },
                initial,
            )),
            Answer::Result(thread_read(&fixture.slot, "active", turns_with_input(text))),
        ],
    );
}

fn assert_unchanged_payload(requests: &[Value], text: &str) {
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0]["params"]["threadId"], THREAD, "{requests:?}");
    assert_eq!(
        requests[0]["params"]["input"],
        json!([{"type": "text", "text": text}]),
        "{requests:?}"
    );
}

fn assert_refused_before_send(out: &Output, expected: &str, fixture: &Fixture) {
    let text = output_text(out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(
        text.contains(expected) && text.contains("refusing before any send"),
        "{text}"
    );
    assert!(
        fixture.server.requests().is_empty(),
        "refusal reached the executor endpoint: {:?}",
        fixture.server.requests()
    );
}

#[test]
fn reply_to_steers_the_recorded_executor_thread_with_only_the_message_id() {
    let run = ReplyRoundTrip::new("reply-steer");
    let question = "which input contract applies to sample-17?";
    let id = run.issue(question);
    let answer = "use the versioned input contract\nsecond line: $(throw), %PATH%, `whoami`";
    script_executor_reply(&run.fixture, true, answer);
    let out = run.reply(LEAD_THREAD, &["--reply-to", &id, "--text", answer]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains(": delivered in") && text.contains(THREAD),
        "{text}"
    );
    assert!(!text.contains("--source"), "{text}");
    let steered = run.fixture.server.requests_for("turn/steer");
    assert_unchanged_payload(&steered, answer);
    assert_eq!(steered[0]["params"]["expectedTurnId"], TURN, "{steered:?}");
    assert!(run.fixture.server.requests_for("turn/start").is_empty());
    assert!(run.fixture.server.requests_for("turn/interrupt").is_empty());
}

#[test]
fn reply_to_starts_an_idle_executor_thread_from_a_utf8_file() {
    let run = ReplyRoundTrip::new("reply-file");
    let id = run.issue("need the file reply");
    let answer = "\u{43f}\u{440}\u{438}\u{432}\u{435}\u{442} from the file\nsecond line: $(throw), %PATH%, `whoami`\n";
    let file = run.fixture.home.join("reply.txt");
    fs::write(&file, answer).unwrap();
    script_executor_reply(&run.fixture, false, answer);
    let out = run.reply(
        LEAD_THREAD,
        &["--reply-to", &id, "--file", file.to_str().unwrap()],
    );
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains(": delivered in") && text.contains(THREAD),
        "{text}"
    );
    let started = run.fixture.server.requests_for("turn/start");
    assert_unchanged_payload(&started, answer);
    assert!(run.fixture.server.requests_for("turn/steer").is_empty());
}

#[test]
fn reply_to_resolves_only_the_answered_request() {
    let run = ReplyRoundTrip::new("reply-one");
    let answered = run.issue("which contract applies to sample-17?");
    let open = "lead-0123456789abcdef01234567";
    run.rewrite_receipt(|receipt| {
        let mut other = receipt["leadMessages"][0].clone();
        other["id"] = json!(open);
        other["status"] = json!("delivered");
        receipt["leadMessages"].as_array_mut().unwrap().push(other);
    });
    let answer = "use the versioned input contract";
    script_executor_reply(&run.fixture, true, answer);
    let out = run.reply(LEAD_THREAD, &["--reply-to", &answered, "--text", answer]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    let receipt = run.fixture.receipt();
    let messages = receipt["leadMessages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "{receipt}");
    assert_eq!(messages[0]["id"], answered);
    assert_eq!(messages[0]["status"], "resolved", "{messages:?}");
    assert_eq!(messages[1]["id"], open);
    assert_eq!(messages[1]["status"], "delivered", "{messages:?}");
    assert_eq!(messages[1]["kind"], "reply-request");
}

#[test]
fn reply_to_refuses_unknown_retired_reused_and_contradictory_addresses_before_send() {
    let run = ReplyRoundTrip::new("reply-refuse");
    let unknown = run.reply(
        LEAD_THREAD,
        &[
            "--reply-to",
            "lead-0123456789abcdef01234567",
            "--text",
            "missing",
        ],
    );
    assert_refused_before_send(&unknown, "unknown message id", &run.fixture);

    let id = run.issue("which contract?");
    let contradictory = run.reply(
        LEAD_THREAD,
        &[
            "--reply-to",
            &id,
            "--owner",
            "exec-other",
            "--text",
            "wrong owner",
        ],
    );
    assert_refused_before_send(
        &contradictory,
        "contradictory explicit address",
        &run.fixture,
    );

    let other_lead = run.reply(
        OTHER_LEAD_THREAD,
        &["--reply-to", &id, "--text", "from another lead"],
    );
    assert_refused_before_send(&other_lead, "another lead", &run.fixture);

    run.rewrite_receipt(|receipt| {
        receipt["originatingLead"]["runGeneration"] = json!("generation-reused");
    });
    let reused_generation = run.reply(LEAD_THREAD, &["--reply-to", &id, "--text", "late"]);
    assert_refused_before_send(&reused_generation, "generation", &run.fixture);
    run.rewrite_receipt(|receipt| {
        receipt["originatingLead"]["runGeneration"] = json!(run.generation);
    });

    run.rewrite_receipt(|receipt| {
        receipt["observation"]["state"] = json!("completed");
    });
    let retired = run.reply(LEAD_THREAD, &["--reply-to", &id, "--text", "after end"]);
    assert_refused_before_send(&retired, "retired message id", &run.fixture);
    run.rewrite_receipt(|receipt| {
        receipt["observation"]["state"] = json!("running");
    });

    let slot_record = run.fixture.receipt.parent().unwrap().join("slot-1.json");
    let mut slot: Value = serde_json::from_slice(&fs::read(&slot_record).unwrap()).unwrap();
    slot["owner"] = json!("exec-other");
    fs::write(&slot_record, serde_json::to_vec_pretty(&slot).unwrap()).unwrap();
    let reused_slot = run.reply(LEAD_THREAD, &["--reply-to", &id, "--text", "reused"]);
    assert_refused_before_send(&reused_slot, "reused slot", &run.fixture);
}

#[test]
fn reply_to_the_same_words_answers_two_requests_as_two_inputs() {
    let run = ReplyRoundTrip::new("reply-two");
    let first = run.issue("which contract applies to sample-17?");
    let second = "lead-0123456789abcdef01234567";
    edit_receipt(&run.fixture.receipt, |receipt| {
        let mut other = receipt["leadMessages"][0].clone();
        other["id"] = json!(second);
        receipt["leadMessages"].as_array_mut().unwrap().push(other);
    });
    // The reply lookup has to resolve the second request as well.
    let index = run
        .fixture
        .home
        .join("harness/executor-pool/message-index")
        .join(format!("{second}.json"));
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    fs::write(
        &index,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "id": second,
            "receipt": run.fixture.receipt.to_str().unwrap(),
        }))
        .unwrap(),
    )
    .unwrap();

    let answer = "use the versioned input contract";
    script_executor_reply(&run.fixture, true, answer);
    let out = run.reply(LEAD_THREAD, &["--reply-to", &first, "--text", answer]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    // The same words for the other request are a new input with their own
    // identity, not a repeat of the first answer.
    script_executor_reply(&run.fixture, true, answer);
    let out = run.reply(LEAD_THREAD, &["--reply-to", second, "--text", answer]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    assert_eq!(
        run.fixture.server.requests_for("turn/steer").len(),
        2,
        "{text}"
    );
    let receipt = run.fixture.receipt();
    let messages = receipt["leadMessages"].as_array().unwrap();
    assert_eq!(messages[0]["id"], first, "{receipt}");
    assert_eq!(messages[0]["status"], "resolved", "{receipt}");
    assert_eq!(messages[1]["id"], second, "{receipt}");
    assert_eq!(messages[1]["status"], "resolved", "{receipt}");
    let attempts = run.fixture.attempts();
    assert_eq!(attempts.len(), 2, "{attempts:?}");
    for attempt in &attempts {
        assert_eq!(attempt["status"], "delivered", "{attempt}");
    }
    assert_ne!(attempts[0]["id"], attempts[1]["id"], "{attempts:?}");

    // Re-running the same reply is still a repeat of its own one input.
    let out = run.reply(LEAD_THREAD, &["--reply-to", second, "--text", answer]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": already delivered in"), "{text}");
    assert_eq!(
        run.fixture.server.requests_for("turn/steer").len(),
        2,
        "the repeat must not reach the executor endpoint: {text}"
    );
}

#[test]
fn lead_message_reconnects_to_the_same_recorded_thread_after_its_endpoint_is_lost() {
    let mut fixture = LeadFixture::new("lead-reconnect", LEAD_THREAD, true);
    let payload = "reconnect question: is the recorded lead reachable again?";
    // The recorded endpoint process is gone. Availability is reported
    // truthfully: nothing is sent, started, reparented or recorded as a hold.
    fixture.endpoint_child.kill().unwrap();
    fixture.endpoint_child.wait().unwrap();
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(
        text.contains("no verified lead endpoint") && text.contains("Nothing was sent"),
        "{text}"
    );
    assert!(
        text.contains(LEAD_THREAD),
        "the recorded lead thread must be named: {text}"
    );
    assert!(
        fixture.server.requests().is_empty(),
        "an endpoint that is not live was addressed: {:?}",
        fixture.server.requests()
    );
    let lost: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    assert_eq!(
        lost["originatingLead"]["threadId"], LEAD_THREAD,
        "the recorded lead relationship changed: {lost}"
    );
    assert!(
        lost.get("leadMessages").is_none(),
        "an unavailable endpoint recorded a phantom request: {lost}"
    );

    // The same lead thread reconnects through a new verified endpoint. The
    // recorded relationship still names that thread, so the retry reaches it
    // and nowhere else.
    let release = fixture._root.path().join("reconnect-release");
    let (mut child, identity) = fixture_child(
        &fixture._root.path().join("reconnect-process.json"),
        &release,
    );
    fs::write(
        fixture.state.join("lead-endpoint-1.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "port": fixture.server.port,
            "token": TOKEN,
            "threadId": LEAD_THREAD,
            "process": {
                "pid": identity["pid"],
                "creationTime": identity["creation_time"],
                "program": launcher(),
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let id = lead_message_id(&fixture.generation, "reply-request", 1, payload);
    let envelope = lead_envelope(&fixture, "reply-request", payload, &id);
    script_delivery(&fixture, false, &envelope, &id);
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("lead message: delivered"), "{text}");
    assert_metadata(&text, &fixture, "reply-request", &id);
    let started = fixture.server.requests_for("turn/start");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(
        started[0]["params"]["threadId"], LEAD_THREAD,
        "the retry was delivered to another conversation: {started:?}"
    );
    let delivered: Value =
        serde_json::from_slice(&fs::read(fixture.state.join("spawn-1.json")).unwrap()).unwrap();
    assert_eq!(
        delivered["leadMessages"][0]["status"], "delivered",
        "{delivered}"
    );
    assert_eq!(
        delivered["leadMessages"][0]["leadThreadId"], LEAD_THREAD,
        "{delivered}"
    );
    assert_eq!(
        delivered["originatingLead"]["threadId"], LEAD_THREAD,
        "the reconnected thread replaced the recorded lead: {delivered}"
    );
    assert_eq!(
        delivered["originatingLead"]["runGeneration"], fixture.generation,
        "{delivered}"
    );

    // A repeat of the same words is answered from the record: the reconnected
    // conversation receives no second copy.
    let out = fixture.sender(&["--text", payload], None);
    let text = output_text(&out);
    assert!(text.contains("cannot deliver it twice"), "{text}");
    assert_eq!(
        fixture.server.requests_for("turn/start").len(),
        1,
        "the repeat reached the reconnected lead endpoint: {text}"
    );
    let _ = fs::write(&release, b"release");
    let _ = child.kill();
    let _ = child.wait();
}

/// Installs the originating-lead relationship a reply is verified against, with
/// this test process as the live dispatching lead.
fn plant_lead_identity(fixture: &Fixture, generation: &str) {
    let program = std::env::current_exe().unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    let identity = ServiceProcess::observe(std::process::id(), &program, 0, &user)
        .unwrap()
        .identity();
    let mut receipt = fixture.receipt();
    receipt["originatingLead"] = json!({
        "schema": 1,
        "threadId": LEAD_THREAD,
        "runGeneration": generation,
        "dispatcher": {
            "pid": identity.pid,
            "creationTime": identity.creation_time,
            "program": program,
        }
    });
    fs::write(
        &fixture.receipt,
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

/// Records one published reply-request on the receipt and the local lookup its
/// reply reference resolves through, exactly as `lead message` publishes them.
fn record_reply_request(fixture: &Fixture, id: &str, generation: &str) {
    let mut receipt = fixture.receipt();
    let session = receipt["observation"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let entry = json!({
        "schema": 1,
        "id": id,
        "kind": "reply-request",
        "status": "delivered",
        "requiresReply": true,
        "leadThreadId": LEAD_THREAD,
        "runGeneration": generation,
        "session": session,
        "owner": OWNER,
        "slot": 1
    });
    let mut messages = receipt["leadMessages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    messages.push(entry);
    receipt["leadMessages"] = Value::Array(messages);
    fs::write(
        &fixture.receipt,
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    let index = fixture
        .home
        .join("harness/executor-pool/message-index")
        .join(format!("{id}.json"));
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    fs::write(
        &index,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "id": id,
            "receipt": fixture.receipt.to_str().unwrap(),
        }))
        .unwrap(),
    )
    .unwrap();
}

/// The reply invocation one lead uses: the message id and text, no address.
fn reply_to(fixture: &Fixture, args: &[&str]) -> Output {
    let cwd = fixture.home.join("lead-cwd");
    fs::create_dir_all(&cwd).unwrap();
    let mut command = lead_command();
    command
        .args(["executor", "message"])
        .args(args)
        .env("CODEX_HOME", &fixture.home)
        .env("CODEX_THREAD_ID", LEAD_THREAD)
        .env_remove("HARNESS_EXECUTOR_RUN")
        .env_remove("HARNESS_ORIGINATING_LEAD")
        .env_remove("HARNESS_LEAD_THREAD")
        .env_remove("HARNESS_LEAD_RECIPIENT")
        .current_dir(&cwd);
    command.output().unwrap()
}

#[test]
fn a_reply_delivered_before_a_stop_keeps_its_input_and_a_later_reply_is_refused() {
    let generation = "generation-reply-stop";
    let fixture = Fixture::new("reply-stop", "running", true);
    plant_lead_identity(&fixture, generation);
    let answered = "lead-0123456789abcdef01234567";
    let unanswered = "lead-fedcba9876543210fedcba98";
    record_reply_request(&fixture, answered, generation);
    record_reply_request(&fixture, unanswered, generation);

    // One side of the race: the answer is observed in the conversation before
    // the stop, so the stop cannot rewrite it into an undelivered input.
    let answer = "use the versioned input contract";
    script_executor_reply(&fixture, true, answer);
    let out = reply_to(&fixture, &["--reply-to", answered, "--text", answer]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains(": delivered in") && text.contains(THREAD),
        "{text}"
    );
    assert_unchanged_payload(&fixture.server.requests_for("turn/steer"), answer);
    let live = fixture.receipt();
    assert_eq!(
        live["leadMessages"][0]["status"], "resolved",
        "the answered request kept its hold: {live}"
    );
    assert_eq!(fixture.attempts()[0]["status"], "delivered", "{live}");

    let out = lead_command()
        .args([
            "executor",
            "stop",
            "--source",
            fixture.source.to_str().unwrap(),
            "--codex-home",
            fixture.home.to_str().unwrap(),
            "--slot",
            "1",
            "--owner",
            OWNER,
            "--timeout",
            "20",
        ])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    let stopped = fixture.receipt();
    assert_eq!(stopped["observation"]["state"], "stopped", "{stopped}");
    assert_eq!(
        fixture.attempts()[0]["status"],
        "delivered",
        "the stop rewrote an observed input: {stopped}"
    );
    assert!(
        fixture.attempts()[0].get("undeliveredMs").is_none(),
        "{stopped}"
    );
    assert_eq!(
        stopped["leadMessages"][0]["status"], "resolved",
        "{stopped}"
    );

    // The other side: the answer that had not landed is invalidated by the
    // stop. It reaches neither the ended run nor a replacement conversation,
    // and the unanswered request stays recorded for explicit recovery.
    let steers = fixture.server.requests_for("turn/steer").len();
    let turns = fixture.server.requests_for("turn/start").len();
    let out = reply_to(&fixture, &["--reply-to", unanswered, "--text", answer]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(
        text.contains(&format!("retired message id {unanswered}")),
        "{text}"
    );
    assert!(
        text.contains("stopped"),
        "the refusal must report the original run's state: {text}"
    );
    assert!(
        text.contains("codex-harness executor resume --source") && text.contains(THREAD),
        "the refusal must name the supported continuation of that exact session: {text}"
    );
    assert_eq!(
        fixture.server.requests_for("turn/steer").len(),
        steers,
        "a refused reply reached the stopped thread: {text}"
    );
    assert_eq!(
        fixture.server.requests_for("turn/start").len(),
        turns,
        "a refused reply started a conversation: {text}"
    );
    assert!(
        fixture.server.requests_for("thread/resume").is_empty(),
        "a refused reply resumed the thread"
    );
    let after = fixture.receipt();
    assert_eq!(
        after["leadMessages"][1]["status"], "delivered",
        "the unanswered request was rewritten: {after}"
    );
    assert_eq!(
        after["originatingLead"]["threadId"], LEAD_THREAD,
        "the stopped run was reparented: {after}"
    );
}

#[test]
fn reply_by_id_uses_the_exact_lead_thread_after_its_dispatcher_exits() {
    let run = ReplyRoundTrip::new("reply-dead-dispatcher");
    let id = run.issue("question whose spawn command already exited");
    run.rewrite_receipt(|receipt| {
        receipt["originatingLead"]["dispatcher"] = json!({
            "pid": 4242,
            "creationTime": 1,
            "program": r"C:\gone\codex-harness.exe",
        });
    });
    let answer = "answer from the exact lead thread";
    assert_refused_before_send(
        &run.reply(OTHER_LEAD_THREAD, &["--reply-to", &id, "--text", answer]),
        "another lead",
        &run.fixture,
    );
    script_executor_reply(&run.fixture, true, answer);
    let out = run.reply(LEAD_THREAD, &["--reply-to", &id, "--text", answer]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
}

#[test]
fn an_indeterminate_reply_is_never_sent_twice_and_resolves_nothing() {
    let run = ReplyRoundTrip::new("reply-uncertain");
    let id = run.issue("question whose answer never lands");
    let answer = "an answer that never lands";
    run.fixture.server.answer_sequence(
        "thread/read",
        vec![Answer::Result(thread_read(
            &run.fixture.slot,
            "active",
            active_turns(),
        ))],
    );
    // An acceptance without a turn identity, and no observed item: whether the
    // input was applied is unknown.
    run.fixture.server.answer(
        "turn/steer",
        Answer::Record(json!({"id": "@request", "result": {}})),
    );
    let out = run.reply(LEAD_THREAD, &["--reply-to", &id, "--text", answer]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains(": indeterminate in"), "{text}");
    let receipt = run.fixture.receipt();
    // An uncertain answer resolves nothing: the unanswered request keeps its
    // hold.
    assert_eq!(
        receipt["leadMessages"][0]["status"], "delivered",
        "{receipt}"
    );
    assert_eq!(run.fixture.attempts()[0]["status"], "indeterminate");

    // The repeat of the same reply cannot deliver it twice, and it still
    // resolves nothing.
    let out = run.reply(LEAD_THREAD, &["--reply-to", &id, "--text", answer]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("stays refused"), "{text}");
    assert!(text.contains("nothing is reported as delivered"), "{text}");
    assert_eq!(
        run.fixture.server.requests_for("turn/steer").len(),
        1,
        "the repeat must not reach the executor endpoint: {text}"
    );
    let receipt = run.fixture.receipt();
    assert_eq!(
        receipt["leadMessages"][0]["status"], "delivered",
        "{receipt}"
    );
    assert_eq!(run.fixture.attempts()[0]["status"], "indeterminate");
}

// ---------------------------------------------------------------------------
// Minimal content sourcing, automatic spill and recorded-address defaults.
//
// Every check below runs the real command entry point: the content arrives on a
// pipe or in a file exactly as a caller would supply it, and the run it is
// addressed to is the fixture's own recorded receipt, lease, slot binding, host
// process and canned control endpoint.
// ---------------------------------------------------------------------------

/// One literal payload of exactly `bytes` bytes with real line breaks, shell
/// metacharacters and a multibyte character. The size is the point: nothing in
/// it is interpreted, and a shell would visibly rewrite it.
fn payload_of(bytes: usize) -> String {
    assert!(bytes > 65);
    let line = "CORRECTION_LINE: $(throw), %PATH%, `whoami`, \u{00e4}\u{00f6}\u{00fc} literal\n";
    let mut text = String::new();
    while text.len() + line.len() < bytes {
        text.push_str(line);
    }
    while text.len() + 1 < bytes {
        text.push('x');
    }
    text.push('\n');
    assert_eq!(text.len(), bytes);
    text
}

/// Runs one command with this standard input, as a shell pipe delivers it. The
/// writer is detached, so a command that refuses the stream early can never
/// block the check on a full pipe.
fn piped(command: &mut Command, input: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let bytes = input.to_vec();
    let writer = thread::spawn(move || {
        use std::io::Write as _;
        let _ = stdin.write_all(&bytes);
    });
    let out = child.wait_with_output().unwrap();
    let _ = writer.join();
    out
}

/// The pointer envelope one spilled message delivers, as the command composes
/// it: the identity, the exact size and the absolute path of the complete
/// payload, with the instruction the recipient needs.
fn pointer_envelope(id: &str, bytes: usize, path: &Path) -> String {
    format!(
        "[message {id} is {bytes} bytes - larger than one conversation input, so the complete literal payload was written to a harness file and must be read now]\npath: {}\nbytes: {bytes}\nread: open that file with your ordinary file tools and treat its whole content as this message; nothing was truncated, summarized or split",
        path.display()
    )
}

/// The fixture thread's own record of one delivered input whose conversation
/// text is not what the command correlates: the identity the input carries is
/// what makes it evidence.
fn turns_with_input_identity(identity: &str, text: &str) -> Value {
    json!([
        {
            "id": TURN,
            "status": "inProgress",
            "items": [
                {"id": "c1", "type": "commandExecution", "command": "fixture long command"},
                {
                    "id": "u1",
                    "type": "userMessage",
                    "clientId": identity,
                    "content": [{"type": "text", "text": text}]
                }
            ]
        },
        {
            "id": NEXT_TURN,
            "status": "completed",
            "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
        }
    ])
}

/// The message directory of one kit home: where spilled payloads live.
fn messages_dir(home: &Path) -> PathBuf {
    home.join("harness/messages")
}

/// The spilled payload file of one message identity.
fn spill_path(home: &Path, id: &str) -> PathBuf {
    messages_dir(home).join(format!("{id}.txt"))
}

impl Fixture {
    /// The addressed `executor message` command of this fixture.
    fn addressed_command(&self, extra: &[&str]) -> Command {
        let mut command = lead_command();
        command
            .args([
                "executor",
                "message",
                "--source",
                self.source.to_str().unwrap(),
                "--codex-home",
                self.home.to_str().unwrap(),
                "--slot",
                "1",
                "--owner",
                OWNER,
            ])
            .args(extra)
            .current_dir(self.source.as_path())
            .env_remove("CODEX_HOME");
        command
    }

    /// The address-free form: the kit home comes from the environment, and the
    /// checkout and the run are resolved from the recorded installation, lease,
    /// slot binding and receipt.
    fn unaddressed_command(&self, extra: &[&str]) -> Command {
        let mut command = lead_command();
        command
            .args(["executor", "message"])
            .args(extra)
            .env("CODEX_HOME", &self.home)
            .current_dir(self.source.as_path());
        command
    }

    /// Records the installation whose source checkout this fixture uses.
    fn record_installation(&self) {
        fs::create_dir_all(self.home.join("harness")).unwrap();
        fs::write(
            self.home.join("harness/installation.json"),
            serde_json::to_vec_pretty(&json!({
                "schemaVersion": 2,
                "settings": {"sourceRoot": self.source},
            }))
            .unwrap(),
        )
        .unwrap();
    }

    /// Adds a second live pooled run in slot 2, recorded exactly as a dispatch
    /// records one, so an address that names no slot is genuinely ambiguous.
    fn add_live_run(&self, owner: &str, session: &str) -> Child {
        let state_dir =
            harness_core::task_worktree::pool_state_dir(&self.home, &self.source).unwrap();
        fs::write(
            state_dir.join("slot-2.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "source": self.source,
                "index": 2,
                "path": self.slot,
                "state": "occupied",
                "owner": owner,
                "base": "abc123",
                "disposition": null,
                "reason": null,
            }))
            .unwrap(),
        )
        .unwrap();
        let (child, identity) =
            fixture_child(&self._root.path().join("host-2.json"), &self.release);
        fs::write(
            state_dir.join("lease-2.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "owner": owner,
                "index": 2,
                "path": self.slot,
                "pid": identity["pid"],
                "created": identity["creation_time"],
                "program": launcher(),
            }))
            .unwrap(),
        )
        .unwrap();
        let mut receipt = self.receipt();
        receipt["slot"]["index"] = json!(2);
        receipt["slot"]["owner"] = json!(owner);
        receipt["observation"]["session"] = json!(session);
        receipt["observation"]["host"] = json!({
            "pid": identity["pid"],
            "created": identity["creation_time"],
            "program": launcher(),
        });
        fs::write(
            state_dir.join("spawn-2.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        child
    }
}

#[test]
fn message_content_is_piped_literally_and_an_empty_stream_is_no_content() {
    let fixture = Fixture::new("stdin", "running", true);
    let delivered = "piped correction \u{43f}\u{440}\u{438}\u{432}\u{435}\u{442}\n\
$(throw), %PATH%, `whoami`, \u{00e4}\u{00f6}\u{00fc} \u{2713}\r\nsecond line\n";
    fixture.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(thread_read(
                &fixture.slot,
                "idle",
                json!([{
                    "id": TURN,
                    "status": "completed",
                    "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
                }]),
            )),
            Answer::Result(thread_read(
                &fixture.slot,
                "active",
                turns_with_input(delivered),
            )),
        ],
    );
    // No content flag at all: the piped stream is the message.
    let out = piped(&mut fixture.addressed_command(&[]), delivered.as_bytes());
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    let started = fixture.server.requests_for("turn/start");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(
        started[0]["params"]["input"],
        json!([{"type": "text", "text": delivered}]),
        "the piped content must arrive verbatim, without shell evaluation: {started:?}"
    );
    assert_eq!(
        started[0]["params"]["clientUserMessageId"],
        content_id(delivered),
        "{started:?}"
    );
    // An inline-sized message keeps exactly the delivery evidence it always had:
    // no spill fields are invented for it.
    let attempt = fixture.attempts().into_iter().next().unwrap();
    assert_eq!(attempt["status"], "delivered", "{attempt}");
    assert!(attempt.get("delivery").is_none(), "{attempt}");
    assert!(attempt.get("payloadPath").is_none(), "{attempt}");

    // `--text -` selects the same source explicitly.
    let explicit = "explicit stream selection\n";
    fixture.server.answer_sequence(
        "thread/read",
        vec![Answer::Result(thread_read(
            &fixture.slot,
            "active",
            turns_with_input(explicit),
        ))],
    );
    let out = piped(
        &mut fixture.addressed_command(&["--text", "-"]),
        explicit.as_bytes(),
    );
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    assert_eq!(
        fixture.server.requests_for("turn/start").len()
            + fixture.server.requests_for("turn/steer").len(),
        2,
        "the explicit stream form delivers its own message: {text}"
    );

    // An empty stream is no content: nothing is sent.
    let out = piped(&mut fixture.addressed_command(&[]), b"");
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("the message is empty"), "{text}");
    assert_eq!(
        fixture.server.requests_for("turn/start").len()
            + fixture.server.requests_for("turn/steer").len(),
        2,
        "an empty stream must not reach the conversation: {text}"
    );
    assert_eq!(fixture.attempts().len(), 2, "no attempt is recorded for it");
}

#[test]
fn a_terminal_or_absent_standard_input_never_blocks_and_sends_nothing() {
    let fixture = Fixture::new("terminal", "running", true);
    // The caller's own standard input, whatever it is: an interactive terminal
    // refuses with the supported sources, an empty non-terminal stream refuses
    // as no content. Either way the command must return by itself.
    let mut child = fixture
        .addressed_command(&[])
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "a standard input with no content must not block the command"
        );
        thread::sleep(Duration::from_millis(25));
    }
    let out = child.wait_with_output().unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("interactive terminal") || text.contains("the message is empty"),
        "the refusal must name the supported content sources: {text}"
    );
    assert!(
        fixture.server.requests().is_empty(),
        "nothing may be sent without content: {text}"
    );

    // A null device is a non-terminal stream with no content: the same refusal,
    // reached without blocking.
    let out = fixture
        .addressed_command(&[])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("the message is empty"), "{text}");
    assert!(fixture.server.requests().is_empty(), "{text}");
}

#[test]
fn an_oversized_message_is_delivered_as_a_pointer_to_its_harness_file() {
    let fixture = Fixture::new("spill", "running", true);
    let payload = payload_of(300 * 1024);
    let id = content_id(&payload);
    let path = spill_path(&fixture.home, &id);
    // The conversation's own record carries the recorded identity, which is what
    // the command correlates the delivered pointer envelope by.
    fixture.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(thread_read(
                &fixture.slot,
                "idle",
                json!([{
                    "id": TURN,
                    "status": "completed",
                    "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
                }]),
            )),
            Answer::Result(thread_read(
                &fixture.slot,
                "active",
                turns_with_input_identity(&id, &payload),
            )),
        ],
    );
    let file = fixture.home.join("correction.txt");
    fs::write(&file, &payload).unwrap();
    let out = fixture.message(&["--file", file.to_str().unwrap()]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    assert!(text.contains("delivery: spill"), "{text}");
    assert!(
        text.contains(&format!("payloadPath: {}", path.display())),
        "{text}"
    );

    // The complete payload is in the harness-owned file, byte for byte, and it
    // is outside every worktree and repository.
    let stored = fs::read(&path).unwrap();
    assert_eq!(
        stored,
        payload.as_bytes(),
        "the spill must persist the complete literal payload"
    );
    assert!(path.starts_with(messages_dir(&fixture.home)), "{path:?}");
    assert!(
        !path.starts_with(&fixture.slot),
        "the payload must not be written into the executor's worktree"
    );

    // The conversation receives the pointer envelope, never the payload, and the
    // envelope names the identity, the exact size and the absolute path.
    let started = fixture.server.requests_for("turn/start");
    assert_eq!(started.len(), 1, "{started:?}");
    let submitted = started[0]["params"]["input"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        submitted,
        pointer_envelope(&id, payload.len(), &path),
        "the delivered text must be the compact pointer envelope"
    );
    assert!(!submitted.contains("CORRECTION_LINE"), "{submitted}");

    // The receipt records spilled delivery as its own class, with the path and
    // the exact size, and still never copies the payload.
    let attempt = fixture.attempts().into_iter().next().unwrap();
    assert_eq!(attempt["status"], "delivered", "{attempt}");
    assert_eq!(attempt["delivery"], "spill", "{attempt}");
    assert_eq!(
        attempt["payloadBytes"],
        json!(payload.len() as u64),
        "{attempt}"
    );
    assert_eq!(
        attempt["payloadPath"],
        json!(path.to_str().unwrap()),
        "{attempt}"
    );
    assert_eq!(attempt["bytes"], json!(payload.len()), "{attempt}");
    assert!(
        !fixture.receipt().to_string().contains("CORRECTION_LINE"),
        "the receipt must not copy the spilled payload"
    );
}

#[test]
fn an_oversized_payload_above_the_ceiling_is_refused_without_sending() {
    let fixture = Fixture::new("ceiling", "running", true);
    let ceiling = 8 * 1024 * 1024;
    let payload = payload_of(ceiling + 1);
    let out = piped(&mut fixture.addressed_command(&[]), payload.as_bytes());
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("8388608") && text.contains("ceiling of one message"),
        "the refusal must name the actual ceiling: {text}"
    );
    assert!(
        text.contains("nothing was truncated"),
        "the refusal must say nothing partial was sent: {text}"
    );
    assert!(
        fixture.server.requests().is_empty(),
        "nothing may be sent above the ceiling: {text}"
    );
    assert!(
        !messages_dir(&fixture.home).exists(),
        "a refused payload leaves no spill file"
    );
    assert!(
        fixture.receipt().get("messages").is_none(),
        "a refused payload leaves no phantom attempt"
    );
}

#[test]
fn standard_input_that_is_not_utf8_is_refused_without_sending() {
    let fixture = Fixture::new("nonutf8", "running", true);
    let out = piped(
        &mut fixture.addressed_command(&[]),
        b"correction \xff\xfe not text\n",
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("is not UTF-8 text"), "{text}");
    assert!(fixture.server.requests().is_empty(), "{text}");
}

/// The spill directory is bounded honestly: a message that would push it past
/// its bound is refused with the numbers, and nothing already there is evicted
/// to make room.
#[test]
fn a_full_message_directory_is_refused_honestly_before_anything_is_sent() {
    let fixture = Fixture::new("messages-bound", "running", true);
    let payload = payload_of(300 * 1024);
    let id = content_id(&payload);
    let file = fixture.home.join("correction.txt");
    fs::write(&file, &payload).unwrap();
    // Unrelated messages already occupy the directory beyond its bound; sparse
    // files hold the recorded size without writing that much data.
    let dir = messages_dir(&fixture.home);
    fs::create_dir_all(&dir).unwrap();
    for index in 0..4 {
        fs::File::create(dir.join(format!("occupied-{index}.txt")))
            .unwrap()
            .set_len(40 * 1024 * 1024)
            .unwrap();
    }
    let out = fixture.message(&["--file", file.to_str().unwrap()]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("would exceed its 134217728-byte bound"),
        "the refusal must name the occupied size and the bound: {text}"
    );
    assert!(text.contains("nothing was sent"), "{text}");
    assert!(
        !spill_path(&fixture.home, &id).exists(),
        "no payload may be written past the bound"
    );
    assert_eq!(
        fs::read_dir(&dir).unwrap().count(),
        4,
        "nothing already recorded may be evicted to make room"
    );
    assert!(
        fixture.receipt().get("messages").is_none(),
        "a refused payload leaves no phantom attempt"
    );
    assert!(
        fixture.server.requests_for("turn/start").is_empty()
            && fixture.server.requests_for("turn/steer").is_empty(),
        "a refused payload must not reach the conversation: {text}"
    );
}

/// The lead-side half of the spill: an oversized reply and an oversized question
/// both travel as pointer envelopes naming their harness-owned file, and the
/// reply still resolves the request it answers.
impl ReplyRoundTrip {
    fn reply_piped(&self, lead_thread: &str, args: &[&str], input: &[u8]) -> Output {
        let cwd = self.fixture.home.join("lead-cwd");
        fs::create_dir_all(&cwd).unwrap();
        let mut command = lead_command();
        command
            .args(["executor", "message"])
            .args(args)
            .env("CODEX_HOME", &self.fixture.home)
            .env("CODEX_THREAD_ID", lead_thread)
            .env_remove("HARNESS_EXECUTOR_RUN")
            .env_remove("HARNESS_ORIGINATING_LEAD")
            .env_remove("HARNESS_LEAD_THREAD")
            .env_remove("HARNESS_LEAD_RECIPIENT")
            .current_dir(&cwd);
        piped(&mut command, input)
    }

    /// Sends one real oversized lead message from a file and returns its id.
    fn issue_oversized(&self, payload: &str) -> String {
        let file = self.fixture.home.join("oversized-question.txt");
        fs::write(&file, payload).unwrap();
        let id = lead_message_id(&self.generation, "reply-request", 1, payload);
        self.lead_server.answer_sequence(
            "thread/read",
            vec![Answer::Result(lead_thread_read(
                LEAD_THREAD,
                "active",
                json!([{
                    "id": TURN,
                    "status": "inProgress",
                    "items": [{
                        "id": "u1",
                        "type": "userMessage",
                        "clientId": id,
                        "content": [{"type": "text", "text": payload}]
                    }]
                }]),
            ))],
        );
        let mut command = Command::new(manager());
        command.args(["lead", "message", "--file", file.to_str().unwrap()]);
        command.env("CODEX_HOME", &self.fixture.home);
        command.env("HARNESS_EXECUTOR_RUN", &self.generation);
        command.env("HARNESS_EXECUTOR_SESSION", "1");
        command.env("CODEX_THREAD_ID", "most-recent-session-is-not-authority");
        command.env_remove("HARNESS_ORIGINATING_LEAD");
        command.env_remove("HARNESS_LEAD_THREAD");
        command.env_remove("HARNESS_LEAD_RECIPIENT");
        command.current_dir(&self.fixture.slot);
        let out = command.output().unwrap();
        let text = output_text(&out);
        assert_eq!(out.status.code(), Some(0), "{text}");
        id
    }
}

#[test]
fn an_oversized_reply_is_a_pointer_envelope_and_still_resolves_the_request() {
    let run = ReplyRoundTrip::new("spill-reply");
    let question = "answer this before the long path is used";
    let id = run.issue(question);
    let answer = payload_of(300 * 1024);
    let identity = content_id_for(THREAD, &format!("reply-to {id}\n{answer}"));
    let path = spill_path(&run.fixture.home, &identity);
    run.fixture.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(thread_read(&run.fixture.slot, "active", active_turns())),
            Answer::Result(thread_read(
                &run.fixture.slot,
                "active",
                turns_with_input_identity(&identity, &answer),
            )),
        ],
    );
    let out = run.reply_piped(LEAD_THREAD, &["--reply-to", &id], answer.as_bytes());
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": delivered in"), "{text}");
    assert!(text.contains("delivery: spill"), "{text}");
    assert!(
        text.contains(&format!("payloadPath: {}", path.display())),
        "{text}"
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        answer.as_bytes(),
        "the reply's complete payload is in its harness-owned file"
    );
    let steer = run.fixture.server.requests_for("turn/steer");
    assert_eq!(steer.len(), 1, "{steer:?}");
    assert_eq!(
        steer[0]["params"]["input"][0]["text"].as_str().unwrap(),
        pointer_envelope(&identity, answer.len(), &path),
        "the conversation receives the pointer envelope, not the payload"
    );

    // The delivered reply resolves exactly the request it answered, and its
    // attempt records the spill as its own class.
    let receipt = run.fixture.receipt();
    assert_eq!(
        receipt["leadMessages"][0]["status"], "resolved",
        "{receipt}"
    );
    assert_eq!(
        receipt["leadMessages"][0]["resolvedBy"],
        json!(identity),
        "{receipt}"
    );
    let attempt = &receipt["messages"][0];
    assert_eq!(attempt["status"], "delivered", "{attempt}");
    assert_eq!(attempt["delivery"], "spill", "{attempt}");
    assert_eq!(
        attempt["payloadBytes"],
        json!(answer.len() as u64),
        "{attempt}"
    );

    // A repeat of the same reply cannot deliver it twice, and it still reports
    // the recorded spill instead of pretending an inline delivery.
    let out = run.reply_piped(LEAD_THREAD, &["--reply-to", &id], answer.as_bytes());
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains(": already delivered in"), "{text}");
    assert!(text.contains("payloadPath:"), "{text}");
    assert_eq!(
        run.fixture.server.requests_for("turn/steer").len(),
        1,
        "the repeat must not reach the executor conversation: {text}"
    );
}

#[test]
fn an_oversized_question_is_a_pointer_envelope_and_appears_in_watch() {
    let run = ReplyRoundTrip::new("spill-question");
    run.fixture.record_installation();
    let question = payload_of(300 * 1024);
    let id = run.issue_oversized(&question);
    let path = spill_path(&run.fixture.home, &id);
    assert_eq!(
        fs::read(&path).unwrap(),
        question.as_bytes(),
        "the question's complete payload is in its harness-owned file"
    );
    let steer = run.lead_server.requests_for("turn/steer");
    assert_eq!(steer.len(), 1, "{steer:?}");
    assert_eq!(
        steer[0]["params"]["input"][0]["text"].as_str().unwrap(),
        {
            let header = steer[0]["params"]["input"][0]["text"]
                .as_str()
                .unwrap()
                .split("\n---payload---\n")
                .next()
                .unwrap()
                .to_owned();
            format!(
                "{header}\n---payload---\n{}",
                pointer_envelope(&id, question.len(), &path)
            )
        },
        "the lead conversation receives the recorded header plus the pointer"
    );
    let recorded = run.fixture.receipt();
    assert_eq!(
        recorded["leadMessages"][0]["delivery"], "spill",
        "{recorded}"
    );
    assert_eq!(
        recorded["leadMessages"][0]["payloadBytes"],
        json!(question.len() as u64),
        "{recorded}"
    );

    // The existing watch surface shows the spilled request: its path and size,
    // and the one-command reply. The address comes from recorded state alone.
    let mut watch = lead_command();
    watch
        .args(["executor", "watch", "--timeout", "10"])
        .env("CODEX_HOME", &run.fixture.home)
        .current_dir(&run.fixture.slot);
    let out = watch.output().unwrap();
    let text = output_text(&out);
    assert_eq!(
        out.status.code(),
        Some(3),
        "a live run with an unanswered request is action required: {text}"
    );
    assert!(
        text.contains("action required: waiting for reply"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "payloadPath: {} payloadBytes: {}",
            path.display(),
            question.len()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("--reply-to {id}")),
        "the request keeps its one-command reply: {text}"
    );
}

#[test]
fn an_indeterminate_spilled_message_is_never_delivered_twice() {
    let fixture = Fixture::new("spill-indeterminate", "running", true);
    let payload = payload_of(300 * 1024);
    let id = content_id(&payload);
    let path = spill_path(&fixture.home, &id);
    let file = fixture.home.join("correction.txt");
    fs::write(&file, &payload).unwrap();
    // The native endpoint never answers the steering request, so whether the
    // input reached the conversation is unknown.
    fixture.server.answer("turn/steer", Answer::Silence);
    let out = fixture.message(&["--file", file.to_str().unwrap()]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains(": indeterminate in"), "{text}");
    assert!(
        text.contains(&format!("payloadPath: {}", path.display())),
        "an unresolved spilled attempt reports its own file: {text}"
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        payload.as_bytes(),
        "the payload is persisted complete before the attempt"
    );
    assert_eq!(fixture.server.requests_for("turn/steer").len(), 1);
    let attempt = fixture.attempts().into_iter().next().unwrap();
    assert_eq!(attempt["status"], "indeterminate", "{attempt}");
    assert_eq!(attempt["delivery"], "spill", "{attempt}");

    // The retry reconciles the recorded identity before anything is sent: it
    // cannot deliver the same payload twice.
    let out = fixture.message(&["--file", file.to_str().unwrap()]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("stays refused"), "{text}");
    assert!(
        text.contains(&format!("payloadPath: {}", path.display())),
        "{text}"
    );
    assert_eq!(
        fixture.server.requests_for("turn/steer").len(),
        1,
        "the repeat must not reach the conversation again: {text}"
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        payload.as_bytes(),
        "the retry rewrites the same one file with the same bytes"
    );
}

#[test]
fn releasing_a_run_removes_only_its_own_spilled_messages() {
    let mut fixture = Fixture::new("cleanup", "running", true);
    let payload = payload_of(300 * 1024);
    let id = content_id(&payload);
    let path = spill_path(&fixture.home, &id);
    let file = fixture.home.join("correction.txt");
    fs::write(&file, &payload).unwrap();
    fixture.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(thread_read(
                &fixture.slot,
                "idle",
                json!([{
                    "id": TURN,
                    "status": "completed",
                    "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
                }]),
            )),
            Answer::Result(thread_read(
                &fixture.slot,
                "active",
                turns_with_input_identity(&id, &payload),
            )),
        ],
    );
    let out = fixture.message(&["--file", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", output_text(&out));
    assert!(path.is_file(), "{path:?}");

    // A neighboring run's message, recorded on its own receipt, is none of this
    // run's business.
    let neighbor = spill_path(&fixture.home, "msg-000000000000000000000000");
    fs::write(&neighbor, b"neighbor payload\n").unwrap();
    let state_dir =
        harness_core::task_worktree::pool_state_dir(&fixture.home, &fixture.source).unwrap();
    let mut neighbor_receipt = fixture.receipt();
    neighbor_receipt["slot"]["index"] = json!(2);
    neighbor_receipt["slot"]["owner"] = json!("exec-neighbor");
    neighbor_receipt["messages"] = json!([{
        "schema": 1,
        "id": "msg-000000000000000000000000",
        "status": "delivered",
        "delivery": "spill",
        "payloadPath": neighbor,
        "payloadBytes": 17,
    }]);
    fs::write(
        state_dir.join("spawn-2.json"),
        serde_json::to_vec_pretty(&neighbor_receipt).unwrap(),
    )
    .unwrap();

    // The release owns the run's records: the host ends, then the run's message
    // files go with them.
    fs::write(&fixture.release, b"release").unwrap();
    wait_for(
        || {
            let host = fixture.host.try_wait().unwrap();
            host.is_some()
        },
        "the fixture host ends when its release file appears",
    );
    let mut release = lead_command();
    release
        .args([
            "executor",
            "release",
            "--source",
            fixture.source.to_str().unwrap(),
            "--codex-home",
            fixture.home.to_str().unwrap(),
            "--slot",
            "1",
            "--disposition",
            "discarded",
            "--reason",
            "spilled message cleanup",
        ])
        .current_dir(&fixture.source);
    let out = release.output().unwrap();
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("message files: 1 spilled message file(s)"),
        "{text}"
    );
    assert!(!path.exists(), "the released run's spill file is removed");
    assert!(
        neighbor.is_file(),
        "a neighboring run's message survives this run's release"
    );
}

#[test]
fn the_only_live_run_is_addressed_without_any_address_field() {
    let fixture = Fixture::new("defaults", "running", true);
    fixture.record_installation();
    let delivered = "address-free correction\n";
    fixture.server.answer_sequence(
        "thread/read",
        vec![
            Answer::Result(thread_read(
                &fixture.slot,
                "idle",
                json!([{
                    "id": TURN,
                    "status": "completed",
                    "items": [{"id": "m1", "type": "agentMessage", "text": FINAL}]
                }]),
            )),
            Answer::Result(thread_read(
                &fixture.slot,
                "active",
                turns_with_input(delivered),
            )),
        ],
    );
    // The working directory is a different checkout whose configuration names a
    // different profile: resolution reads the harness records only, never cwd.
    let elsewhere = fixture.home.join("unrelated-checkout");
    fs::create_dir_all(elsewhere.join("global")).unwrap();
    fs::write(elsewhere.join("global/orchestration.toml"), orchestration()).unwrap();
    let mut command = fixture.unaddressed_command(&["--text", delivered]);
    command.current_dir(&elsewhere);
    let out = command.output().unwrap();
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains(&format!(
            "executor message: slot 1 owner {OWNER} session {THREAD}"
        )),
        "the resolved run is named exactly as a typed address would be: {text}"
    );
    let started = fixture.server.requests_for("turn/start");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(started[0]["params"]["threadId"], THREAD, "{started:?}");
    assert_eq!(started[0]["params"]["input"][0]["text"], delivered);
}

#[test]
fn several_live_runs_refuse_the_default_and_a_named_slot_still_wins() {
    let fixture = Fixture::new("ambiguous", "running", true);
    fixture.record_installation();
    let mut other = fixture.add_live_run("exec-other", "01a0c719-f4d4-7880-a9d2-1a96ee0f29aa");
    let out = fixture
        .unaddressed_command(&["--text", "correction"])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("addresses exactly one run and 2 match"),
        "the refusal must list the live runs: {text}"
    );
    assert!(text.contains(&format!("slot 1 owner {OWNER}")), "{text}");
    assert!(text.contains("slot 2 owner exec-other"), "{text}");
    assert!(text.contains("pass --slot N"), "{text}");
    assert!(text.contains("nothing was sent"), "{text}");
    assert!(
        fixture.server.requests().is_empty(),
        "an ambiguous address must send nothing: {text}"
    );

    // One explicit field is enough to choose: the owner and the session still
    // come from the recorded binding of that slot, and never from the other run.
    let delivered = "named slot correction\n";
    fixture.server.answer_sequence(
        "thread/read",
        vec![Answer::Result(thread_read(
            &fixture.slot,
            "active",
            turns_with_input(delivered),
        ))],
    );
    let out = fixture
        .unaddressed_command(&["--slot", "1", "--text", delivered])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains(&format!("slot 1 owner {OWNER} session {THREAD}")),
        "{text}"
    );
    other.kill().unwrap();
    let _ = other.wait();
}

#[test]
fn an_absent_or_stale_run_reports_its_recorded_state_without_sending() {
    let mut fixture = Fixture::new("stale", "running", true);
    fixture.record_installation();
    // No live lease at all: no live run is recorded, so the address is refused
    // instead of guessed.
    fs::remove_file(&fixture.lease).unwrap();
    let out = fixture
        .unaddressed_command(&["--text", "correction"])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("no live executor run is recorded"), "{text}");
    assert!(text.contains("nothing was sent"), "{text}");
    assert!(fixture.server.requests().is_empty(), "{text}");

    // A named slot keeps the existing honest state error: the recorded run's own
    // host is gone, and that is what is reported.
    fixture.host.kill().unwrap();
    let _ = fixture.host.wait();
    let out = fixture
        .unaddressed_command(&["--slot", "1", "--text", "correction"])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(text.contains("not delivered (unavailable)"), "{text}");
    assert!(text.contains("is not running"), "{text}");
    assert!(text.contains("nothing was sent"), "{text}");
    assert!(fixture.server.requests().is_empty(), "{text}");
}

#[test]
fn two_sources_sharing_a_home_and_two_homes_sharing_a_terminal_never_mix() {
    let fixture = Fixture::new("cross", "running", true);
    fixture.record_installation();

    // Two checkouts under one kit home: the explicit source has no live run, and
    // the other source's run is never substituted for it.
    let other_source = fixture._root.path().join("proj-other");
    fs::create_dir_all(&other_source).unwrap();
    let out = fixture
        .unaddressed_command(&[
            "--source",
            other_source.to_str().unwrap(),
            "--text",
            "correction",
        ])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("no live executor run is recorded"), "{text}");
    assert!(text.contains("proj-other"), "{text}");
    assert!(fixture.server.requests().is_empty(), "{text}");

    // Two kit homes in one terminal: the explicit home wins, and the live run
    // recorded under the other home is not used.
    let other_home = fixture._root.path().join("other-home");
    fs::create_dir_all(other_home.join("harness")).unwrap();
    fs::write(
        other_home.join("harness/installation.json"),
        serde_json::to_vec_pretty(&json!({
            "schemaVersion": 2,
            "settings": {"sourceRoot": fixture.source},
        }))
        .unwrap(),
    )
    .unwrap();
    let mut command = fixture.unaddressed_command(&["--text", "correction"]);
    command.env("CODEX_HOME", &other_home);
    let out = command.output().unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("no live executor run is recorded"), "{text}");
    assert!(text.contains("other-home"), "{text}");
    assert!(
        fixture.server.requests().is_empty(),
        "the other home's live run must stay untouched: {text}"
    );
}

#[test]
fn a_reused_slot_refuses_the_resolved_address_with_the_observed_mismatch() {
    let fixture = Fixture::new("rebound", "running", true);
    fixture.record_installation();
    let state_dir =
        harness_core::task_worktree::pool_state_dir(&fixture.home, &fixture.source).unwrap();
    let mut record: Value =
        serde_json::from_slice(&fs::read(state_dir.join("slot-1.json")).unwrap()).unwrap();
    record["owner"] = json!("exec-reused");
    fs::write(
        state_dir.join("slot-1.json"),
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();

    // The live lease still names the old owner while the binding names another:
    // the default address refuses instead of guessing a replacement.
    let out = fixture
        .unaddressed_command(&["--text", "correction"])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("no live executor run is recorded"), "{text}");
    assert!(
        text.contains(&format!("slot 1: not bound to {OWNER}")),
        "{text}"
    );
    assert!(fixture.server.requests().is_empty(), "{text}");

    // A named slot resolves the recorded owner, and the unchanged verification
    // then reports the mismatch between that binding and the dispatch receipt.
    let out = fixture
        .unaddressed_command(&["--slot", "1", "--text", "correction"])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains(&format!("belongs to owner {OWNER} instead of exec-reused")),
        "{text}"
    );
    assert!(
        fixture.server.requests().is_empty(),
        "the reused slot must receive nothing: {text}"
    );
}
