#![cfg(windows)]

//! Native checks of `executor stop` through the real CLI entry point: one bound
//! pooled slot, a real tab-host process running the owned executor fixture, and
//! the actual termination, verification, receipt and refusal paths.

use harness_core::process::ProcessIdentity;
use harness_core::process_service::{self, ServiceProcess};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const SESSION: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

/// The owned launcher double that emits the CLI's `exec --json` event stream.
fn launcher() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"))
}

/// A lead invocation: no inherited executor marker, so the kit's nested
/// dispatch refusal does not apply to the calling side.
fn lead_command() -> Command {
    let mut command = Command::new(manager());
    command.env_remove("HARNESS_EXECUTOR_SESSION");
    command.env_remove("WT_SESSION");
    command
}

fn text(out: &Output) -> String {
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

fn file_url(path: &Path) -> String {
    format!("file:///{}", path.to_str().unwrap().replace('\\', "/"))
}

fn orchestration(pool_size: u32) -> String {
    format!(
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = {pool_size}\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\nworktree_limit = 1\n"
    )
}

fn receipt_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn identity_of(value: &Value) -> ProcessIdentity {
    ProcessIdentity {
        pid: value["pid"].as_u64().unwrap() as u32,
        creation_time: value["creation_time"].as_u64().unwrap(),
    }
}

/// Waits for an owned fixture identity marker (pid plus creation time).
fn wait_for_marker(path: &Path) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(bytes) = fs::read(path)
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
        {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "fixture marker {} never appeared",
            path.display()
        );
        thread::sleep(Duration::from_millis(50));
    }
}

