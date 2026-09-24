//! Observed executor lifecycle: the owned native event fixture drives the
//! tab host, the recorded receipt, `executor watch`, `executor resume` and
//! `executor release`. The pool slots are ordinary Git worktrees of a
//! synthetic `file://` upstream; no check here touches a model, a network
//! provider or a subscription.
#![cfg(windows)]

use serde_json::{Value, json};
#[path = "fixtures/cache_usage.rs"]
mod cache_usage;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn manager() -> PathBuf {
    std::env::var_os("HARNESS_OBSERVATION_MANAGER_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_codex-harness")))
}

/// Owned native event fixture: emits the CLI's `exec --json` control-plane
/// contract without a model.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"))
}

/// A dispatch invocation that does not inherit the caller's session identity
/// or a terminal host address.
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

fn receipt_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// One seeded slotless receipt whose recorded run is the owned fixture. The
/// observation block is exactly what the dispatcher writes before the host
/// opens, so the checks exercise the real host path rather than a stub.
struct SeededRun {
    root: PathBuf,
    _cleanup: Option<tempfile::TempDir>,
    receipt: PathBuf,
    result: PathBuf,
    detail: PathBuf,
}

impl SeededRun {
    fn new() -> Self {
        let cleanup = tempfile::tempdir().unwrap();
        Self::at(cleanup.path().to_path_buf(), Some(cleanup))
    }

    /// Places the seeded receipt in an explicit kit-local state directory,
    /// exactly where the dispatcher writes it.
    fn at(root: PathBuf, cleanup: Option<tempfile::TempDir>) -> Self {
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let result = root.join("message-1.txt");
        let detail = root.join("stream-1.jsonl");
        let receipt = root.join("spawn-1.json");
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "launcher": fixture().to_string_lossy(),
                "profile": "ds",
                "mode": "exec",
                "args": [
                    "--profile", "ds", "-c", "agents.enabled=false",
                    "exec", "--json", "--skip-git-repo-check",
                    "-C", workspace.to_string_lossy(),
                    "--output-last-message", result.to_string_lossy(),
                    "fixture assignment text"
                ],
                "visible": true,
                "host": "windows-terminal-tab",
                "terminal": Value::Null,
                "isolation": false,
                "slot": Value::Null,
                "model": "deepseek-flash",
                "modelProvider": "deepseek",
                "reasoningEffort": "max",
                "window": Value::Null,
                // Receipts written before shell recording fall back to the
                // launcher preflight, which the owned fixture answers.
                "shell": Value::Null,
                "observation": {
                    "schema": 1,
                    "coverage": "native",
                    "reason": Value::Null,
                    "state": "dispatch-accepted",
                    "session": Value::Null,
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
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            root,
            _cleanup: cleanup,
            receipt,
            result,
            detail,
        }
    }

    fn observed(&self, mode: &str) -> Output {
        self.observed_with(mode, &[])
    }

    fn observed_with(&self, mode: &str, envs: &[(&str, &Path)]) -> Output {
        let mut command = lead_command();
        command
            .args(["executor", "run", "--file"])
            .arg(&self.receipt)
            .env("HARNESS_EXECUTOR_FIXTURE_MODE", mode)
            .stdin(Stdio::null());
        for (name, value) in envs {
            command.env(name, value);
        }
        command.output().unwrap()
    }

    /// Path the host spools the owned child's raw stdout into.
    fn spool(&self) -> PathBuf {
        self.receipt.with_extension("running.jsonl")
    }

    fn host(&self, mode: &str, envs: &[(&str, &Path)]) -> std::process::Child {
        let mut command = lead_command();
        command
            .args(["executor", "run", "--file"])
            .arg(&self.receipt)
            .env("HARNESS_EXECUTOR_FIXTURE_MODE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (name, value) in envs {
            command.env(name, value);
        }
        command.spawn().unwrap()
    }

    fn watch(&self, extra: &[&str]) -> Output {
        lead_command()
            .args(["executor", "watch", "--receipt"])
            .arg(&self.receipt)
            .args(extra)
            .output()
            .unwrap()
    }
}

#[test]
fn observed_run_records_identity_and_renders_readable_events() {
    let run = SeededRun::new();
    let session = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
    let out = lead_command()
        .args(["executor", "run", "--file"])
        .arg(&run.receipt)
        .env("HARNESS_EXECUTOR_FIXTURE_SESSION", session)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let output = text(&out);
    assert_eq!(out.status.code(), Some(0), "{output}");
    // The visible surface carries the dispatched identity and the assignment.
    assert!(
        output.contains(
            "executor session: profile=ds model=deepseek-flash provider=deepseek effort=max"
        ),
        "{output}"
    );
    assert!(output.contains("assignment:"), "{output}");
    assert!(output.contains("fixture assignment text"), "{output}");
    // Native identity, readable messages, tool activity and state.
    assert!(
        output.contains(&format!("native start (session {session})")),
        "{output}"
    );
    assert!(output.contains("assistant:"), "{output}");
    assert!(output.contains("FIXTURE_OUTCOME_DONE"), "{output}");
    assert!(
        output.contains("tool: exec fixture check -> completed (exit 0)"),
        "{output}"
    );
    assert!(output.contains("turn completed"), "{output}");
    assert!(
        output.contains("result: completed") && output.contains("result message:"),
        "{output}"
    );
    let receipt = receipt_json(&run.receipt);
    let observation = &receipt["observation"];
    assert_eq!(observation["state"], "completed", "{receipt}");
    assert_eq!(observation["session"], session, "{receipt}");
    assert_eq!(observation["exitCode"], 0, "{receipt}");
    assert!(observation["events"].as_u64().unwrap() >= 5, "{receipt}");
    assert_eq!(observation["messages"], 1, "{receipt}");
    assert_eq!(observation["toolCalls"], 1, "{receipt}");
    assert_eq!(observation["malformed"], 0, "{receipt}");
    // The host identity is a full identity, not a bare pid.
    let host = &observation["host"];
    assert!(host["pid"].as_u64().unwrap() > 0, "{receipt}");
    assert!(host["created"].as_u64().unwrap() > 0, "{receipt}");
    assert!(
        host["program"]
            .as_str()
            .unwrap()
            .ends_with("codex-harness.exe"),
        "{receipt}"
    );
    assert!(
        fs::read_to_string(&run.result)
            .unwrap()
            .contains("FIXTURE_OUTCOME_DONE"),
        "the recorded result file is the returned final message"
    );
    assert!(
        fs::read_to_string(&run.detail)
            .unwrap()
            .contains("thread.started"),
        "the bounded detail file keeps the raw stream beside the receipt"
    );
}

#[test]
fn empty_or_missing_final_results_are_named_output_defects() {
    for mode in ["empty", "nofinal"] {
        let run = SeededRun::new();
        let out = run.observed(mode);
        let output = text(&out);
        assert_eq!(
            out.status.code(),
            Some(3),
            "an empty completion is an output defect, not success: {output}"
        );
        let receipt = receipt_json(&run.receipt);
        assert_eq!(receipt["observation"]["state"], "defect", "{receipt}");
        let cause = receipt["observation"]["cause"].as_str().unwrap();
        assert!(output.contains("result: defect"), "{output}");
        match mode {
            "empty" => assert!(cause.contains("output defect"), "{cause}"),
            _ => assert!(cause.contains("--output-last-message"), "{cause}"),
        }
    }
}

#[test]
fn native_error_and_failed_launcher_start_are_named() {
    let run = SeededRun::new();
    let out = run.observed("error");
    let output = text(&out);
    assert_eq!(out.status.code(), Some(1), "{output}");
    let receipt = receipt_json(&run.receipt);
    assert_eq!(receipt["observation"]["state"], "failed", "{receipt}");
    assert!(
        receipt["observation"]["cause"]
            .as_str()
            .unwrap()
            .contains("fixture"),
        "{receipt}"
    );
    assert!(
        output.contains("turn failed: fixture turn failed")
            || output.contains("error: fixture provider error"),
        "{output}"
    );

    let run = SeededRun::new();
    let out = run.observed("nonzero");
    let output = text(&out);
    assert_eq!(
        out.status.code(),
        Some(19),
        "the launcher's own exit code must reach the caller: {output}"
    );
    let receipt = receipt_json(&run.receipt);
    assert_eq!(receipt["observation"]["state"], "failed", "{receipt}");
    assert_eq!(receipt["observation"]["exitCode"], 19, "{receipt}");
    assert!(
        receipt["observation"]["cause"]
            .as_str()
            .unwrap()
            .contains("native start was never observed"),
        "{receipt}"
    );
    assert!(
        !output.contains("result: completed"),
        "a failed start is never reported as a completed run: {output}"
    );
}

#[test]
fn malformed_and_truncated_events_stay_bounded_and_fail_closed() {
    let run = SeededRun::new();
    let out = run.observed("malformed");
    let output = text(&out);
    assert_eq!(out.status.code(), Some(0), "{output}");
    assert!(
        output.contains("unparsed event: not a JSON event"),
        "{output}"
    );
    let receipt = receipt_json(&run.receipt);
    assert_eq!(receipt["observation"]["state"], "completed", "{receipt}");
    assert_eq!(receipt["observation"]["malformed"], 1, "{receipt}");

    let run = SeededRun::new();
    let out = run.observed("truncated");
    let output = text(&out);
    assert_eq!(
        out.status.code(),
        Some(5),
        "a truncated stream with a nonzero exit stays a failure: {output}"
    );
    assert!(output.contains("unparsed event"), "{output}");
    let receipt = receipt_json(&run.receipt);
    assert_eq!(receipt["observation"]["state"], "failed", "{receipt}");
    assert_ne!(
        receipt["observation"]["state"], "completed",
        "a truncated stream never reads as completion: {receipt}"
    );
}