/// Asserts that the exact recorded process is gone within a bounded wait.
fn wait_gone(identity: ProcessIdentity, program: &Path, label: &str) {
    let user = process_service::current_user().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let alive = matches!(
            ServiceProcess::inspect(identity, program, &user),
            Ok(Some(_))
        );
        if !alive {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{label} {identity:?} is still running after the stop"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

fn is_alive(identity: ProcessIdentity, program: &Path) -> bool {
    let user = process_service::current_user().unwrap();
    matches!(
        ServiceProcess::inspect(identity, program, &user),
        Ok(Some(_))
    )
}

/// Bounded wait for an owned host process instead of an unbounded wait.
fn wait_host(host: &mut std::process::Child, label: &str) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = host.try_wait().unwrap() {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "{label} did not exit within the bound"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

/// One bound pooled slot, its kit-local state directory and a receipt the
/// dispatcher would have written for it.
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("executor-stop-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let bare = root.join("remote.git");
        git(
            &root,
            &[
                "init",
                "--bare",
                "-q",
                "--initial-branch=main",
                bare.to_str().unwrap(),
            ],
        );
        let seed = root.join("seed");
        git(
            &root,
            &[
                "init",
                "-q",
                "--initial-branch=main",
                seed.to_str().unwrap(),
            ],
        );
        git(&seed, &["config", "user.email", "stop@example.test"]);
        git(&seed, &["config", "user.name", "Stop"]);
        fs::write(seed.join("README.md"), "seed\n").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-qm", "seed"]);
        git(&seed, &["remote", "add", "origin", &file_url(&bare)]);
        git(&seed, &["push", "-q", "origin", "main"]);
        let source = root.join("proj");
        git(
            &root,
            &["clone", "-q", &file_url(&bare), source.to_str().unwrap()],
        );
        git(&source, &["config", "user.email", "stop@example.test"]);
        git(&source, &["config", "user.name", "Stop"]);
        fs::create_dir_all(source.join("global")).unwrap();
        fs::write(source.join("global/orchestration.toml"), orchestration(1)).unwrap();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("ds.config.toml"), "model = 'deepseek-flash'\n").unwrap();
        Self { root, source, home }
    }

    fn slot(&self) -> PathBuf {
        self.root.join("proj-wt1")
    }

    fn state_dir(&self) -> PathBuf {
        let state = self.home.join("harness/executor-pool");
        fs::read_dir(&state)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.is_dir())
            .expect("one pool state directory per source checkout")
    }

    fn receipt(&self) -> PathBuf {
        self.state_dir().join("spawn-1.json")
    }

    fn lease(&self) -> PathBuf {
        self.state_dir().join("lease-1.json")
    }

    fn slot_record(&self) -> Value {
        serde_json::from_slice(&fs::read(self.state_dir().join("slot-1.json")).unwrap()).unwrap()
    }

    /// Binds slot 1 to one owner: a dispatch whose launcher preflight fails
    /// still records the session binding, exactly as pooled dispatch does.
    fn bind(&self, owner: &str) {
        let out = lead_command()
            .args([
                "executor",
                "spawn",
                "--source",
                self.source.to_str().unwrap(),
                "--codex-home",
                self.home.to_str().unwrap(),
                "--profile",
                "ds",
                "--owner",
                owner,
                "--exec",
                "assignment text",
            ])
            .output()
            .unwrap();
        assert!(!out.status.success(), "{}", text(&out));
        assert_eq!(self.slot_record()["owner"], owner);
    }

    /// The receipt the dispatcher writes for slot 1, with the recorded terminal
    /// tab identity and the observation the host then keeps current.
    fn seed(&self, owner: &str, state: &str) -> Value {
        let workspace = self.slot();
        let result = self.state_dir().join("message-1.txt");
        let detail = self.state_dir().join("stream-1.jsonl");
        let receipt = self.receipt();
        let title = format!("harness stop fixture {}", std::process::id());
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let binding = json!({
            "index": 1,
            "path": workspace.to_string_lossy(),
            "source": self.source.to_string_lossy(),
            "owner": owner,
            "base": self.slot_record()["base"],
            "remote": "origin",
            "branch": "main"
        });
        let value = json!({
            "schema": 1,
            "launcher": launcher().to_string_lossy(),
            "profile": "ds",
            "mode": "exec",
            "args": [
                "--profile", "ds", "exec", "--json", "--skip-git-repo-check",
                "-C", workspace.to_string_lossy(),
                "--output-last-message", result.to_string_lossy(),
                "fixture assignment text"
            ],
            "visible": true,
            "host": "windows-terminal-tab",
            // The recorded identity of exactly this run's tab; the fixture host
            // runs outside a real terminal, so no window carries this title.
            "terminal": [
                "-w", "codex-harness-stop-fixture", "new-tab",
                "--title", title, "--suppressApplicationTitle",
                "-d", workspace.to_string_lossy(),
                manager().to_string_lossy(), "executor", "run",
                "--file", receipt.to_string_lossy()
            ],
            "isolation": false,
            "slot": binding,
            "model": "deepseek-flash",
            "modelProvider": "deepseek",
            "reasoningEffort": "max",
            "window": Value::Null,
            "shell": Value::Null,
            "observation": {
                "schema": 1,
                "coverage": "native",
                "reason": Value::Null,
                "state": state,
                "session": SESSION,
                "previousSession": Value::Null,
                "exitCode": Value::Null,
                "events": 0,
                "messages": 0,
                "toolCalls": 0,
                "malformed": 0,
                "cause": Value::Null,
                "host": Value::Null,
                "result": result.to_string_lossy(),
                "detail": detail.to_string_lossy(),
                "updatedMs": now_ms
            }
        });
        fs::write(&receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        value
    }

    /// Runs the real tab host for the seeded receipt, exactly as the terminal
    /// tab would, with the owned fixture emitting the run's event stream.
    fn host(&self, mode: &str, envs: &[(&str, &Path)]) -> std::process::Child {
        let mut command = lead_command();
        command
            .args(["executor", "run", "--file"])
            .arg(self.receipt())
            .env("CODEX_HOME", &self.home)
            .env("HARNESS_EXECUTOR_FIXTURE_MODE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (name, value) in envs {
            command.env(name, value);
        }
        command.spawn().unwrap()
    }

    fn stop(&self, owner: &str, extra: &[&str]) -> Output {
        lead_command()
            .args([
                "executor",
                "stop",
                "--source",
                self.source.to_str().unwrap(),
                "--codex-home",
                self.home.to_str().unwrap(),
                "--slot",
                "1",
                "--owner",
                owner,
            ])
            .args(extra)
            .output()
            .unwrap()
    }

    /// Waits until the host recorded its own exact identity in the receipt.
    fn wait_recorded_host(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let recorded = receipt_json(&self.receipt())["observation"]["host"].clone();
            if recorded["pid"].as_u64().is_some() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the host never recorded its identity"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn drop(self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A decoy run-like process the fixture emits the exact identity of; it is never
/// a descendant of the stopped host, so only a recorded-identity action can end
/// it.
fn decoy(root: &Path, name: &str) -> (std::process::Child, Value) {
    let marker = root.join(format!("{name}-identity.json"));
    let release = root.join(format!("{name}-release"));
    let child = Command::new(launcher())
        .env("HARNESS_EXECUTOR_FIXTURE_MODE", "hang")
        .env("HARNESS_EXECUTOR_FIXTURE_STARTED", &marker)
        .env("HARNESS_EXECUTOR_FIXTURE_RELEASE", &release)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    (child, wait_for_marker(&marker))
}

#[test]
fn stop_terminates_the_owned_tree_and_preserves_the_run() {
    let fixture = Fixture::new("tree");
    let owner = "exec-ds-stop-1";
    fixture.bind(owner);
    let mut receipt = fixture.seed(owner, "running");
    // One queued message has to be marked undelivered; a delivered one must not
    // be rewritten.
    receipt["messages"] = json!([
        {"id": "m-queued", "status": "queued"},
        {"id": "m-delivered", "status": "delivered"}
    ]);
    fs::write(
        fixture.receipt(),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    // Partial work that a stop must never touch.
    fs::write(fixture.slot().join("README.md"), "partial work\n").unwrap();
    fs::write(fixture.slot().join("untracked.txt"), "keep me\n").unwrap();
    let started = fixture.root.join("launcher-identity.json");
    let descendant = fixture.root.join("descendant-identity.json");
    let mut host = fixture.host(
        "descendant",
        &[
            ("HARNESS_EXECUTOR_FIXTURE_STARTED", &started),
            ("HARNESS_EXECUTOR_FIXTURE_CHILD_MARKER", &descendant),
        ],
    );
    let owned = wait_for_marker(&started);
    let child = wait_for_marker(&descendant);
    fixture.wait_recorded_host();
    let record_before = fixture.slot_record();

    let out = fixture.stop(owner, &["--session", SESSION]);
    let output = text(&out);
    assert_eq!(out.status.code(), Some(0), "{output}");
    assert!(output.contains(": stopped in "), "{output}");
    assert!(
        output.contains("no reset, clean, release or completion claim"),
        "{output}"
    );
    assert!(output.contains("marked undelivered"), "{output}");
    assert!(output.contains("exit code unknown"), "{output}");
    assert!(
        output.contains("terminal: the recorded tab ") && output.contains(" closed"),
        "the recorded tab identity is verified closed through the terminal surface: {output}"
    );

    wait_host(&mut host, "stopped host");
    wait_gone(identity_of(&owned), &launcher(), "the owned launcher");
    wait_gone(identity_of(&child), &launcher(), "the owned descendant");

    let recorded = receipt_json(&fixture.receipt());
    assert_eq!(recorded["observation"]["state"], "stopped", "{recorded}");
    assert_eq!(recorded["stop"]["outcome"], "stopped", "{recorded}");
    assert_eq!(recorded["stop"]["survivors"], json!([]), "{recorded}");
    assert_eq!(recorded["stop"]["undelivered"], json!(["m-queued"]));
    assert!(
        recorded["stop"]["durationMs"].as_u64().is_some(),
        "the stop records its measured duration: {recorded}"
    );
    assert!(
        recorded["stop"]["completedMs"].as_u64() >= recorded["stop"]["requestedMs"].as_u64(),
        "{recorded}"
    );
    // An exit code that was never observed stays unknown, and the host's own
    // termination request is not presented as the run's result.
    assert!(recorded["stop"]["exitCode"].is_null(), "{recorded}");
    assert!(recorded["observation"]["exitCode"].is_null(), "{recorded}");
    assert_eq!(recorded["messages"][0]["status"], "undelivered");
    assert!(recorded["messages"][0]["undeliveredMs"].is_u64());
    assert_eq!(recorded["messages"][1]["status"], "delivered");
    // No release, no completion, no slot or lease mutation.
    let record_after = fixture.slot_record();
    assert_eq!(record_after["owner"], record_before["owner"]);
    assert!(record_after["disposition"].is_null(), "{record_after}");
    assert_eq!(record_after["state"], record_before["state"]);
    assert!(
        fixture.lease().is_file(),
        "stop leaves the recorded lease alone"
    );
    assert_eq!(
        fs::read_to_string(fixture.slot().join("README.md")).unwrap(),
        "partial work\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.slot().join("untracked.txt")).unwrap(),
        "keep me\n"
    );

    // A repeated stop reports the recorded state instead of acting again: the
    // first stop's outcome, timestamps and measured duration stand.
    let before = recorded["stop"].clone();
    let repeat = fixture.stop(owner, &["--session", SESSION]);
    let repeat_text = text(&repeat);
    assert_eq!(repeat.status.code(), Some(0), "{repeat_text}");
    assert!(repeat_text.contains("already-stopped"), "{repeat_text}");
    assert!(
        repeat_text.contains("repeated stop request"),
        "{repeat_text}"
    );
    let repeated = receipt_json(&fixture.receipt());
    assert_eq!(repeated["stop"]["outcome"], "stopped", "{repeated}");
    assert_eq!(repeated["stop"]["repeats"], 1, "{repeated}");
    assert_eq!(repeated["stop"]["completedMs"], before["completedMs"]);
    assert_eq!(repeated["stop"]["durationMs"], before["durationMs"]);
    assert!(
        repeated["stop"]["detail"]
            .as_str()
            .unwrap()
            .contains(before["detail"].as_str().unwrap()),
        "{repeated}"
    );
    assert_eq!(repeated["observation"]["state"], "stopped", "{repeated}");
    assert!(!is_alive(identity_of(&owned), &launcher()));
    fixture.drop();
}

#[test]
fn stale_host_identity_is_refused_and_nothing_is_terminated() {
    let fixture = Fixture::new("stale");
    let owner = "exec-ds-stop-2";
    fixture.bind(owner);
    fixture.seed(owner, "running");
    let (mut decoy, identity) = decoy(&fixture.root, "decoy");
    // The record names this live process by a *different* creation time, so the
    // addressed identity is stale and must not authorize any termination.
    let recorded = json!({
        "schema": 1,
        "owner": owner,
        "index": 1,
        "path": fixture.slot().to_string_lossy(),
        "pid": identity_of(&identity).pid,
        "created": identity_of(&identity).creation_time + 1,
        "program": launcher().to_string_lossy()
    });
    fs::write(
        fixture.lease(),
        serde_json::to_vec_pretty(&recorded).unwrap(),
    )
    .unwrap();

    // An exact-session address that does not match the recorded session is
    // refused before anything is acted on.
    let mismatched = fixture.stop(
        owner,
        &["--session", "01a0c719-0000-0000-0000-000000000000"],
    );
    let mismatched_text = text(&mismatched);
    assert_eq!(mismatched.status.code(), Some(2), "{mismatched_text}");
    assert!(mismatched_text.contains(SESSION), "{mismatched_text}");

    let out = fixture.stop(owner, &["--session", SESSION]);
    let output = text(&out);
    assert_eq!(out.status.code(), Some(1), "{output}");
    assert!(
        output.contains("no live process matches the recorded host identity"),
        "{output}"
    );
    assert!(output.contains("terminated nothing"), "{output}");
    assert!(output.contains("exit code unknown"), "{output}");
    // The live decoy was never terminated on stale identity.
    assert!(is_alive(identity_of(&identity), &launcher()));
    let recorded_receipt = receipt_json(&fixture.receipt());
    assert_eq!(recorded_receipt["stop"]["outcome"], "error");
    assert_eq!(recorded_receipt["observation"]["state"], "interrupted");
    assert!(recorded_receipt["stop"]["survivors"] == json!([]));
    assert_eq!(fixture.slot_record()["owner"], owner);
    let _ = decoy.kill();
    let _ = decoy.wait();
    fixture.drop();
}

#[test]
fn host_death_reaps_the_owned_tree_and_stop_reports_the_unobserved_end() {
    let fixture = Fixture::new("reap");
    let owner = "exec-ds-stop-3";
    fixture.bind(owner);
    fixture.seed(owner, "running");
    let started = fixture.root.join("reap-launcher.json");
    let descendant = fixture.root.join("reap-descendant.json");
    let mut host = fixture.host(
        "descendant",
        &[
            ("HARNESS_EXECUTOR_FIXTURE_STARTED", &started),
            ("HARNESS_EXECUTOR_FIXTURE_CHILD_MARKER", &descendant),
        ],
    );
    let owned = wait_for_marker(&started);
    let child = wait_for_marker(&descendant);
    fixture.wait_recorded_host();
    // The host dies abnormally, exactly the case kill-on-close must cover.
    let user = process_service::current_user().unwrap();
    let recorded = receipt_json(&fixture.receipt())["observation"]["host"].clone();
    let host_identity = ProcessIdentity {
        pid: recorded["pid"].as_u64().unwrap() as u32,
        creation_time: recorded["created"].as_u64().unwrap(),
    };
    let process = ServiceProcess::inspect(host_identity, &manager(), &user)
        .unwrap()
        .expect("the recorded host is live");
    process.terminate(9).unwrap();
    wait_host(&mut host, "killed host");
    wait_gone(identity_of(&owned), &launcher(), "the owned launcher");
    wait_gone(identity_of(&child), &launcher(), "the owned descendant");

    let out = fixture.stop(owner, &[]);
    let output = text(&out);
    assert_eq!(out.status.code(), Some(1), "{output}");
    assert!(output.contains("terminated nothing"), "{output}");
    assert!(output.contains("exit code is unknown"), "{output}");
    let recorded = receipt_json(&fixture.receipt());
    assert_eq!(recorded["stop"]["outcome"], "error", "{recorded}");
    assert_eq!(
        recorded["observation"]["state"], "interrupted",
        "{recorded}"
    );
    assert!(recorded["observation"]["exitCode"].is_null(), "{recorded}");
    assert_eq!(fixture.slot_record()["owner"], owner);
    fixture.drop();
}

#[test]
fn a_recorded_member_surviving_the_host_is_terminated_by_identity() {
    let fixture = Fixture::new("member");
    let owner = "exec-ds-stop-4";
    fixture.bind(owner);
    fixture.seed(owner, "running");
    // A recorded process of the run that is not inside the host's job; only the
    // stop's own identity-verified termination can end it.
    let (mut member, identity) = decoy(&fixture.root, "member");
    let exact = identity_of(&identity);
    fs::write(
        fixture.state_dir().join("endpoint-1.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "port": 1,
            "token": "synthetic-owned-token",
            "threadId": SESSION,
            "process": {
                "pid": exact.pid,
                "creationTime": exact.creation_time,
                "program": launcher().to_string_lossy()
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let started = fixture.root.join("member-launcher.json");
    let mut host = fixture.host("hang", &[("HARNESS_EXECUTOR_FIXTURE_STARTED", &started)]);
    let owned = wait_for_marker(&started);
    fixture.wait_recorded_host();

    let out = fixture.stop(owner, &[]);
    let output = text(&out);
    assert_eq!(out.status.code(), Some(0), "{output}");
    assert!(output.contains(": stopped in "), "{output}");
    assert!(output.contains("did not accept a connection"), "{output}");
    wait_host(&mut host, "stopped host");
    wait_gone(identity_of(&owned), &launcher(), "the owned launcher");
    wait_gone(exact, &launcher(), "the recorded member");
    let _ = member.wait();
    let recorded = receipt_json(&fixture.receipt());
    assert_eq!(recorded["stop"]["outcome"], "stopped", "{recorded}");
    assert_eq!(recorded["stop"]["survivors"], json!([]), "{recorded}");
    assert!(
        recorded["stop"]["ended"].as_u64().unwrap() >= 2,
        "the host and the recorded member are verified gone: {recorded}"
    );
    fixture.drop();
}

#[test]
fn an_unverifiable_recorded_member_is_a_partial_stop() {
    let fixture = Fixture::new("partial");
    let owner = "exec-ds-stop-5";
    fixture.bind(owner);
    fixture.seed(owner, "running");
    let (mut member, identity) = decoy(&fixture.root, "unverified");
    let exact = identity_of(&identity);
    // The record names the live pid and creation time but another image, so the
    // process cannot be verified and must never be terminated on that record.
    fs::write(
        fixture.state_dir().join("endpoint-1.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "port": 1,
            "token": "synthetic-owned-token",
            "threadId": SESSION,
            "process": {
                "pid": exact.pid,
                "creationTime": exact.creation_time,
                "program": manager().to_string_lossy()
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let started = fixture.root.join("partial-launcher.json");
    let mut host = fixture.host("hang", &[("HARNESS_EXECUTOR_FIXTURE_STARTED", &started)]);
    wait_for_marker(&started);
    fixture.wait_recorded_host();

    let out = fixture.stop(owner, &[]);
    let output = text(&out);
    assert_eq!(out.status.code(), Some(2), "{output}");
    assert!(output.contains(": partial in "), "{output}");
    assert!(output.contains("survivor: process pid"), "{output}");
    assert!(output.contains("next action:"), "{output}");
    wait_host(&mut host, "stopped host");
    assert!(
        is_alive(exact, &launcher()),
        "an unverified recorded process must never be terminated"
    );
    let recorded = receipt_json(&fixture.receipt());
    assert_eq!(recorded["stop"]["outcome"], "partial", "{recorded}");
    assert_eq!(
        recorded["observation"]["state"], "partial-stop",
        "{recorded}"
    );
    assert_eq!(
        recorded["stop"]["survivors"][0]["pid"].as_u64().unwrap() as u32,
        exact.pid
    );
    assert!(
        recorded["stop"]["survivors"][0]["nextAction"]
            .as_str()
            .is_some_and(|action| !action.is_empty())
    );
    // The addressed slot is still bound to its owner and nothing was released.
    assert_eq!(fixture.slot_record()["owner"], owner);
    let _ = member.kill();
    let _ = member.wait();
    fixture.drop();
}

#[test]
fn stop_racing_natural_completion_reports_the_completed_result() {
    let fixture = Fixture::new("race");
    let owner = "exec-ds-stop-6";
    fixture.bind(owner);
    fixture.seed(owner, "completed");
    let result = fixture.state_dir().join("message-1.txt");
    fs::write(&result, "FIXTURE_OUTCOME_DONE\n").unwrap();
    // A host identity still live while the recorded lifecycle already completed:
    // the stop must report the completed result and touch nothing.
    let (mut decoy, identity) = decoy(&fixture.root, "completing");
    let exact = identity_of(&identity);
    fs::write(
        fixture.lease(),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "owner": owner,
            "index": 1,
            "path": fixture.slot().to_string_lossy(),
            "pid": exact.pid,
            "created": exact.creation_time,
            "program": launcher().to_string_lossy()
        }))
        .unwrap(),
    )
    .unwrap();

    let out = fixture.stop(owner, &[]);
    let output = text(&out);
    assert_eq!(out.status.code(), Some(0), "{output}");
    assert!(output.contains("already-completed"), "{output}");
    assert!(output.contains("message-1.txt"), "{output}");
    assert!(is_alive(exact, &launcher()));
    let recorded = receipt_json(&fixture.receipt());
    assert_eq!(recorded["observation"]["state"], "completed", "{recorded}");
    assert_eq!(
        recorded["stop"]["outcome"], "already-completed",
        "{recorded}"
    );
    let _ = decoy.kill();
    let _ = decoy.wait();
    fixture.drop();
}