#[test]
fn watch_blocks_on_a_live_run_and_returns_the_compact_result() {
    let pool = PoolFixture::new("watch");
    pool.claim("exec-1");
    let slot = pool.slot();
    fs::write(slot.join("partial.rs"), "fn partial() {}\n").unwrap();
    let run = pool.seeded_slot_receipt("exec-1");
    // The host runs in the background with a slow turn; watch must block on
    // the recorded lifecycle and then return the completed run.
    let mut host = lead_command()
        .args(["executor", "run", "--file"])
        .arg(&run.receipt)
        .env("HARNESS_EXECUTOR_FIXTURE_MODE", "slow")
        .env("HARNESS_EXECUTOR_FIXTURE_DELAY_MS", "2000")
        .env("CODEX_HOME", pool.home.to_str().unwrap())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let out = lead_command()
        .args([
            "executor",
            "watch",
            "--source",
            pool.source.to_str().unwrap(),
            "--codex-home",
            pool.home.to_str().unwrap(),
            "--slot",
            "1",
            "--timeout",
            "60",
            "--poll",
            "100",
        ])
        .output()
        .unwrap();
    let output = text(&out);
    assert_eq!(out.status.code(), Some(0), "{output}");
    assert!(
        started.elapsed() >= Duration::from_millis(1000),
        "watch must block on the live run instead of returning early: {output}"
    );
    assert!(output.contains("state=completed"), "{output}");
    assert!(
        output.contains("session: 01a0c719-f4d4-7880-a9d2-1a96ee0f23f4"),
        "{output}"
    );
    assert!(output.contains("slot: 1"), "{output}");
    assert!(output.contains("owner: exec-1"), "{output}");
    assert!(
        output.contains(&format!("checkout: {}", slot.display())),
        "{output}"
    );
    assert!(output.contains("base: "), "{output}");
    assert!(
        output.contains("changed files: 1 (committed 0 since ")
            && output.contains("; working tree 1)"),
        "the review data separates committed from working-tree changes: {output}"
    );
    assert!(output.contains("  working: ?? partial.rs"), "{output}");
    assert!(
        output.contains("returned (reported by the executor, not verified acceptance):"),
        "{output}"
    );
    assert!(output.contains("FIXTURE_OUTCOME_DONE"), "{output}");
    assert!(output.contains("result: "), "{output}");
    assert!(output.contains("receipt: "), "{output}");
    assert!(output.contains("exit: 0"), "{output}");
    let status = wait_host(&mut host, "slow fixture host");
    assert_eq!(status.code(), Some(0));
    let receipt = receipt_json(&run.receipt);
    assert_eq!(receipt["observation"]["state"], "completed", "{receipt}");
    assert!(
        !run.spool().exists(),
        "the transient event spool is removed after the run"
    );
    // A committed executor result is not "no changes": the next review
    // compares the recorded base with HEAD and still reports untracked work.
    git(&slot, &["add", "partial.rs"]);
    git(&slot, &["commit", "-qm", "commit the executor result"]);
    fs::write(slot.join("untracked.rs"), "fn extra() {}\n").unwrap();
    let out = lead_command()
        .args([
            "executor",
            "watch",
            "--source",
            pool.source.to_str().unwrap(),
            "--codex-home",
            pool.home.to_str().unwrap(),
            "--slot",
            "1",
            "--timeout",
            "10",
        ])
        .output()
        .unwrap();
    let output = text(&out);
    assert_eq!(out.status.code(), Some(0), "{output}");
    assert!(
        output.contains("changed files: 2 (committed 1 since "),
        "{output}"
    );
    assert!(output.contains("; working tree 1)"), "{output}");
    assert!(
        output.contains("  committed: A partial.rs"),
        "a committed executor result must be reported: {output}"
    );
    assert!(output.contains("  working: ?? untracked.rs"), "{output}");
    pool.drop();
}

#[test]
fn watch_reports_failure_interruption_and_never_waits_forever() {
    // A failed run returns its state and cause without any model polling.
    let run = SeededRun::new();
    run.observed("error");
    let out = run.watch(&["--timeout", "5"]);
    let output = text(&out);
    assert_eq!(out.status.code(), Some(1), "{output}");
    assert!(output.contains("state=failed"), "{output}");
    assert!(output.contains("cause: fixture"), "{output}");

    // A recorded host whose exact process identity is gone is an interrupted
    // run: unknown exit code, no fabricated completion.
    let run = SeededRun::new();
    let mut receipt = receipt_json(&run.receipt);
    receipt["observation"]["state"] = json!("running");
    receipt["observation"]["session"] = json!("01a0c719-f4d4-7880-a9d2-1a96ee0f23f4");
    receipt["observation"]["host"] = json!({
        "pid": 4242,
        "created": 1,
        "program": run.root.join("absent-host.exe").to_string_lossy()
    });
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let out = run.watch(&["--timeout", "30"]);
    let output = text(&out);
    assert_eq!(out.status.code(), Some(1), "{output}");
    assert!(output.contains("state=interrupted"), "{output}");
    assert!(
        output.contains("no longer running and no terminal event was recorded"),
        "{output}"
    );

    // A receipt without any host identity and no update is also interrupted.
    let run = SeededRun::new();
    let mut receipt = receipt_json(&run.receipt);
    receipt["observation"]["state"] = json!("dispatch-accepted");
    receipt["observation"]["updatedMs"] = json!(1);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let out = run.watch(&["--timeout", "30"]);
    let output = text(&out);
    assert_eq!(out.status.code(), Some(1), "{output}");
    assert!(output.contains("state=interrupted"), "{output}");
    assert!(
        output.contains("no session host was ever observed"),
        "{output}"
    );

    // A live but unfinished run times out with its state instead of hanging.
    let run = SeededRun::new();
    let mut receipt = receipt_json(&run.receipt);
    receipt["observation"]["state"] = json!("running");
    receipt["observation"]["updatedMs"] = json!(u64::MAX / 2);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let out = run.watch(&["--timeout", "1", "--poll", "50"]);
    let output = text(&out);
    assert_eq!(out.status.code(), Some(2), "{output}");
    assert!(output.contains("state=running"), "{output}");
    assert!(
        output.contains("watch timed out after 1s while the run was still running"),
        "{output}"
    );

    // A missing receipt names the missing input instead of pretending.
    let missing = std::env::temp_dir().join(format!(
        "executor-watch-missing-{}.json",
        std::process::id()
    ));
    let out = lead_command()
        .args(["executor", "watch", "--receipt"])
        .arg(&missing)
        .args(["--timeout", "1", "--poll", "50"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("no dispatch receipt"), "{}", text(&out));
}

#[test]
fn legacy_and_tui_receipts_keep_their_documented_coverage_limits() {
    // A legacy receipt has no observation record: run stays pass-through.
    let run = SeededRun::new();
    let mut receipt = receipt_json(&run.receipt);
    receipt.as_object_mut().unwrap().remove("observation");
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let out = run.observed("nonzero");
    assert_eq!(
        out.status.code(),
        Some(19),
        "legacy receipts keep the child outcome: {}",
        text(&out)
    );
    let out = run.watch(&["--timeout", "1"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("legacy receipt"), "{}", text(&out));

    // A tui receipt records unavailable coverage instead of an identity.
    let run = SeededRun::new();
    let mut receipt = receipt_json(&run.receipt);
    receipt["mode"] = json!("tui");
    receipt["args"] = json!([
        "--profile",
        "ds",
        "-c",
        "agents.enabled=false",
        "some tui prompt"
    ]);
    receipt["observation"] = json!({
        "schema": 1,
        "coverage": "unavailable",
        "reason": "tui mode keeps a human conversation with no machine-readable event stream; native identity and result coverage are unavailable",
        "state": "unobserved",
        "session": Value::Null,
        "exitCode": Value::Null,
        "events": 0,
        "messages": 0,
        "toolCalls": 0,
        "malformed": 0,
        "cause": Value::Null,
        "host": Value::Null,
        "result": Value::Null,
        "detail": Value::Null,
        "updatedMs": 0
    });
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let out = run.observed("complete");
    assert_eq!(
        out.status.code(),
        Some(0),
        "a tui receipt stays a pass-through host: {}",
        text(&out)
    );
    let out = run.watch(&["--timeout", "1"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("no native coverage"), "{}", text(&out));
    assert!(
        text(&out).contains("no machine-readable event stream"),
        "{}",
        text(&out)
    );
}

#[test]
fn resume_consumes_the_recorded_identity_and_preserves_partial_work() {
    let pool = PoolFixture::new("resume");
    pool.claim("exec-1");
    let slot = pool.slot();
    fs::write(slot.join("partial.txt"), "interrupted work\n").unwrap();
    let run = pool.seeded_slot_receipt("exec-1");
    // The recorded run observes the exact identity the resume path consumes.
    let out = lead_command()
        .args(["executor", "run", "--file"])
        .arg(&run.receipt)
        .env("CODEX_HOME", pool.home.to_str().unwrap())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let session = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
    assert_eq!(
        receipt_json(&run.receipt)["observation"]["session"],
        session
    );

    // Resume without --session consumes the recorded exact identity; the
    // launcher is intentionally absent, so the run stops after the identity
    // is resolved and printed, never before it.
    let out = pool.resume(&["--owner", "exec-1", "--exec", "continue the assignment"]);
    let output = text(&out);
    assert!(!out.status.success(), "{output}");
    assert!(
        output.contains(&format!(
            "executor resume: slot=1 owner=exec-1 session={session} identity=recorded dispatch receipt"
        )),
        "{output}"
    );
    assert!(
        output.contains("installed Codex launcher is missing"),
        "{output}"
    );
    assert!(
        fs::read_to_string(slot.join("partial.txt"))
            .unwrap()
            .contains("interrupted work"),
        "partial work survives a resume attempt"
    );
    assert_eq!(pool.slot_record()["owner"], "exec-1");
    assert_eq!(pool.slot_record()["state"], "occupied");

    // A wrong owner is refused instead of sharing the checkout.
    let out = pool.resume(&["--owner", "exec-other", "--exec", "continue"]);
    let output = text(&out);
    assert!(!out.status.success(), "{output}");
    assert!(
        output.contains("belongs to owner exec-1 instead of exec-other"),
        "{output}"
    );
    assert!(slot.join("partial.txt").is_file());

    // An explicit --session stays supported and takes precedence.
    let explicit = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f9";
    let out = pool.resume(&[
        "--owner",
        "exec-1",
        "--session",
        explicit,
        "--exec",
        "continue",
    ]);
    let output = text(&out);
    assert!(
        output.contains(&format!("session={explicit} identity=explicit --session")),
        "{output}"
    );

    // A missing receipt refuses with the explicit remedies.
    fs::remove_file(&run.receipt).unwrap();
    let out = pool.resume(&["--owner", "exec-1", "--exec", "continue"]);
    let output = text(&out);
    assert!(!out.status.success(), "{output}");
    assert!(output.contains("pass --session"), "{output}");
    pool.drop();
}

#[test]
fn release_requires_disposition_refuses_a_live_owner_and_reports_the_run() {
    let pool = PoolFixture::new("release");
    pool.claim("exec-1");
    let run = pool.seeded_slot_receipt("exec-1");
    let out = lead_command()
        .args(["executor", "run", "--file"])
        .arg(&run.receipt)
        .env("CODEX_HOME", pool.home.to_str().unwrap())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));

    // The disposition is explicit; without it nothing is released.
    let out = pool.release(&["--slot", "1", "--reason", "no disposition"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("--disposition merged|discarded is required"),
        "{}",
        text(&out)
    );

    // A live session owns the slot: release refuses and preserves the tree.
    pool.record_live_lease("exec-1");
    let dirty = pool.slot().join("unreviewed.txt");
    fs::write(&dirty, "unreviewed\n").unwrap();
    let out = pool.release(&[
        "--slot",
        "1",
        "--disposition",
        "merged",
        "--reason",
        "premature",
    ]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("still owned by live session exec-1"),
        "{}",
        text(&out)
    );
    assert!(
        dirty.is_file(),
        "a live slot is never reset beneath the lead"
    );
    pool.remove_lease();

    // With the host gone the explicit disposition releases the slot and the
    // observed run is part of the release evidence.
    let out = pool.release(&[
        "--slot",
        "1",
        "--disposition",
        "merged",
        "--reason",
        "accepted assignment",
    ]);
    let output = text(&out);
    assert!(out.status.success(), "{output}");
    assert!(output.contains("released as merged"), "{output}");
    assert!(
        output.contains("last run: state=completed session=01a0c719-f4d4-7880-a9d2-1a96ee0f23f4"),
        "{output}"
    );
    assert_eq!(pool.slot_record()["state"], "released");
    assert_eq!(pool.slot_record()["disposition"], "merged");
    pool.drop();
}

/// Owned pool fixture: a synthetic `file://` upstream plus a Codex home whose
/// launcher is intentionally absent, so dispatch binds a slot and stops before
/// any process starts.
struct PoolFixture {
    root: PathBuf,
    source: PathBuf,
    home: PathBuf,
}

impl PoolFixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "executor-observation-{name}-{}",
            std::process::id()
        ));
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
        git(&seed, &["config", "user.email", "observation@example.test"]);
        git(&seed, &["config", "user.name", "Observation"]);
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
        git(
            &source,
            &["config", "user.email", "observation@example.test"],
        );
        git(&source, &["config", "user.name", "Observation"]);
        fs::create_dir_all(source.join("global")).unwrap();
        fs::write(
            source.join("global/orchestration.toml"),
            "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\nworktree_limit = 1\n",
        )
        .unwrap();
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

    fn slot_record(&self) -> Value {
        serde_json::from_slice(&fs::read(self.state_dir().join("slot-1.json")).unwrap()).unwrap()
    }

    /// A dispatch whose launcher is absent binds the slot and stops before any
    /// process starts.
    fn claim(&self, owner: &str) {
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

    fn resume(&self, extra: &[&str]) -> Output {
        lead_command()
            .args([
                "executor",
                "resume",
                "--source",
                self.source.to_str().unwrap(),
                "--codex-home",
                self.home.to_str().unwrap(),
                "--slot",
                "1",
            ])
            .args(extra)
            .output()
            .unwrap()
    }

    fn release(&self, extra: &[&str]) -> Output {
        lead_command()
            .args([
                "executor",
                "release",
                "--source",
                self.source.to_str().unwrap(),
                "--codex-home",
                self.home.to_str().unwrap(),
            ])
            .args(extra)
            .output()
            .unwrap()
    }

    /// The receipt the dispatcher would have written for slot 1, bound to the
    /// recorded slot mapping and observing the owned fixture.
    fn seeded_slot_receipt(&self, owner: &str) -> SeededRun {
        let run = SeededRun::at(self.state_dir(), None);
        let binding = json!({
            "index": 1,
            "path": self.slot().to_string_lossy(),
            "source": self.source.to_string_lossy(),
            "owner": owner,
            "base": self.slot_record()["base"],
            "remote": "origin",
            "branch": "main"
        });
        let mut receipt = receipt_json(&run.receipt);
        receipt["slot"] = binding;
        fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        run
    }

    /// A lease owned by this test process, exercising the existing liveness
    /// check with a real process identity.
    fn record_live_lease(&self, owner: &str) {
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
        fs::write(
            self.state_dir().join("lease-1.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "owner": owner,
                "index": 1,
                "path": self.slot().to_string_lossy(),
                "pid": identity.pid,
                "created": identity.creation_time,
                "program": program.to_string_lossy()
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn remove_lease(&self) {
        let _ = fs::remove_file(self.state_dir().join("lease-1.json"));
    }

    fn drop(self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn file_url(path: &Path) -> String {
    format!("file:///{}", path.to_str().unwrap().replace('\\', "/"))
}

fn identity_of(value: &Value) -> harness_core::process::ProcessIdentity {
    harness_core::process::ProcessIdentity {
        pid: value["pid"].as_u64().unwrap() as u32,
        creation_time: value["creation_time"].as_u64().unwrap(),
    }
}

/// Waits for an owned fixture identity marker (pid plus creation time).
fn wait_for_marker(path: &Path) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
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
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Asserts that the exact recorded process is gone within a bounded wait.
fn wait_gone(identity: harness_core::process::ProcessIdentity, label: &str) {
    let program = fixture();
    let user = harness_core::process_service::current_user().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let alive = matches!(
            harness_core::process_service::ServiceProcess::inspect(identity, &program, &user),
            Ok(Some(_)) | Err(_)
        );
        if !alive {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{label} {identity:?} is still running after the owned tree was reaped"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
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
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn observed_host_fails_before_the_model_without_its_record() {
    let run = SeededRun::new();
    let started = run.root.join("fixture-started.json");
    // An unwritable event spool is a setup failure: no launcher may start.
    fs::create_dir_all(run.spool()).unwrap();
    let out = run.observed_with(
        "complete",
        &[("HARNESS_EXECUTOR_FIXTURE_STARTED", &started)],
    );
    let output = text(&out);
    assert!(!out.status.success(), "{output}");
    assert!(output.contains("event spool"), "{output}");
    assert!(output.contains("before the model starts"), "{output}");
    assert!(
        !started.exists(),
        "no launcher may start when the observation record cannot be prepared"
    );
    let receipt = receipt_json(&run.receipt);
    assert_eq!(
        receipt["observation"]["state"], "dispatch-accepted",
        "the recorded state stays where it truthfully is: {receipt}"
    );
    let lock = run.receipt.with_extension("lock");
    if lock.exists() {
        assert!(
            harness_core::process::ExclusiveFileLock::try_acquire(&lock)
                .unwrap()
                .is_some(),
            "no writer lock may stay held after the setup failure"
        );
    }
    assert!(!run.receipt.with_extension("running.jsonl").is_file());
    let _ = fs::remove_dir_all(run.spool());
}

#[test]
fn observer_failure_terminates_the_owned_tree() {
    let run = SeededRun::new();
    let started = run.root.join("fixture-started.json");
    let child_marker = run.root.join("fixture-child.json");
    let host_stdout = fs::File::create(run.root.join("host-stdout.txt")).unwrap();
    let host_stderr = fs::File::create(run.root.join("host-stderr.txt")).unwrap();
    let mut host = lead_command()
        .args(["executor", "run", "--file"])
        .arg(&run.receipt)
        .env("HARNESS_EXECUTOR_FIXTURE_MODE", "descendant")
        .env("HARNESS_EXECUTOR_FIXTURE_STARTED", &started)
        .env("HARNESS_EXECUTOR_FIXTURE_CHILD_MARKER", &child_marker)
        .stdin(Stdio::null())
        .stdout(Stdio::from(host_stdout))
        .stderr(Stdio::from(host_stderr))
        .spawn()
        .unwrap();
    let launcher = identity_of(&wait_for_marker(&started));
    let descendant = identity_of(&wait_for_marker(&child_marker));
    // The retained detail file becomes unwritable while the run is live: the
    // observer must stop the owned tree instead of letting it run unseen.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let _ = fs::remove_file(&run.detail);
        if fs::create_dir(&run.detail).is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "could not replace the detail file {} with a directory",
            run.detail.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let status = wait_host(&mut host, "observation host");
    let stdout = fs::read_to_string(run.root.join("host-stdout.txt")).unwrap();
    let stderr = fs::read_to_string(run.root.join("host-stderr.txt")).unwrap();
    assert!(!status.success(), "{stdout}\n{stderr}");
    assert!(
        stderr.contains("executor observation failed") && stderr.contains("terminated"),
        "{stderr}"
    );
    assert!(
        stdout.contains("result: failed") && stdout.contains("observation failed"),
        "{stdout}"
    );
    wait_gone(launcher, "launcher");
    wait_gone(descendant, "descendant");
    let receipt = receipt_json(&run.receipt);
    assert_eq!(receipt["observation"]["state"], "failed", "{receipt}");
    assert!(
        receipt["observation"]["cause"]
            .as_str()
            .unwrap()
            .contains("executor observation failed"),
        "{receipt}"
    );
    let out = run.watch(&["--timeout", "10"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("state=failed"), "{}", text(&out));
}

#[test]
fn cache_loss_stops_the_observed_launcher_and_descendant_preserving_work() {
    let run = SeededRun::new();
    let started = run.root.join("cache-started.json");
    let descendant_marker = run.root.join("cache-descendant.json");
    let preserved = run.root.join("workspace/partial.txt");
    fs::write(&preserved, "partial work stays").unwrap();
    let mut host = run.host(
        "descendant",
        &[
            ("CODEX_HOME", &run.root),
            ("HARNESS_EXECUTOR_FIXTURE_STARTED", &started),
            ("HARNESS_EXECUTOR_FIXTURE_CHILD_MARKER", &descendant_marker),
        ],
    );
    let launcher = identity_of(&wait_for_marker(&started));
    let descendant = identity_of(&wait_for_marker(&descendant_marker));
    let session = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
    let rollout = cache_usage::write_loss(&run.root, session);
    let status = wait_host(&mut host, "cache-loss host");
    assert!(!status.success());
    wait_gone(launcher, "cache-loss launcher");
    wait_gone(descendant, "cache-loss descendant");
    assert_eq!(fs::read_to_string(preserved).unwrap(), "partial work stays");
    assert!(rollout.exists());
    let receipt = receipt_json(&run.receipt);
    assert_eq!(receipt["observation"]["state"], "stopped", "{receipt}");
    assert_eq!(receipt["observation"]["session"], session);
    assert_eq!(receipt["cacheGuard"]["consecutiveMisses"], 3);
    let watch = run.watch(&["--timeout", "1"]);
    assert_eq!(watch.status.code(), Some(1), "{}", text(&watch));
    assert!(
        text(&watch).contains("DeepSeek cache loss"),
        "{}",
        text(&watch)
    );
}

#[test]
fn killing_the_host_reaps_the_owned_launcher_tree() {
    let run = SeededRun::new();
    let started = run.root.join("fixture-started.json");
    let child_marker = run.root.join("fixture-child.json");
    let mut host = run.host(
        "descendant",
        &[
            ("HARNESS_EXECUTOR_FIXTURE_STARTED", &started),
            ("HARNESS_EXECUTOR_FIXTURE_CHILD_MARKER", &child_marker),
        ],
    );
    let launcher = identity_of(&wait_for_marker(&started));
    let descendant = identity_of(&wait_for_marker(&child_marker));
    host.kill().unwrap();
    let _ = host.wait();
    // Closing the killed host's job is the cleanup authority: both the owned
    // launcher and its descendant must stop without any PID-name cleanup.
    wait_gone(launcher, "launcher");
    wait_gone(descendant, "descendant");
    // The record then names the interruption instead of a hidden conversation
    // that still runs without its visible surface.
    let out = run.watch(&["--timeout", "30"]);
    let output = text(&out);
    assert_eq!(out.status.code(), Some(1), "{output}");
    assert!(output.contains("state=interrupted"), "{output}");
    assert!(
        output.contains("no longer running and no terminal event was recorded"),
        "{output}"
    );
}

#[test]
fn failed_launcher_shows_a_bounded_stderr_tail() {
    let run = SeededRun::new();
    let out = run.observed("stderr-noise");
    let output = text(&out);
    assert_eq!(out.status.code(), Some(19), "{output}");
    assert!(
        output.contains("launcher stderr (bounded tail):"),
        "{output}"
    );
    assert!(
        output.contains("FIXTURE_STDERR_SENTINEL"),
        "the launcher's own error stays human-visible: {output}"
    );
    assert!(
        output.contains("stderr log: ") && output.contains("stderr.log"),
        "{output}"
    );
    let receipt = receipt_json(&run.receipt);
    assert_eq!(receipt["observation"]["exitCode"], 19, "{receipt}");
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

/// Opt-in: runs the actual installed native CLI against an owned synthetic
/// Responses provider and asserts the observed event contract through this
/// host. No model, network provider or subscription is used.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE (an absolute native Codex CLI); runs no model request"]
fn installed_native_exec_json_event_shape_stays_observed() {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    assert!(exe.is_absolute() && exe.is_file());
    let root = tempfile::tempdir().unwrap();
    let evidence = root.path().join("evidence");
    let home = root.path().join("home");
    let workspace = root.path().join("workspace");
    for dir in [&evidence, &home, &workspace] {
        fs::create_dir_all(dir).unwrap();
    }
    let responses = fixture_responses::Responses::start(evidence);
    let trusted = workspace.to_string_lossy().to_lowercase();
    fs::write(
        home.join("config.toml"),
        format!(
            "model = \"gpt-6-astra\"\nmodel_reasoning_effort = \"low\"\nmodel_provider = \"control_fixture\"\napproval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\n[model_providers.control_fixture]\nname = \"Owned observation fixture\"\nbase_url = \"http://127.0.0.1:{}/v1\"\nwire_api = \"responses\"\nenv_key = \"HARNESS_CONTROL_FIXTURE_KEY\"\nrequires_openai_auth = false\nrequest_max_retries = 0\nstream_max_retries = 0\nsupports_websockets = false\n[analytics]\nenabled = false\n[projects.'{trusted}']\ntrust_level = \"trusted\"\n",
            responses.port
        ),
    )
    .unwrap();
    let run = SeededRun::new();
    let mut receipt = receipt_json(&run.receipt);
    receipt["launcher"] = json!(exe.to_string_lossy());
    receipt["model"] = json!("gpt-6-astra");
    receipt["modelProvider"] = json!("control_fixture");
    receipt["reasoningEffort"] = json!("low");
    let mut args: Vec<Value> = [
        "exec",
        "--json",
        "--skip-git-repo-check",
        "-C",
        workspace.to_str().unwrap(),
        "-o",
    ]
    .iter()
    .map(|arg| json!(arg))
    .collect();
    args.push(json!(run.result.to_string_lossy()));
    args.push(json!(
        "Reply with the owned fixture acknowledgement and stop."
    ));
    receipt["args"] = Value::Array(args);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let out = lead_command()
        .args(["executor", "run", "--file"])
        .arg(&run.receipt)
        .env("CODEX_HOME", home.to_str().unwrap())
        .env("HARNESS_CONTROL_FIXTURE_KEY", "synthetic-owned-fixture")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let output = text(&out);
    assert_eq!(out.status.code(), Some(0), "{output}");
    assert!(output.contains("native start (session "), "{output}");
    assert!(output.contains("assistant:"), "{output}");
    assert!(output.contains(fixture_responses::SEED_FINAL), "{output}");
    let recorded = receipt_json(&run.receipt);
    assert_eq!(recorded["observation"]["state"], "completed", "{recorded}");
    assert!(
        recorded["observation"]["session"]
            .as_str()
            .is_some_and(|session| session.len() >= 8),
        "{recorded}"
    );
    assert_eq!(
        fs::read_to_string(&run.result).unwrap().trim(),
        fixture_responses::SEED_FINAL
    );
}

/// The existing owned Responses provider: canned events only, never a model
/// or subscription.
#[path = "fixtures/succession_responses.rs"]
mod fixture_responses;

#[test]
#[ignore = "requires native Codex and owner PowerShell 7; all Responses are local canned events"]
fn installed_native_cache_loss_stops_before_more_requests() {
    let exe =
        PathBuf::from(std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("native Codex path"));
    assert!(exe.is_absolute() && exe.is_file());
    let run = SeededRun::new();
    let home = run.root.join("home");
    let evidence = run.root.join("evidence");
    let workspace = run.root.join("workspace");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&evidence).unwrap();
    fs::write(evidence.join("cache-loss"), "synthetic counters").unwrap();
    let responses = fixture_responses::Responses::start(evidence.clone());
    fs::write(home.join("config.toml"), format!(
        "model = 'gpt-6-astra'\nmodel_reasoning_effort = 'low'\nmodel_provider = 'deepseek'\nmodel_context_window = 1000000\nmodel_auto_compact_token_limit = 990000\napproval_policy = 'never'\nsandbox_mode = 'danger-full-access'\n[model_providers.deepseek]\nname = 'Owned cache fixture'\nbase_url = 'http://127.0.0.1:{}/v1'\nwire_api = 'responses'\nenv_key = 'HARNESS_CONTROL_FIXTURE_KEY'\nrequires_openai_auth = false\nrequest_max_retries = 0\nstream_max_retries = 0\nsupports_websockets = false\n[analytics]\nenabled = false\n[projects.'{}']\ntrust_level = 'trusted'\n", responses.port, workspace.to_string_lossy())).unwrap();
    let mut receipt = receipt_json(&run.receipt);
    receipt["launcher"] = json!(exe);
    receipt["model"] = json!("gpt-6-astra");
    receipt["modelProvider"] = json!("deepseek");
    receipt["reasoningEffort"] = json!("low");
    receipt["args"] = json!([
        "exec",
        "--json",
        "--skip-git-repo-check",
        "-C",
        workspace,
        "-o",
        run.result,
        "Run the owned fixture commands."
    ]);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let stdout = fs::File::create(run.root.join("native-cache-stdout.txt")).unwrap();
    let stderr = fs::File::create(run.root.join("native-cache-stderr.txt")).unwrap();
    let mut host = lead_command()
        .args(["executor", "run", "--file"])
        .arg(&run.receipt)
        .env("CODEX_HOME", &home)
        .env("HARNESS_CONTROL_FIXTURE_KEY", "synthetic-owned-fixture")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .unwrap();
    let status = wait_host(&mut host, "native cache-loss host");
    let output = fs::read_to_string(run.root.join("native-cache-stdout.txt")).unwrap();
    let error = fs::read_to_string(run.root.join("native-cache-stderr.txt")).unwrap();
    let receipt = receipt_json(&run.receipt);
    assert_eq!(status.code(), Some(1), "{output}\n{error}");
    assert_eq!(
        receipt["observation"]["state"], "stopped",
        "{receipt}\n{output}\n{error}"
    );
    assert_eq!(receipt["cacheGuard"]["consecutiveMisses"], 3);
    assert_eq!(receipt["cacheGuard"]["lastMissTokens"], 196_000);
    let requests = fs::read_dir(evidence)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("provider-"))
        .count();
    assert!(
        (4..=5).contains(&requests),
        "request count after real-time stop: {requests}"
    );
    println!(
        "native cache stop: {requests} local requests; per-response counters persisted and the guard stopped the live CLI"
    );
}
