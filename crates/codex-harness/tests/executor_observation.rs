//! Observed executor lifecycle: the owned native event fixture drives the
//! tab host, the recorded receipt, `executor watch`, `executor resume` and
//! `executor release`. The pool slots are ordinary Git worktrees of a
//! synthetic `file://` upstream; no check here touches a model, a network
//! provider or a subscription.
#![cfg(windows)]

use harness_core::console::{ConsoleSession, ConsoleSpec};
use harness_core::process::{Cancellation, CommandSpec, Deadline};
use serde_json::{Value, json};
#[path = "fixtures/cache_usage.rs"]
mod cache_usage;
use std::{
    fs, io,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
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

    // A historical unmanaged tui receipt records unavailable coverage instead
    // of an identity. Current explicit tui dispatch is observed; this receipt
    // is the old shape and must stay honestly limited.
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
        output.contains("presentation=native-tui"),
        "resume must select the managed native presentation: {output}"
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

#[path = "fixtures/control_endpoint.rs"]
mod control_endpoint;

const CONTROL_THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
const CONTROL_TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f2401";
const CONTROL_MODEL: &str = "deepseek-v4-flash";
const CONTROL_PROVIDER: &str = "deepseek-fixture";
const CONTROL_EFFORT: &str = "max";
const CONTROL_ASSIGNMENT: &str = "fixture control assignment text";
const CONTROL_FINAL: &str = "CONTROL_FIXTURE_FINAL_MESSAGE";
const CONTROL_OWNER: &str = "exec-deepseek-host";

struct ControlHost {
    home: PathBuf,
    source: PathBuf,
    slot: PathBuf,
    state: PathBuf,
    receipt: PathBuf,
    server: Option<control_endpoint::Server>,
}

impl ControlHost {
    fn new(name: &str) -> Self {
        let root = tempfile::tempdir().unwrap().keep();
        let source = root.join(format!("source-{name}"));
        let home = root.join("home");
        let slot = root.join("slot");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&slot).unwrap();
        let source = source.canonicalize().unwrap();
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
                "owner": CONTROL_OWNER,
                "base": "abc123",
                "disposition": null,
                "reason": null,
            }))
            .unwrap(),
        )
        .unwrap();
        let server = control_endpoint::Server::start(control_endpoint::Bearer::File(
            state.join("endpoint-1.token"),
        ));
        server.answer("initialize", control_endpoint::Answer::Result(json!({})));
        server.answer(
            "thread/start",
            control_endpoint::Answer::Result(json!({
                "thread": {"id": CONTROL_THREAD, "cwd": slot},
                "model": CONTROL_MODEL,
                "modelProvider": CONTROL_PROVIDER,
                "reasoningEffort": CONTROL_EFFORT
            })),
        );
        server.answer(
            "thread/name/set",
            control_endpoint::Answer::Result(json!({})),
        );
        server.answer(
            "thread/resume",
            control_endpoint::Answer::Result(json!({
                "thread": {"id": CONTROL_THREAD, "cwd": slot},
                "model": CONTROL_MODEL,
                "modelProvider": CONTROL_PROVIDER,
                "reasoningEffort": CONTROL_EFFORT
            })),
        );
        server.answer(
            "turn/start",
            control_endpoint::Answer::Result(
                json!({"turn": {"id": CONTROL_TURN, "status": "inProgress"}}),
            ),
        );
        server.answer(
            "turn/interrupt",
            control_endpoint::Answer::Result(json!({})),
        );
        server.answer_sequence(
            "thread/read",
            vec![
                control_endpoint::Answer::Result(json!({"thread": {
                    "id": CONTROL_THREAD,
                    "cwd": slot,
                    "turns": []
                }})),
                control_endpoint::Answer::Result(json!({"thread": {
                    "id": CONTROL_THREAD,
                    "cwd": slot,
                    "turns": [{
                        "id": CONTROL_TURN,
                        "status": "completed",
                        "items": [{"id": "m1", "type": "agentMessage", "text": CONTROL_FINAL}]
                    }]
                }})),
            ],
        );
        let receipt = state.join("spawn-1.json");
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "launcher": fixture(),
                "profile": "deepseek",
                "mode": "exec",
                "args": [],
                "visible": true,
                "host": "owned-console",
                "control": {
                    "schema": 1,
                    "assignment": CONTROL_ASSIGNMENT,
                    "identity": {
                        "profile": "deepseek",
                        "model": CONTROL_MODEL,
                        "modelProvider": CONTROL_PROVIDER,
                        "reasoningEffort": CONTROL_EFFORT
                    },
                    "presentation": "native-inline",
                    "port": server.port
                },
                "terminal": null,
                "isolation": false,
                "slot": {
                    "index": 1,
                    "path": slot,
                    "source": source,
                    "owner": CONTROL_OWNER,
                    "base": "abc123",
                    "remote": "origin",
                    "branch": "main"
                },
                "model": CONTROL_MODEL,
                "modelProvider": CONTROL_PROVIDER,
                "reasoningEffort": CONTROL_EFFORT,
                "window": null,
                "shell": {
                    "path": std::env::var_os("PATH").unwrap(),
                    "executable": fixture(),
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
        Self {
            home,
            source,
            slot,
            state,
            receipt,
            server: Some(server),
        }
    }

    fn server(&self) -> &control_endpoint::Server {
        self.server
            .as_ref()
            .expect("control endpoint is still serving")
    }

    fn disconnect(&mut self) {
        self.server.take();
    }

    fn title(&self) -> String {
        format!("CEx (deepseek) - {CONTROL_OWNER}")
    }

    fn register_frontend(&self, program: &Path) {
        let launch = self.home.join("harness");
        fs::create_dir_all(&launch).unwrap();
        fs::write(
            launch.join("native-launch.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": 2,
                "upstream": {"executable": program}
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(self.home.join("frontend-title.txt"), self.title()).unwrap();
    }

    fn use_presentation(&self, mode: &str, presentation: &str) {
        let mut receipt = receipt_json(&self.receipt);
        receipt["mode"] = json!(mode);
        receipt["control"]["presentation"] = json!(presentation);
        fs::write(&self.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    }
}

fn host_spec(host: &ControlHost) -> CommandSpec {
    let mut spec = CommandSpec::new(manager());
    spec.args = vec![
        "executor".into(),
        "run".into(),
        "--file".into(),
        host.receipt.as_os_str().to_owned(),
    ];
    spec.env
        .insert("CODEX_HOME".into(), Some(host.home.as_os_str().to_owned()));
    spec.env
        .insert("HARNESS_EXECUTOR_FIXTURE_MODE".into(), None);
    spec.env.insert("HARNESS_EXECUTOR_SESSION".into(), None);
    spec.env.insert("WT_SESSION".into(), None);
    spec
}

fn completion_burst(server: &control_endpoint::Server) {
    server.push(
        json!({"method":"item/completed","params":{"threadId":CONTROL_THREAD,"item":{
            "id":"c1","type":"commandExecution","command":"fixture check","exitCode":0
        }}}),
    );
    server.push(
        json!({"method":"item/completed","params":{"threadId":CONTROL_THREAD,"item":{
            "id":"m1","type":"agentMessage","text":CONTROL_FINAL
        }}}),
    );
    server.push(json!({"method":"turn/completed","params":{"threadId":CONTROL_THREAD,"turn":{"id":CONTROL_TURN,"status":"completed"}}}));
}

fn frontend_phases(host: &ControlHost) -> Vec<Value> {
    let path = host.state.join("frontend-1.json");
    fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn attachment_failure_is_reported_before_the_assignment() {
    for (name, mode, presentation) in [
        ("attach-fail-exec", "exec", "native-inline"),
        ("attach-fail-tui", "tui", "native-tui"),
    ] {
        let host = ControlHost::new(name);
        host.use_presentation(mode, presentation);
        let out = lead_command()
            .args(["executor", "run", "--file"])
            .arg(&host.receipt)
            .env("CODEX_HOME", &host.home)
            .env_remove("HARNESS_EXECUTOR_FIXTURE_MODE")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let output = text(&out);
        assert_ne!(out.status.code(), Some(0), "{mode}: {output}");
        assert!(
            output.contains("no live terminal surface")
                && output.contains("assignment was not submitted"),
            "{mode}: {output}"
        );
        assert!(
            host.server().requests_for("turn/start").is_empty(),
            "{mode}: a failed attachment must not submit the assignment: {:?}",
            host.server().requests_for("turn/start")
        );
        assert_eq!(
            host.server().requests_for("thread/start").len(),
            1,
            "{mode}"
        );
        let watched = lead_command()
            .args(["executor", "watch", "--receipt"])
            .arg(&host.receipt)
            .output()
            .unwrap();
        assert_eq!(watched.status.code(), Some(1), "{mode}: {}", text(&watched));
        assert!(
            !text(&watched).contains("no native coverage"),
            "{mode}: attachment failure must stay observed: {}",
            text(&watched)
        );
    }
}

#[path = "fixtures/control_responses.rs"]
mod native_responses;

/// The actual spawn host, native TUI and watch path. The canned Responses
/// provider supplies the tool and final answer; no subscription is used.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned native TUI and canned responses"]
fn installed_native_frontend_shows_the_assignment_and_watch_keeps_the_result() {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    assert!(exe.is_absolute() && exe.is_file(), "{}", exe.display());
    let host = ControlHost::new("native-tui");
    let evidence = host.home.join("evidence");
    fs::create_dir_all(&evidence).unwrap();
    let responses = native_responses::Responses::start(evidence.clone(), true);
    let trusted = host.slot.to_string_lossy().to_lowercase();
    fs::write(
        host.home.join("config.toml"),
        format!(
            "model = \"gpt-6-astra\"\nmodel_reasoning_effort = \"low\"\nmodel_provider = \"control_fixture\"\napproval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\n[model_providers.control_fixture]\nname = \"Owned observation fixture\"\nbase_url = \"http://127.0.0.1:{}/v1\"\nwire_api = \"responses\"\nenv_key = \"HARNESS_CONTROL_FIXTURE_KEY\"\nrequires_openai_auth = false\nrequest_max_retries = 0\nstream_max_retries = 0\nsupports_websockets = false\n[analytics]\nenabled = false\n[projects.'{trusted}']\ntrust_level = \"trusted\"\n",
            responses.port
        ),
    )
    .unwrap();
    host.register_frontend(&exe);
    let mut receipt = receipt_json(&host.receipt);
    receipt["launcher"] = json!(exe);
    receipt["profile"] = json!("default");
    receipt["model"] = json!("gpt-6-astra");
    receipt["modelProvider"] = json!("control_fixture");
    receipt["reasoningEffort"] = json!("low");
    receipt["control"]["identity"] = json!({
        "profile": "default",
        "model": "gpt-6-astra",
        "modelProvider": "control_fixture",
        "reasoningEffort": "low"
    });
    receipt["control"]["assignment"] =
        json!("Perform the owned proof command and return its consumed result.");
    receipt["control"]["port"] = Value::Null;
    receipt["shell"]["sandbox_mode"] = json!("danger-full-access");
    fs::write(&host.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    fs::write(
        host.home.join("frontend-title.txt"),
        format!("CEx (default) - {CONTROL_OWNER}"),
    )
    .unwrap();
    let mut spec = host_spec(&host);
    spec.env.insert(
        "HARNESS_CONTROL_FIXTURE_KEY".into(),
        Some("synthetic-owned-fixture".into()),
    );
    let session = ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap();
    let until = Instant::now() + Duration::from_secs(90);
    let result = host.state.join("message-1.txt");
    while !result.is_file() && Instant::now() < until {
        let recorded = receipt_json(&host.receipt);
        if recorded["observation"]["state"] == "failed" {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let finished = session
        .wait(
            Deadline::after(Duration::from_secs(30)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    let transcript = finished.transcript;
    assert_eq!(
        finished.outcome.exit_code,
        0,
        "{transcript}\n{}",
        fs::read_to_string(host.state.join("endpoint-1.log")).unwrap_or_default()
    );
    assert!(
        transcript.contains(native_responses::FINAL),
        "native TUI did not show the final answer: {transcript}"
    );
    assert!(
        transcript.contains("exec") || transcript.contains("proof"),
        "native TUI did not show tool activity: {transcript}"
    );
    assert!(
        !transcript.contains("conversation=control"),
        "controller output corrupted the TUI: {transcript}"
    );
    assert_eq!(
        fs::read_to_string(host.slot.join("proof.txt")).unwrap(),
        "one"
    );
    let watched = lead_command()
        .args(["executor", "watch", "--receipt"])
        .arg(&host.receipt)
        .output()
        .unwrap();
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(0), "{watched_text}");
    assert!(
        watched_text.contains(native_responses::FINAL),
        "{watched_text}"
    );
    let _ = responses;
}

#[test]
fn managed_host_attaches_one_frontend_then_watch_returns_the_persisted_result() {
    assert_managed_presentation("attach-watch", "exec", "native-inline", true);
}

#[test]
fn explicit_tui_spelling_is_observed_on_the_native_frontend() {
    assert_managed_presentation("attach-tui", "tui", "native-tui", false);
}

fn assert_managed_presentation(name: &str, mode: &str, presentation: &str, inline: bool) {
    let host = ControlHost::new(name);
    host.use_presentation(mode, presentation);
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    host.register_frontend(&double);
    let mut neighbor = Command::new("pwsh")
        .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 90"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let session = ConsoleSession::spawn(ConsoleSpec::new(host_spec(&host))).unwrap();
    let until = Instant::now() + Duration::from_secs(25);
    while host.server().requests_for("turn/start").is_empty() && Instant::now() < until {
        thread::sleep(Duration::from_millis(40));
    }
    let turns = host.server().requests_for("turn/start");
    assert_eq!(turns.len(), 1, "{mode}: one assignment: {turns:?}");
    assert_eq!(turns[0]["params"]["threadId"], CONTROL_THREAD);
    assert_eq!(
        turns[0]["params"]["input"],
        json!([{"type": "text", "text": CONTROL_ASSIGNMENT}])
    );
    let started = host.server().requests_for("thread/start");
    assert_eq!(started.len(), 1, "{mode}: {started:?}");
    assert_eq!(started[0]["params"]["model"], CONTROL_MODEL);
    assert_eq!(started[0]["params"]["modelProvider"], CONTROL_PROVIDER);
    assert_eq!(
        started[0]["params"]["config"]["model_reasoning_effort"],
        CONTROL_EFFORT
    );
    assert!(
        started[0]["params"]["cwd"]
            .as_str()
            .unwrap_or_default()
            .eq_ignore_ascii_case(&host.slot.to_string_lossy()),
        "{mode}: {started:?}"
    );
    let argv = fs::read_to_string(host.home.join("frontend-argv.txt")).unwrap();
    assert!(argv.contains("resume"), "{mode}: {argv}");
    assert!(argv.contains(CONTROL_THREAD), "{mode}: {argv}");
    assert!(argv.contains("agents.enabled=false"), "{mode}: {argv}");
    assert_eq!(
        argv.contains("--no-alt-screen"),
        inline,
        "{mode}: inline={inline} argv={argv}"
    );
    assert!(!argv.contains(CONTROL_ASSIGNMENT), "{mode}: {argv}");
    assert!(!argv.contains("--sandbox"), "{mode}: {argv}");
    assert!(!argv.contains("--worktree"), "{mode}: {argv}");
    assert!(!argv.contains("exec"), "{mode}: {argv}");
    assert!(!argv.to_lowercase().contains("token="), "{mode}: {argv}");
    let token = fs::read_to_string(host.state.join("endpoint-1.token")).unwrap();
    assert!(
        !argv.contains(token.trim()),
        "{mode}: the capability token leaked into argv"
    );
    let backend = fs::read_to_string(host.state.join("endpoint-1.command.txt")).unwrap_or_default();
    assert!(
        backend.contains("app-server command:") && backend.contains("agents.enabled=false"),
        "{mode}: backend single-agent restriction missing: {backend}"
    );
    assert!(
        !backend.contains(token.trim()),
        "{mode}: the capability token leaked into the control log"
    );
    completion_burst(host.server());
    let finished = session
        .wait(
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(
        finished.outcome.exit_code, 0,
        "{mode}: {}",
        finished.transcript
    );
    assert!(
        !finished.transcript.contains("conversation=control"),
        "{mode}: controller output corrupted the frontend surface: {}",
        finished.transcript
    );
    assert_eq!(
        fs::read_to_string(host.state.join("message-1.txt"))
            .unwrap()
            .trim(),
        CONTROL_FINAL
    );
    let phases = frontend_phases(&host);
    assert!(
        phases
            .iter()
            .any(|phase| phase["phase"] == "attached" && phase["alive"] == true),
        "{mode}: {phases:?}"
    );
    assert!(
        phases
            .iter()
            .any(|phase| phase["phase"] == "persisted" && phase["alive"] == true),
        "{mode}: {phases:?}"
    );
    let pid = phases[0]["pid"].as_u64().unwrap() as u32;
    let created = phases[0]["creationTime"].as_u64().unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    let gone = harness_core::process_service::ServiceProcess::inspect(
        harness_core::process::ProcessIdentity {
            pid,
            creation_time: created,
        },
        &double,
        &user,
    )
    .unwrap();
    assert!(
        gone.is_none(),
        "{mode}: the owned frontend was still running"
    );
    assert!(
        neighbor.try_wait().unwrap().is_none(),
        "{mode}: closing the owned frontend stopped a neighboring process"
    );
    let _ = neighbor.kill();
    let watched = lead_command()
        .args(["executor", "watch", "--receipt"])
        .arg(&host.receipt)
        .output()
        .unwrap();
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(0), "{mode}: {watched_text}");
    assert!(
        watched_text.contains("state=completed"),
        "{mode}: {watched_text}"
    );
    assert!(
        watched_text.contains(CONTROL_FINAL),
        "{mode}: {watched_text}"
    );
    assert!(
        watched_text.contains(CONTROL_THREAD),
        "{mode}: {watched_text}"
    );
    assert!(
        !watched_text.contains("no native coverage"),
        "{mode}: native presentation must stay observable: {watched_text}"
    );
    let receipt = receipt_json(&host.receipt);
    assert!(
        receipt.get("cleanup").is_none() || receipt["cleanup"].is_null(),
        "{mode}: successful close recorded a cleanup failure: {receipt}"
    );
    assert_eq!(receipt["observation"]["exitCode"], 0, "{mode}: {receipt}");
    assert_frontend_gone(&host, &double);
    assert_backend_released(&host);
    let _ = host.source;
}

#[test]
fn losing_the_frontend_does_not_leave_the_run_working() {
    let host = ControlHost::new("frontend-loss");
    host.server().answer_sequence(
        "thread/read",
        vec![
            control_endpoint::Answer::Result(json!({"thread": {
                "id": CONTROL_THREAD,
                "cwd": host.slot,
                "turns": []
            }})),
            control_endpoint::Answer::Result(json!({"thread": {
                "id": CONTROL_THREAD,
                "cwd": host.slot,
                "turns": [{"id": CONTROL_TURN, "status": "inProgress"}]
            }})),
        ],
    );
    let partial = host.slot.join("partial-work.txt");
    fs::write(&partial, "kept partial work").unwrap();
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    host.register_frontend(&double);
    let session = ConsoleSession::spawn(ConsoleSpec::new(host_spec(&host))).unwrap();
    let until = Instant::now() + Duration::from_secs(25);
    while host.server().requests_for("turn/start").is_empty() && Instant::now() < until {
        thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(host.server().requests_for("turn/start").len(), 1);
    fs::write(host.home.join("frontend-release"), "release").unwrap();
    let finished = session
        .wait(
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let receipt = receipt_json(&host.receipt);
    assert_eq!(receipt["observation"]["state"], "interrupted", "{receipt}");
    assert_eq!(
        receipt["observation"]["session"], CONTROL_THREAD,
        "{receipt}"
    );
    assert_ne!(receipt["observation"]["state"], "completed", "{receipt}");
    assert!(
        receipt["observation"]["cause"]
            .as_str()
            .is_some_and(|cause| cause.contains("frontend exited") && cause.contains("resume")),
        "{receipt}"
    );
    assert_eq!(fs::read_to_string(&partial).unwrap(), "kept partial work");
    let slot: Value =
        serde_json::from_slice(&fs::read(host.state.join("slot-1.json")).unwrap()).unwrap();
    assert_eq!(slot["state"], "occupied");
    assert_eq!(slot["owner"], CONTROL_OWNER);
    assert_eq!(host.server().requests_for("turn/start").len(), 1);
    assert_eq!(
        host.server().requests_for("turn/interrupt").len(),
        1,
        "view loss must interrupt the active turn"
    );
    assert_backend_released(&host);
    let watched = lead_command()
        .args(["executor", "watch", "--receipt"])
        .arg(&host.receipt)
        .output()
        .unwrap();
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(1), "{watched_text}");
    assert!(
        watched_text.contains("state=interrupted") && watched_text.contains("frontend exited"),
        "{watched_text}"
    );
    assert!(watched_text.contains(CONTROL_THREAD), "{watched_text}");
    assert!(watched_text.contains("resume"), "{watched_text}");
    assert!(!watched_text.contains("state=completed"), "{watched_text}");
}

#[test]
fn completion_racing_frontend_exit_keeps_the_retained_result() {
    let host = ControlHost::new("frontend-race");
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    host.register_frontend(&double);
    let session = ConsoleSession::spawn(ConsoleSpec::new(host_spec(&host))).unwrap();
    let until = Instant::now() + Duration::from_secs(25);
    while host.server().requests_for("turn/start").is_empty() && Instant::now() < until {
        thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(host.server().requests_for("turn/start").len(), 1);
    // The thread already holds the completed turn, and the terminal event is
    // pushed in the same moment the frontend exits. Either signal may win the
    // race; frontend exit must not replace the retained result.
    completion_burst(host.server());
    fs::write(host.home.join("frontend-release"), "release").unwrap();
    let finished = session
        .wait(
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert_eq!(
        fs::read_to_string(host.state.join("message-1.txt"))
            .unwrap()
            .trim(),
        CONTROL_FINAL
    );
    let receipt = receipt_json(&host.receipt);
    assert_eq!(receipt["observation"]["state"], "completed", "{receipt}");
    assert_eq!(receipt["observation"]["exitCode"], 0, "{receipt}");
    assert_eq!(
        receipt["observation"]["session"], CONTROL_THREAD,
        "{receipt}"
    );
    assert!(
        receipt["observation"]["cause"].is_null()
            || !receipt["observation"]["cause"]
                .as_str()
                .unwrap_or("")
                .contains("frontend exited"),
        "frontend exit must not replace the retained result: {receipt}"
    );
    assert_eq!(host.server().requests_for("turn/start").len(), 1);
    let watched = lead_command()
        .args(["executor", "watch", "--receipt"])
        .arg(&host.receipt)
        .output()
        .unwrap();
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(0), "{watched_text}");
    assert!(watched_text.contains("state=completed"), "{watched_text}");
    assert!(watched_text.contains(CONTROL_FINAL), "{watched_text}");
    assert!(watched_text.contains(CONTROL_THREAD), "{watched_text}");
    assert!(
        !watched_text.contains("state=interrupted"),
        "{watched_text}"
    );
}

#[test]
fn unfocused_tab_remains_a_valid_surface() {
    let host = ControlHost::new("frontend-unfocused");
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    host.register_frontend(&double);
    let session = ConsoleSession::spawn(ConsoleSpec::new(host_spec(&host))).unwrap();
    let until = Instant::now() + Duration::from_secs(25);
    while host.server().requests_for("turn/start").is_empty() && Instant::now() < until {
        thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(host.server().requests_for("turn/start").len(), 1);
    let phases = frontend_phases(&host);
    assert!(
        phases
            .iter()
            .any(|phase| phase["phase"] == "attached" && phase["alive"] == true),
        "the unselected surface must still be attached: {phases:?}"
    );
    // This pseudoconsole is not a selected Windows Terminal tab. Waiting here
    // would interrupt the run if focus or selection were treated as view loss.
    let _foreground = harness_core::task_view::foreground_window();
    thread::sleep(Duration::from_millis(500));
    assert!(
        host.server().requests_for("turn/interrupt").is_empty(),
        "an unfocused tab must not interrupt the run"
    );
    assert!(
        !host.home.join("frontend-release").exists(),
        "the frontend process is still the surface"
    );
    completion_burst(host.server());
    let finished = session
        .wait(
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert!(host.server().requests_for("turn/interrupt").is_empty());
    assert_eq!(host.server().requests_for("turn/start").len(), 1);
    let receipt = receipt_json(&host.receipt);
    assert_eq!(receipt["observation"]["state"], "completed", "{receipt}");
    assert_eq!(
        receipt["observation"]["session"], CONTROL_THREAD,
        "{receipt}"
    );
    let watched = lead_command()
        .args(["executor", "watch", "--receipt"])
        .arg(&host.receipt)
        .output()
        .unwrap();
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(0), "{watched_text}");
    assert!(watched_text.contains("state=completed"), "{watched_text}");
    assert!(watched_text.contains(CONTROL_FINAL), "{watched_text}");
    assert!(!watched_text.contains("frontend exited"), "{watched_text}");
}

fn host_command(host: &ControlHost, close_tab: bool) -> CommandSpec {
    let mut spec = host_spec(host);
    if close_tab {
        spec.args.push("--close-tab".into());
    }
    spec
}

fn process_gone(pid: u32, created: u64, program: &Path) -> bool {
    let user = harness_core::process_service::current_user().unwrap();
    harness_core::process_service::ServiceProcess::inspect(
        harness_core::process::ProcessIdentity {
            pid,
            creation_time: created,
        },
        program,
        &user,
    )
    .unwrap()
    .is_none()
}

fn endpoint_backend(host: &ControlHost) -> (u32, u64, PathBuf) {
    let value = receipt_json(&host.state.join("endpoint-1.json"));
    let process = &value["process"];
    (
        process["pid"].as_u64().expect("endpoint pid") as u32,
        process["creationTime"].as_u64().expect("endpoint creation"),
        PathBuf::from(process["program"].as_str().expect("endpoint program")),
    )
}

fn wait_turn(host: &ControlHost) {
    let until = Instant::now() + Duration::from_secs(25);
    while host.server().requests_for("turn/start").is_empty() && Instant::now() < until {
        thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(host.server().requests_for("turn/start").len(), 1);
}

fn assert_frontend_gone(host: &ControlHost, double: &Path) {
    let phases = frontend_phases(host);
    let pid = phases[0]["pid"].as_u64().unwrap() as u32;
    let created = phases[0]["creationTime"].as_u64().unwrap();
    assert!(
        process_gone(pid, created, double),
        "owned frontend still running: {phases:?}"
    );
}

fn assert_backend_released(host: &ControlHost) {
    let (pid, created, program) = endpoint_backend(host);
    assert!(
        process_gone(pid, created, &program),
        "closing the owned frontend left this run's backend running: pid {pid}"
    );
    assert!(
        !host.server().requests_for("turn/start").is_empty(),
        "releasing this run's backend dropped the neighboring control session"
    );
}

fn spawn_neighbor() -> std::process::Child {
    Command::new("pwsh")
        .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 90"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn successful_and_unsuccessful_results_survive_tab_closure() {
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    for (name, failed) in [("tab-close-ok", false), ("tab-close-failed", true)] {
        let host = ControlHost::new(name);
        host.register_frontend(&double);
        let mut neighbor = spawn_neighbor();
        let session = ConsoleSession::spawn(ConsoleSpec::new(host_command(&host, true))).unwrap();
        wait_turn(&host);
        if failed {
            host.server().push(json!({
                "method": "turn/completed",
                "params": {
                    "threadId": CONTROL_THREAD,
                    "turn": {
                        "id": CONTROL_TURN,
                        "status": "failed",
                        "error": {"message": "fixture turn failed"}
                    }
                }
            }));
        } else {
            completion_burst(host.server());
        }
        let finished = session
            .wait(
                Deadline::after(Duration::from_secs(20)).unwrap(),
                &Cancellation::default(),
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(
            finished.outcome.exit_code, 0,
            "{name}: the tab host must exit 0 so the tab closes: {}",
            finished.transcript
        );
        let receipt = receipt_json(&host.receipt);
        let observation = &receipt["observation"];
        if failed {
            assert_eq!(observation["state"], "failed", "{name}: {receipt}");
            assert_eq!(observation["exitCode"], 1, "{name}: {receipt}");
            assert!(
                observation["cause"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("fixture turn failed"),
                "{name}: {receipt}"
            );
        } else {
            assert_eq!(observation["state"], "completed", "{name}: {receipt}");
            assert_eq!(observation["exitCode"], 0, "{name}: {receipt}");
            assert_eq!(
                fs::read_to_string(host.state.join("message-1.txt"))
                    .unwrap()
                    .trim(),
                CONTROL_FINAL
            );
        }
        assert!(
            receipt.get("cleanup").is_none() || receipt["cleanup"].is_null(),
            "{name}: a successful close must not be recorded as a cleanup failure: {receipt}"
        );
        assert_frontend_gone(&host, &double);
        assert_backend_released(&host);
        assert!(
            neighbor.try_wait().unwrap().is_none(),
            "{name}: closing this run stopped a neighboring process"
        );
        let _ = neighbor.kill();
        let watched = lead_command()
            .args(["executor", "watch", "--receipt"])
            .arg(&host.receipt)
            .output()
            .unwrap();
        let watched_text = text(&watched);
        if failed {
            assert_eq!(watched.status.code(), Some(1), "{name}: {watched_text}");
            assert!(
                watched_text.contains("state=failed") && watched_text.contains("exit: 1"),
                "{name}: {watched_text}"
            );
        } else {
            assert_eq!(watched.status.code(), Some(0), "{name}: {watched_text}");
            assert!(
                watched_text.contains("state=completed") && watched_text.contains(CONTROL_FINAL),
                "{name}: {watched_text}"
            );
        }
    }
}

#[test]
fn owned_console_returns_the_failed_run_exit_code() {
    let host = ControlHost::new("console-failed");
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    host.register_frontend(&double);
    let mut neighbor = spawn_neighbor();
    let session = ConsoleSession::spawn(ConsoleSpec::new(host_command(&host, false))).unwrap();
    wait_turn(&host);
    host.server().push(json!({
        "method": "turn/completed",
        "params": {
            "threadId": CONTROL_THREAD,
            "turn": {
                "id": CONTROL_TURN,
                "status": "failed",
                "error": {"message": "fixture turn failed"}
            }
        }
    }));
    let finished = session
        .wait(
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(
        finished.outcome.exit_code, 1,
        "an owned console must return the run exit code, not the tab close signal: {}",
        finished.transcript
    );
    let receipt = receipt_json(&host.receipt);
    assert_eq!(receipt["observation"]["state"], "failed", "{receipt}");
    assert_eq!(receipt["observation"]["exitCode"], 1, "{receipt}");
    assert_frontend_gone(&host, &double);
    assert_backend_released(&host);
    assert!(neighbor.try_wait().unwrap().is_none());
    let _ = neighbor.kill();
    let watched = lead_command()
        .args(["executor", "watch", "--receipt"])
        .arg(&host.receipt)
        .output()
        .unwrap();
    assert_eq!(watched.status.code(), Some(1), "{}", text(&watched));
    assert!(text(&watched).contains("exit: 1"), "{}", text(&watched));
}

#[test]
fn cleanup_failure_names_survivors_without_changing_the_outcome() {
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    for (name, close_tab) in [("cleanup-console", false), ("cleanup-tab", true)] {
        let host = ControlHost::new(name);
        host.register_frontend(&double);
        let mut neighbor = spawn_neighbor();
        let session =
            ConsoleSession::spawn(ConsoleSpec::new(host_command(&host, close_tab))).unwrap();
        wait_turn(&host);
        fs::write(host.home.join("frontend-cleanup-fail"), "fail").unwrap();
        completion_burst(host.server());
        let finished = session
            .wait(
                Deadline::after(Duration::from_secs(20)).unwrap(),
                &Cancellation::default(),
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(
            finished.outcome.exit_code, 0,
            "{name}: cleanup must not replace the run exit; a tab host still exits 0: {}",
            finished.transcript
        );
        let receipt = receipt_json(&host.receipt);
        assert_eq!(
            receipt["observation"]["state"], "completed",
            "{name}: {receipt}"
        );
        assert_eq!(receipt["observation"]["exitCode"], 0, "{name}: {receipt}");
        assert_eq!(
            fs::read_to_string(host.state.join("message-1.txt"))
                .unwrap()
                .trim(),
            CONTROL_FINAL,
            "{name}: the final message was lost"
        );
        assert_eq!(receipt["cleanup"]["closed"], false, "{name}: {receipt}");
        let survivor = &receipt["cleanup"]["survivors"][0];
        let phases = frontend_phases(&host);
        assert_eq!(survivor["kind"], "frontend", "{name}: {receipt}");
        assert_eq!(survivor["pid"], phases[0]["pid"], "{name}: {receipt}");
        assert_eq!(
            survivor["created"], phases[0]["creationTime"],
            "{name}: {receipt}"
        );
        assert!(
            survivor["cause"]
                .as_str()
                .unwrap_or_default()
                .contains("surviving"),
            "{name}: {receipt}"
        );
        let recovery = receipt["cleanup"]["recovery"].as_str().unwrap_or_default();
        assert!(
            recovery.contains("does not change the recorded assignment outcome"),
            "{name}: {recovery}"
        );
        let log = fs::read_to_string(host.state.join("endpoint-1.log")).unwrap_or_default();
        assert!(
            log.contains("cleanup failed") && log.contains("surviving"),
            "{name}: cleanup failure was not reported: {log}"
        );
        assert_backend_released(&host);
        assert!(
            neighbor.try_wait().unwrap().is_none(),
            "{name}: cleanup stopped a neighboring process"
        );
        let _ = neighbor.kill();
        let watched = lead_command()
            .args(["executor", "watch", "--receipt"])
            .arg(&host.receipt)
            .output()
            .unwrap();
        let watched_text = text(&watched);
        assert_eq!(watched.status.code(), Some(0), "{name}: {watched_text}");
        assert!(
            watched_text.contains("state=completed") && watched_text.contains(CONTROL_FINAL),
            "{name}: {watched_text}"
        );
    }
}

const STALE_TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f24aa";
const CORRECTION_TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f24bb";
const REPLY_TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f24cc";
const LATE_TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f24dd";
const STALE_FINAL: &str = "STALE_PREVIOUS_RESULT";
const CORRECTION_FINAL: &str = "CORRECTION_ACCEPTED_RESULT";
const REPLY_FINAL: &str = "REPLY_CONTINUED_RESULT";

fn spawn_managed(name: &str) -> (ControlHost, ConsoleSession) {
    let host = ControlHost::new(name);
    host.use_presentation("tui", "native-tui");
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    host.register_frontend(&double);
    let session = ConsoleSession::spawn(ConsoleSpec::new(host_spec(&host))).unwrap();
    wait_turn(&host);
    (host, session)
}

fn watch_receipt(host: &ControlHost, timeout: &str) -> Output {
    lead_command()
        .args(["executor", "watch", "--receipt"])
        .arg(&host.receipt)
        .args(["--timeout", timeout, "--poll", "50"])
        .output()
        .unwrap()
}

/// The watch exit code for a live run waiting for a reply: action required.
const WATCH_WAITING_EXIT: i32 = 3;

/// The same watch call a machine consumer reads: compact JSON review data.
fn watch_json(host: &ControlHost, timeout: &str) -> Output {
    lead_command()
        .args(["executor", "watch", "--receipt"])
        .arg(&host.receipt)
        .args(["--json", "--timeout", timeout, "--poll", "50"])
        .output()
        .unwrap()
}

/// The captured output of a watch call this check spawned itself.
fn child_text(child: &mut Child) -> String {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        io::Read::read_to_end(&mut pipe, &mut bytes).unwrap();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn wait_session(session: ConsoleSession) -> harness_core::console::ConsoleOutcome {
    session
        .wait(
            Deadline::after(Duration::from_secs(25)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap()
}

fn push_completed(host: &ControlHost, turn: &str, status: &str) {
    host.server().push(json!({
        "method": "turn/completed",
        "params": {
            "threadId": CONTROL_THREAD,
            "turn": {"id": turn, "status": status}
        }
    }));
}

fn push_user_input(host: &ControlHost, item_id: &str, turn: &str, text: &str) {
    for method in ["item/started", "item/completed"] {
        host.server().push(json!({
            "method": method,
            "params": {
                "threadId": CONTROL_THREAD,
                "item": {
                    "id": item_id,
                    "type": "userMessage",
                    "text": text,
                    "turnId": turn
                }
            }
        }));
    }
    host.server().push(json!({
        "method": "turn/started",
        "params": {
            "threadId": CONTROL_THREAD,
            "turn": {"id": turn, "status": "inProgress"}
        }
    }));
}

fn agent_turn(id: &str, text: &str) -> Value {
    json!({
        "id": id,
        "status": "completed",
        "items": [{
            "id": format!("agent-{id}"),
            "type": "agentMessage",
            "text": text
        }]
    })
}

fn answer_thread(host: &ControlHost, turns: Value) {
    host.server().answer(
        "thread/read",
        control_endpoint::Answer::Result(json!({"thread": {
            "id": CONTROL_THREAD,
            "cwd": host.slot,
            "turns": turns
        }})),
    );
}

fn write_reply_hold(host: &ControlHost, unresolved: bool) {
    let mut receipt = receipt_json(&host.receipt);
    if unresolved {
        receipt["replyRequests"] = json!([{
            "id": "req-1",
            "status": "unresolved",
            "requiresReply": true
        }]);
    } else if let Some(object) = receipt.as_object_mut() {
        object.remove("replyRequests");
    }
    fs::write(&host.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
}

fn write_lead_message(host: &ControlHost, kind: &str, status: &str) {
    let mut receipt = receipt_json(&host.receipt);
    receipt["leadMessages"] = json!([{
        "id": "lead-0123456789abcdef01234567",
        "kind": kind,
        "status": status,
        "method": "turn/steer",
        "leadThreadId": "01a0c719-f4d4-7880-a9d2-1a96ee0f2301",
        "session": CONTROL_THREAD,
        "owner": CONTROL_OWNER,
        "slot": 1
    }]);
    fs::write(&host.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
}

fn reply_on_same_thread(host: &ControlHost) {
    let endpoint = receipt_json(&host.state.join("endpoint-1.json"));
    let port = endpoint["port"].as_u64().expect("endpoint port") as u16;
    let token = endpoint["token"].as_str().expect("endpoint token");
    assert_eq!(endpoint["threadId"], CONTROL_THREAD);
    let mut connection =
        harness_core::task_control::ControlConnection::connect(port, token, Duration::from_secs(5))
            .expect("the waiting endpoint accepts a reply connection");
    host.server().answer(
        "turn/start",
        control_endpoint::Answer::Result(
            json!({"turn": {"id": REPLY_TURN, "status": "inProgress"}}),
        ),
    );
    connection
        .send(
            &json!({
                "id": 1,
                "method": "initialize",
                "params": {"clientInfo": {"name": "reply", "version": "1"}}
            }),
            Duration::from_secs(5),
        )
        .unwrap();
    let _ = connection.receive(Duration::from_secs(5));
    connection
        .send(
            &json!({
                "id": 2,
                "method": "turn/start",
                "params": {
                    "threadId": CONTROL_THREAD,
                    "input": [{"type": "text", "text": "one reply"}]
                }
            }),
            Duration::from_secs(5),
        )
        .unwrap();
    let answer = connection
        .receive(Duration::from_secs(5))
        .expect("reply turn/start answer");
    assert!(
        answer.is_some(),
        "the idle thread did not accept turn/start"
    );
    drop(connection);
}

#[test]
fn stale_completion_on_resume_does_not_finish_the_current_run() {
    let (host, session) = spawn_managed("stale-resume");
    push_completed(&host, STALE_TURN, "completed");
    host.server().push(json!({
        "method": "item/completed",
        "params": {
            "threadId": CONTROL_THREAD,
            "item": {"id": "stale-m", "type": "agentMessage", "text": STALE_FINAL}
        }
    }));
    thread::sleep(Duration::from_millis(1500));
    let early_receipt = receipt_json(&host.receipt);
    assert_ne!(
        early_receipt["observation"]["state"], "completed",
        "a resumed session's old completion finished this run: {early_receipt}"
    );
    let early = watch_receipt(&host, "1");
    assert_eq!(early.status.code(), Some(2), "{}", text(&early));
    assert!(
        !text(&early).contains(STALE_FINAL),
        "watch treated the stale result as this run: {}",
        text(&early)
    );

    // The accepted turn is not the last turn in the thread record. The
    // persisted result must still be this run's, not the stale one.
    answer_thread(
        &host,
        json!([
            agent_turn(CONTROL_TURN, CONTROL_FINAL),
            agent_turn(STALE_TURN, STALE_FINAL)
        ]),
    );
    completion_burst(host.server());
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert_eq!(
        fs::read_to_string(host.state.join("message-1.txt"))
            .unwrap()
            .trim(),
        CONTROL_FINAL
    );
    let watched = watch_receipt(&host, "10");
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(0), "{watched_text}");
    assert!(watched_text.contains(CONTROL_FINAL), "{watched_text}");
    assert!(!watched_text.contains(STALE_FINAL), "{watched_text}");
    assert_eq!(
        receipt_json(&host.receipt)["observation"]["session"],
        CONTROL_THREAD
    );
}

#[test]
fn native_tui_input_at_a_turn_boundary_is_delivered_once_or_undelivered_after_closure() {
    let (host, session) = spawn_managed("boundary-input");
    push_completed(&host, CONTROL_TURN, "completed");
    push_user_input(&host, "input-1", CORRECTION_TURN, "addressed correction");
    // The started and completed records of one item are one delivery.
    push_user_input(&host, "input-1", CORRECTION_TURN, "addressed correction");
    thread::sleep(Duration::from_millis(1500));
    let mid = receipt_json(&host.receipt);
    assert_ne!(
        mid["observation"]["state"], "completed",
        "the assignment turn's completion finished the correction: {mid}"
    );
    assert_eq!(
        host.server().requests_for("turn/start").len(),
        1,
        "native input must not submit a second assignment"
    );

    answer_thread(
        &host,
        json!([agent_turn(CORRECTION_TURN, CORRECTION_FINAL)]),
    );
    host.server().push(json!({
        "method": "item/completed",
        "params": {
            "threadId": CONTROL_THREAD,
            "item": {"id": "corr-m", "type": "agentMessage", "text": CORRECTION_FINAL}
        }
    }));
    push_completed(&host, CORRECTION_TURN, "completed");
    let until = Instant::now() + Duration::from_secs(8);
    let mut pushed_late = false;
    while Instant::now() < until {
        let receipt = receipt_json(&host.receipt);
        if receipt["inputClosure"] == "begun" && !pushed_late {
            push_user_input(&host, "input-late", LATE_TURN, "late after closure");
            pushed_late = true;
        }
        if pushed_late && receipt["observation"]["state"] == "completed" {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert!(
        pushed_late,
        "closure began without a chance to report the late input undelivered"
    );
    assert_eq!(
        fs::read_to_string(host.state.join("message-1.txt"))
            .unwrap()
            .trim(),
        CORRECTION_FINAL
    );
    let receipt = receipt_json(&host.receipt);
    let late = receipt["messages"].as_array().and_then(|messages| {
        messages
            .iter()
            .find(|message| message["id"] == "input-late")
    });
    assert_eq!(
        late.and_then(|message| message["status"].as_str()),
        Some("undelivered"),
        "{receipt}"
    );
    assert_eq!(
        receipt["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["id"] == "input-late")
            .count(),
        1,
        "a repeated late input must be reported once: {receipt}"
    );
    assert_eq!(host.server().requests_for("turn/start").len(), 1);
    let watched = watch_receipt(&host, "10");
    assert_eq!(watched.status.code(), Some(0), "{}", text(&watched));
    assert!(
        text(&watched).contains(CORRECTION_FINAL),
        "{}",
        text(&watched)
    );
}

#[test]
fn empty_final_on_the_accepted_turn_is_an_output_defect() {
    let (host, session) = spawn_managed("empty-final");
    answer_thread(
        &host,
        json!([{
            "id": CONTROL_TURN,
            "status": "completed",
            "items": [{"id": "empty", "type": "agentMessage", "text": "  "}]
        }]),
    );
    completion_burst(host.server());
    let finished = wait_session(session);
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let receipt = receipt_json(&host.receipt);
    assert_eq!(receipt["observation"]["state"], "defect", "{receipt}");
    let cause = receipt["observation"]["cause"].as_str().unwrap_or_default();
    assert!(cause.contains("output defect"), "{cause}");
    assert!(
        cause.contains("not evidence of model, authentication or quota unavailability"),
        "{cause}"
    );
    let watched = watch_receipt(&host, "10");
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(1), "{watched_text}");
    assert!(watched_text.contains("state=defect"), "{watched_text}");
    assert!(!watched_text.contains("state=completed"), "{watched_text}");
}

#[test]
fn control_disconnection_is_not_success() {
    let (mut host, session) = spawn_managed("disconnect");
    host.disconnect();
    let finished = wait_session(session);
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let receipt = receipt_json(&host.receipt);
    assert_ne!(receipt["observation"]["state"], "completed", "{receipt}");
    let cause = receipt["observation"]["cause"].as_str().unwrap_or_default();
    assert!(
        cause.contains("connection closed")
            || cause.contains("Connection reset")
            || finished.transcript.contains("connection closed")
            || finished.transcript.contains("Connection reset"),
        "{cause}\n{}",
        finished.transcript
    );
    let watched = watch_receipt(&host, "10");
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(1), "{watched_text}");
    assert!(!watched_text.contains("state=completed"), "{watched_text}");
}

#[test]
fn lost_control_host_is_interrupted_without_a_fabricated_result() {
    let (host, session) = spawn_managed("lost-host");
    let pid = receipt_json(&host.receipt)["observation"]["host"]["pid"]
        .as_u64()
        .expect("host identity") as u32;
    let killed = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(killed.success(), "could not stop the host pid {pid}");
    let started = Instant::now();
    let watched = watch_receipt(&host, "15");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "a lost host left watch unbounded: {}",
        text(&watched)
    );
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(1), "{watched_text}");
    assert!(watched_text.contains("state=interrupted"), "{watched_text}");
    assert!(!watched_text.contains("state=completed"), "{watched_text}");
    let _ = wait_session(session);
}

#[test]
fn watch_timeout_does_not_stop_and_a_later_watch_sees_the_same_run() {
    let (host, session) = spawn_managed("watch-timeout");
    let first = watch_receipt(&host, "1");
    let first_text = text(&first);
    assert_eq!(first.status.code(), Some(2), "{first_text}");
    assert!(
        first_text.contains("timed out") && !first_text.contains("state=completed"),
        "{first_text}"
    );
    assert!(
        host.server().requests_for("turn/interrupt").is_empty(),
        "timeout stopped the run: {:?}",
        host.server().requests_for("turn/interrupt")
    );
    let second = watch_receipt(&host, "1");
    assert_eq!(second.status.code(), Some(2), "{}", text(&second));
    assert!(
        host.server().requests_for("turn/interrupt").is_empty(),
        "repeated watch after timeout stopped the run"
    );
    assert_eq!(host.server().requests_for("turn/start").len(), 1);

    answer_thread(&host, json!([agent_turn(CONTROL_TURN, CONTROL_FINAL)]));
    completion_burst(host.server());
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let third = watch_receipt(&host, "10");
    let third_text = text(&third);
    assert_eq!(third.status.code(), Some(0), "{third_text}");
    assert!(third_text.contains(CONTROL_FINAL), "{third_text}");
    assert_eq!(
        fs::read_to_string(host.state.join("message-1.txt"))
            .unwrap()
            .trim(),
        CONTROL_FINAL
    );
}

#[test]
fn unresolved_reply_hold_keeps_the_tui_open_until_one_reply_continues_the_run() {
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    let (host, session) = spawn_managed("reply-hold");
    write_reply_hold(&host, true);
    answer_thread(&host, json!([agent_turn(CONTROL_TURN, " ")]));
    push_completed(&host, CONTROL_TURN, "completed");
    thread::sleep(Duration::from_millis(1600));
    let mid = receipt_json(&host.receipt);
    assert_ne!(mid["observation"]["state"], "completed", "{mid}");
    assert_ne!(
        mid["observation"]["state"], "defect",
        "an unresolved reply was reported as an empty-output defect: {mid}"
    );
    assert_ne!(
        mid["inputClosure"], "begun",
        "a reply hold began closure: {mid}"
    );
    let phases = frontend_phases(&host);
    assert!(
        phases.iter().any(|phase| phase["phase"] == "attached"),
        "{phases:?}"
    );
    assert!(
        !phases.iter().any(|phase| phase["phase"] == "persisted"),
        "the TUI closed while the reply was unresolved: {phases:?}"
    );
    let pid = phases[0]["pid"].as_u64().unwrap() as u32;
    let created = phases[0]["creationTime"].as_u64().unwrap();
    assert!(
        !process_gone(pid, created, &double),
        "frontend closed during the unresolved reply hold"
    );
    let waiting = watch_receipt(&host, "5");
    let waiting_text = text(&waiting);
    assert_eq!(
        waiting.status.code(),
        Some(WATCH_WAITING_EXIT),
        "an unresolved reply is the actionable waiting result: {waiting_text}"
    );
    assert!(
        waiting_text.contains("action required: waiting for reply")
            && waiting_text.contains("--reply-to req-1"),
        "{waiting_text}"
    );
    let watched_live = receipt_json(&host.receipt);
    assert_ne!(
        watched_live["observation"]["state"], "completed",
        "{watched_live}"
    );
    assert_ne!(
        watched_live["observation"]["state"], "defect",
        "{watched_live}"
    );
    assert_ne!(watched_live["inputClosure"], "begun", "{watched_live}");

    write_reply_hold(&host, false);
    answer_thread(&host, json!([agent_turn(REPLY_TURN, REPLY_FINAL)]));
    push_user_input(&host, "reply-1", REPLY_TURN, "one reply");
    host.server().push(json!({
        "method": "item/completed",
        "params": {
            "threadId": CONTROL_THREAD,
            "item": {"id": "reply-m", "type": "agentMessage", "text": REPLY_FINAL}
        }
    }));
    push_completed(&host, REPLY_TURN, "completed");
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert_eq!(
        fs::read_to_string(host.state.join("message-1.txt"))
            .unwrap()
            .trim(),
        REPLY_FINAL
    );
    assert_eq!(
        receipt_json(&host.receipt)["observation"]["session"],
        CONTROL_THREAD,
        "the reply started another session"
    );
    assert_frontend_gone(&host, &double);
    let watched = watch_receipt(&host, "10");
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(0), "{watched_text}");
    assert!(watched_text.contains(REPLY_FINAL), "{watched_text}");
}

#[test]
fn watch_reports_action_required_for_a_waiting_run_and_resumes_after_the_reply() {
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    let (host, session) = spawn_managed("watch-waiting");
    // The executor asks and its turn then ends: the hold keeps the run live.
    write_reply_hold(&host, true);
    answer_thread(&host, json!([agent_turn(CONTROL_TURN, " ")]));
    push_completed(&host, CONTROL_TURN, "completed");
    thread::sleep(Duration::from_millis(1600));
    let waiting = receipt_json(&host.receipt);
    assert_ne!(waiting["observation"]["state"], "completed", "{waiting}");
    assert_ne!(waiting["observation"]["state"], "defect", "{waiting}");

    // A newly invoked watch returns the actionable result promptly instead of
    // blocking to its timeout, and it claims no terminal outcome.
    let started = Instant::now();
    let reported = watch_receipt(&host, "30");
    let elapsed = started.elapsed();
    let reported_text = text(&reported);
    assert_eq!(
        reported.status.code(),
        Some(WATCH_WAITING_EXIT),
        "{reported_text}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "watch waited for its timeout instead of reporting the waiting run: {elapsed:?}"
    );
    assert!(
        reported_text.contains("action required: waiting for reply"),
        "{reported_text}"
    );
    assert!(reported_text.contains("id=req-1"), "{reported_text}");
    assert!(
        reported_text.contains("--reply-to req-1"),
        "{reported_text}"
    );
    assert!(
        reported_text.contains(&format!("session: {CONTROL_THREAD}")),
        "{reported_text}"
    );
    assert!(
        !reported_text.contains("state=completed"),
        "{reported_text}"
    );
    assert!(!reported_text.contains("state=defect"), "{reported_text}");
    assert!(!reported_text.contains("timed out"), "{reported_text}");
    assert!(
        !reported_text.contains("no native coverage"),
        "{reported_text}"
    );
    assert!(!reported_text.contains("resume"), "{reported_text}");
    assert!(
        !reported_text.contains("exit: "),
        "a waiting result claims no run exit: {reported_text}"
    );

    // The same result is available to a machine consumer.
    let json_out = watch_json(&host, "30");
    let json_text = text(&json_out);
    assert_eq!(
        json_out.status.code(),
        Some(WATCH_WAITING_EXIT),
        "{json_text}"
    );
    let machine: Value = serde_json::from_str(&json_text).unwrap();
    assert_eq!(machine["actionRequired"], true, "{machine}");
    assert_eq!(
        machine["waiting"]["requests"][0]["id"], "req-1",
        "{machine}"
    );
    assert_eq!(
        machine["waiting"]["requests"][0]["kind"], "reply-request",
        "{machine}"
    );
    assert_eq!(
        machine["waiting"]["requests"][0]["status"], "unresolved",
        "{machine}"
    );
    assert_eq!(
        machine["waiting"]["requests"][0]["record"], "replyRequests",
        "{machine}"
    );
    assert_eq!(
        machine["waiting"]["requests"][0]["reply"],
        "codex-harness executor message --reply-to req-1 --text '<answer>'",
        "{machine}"
    );
    assert_eq!(machine["waiting"]["omitted"], 0, "{machine}");
    assert_eq!(
        machine["state"], waiting["observation"]["state"],
        "{machine}"
    );
    assert!(machine["returned"].is_null(), "{machine}");
    assert!(machine["exitCode"].is_null(), "{machine}");

    // Waiting alone changes nothing: the run keeps its session, surface,
    // lease and partial work, and none of those is a release or a resume.
    let kept = receipt_json(&host.receipt);
    assert_eq!(kept["observation"]["session"], CONTROL_THREAD, "{kept}");
    assert_ne!(kept["inputClosure"], "begun", "{kept}");
    let lease = receipt_json(&host.state.join("lease-1.json"));
    let lease_pid = lease["pid"].as_u64().unwrap() as u32;
    let lease_created = lease["created"].as_u64().unwrap();
    let lease_program = PathBuf::from(lease["program"].as_str().unwrap());
    assert!(
        !process_gone(lease_pid, lease_created, &lease_program),
        "the host left while the reply was unresolved"
    );
    let phases = frontend_phases(&host);
    assert!(
        phases.iter().any(|phase| phase["phase"] == "attached"),
        "{phases:?}"
    );
    assert!(
        !phases.iter().any(|phase| phase["phase"] == "persisted"),
        "the frontend closed while the reply was unresolved: {phases:?}"
    );

    // One correlated reply continues the same idle thread. A watch taken while
    // that work is running observes it instead of claiming an outcome, and the
    // same watch command later returns the run's own terminal result.
    write_reply_hold(&host, false);
    reply_on_same_thread(&host);
    let started_turns = host.server().requests_for("turn/start");
    assert_eq!(started_turns.len(), 2, "{started_turns:?}");
    assert_eq!(
        started_turns[1]["params"]["threadId"], CONTROL_THREAD,
        "{started_turns:?}"
    );
    let during = watch_receipt(&host, "1");
    let during_text = text(&during);
    assert_eq!(during.status.code(), Some(2), "{during_text}");
    assert!(during_text.contains("timed out"), "{during_text}");
    assert!(!during_text.contains("action required"), "{during_text}");
    assert!(!during_text.contains("state=completed"), "{during_text}");

    push_user_input(&host, "reply-1", REPLY_TURN, "one reply");
    answer_thread(&host, json!([agent_turn(REPLY_TURN, REPLY_FINAL)]));
    host.server().push(json!({
        "method": "item/completed",
        "params": {
            "threadId": CONTROL_THREAD,
            "item": {"id": "reply-m", "type": "agentMessage", "text": REPLY_FINAL}
        }
    }));
    push_completed(&host, REPLY_TURN, "completed");
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let after = watch_receipt(&host, "10");
    let after_text = text(&after);
    assert_eq!(after.status.code(), Some(0), "{after_text}");
    assert!(after_text.contains("state=completed"), "{after_text}");
    assert!(after_text.contains(REPLY_FINAL), "{after_text}");
    assert!(!after_text.contains("action required"), "{after_text}");
    assert_eq!(
        receipt_json(&host.receipt)["observation"]["session"],
        CONTROL_THREAD,
        "the reply started another session"
    );
    assert_eq!(host.server().requests_for("thread/resume").len(), 1);
    assert_frontend_gone(&host, &double);
}

#[test]
fn an_active_watch_returns_action_required_when_the_run_reaches_waiting() {
    let (host, session) = spawn_managed("watch-active");
    let mut watch = lead_command()
        .args(["executor", "watch", "--receipt"])
        .arg(&host.receipt)
        .args(["--timeout", "60", "--poll", "50"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(1200));
    assert!(
        watch.try_wait().unwrap().is_none(),
        "watch returned before the run was waiting"
    );
    // The request becomes unresolved and its turn ends while this watch call
    // is already blocked on the run.
    let asked = Instant::now();
    write_reply_hold(&host, true);
    answer_thread(&host, json!([agent_turn(CONTROL_TURN, " ")]));
    push_completed(&host, CONTROL_TURN, "completed");
    let status = wait_host(&mut watch, "the watch call blocked on the waiting run");
    let output = child_text(&mut watch);
    assert_eq!(status.code(), Some(WATCH_WAITING_EXIT), "{output}");
    assert!(
        asked.elapsed() < Duration::from_secs(20),
        "the active watch waited for its timeout instead of the waiting state: {:?}",
        asked.elapsed()
    );
    assert!(
        output.contains("action required: waiting for reply") && output.contains("id=req-1"),
        "{output}"
    );
    assert!(!output.contains("state=completed"), "{output}");

    // The same run still finishes with its own result after one reply, and no
    // resume was needed for it.
    write_reply_hold(&host, false);
    push_user_input(&host, "reply-1", REPLY_TURN, "one reply");
    answer_thread(&host, json!([agent_turn(REPLY_TURN, REPLY_FINAL)]));
    host.server().push(json!({
        "method": "item/completed",
        "params": {
            "threadId": CONTROL_THREAD,
            "item": {"id": "reply-m", "type": "agentMessage", "text": REPLY_FINAL}
        }
    }));
    push_completed(&host, REPLY_TURN, "completed");
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let after = watch_receipt(&host, "10");
    let after_text = text(&after);
    assert_eq!(after.status.code(), Some(0), "{after_text}");
    assert!(after_text.contains(REPLY_FINAL), "{after_text}");
    assert_eq!(host.server().requests_for("thread/resume").len(), 1);
}

#[test]
fn waiting_at_the_watch_deadline_is_action_required_and_never_a_terminal_claim() {
    let (host, session) = spawn_managed("watch-deadline");
    // A due deadline on a running run without a request stays a timeout.
    let idle = watch_receipt(&host, "0");
    let idle_text = text(&idle);
    assert_eq!(idle.status.code(), Some(2), "{idle_text}");
    assert!(
        idle_text.contains("watch timed out after 0s while the run was still"),
        "{idle_text}"
    );
    assert!(!idle_text.contains("action required"), "{idle_text}");

    // A notification is not a reply request; it never becomes exit 3.
    write_lead_message(&host, "notification", "delivered");
    let notified = watch_receipt(&host, "0");
    let notified_text = text(&notified);
    assert_eq!(notified.status.code(), Some(2), "{notified_text}");
    assert!(
        !notified_text.contains("action required"),
        "{notified_text}"
    );

    // The same due deadline with an established waiting state yields the
    // actionable result instead of hiding the question behind a timeout, and
    // the run stays live with its slot bound.
    write_reply_hold(&host, true);
    answer_thread(&host, json!([agent_turn(CONTROL_TURN, " ")]));
    push_completed(&host, CONTROL_TURN, "completed");
    thread::sleep(Duration::from_millis(1600));
    let boundary = watch_receipt(&host, "0");
    let boundary_text = text(&boundary);
    assert_eq!(
        boundary.status.code(),
        Some(WATCH_WAITING_EXIT),
        "{boundary_text}"
    );
    assert!(
        boundary_text.contains("action required: waiting for reply"),
        "{boundary_text}"
    );
    assert!(boundary_text.contains("id=req-1"), "{boundary_text}");
    assert!(!boundary_text.contains("timed out"), "{boundary_text}");
    let live = receipt_json(&host.receipt);
    assert_ne!(live["observation"]["state"], "completed", "{live}");
    assert!(host.state.join("lease-1.json").is_file(), "{live}");

    // The request list is bounded and the references left out are counted
    // rather than silently dropped.
    let mut many = receipt_json(&host.receipt);
    many["replyRequests"] = Value::Array(
        (1..=6)
            .map(|index| {
                json!({
                    "id": format!("req-{index}"),
                    "status": "unresolved",
                    "requiresReply": true
                })
            })
            .collect(),
    );
    fs::write(&host.receipt, serde_json::to_vec_pretty(&many).unwrap()).unwrap();
    let bounded = watch_json(&host, "30");
    let bounded_text = text(&bounded);
    assert_eq!(
        bounded.status.code(),
        Some(WATCH_WAITING_EXIT),
        "{bounded_text}"
    );
    let machine: Value = serde_json::from_str(&bounded_text).unwrap();
    let requests = machine["waiting"]["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 4, "{machine}");
    assert_eq!(requests[0]["id"], "req-1", "{machine}");
    assert_eq!(requests[3]["id"], "req-4", "{machine}");
    assert_eq!(machine["waiting"]["omitted"], 2, "{machine}");
    let bounded_again = watch_receipt(&host, "30");
    let bounded_again_text = text(&bounded_again);
    assert_eq!(
        bounded_again.status.code(),
        Some(WATCH_WAITING_EXIT),
        "{bounded_again_text}"
    );
    assert!(
        bounded_again_text.contains("2 further unresolved requests stay on the receipt"),
        "{bounded_again_text}"
    );

    // A dead run with the same unresolved request keeps its real outcome
    // instead of appearing reply-capable, and so does a stopped run.
    let run = SeededRun::new();
    let mut dead = receipt_json(&run.receipt);
    dead["observation"]["state"] = json!("running");
    dead["observation"]["session"] = json!(CONTROL_THREAD);
    dead["observation"]["host"] = json!({
        "pid": 4242,
        "created": 1,
        "program": run.root.join("absent-host.exe").to_string_lossy()
    });
    dead["replyRequests"] = json!([{"id": "req-1", "status": "unresolved", "requiresReply": true}]);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&dead).unwrap()).unwrap();
    let interrupted = run.watch(&["--timeout", "5"]);
    let interrupted_text = text(&interrupted);
    assert_eq!(interrupted.status.code(), Some(1), "{interrupted_text}");
    assert!(
        interrupted_text.contains("state=interrupted"),
        "{interrupted_text}"
    );
    assert!(
        !interrupted_text.contains("action required"),
        "{interrupted_text}"
    );

    let run = SeededRun::new();
    let mut stopped = receipt_json(&run.receipt);
    stopped["observation"]["state"] = json!("stopped");
    stopped["observation"]["exitCode"] = json!(1);
    stopped["replyRequests"] =
        json!([{"id": "req-1", "status": "unresolved", "requiresReply": true}]);
    fs::write(&run.receipt, serde_json::to_vec_pretty(&stopped).unwrap()).unwrap();
    let stopped_out = run.watch(&["--timeout", "5"]);
    let stopped_text = text(&stopped_out);
    assert_eq!(stopped_out.status.code(), Some(1), "{stopped_text}");
    assert!(stopped_text.contains("state=stopped"), "{stopped_text}");
    assert!(!stopped_text.contains("action required"), "{stopped_text}");

    // The owned run finishes with its own result once the reply resolves the
    // request, without a resume.
    write_reply_hold(&host, false);
    push_user_input(&host, "reply-1", REPLY_TURN, "one reply");
    answer_thread(&host, json!([agent_turn(REPLY_TURN, REPLY_FINAL)]));
    host.server().push(json!({
        "method": "item/completed",
        "params": {
            "threadId": CONTROL_THREAD,
            "item": {"id": "reply-m", "type": "agentMessage", "text": REPLY_FINAL}
        }
    }));
    push_completed(&host, REPLY_TURN, "completed");
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert_eq!(host.server().requests_for("thread/resume").len(), 1);
}
#[test]
fn unanswered_lead_message_keeps_the_same_thread_reply_capable_without_a_model_call() {
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    let (host, session) = spawn_managed("lead-hold");
    write_lead_message(&host, "reply-request", "delivered");
    answer_thread(&host, json!([agent_turn(CONTROL_TURN, " ")]));
    push_completed(&host, CONTROL_TURN, "completed");
    thread::sleep(Duration::from_millis(1600));
    let mid = receipt_json(&host.receipt);
    assert_ne!(mid["observation"]["state"], "completed", "{mid}");
    assert_ne!(
        mid["observation"]["state"], "defect",
        "an unanswered reply-request was an empty-output defect: {mid}"
    );
    assert_ne!(mid["inputClosure"], "begun", "{mid}");
    assert_eq!(mid["observation"]["session"], CONTROL_THREAD, "{mid}");
    let slot = receipt_json(&host.state.join("slot-1.json"));
    assert_eq!(slot["state"], "occupied", "{slot}");
    assert_eq!(
        PathBuf::from(slot["path"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        host.slot
    );
    let lease = receipt_json(&host.state.join("lease-1.json"));
    assert_eq!(lease["owner"], CONTROL_OWNER, "{lease}");
    assert_eq!(
        PathBuf::from(lease["path"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        host.slot,
        "waiting released the worktree"
    );
    let lease_pid = lease["pid"].as_u64().unwrap() as u32;
    let lease_created = lease["created"].as_u64().unwrap();
    let lease_program = PathBuf::from(lease["program"].as_str().unwrap());
    assert!(
        !process_gone(lease_pid, lease_created, &lease_program),
        "the host left while a reply was unresolved"
    );
    let endpoint = receipt_json(&host.state.join("endpoint-1.json"));
    assert_eq!(endpoint["threadId"], CONTROL_THREAD, "{endpoint}");
    assert_eq!(
        endpoint["port"].as_u64(),
        Some(u64::from(host.server().port)),
        "waiting replaced the conversation endpoint"
    );
    assert_eq!(host.server().requests_for("turn/start").len(), 1);
    assert_eq!(
        host.server().requests_for("thread/resume").len(),
        1,
        "waiting resumed the thread: {:?}",
        host.server().requests_for("thread/resume")
    );
    let phases = frontend_phases(&host);
    assert!(
        phases.iter().any(|phase| phase["phase"] == "attached"),
        "{phases:?}"
    );
    assert!(
        !phases.iter().any(|phase| phase["phase"] == "persisted"),
        "the owned frontend closed while the reply was unresolved: {phases:?}"
    );
    let frontend_pid = phases[0]["pid"].as_u64().unwrap() as u32;
    let frontend_created = phases[0]["creationTime"].as_u64().unwrap();
    assert!(
        !process_gone(frontend_pid, frontend_created, &double),
        "frontend closed during the unresolved reply hold"
    );

    let waiting = watch_receipt(&host, "5");
    let waiting_text = text(&waiting);
    assert_eq!(
        waiting.status.code(),
        Some(WATCH_WAITING_EXIT),
        "{waiting_text}"
    );
    assert!(
        waiting_text.contains("id=lead-0123456789abcdef01234567")
            && waiting_text.contains("--reply-to lead-0123456789abcdef01234567"),
        "the lead's own message record is the reply reference: {waiting_text}"
    );
    assert!(
        waiting_text.contains("lead=01a0c719-f4d4-7880-a9d2-1a96ee0f2301"),
        "the addressed lead stays visible in the waiting result: {waiting_text}"
    );

    reply_on_same_thread(&host);
    let started = host.server().requests_for("turn/start");
    assert_eq!(started.len(), 2, "{started:?}");
    assert_eq!(
        started[1]["params"]["threadId"], CONTROL_THREAD,
        "{started:?}"
    );
    assert!(started[1]["params"].get("expectedTurnId").is_none());
    assert_eq!(
        host.server().requests_for("thread/resume").len(),
        1,
        "the reply resumed instead of continuing the idle thread"
    );
    assert!(
        !process_gone(lease_pid, lease_created, &lease_program),
        "the reply connection replaced the host"
    );

    write_lead_message(&host, "reply-request", "resolved");
    answer_thread(&host, json!([agent_turn(REPLY_TURN, REPLY_FINAL)]));
    push_user_input(&host, "reply-1", REPLY_TURN, "one reply");
    host.server().push(json!({
        "method": "item/completed",
        "params": {
            "threadId": CONTROL_THREAD,
            "item": {"id": "reply-m", "type": "agentMessage", "text": REPLY_FINAL}
        }
    }));
    push_completed(&host, REPLY_TURN, "completed");
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert_eq!(
        fs::read_to_string(host.state.join("message-1.txt"))
            .unwrap()
            .trim(),
        REPLY_FINAL
    );
    assert_eq!(
        receipt_json(&host.receipt)["observation"]["session"],
        CONTROL_THREAD,
        "the reply started another session"
    );
    assert_frontend_gone(&host, &double);
    let watched = watch_receipt(&host, "10");
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(0), "{watched_text}");
    assert!(watched_text.contains(REPLY_FINAL), "{watched_text}");
}

#[test]
fn a_notification_alone_completes_and_closes_the_owned_frontend() {
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    let (host, session) = spawn_managed("lead-notify");
    write_lead_message(&host, "notification", "delivered");
    answer_thread(&host, json!([agent_turn(CONTROL_TURN, CONTROL_FINAL)]));
    host.server().push(json!({
        "method": "item/completed",
        "params": {
            "threadId": CONTROL_THREAD,
            "item": {"id": "m1", "type": "agentMessage", "text": CONTROL_FINAL}
        }
    }));
    push_completed(&host, CONTROL_TURN, "completed");
    let finished = wait_session(session);
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let receipt = receipt_json(&host.receipt);
    assert_eq!(receipt["observation"]["state"], "completed", "{receipt}");
    assert_eq!(
        receipt["observation"]["session"], CONTROL_THREAD,
        "{receipt}"
    );
    assert_ne!(receipt["observation"]["state"], "waiting-for-reply");
    assert_frontend_gone(&host, &double);
    assert_eq!(host.server().requests_for("turn/start").len(), 1);
    assert_eq!(host.server().requests_for("thread/resume").len(), 1);
}
const CONTINUATION_TURN: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f2601";
const FRESH_THREAD: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f2702";
const CONTINUATION_PROMPT: &str = "Continue from the partial edits; do not redo completed checks.";
const PRIOR_FINAL: &str = "COMPLETED_PRIOR_WORK";
const CONTINUATION_FINAL: &str = "CONTINUATION_NOT_REPLAY";
const STALE_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct StaleEndpoint {
    listener: TcpListener,
    port: u16,
}

#[test]
fn a_pending_request_that_loses_its_surface_is_not_waiting() {
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    let (host, session) = spawn_managed("waiting-surface-loss");
    let partial = host.slot.join("partial-work.txt");
    fs::write(&partial, "kept partial work").unwrap();
    // The request becomes unresolved and its turn ends: the live run is
    // actionable on its visible surface and keeps that surface.
    write_reply_hold(&host, true);
    answer_thread(&host, json!([agent_turn(CONTROL_TURN, " ")]));
    push_completed(&host, CONTROL_TURN, "completed");
    let waiting = watch_receipt(&host, "5");
    let waiting_text = text(&waiting);
    assert_eq!(
        waiting.status.code(),
        Some(WATCH_WAITING_EXIT),
        "{waiting_text}"
    );
    let held = receipt_json(&host.receipt);
    assert_ne!(held["observation"]["state"], "completed", "{held}");
    assert_ne!(held["observation"]["state"], "defect", "{held}");
    let phases = frontend_phases(&host);
    assert!(
        phases.iter().any(|phase| phase["phase"] == "attached"),
        "{phases:?}"
    );

    // The visible surface is lost while the request is still unanswered.
    fs::write(host.home.join("frontend-release"), "release").unwrap();
    let finished = wait_session(session);
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let receipt = receipt_json(&host.receipt);
    assert_eq!(receipt["observation"]["state"], "interrupted", "{receipt}");
    assert_eq!(
        receipt["observation"]["session"], CONTROL_THREAD,
        "the interrupted run lost its exact session: {receipt}"
    );
    assert!(
        receipt["observation"]["cause"]
            .as_str()
            .is_some_and(|cause| cause.contains("frontend exited") && cause.contains("resume")),
        "{receipt}"
    );
    // The request identity and the partial work stay for explicit recovery.
    assert_eq!(receipt["replyRequests"][0]["id"], "req-1", "{receipt}");
    assert_eq!(
        receipt["replyRequests"][0]["status"], "unresolved",
        "{receipt}"
    );
    assert_eq!(fs::read_to_string(&partial).unwrap(), "kept partial work");
    let slot: Value =
        serde_json::from_slice(&fs::read(host.state.join("slot-1.json")).unwrap()).unwrap();
    assert_eq!(slot["state"], "occupied", "{slot}");
    assert_eq!(slot["owner"], CONTROL_OWNER, "{slot}");
    assert_frontend_gone(&host, &double);
    assert_backend_released(&host);

    // A run whose surface is gone is never the actionable waiting result.
    let watched = watch_receipt(&host, "10");
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(1), "{watched_text}");
    assert!(watched_text.contains("state=interrupted"), "{watched_text}");
    assert!(
        !watched_text.contains("action required"),
        "a lost surface was reported as waiting: {watched_text}"
    );
}

#[test]
fn a_native_server_failure_while_a_request_is_pending_is_not_waiting() {
    let (mut host, session) = spawn_managed("waiting-server-loss");
    let partial = host.slot.join("partial-work.txt");
    fs::write(&partial, "kept partial work").unwrap();
    write_reply_hold(&host, true);
    answer_thread(&host, json!([agent_turn(CONTROL_TURN, " ")]));
    push_completed(&host, CONTROL_TURN, "completed");
    let waiting = watch_receipt(&host, "5");
    let waiting_text = text(&waiting);
    assert_eq!(
        waiting.status.code(),
        Some(WATCH_WAITING_EXIT),
        "{waiting_text}"
    );

    // The native server this conversation is bound to fails while the request
    // is unanswered.
    host.disconnect();
    let finished = wait_session(session);
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    let receipt = receipt_json(&host.receipt);
    assert_ne!(
        receipt["observation"]["state"], "completed",
        "a lost native server was reported as success: {receipt}"
    );
    assert_eq!(
        receipt["observation"]["session"], CONTROL_THREAD,
        "the failed run lost its exact session: {receipt}"
    );
    assert_eq!(
        receipt["replyRequests"][0]["status"], "unresolved",
        "the unanswered request was dropped: {receipt}"
    );
    assert_eq!(fs::read_to_string(&partial).unwrap(), "kept partial work");
    let slot: Value =
        serde_json::from_slice(&fs::read(host.state.join("slot-1.json")).unwrap()).unwrap();
    assert_eq!(slot["state"], "occupied", "{slot}");
    assert_eq!(slot["owner"], CONTROL_OWNER, "{slot}");

    // The dead conversation is never offered as a reply-capable wait, and its
    // request cannot be answered as if it were still live.
    let watched = watch_receipt(&host, "10");
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(1), "{watched_text}");
    assert!(
        !watched_text.contains("action required"),
        "a failed native server was reported as waiting: {watched_text}"
    );
    assert!(!watched_text.contains("state=completed"), "{watched_text}");
}

/// The recorded reply reference of an unresolved request, published the way
/// `lead message` publishes it, so the reply path itself decides its fate.
const WAITING_REPLY_ID: &str = "lead-0123456789abcdef01234567";

fn plant_waiting_reply_reference(host: &ControlHost) {
    let mut receipt = receipt_json(&host.receipt);
    receipt["leadMessages"] = json!([{
        "schema": 1,
        "id": WAITING_REPLY_ID,
        "kind": "reply-request",
        "status": "delivered",
        "requiresReply": true,
        "leadThreadId": "01a0c719-f4d4-7880-a9d2-1a96ee0f2301",
        "session": CONTROL_THREAD,
        "owner": CONTROL_OWNER,
        "slot": 1
    }]);
    fs::write(&host.receipt, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let index = host
        .home
        .join("harness/executor-pool/message-index")
        .join(format!("{WAITING_REPLY_ID}.json"));
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    fs::write(
        &index,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "id": WAITING_REPLY_ID,
            "receipt": host.receipt.to_str().unwrap(),
        }))
        .unwrap(),
    )
    .unwrap();
}

fn reply_with_reference(host: &ControlHost, id: &str) -> Output {
    let mut command = lead_command();
    command
        .args([
            "executor",
            "message",
            "--reply-to",
            id,
            "--text",
            "one answer for the waiting run",
        ])
        .env("CODEX_HOME", &host.home)
        .env_remove("HARNESS_EXECUTOR_RUN")
        .env_remove("HARNESS_ORIGINATING_LEAD")
        .current_dir(&host.home);
    command.output().unwrap()
}

/// `executor stop` addresses one run through its registered checkout, so the
/// fixture source carries the same minimal project layout a real dispatch uses.
fn register_checkout(source: &Path) {
    fs::create_dir_all(source.join("global")).unwrap();
    fs::write(
        source.join("global/orchestration.toml"),
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\nworktree_limit = 1\n",
    )
    .unwrap();
    git(source, &["init", "-q", "--initial-branch=main"]);
    git(source, &["add", "."]);
    git(
        source,
        &[
            "-c",
            "user.email=stop@example.test",
            "-c",
            "user.name=Stop",
            "commit",
            "-qm",
            "seed",
        ],
    );
}

#[test]
fn stopping_a_waiting_run_invalidates_the_reply_and_preserves_the_slot() {
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    let (host, session) = spawn_managed("waiting-stop");
    register_checkout(&host.source);
    let partial = host.slot.join("partial-work.txt");
    fs::write(&partial, "kept partial work").unwrap();
    plant_waiting_reply_reference(&host);
    answer_thread(&host, json!([agent_turn(CONTROL_TURN, " ")]));
    push_completed(&host, CONTROL_TURN, "completed");
    let waiting = watch_receipt(&host, "5");
    let waiting_text = text(&waiting);
    assert_eq!(
        waiting.status.code(),
        Some(WATCH_WAITING_EXIT),
        "the stop check needs a run that is actually waiting: {waiting_text}"
    );
    assert!(
        waiting_text.contains(&format!("--reply-to {WAITING_REPLY_ID}")),
        "{waiting_text}"
    );

    // The explicit urgent stop ends that exact waiting run.
    let stopped = lead_command()
        .args([
            "executor",
            "stop",
            "--source",
            host.source.to_str().unwrap(),
            "--codex-home",
            host.home.to_str().unwrap(),
            "--slot",
            "1",
            "--owner",
            CONTROL_OWNER,
            "--timeout",
            "20",
        ])
        .output()
        .unwrap();
    let stopped_text = text(&stopped);
    assert_eq!(stopped.status.code(), Some(0), "{stopped_text}");
    let receipt = receipt_json(&host.receipt);
    assert_eq!(receipt["observation"]["state"], "stopped", "{receipt}");
    assert_eq!(receipt["stop"]["outcome"], "stopped", "{receipt}");
    assert_eq!(
        receipt["observation"]["session"], CONTROL_THREAD,
        "the stop ended another conversation: {receipt}"
    );
    let finished = wait_session(session);
    assert_ne!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert_frontend_gone(&host, &double);
    assert_backend_released(&host);

    // Files and slot remain for explicit recovery.
    assert_eq!(fs::read_to_string(&partial).unwrap(), "kept partial work");
    let slot: Value =
        serde_json::from_slice(&fs::read(host.state.join("slot-1.json")).unwrap()).unwrap();
    assert_eq!(slot["state"], "occupied", "{slot}");
    assert_eq!(slot["owner"], CONTROL_OWNER, "{slot}");

    // The stopped run is not waiting, and the recorded request stays visible
    // as an unanswered request of a run that ended.
    let watched = watch_receipt(&host, "10");
    let watched_text = text(&watched);
    assert_eq!(watched.status.code(), Some(1), "{watched_text}");
    assert!(watched_text.contains("state=stopped"), "{watched_text}");
    assert!(
        !watched_text.contains("action required"),
        "a stopped run was reported as waiting: {watched_text}"
    );

    // The pending reply is invalidated: the answer reaches neither the ended
    // run nor a replacement conversation, and nothing is resumed.
    let steers = host.server().requests_for("turn/steer").len();
    let turns = host.server().requests_for("turn/start").len();
    let resumes = host.server().requests_for("thread/resume").len();
    let reply = reply_with_reference(&host, WAITING_REPLY_ID);
    let reply_text = text(&reply);
    assert_eq!(reply.status.code(), Some(2), "{reply_text}");
    assert!(
        reply_text.contains(&format!("retired message id {WAITING_REPLY_ID}")),
        "{reply_text}"
    );
    assert_eq!(
        host.server().requests_for("turn/steer").len(),
        steers,
        "a reply reached the stopped run: {reply_text}"
    );
    assert_eq!(
        host.server().requests_for("turn/start").len(),
        turns,
        "a reply started a conversation on the stopped run: {reply_text}"
    );
    assert!(
        host.server().requests_for("thread/resume").len() == resumes,
        "a reply resumed the stopped run: {reply_text}"
    );
    let after = receipt_json(&host.receipt);
    assert_eq!(after["observation"]["state"], "stopped", "{after}");
    assert_eq!(
        after["leadMessages"][0]["id"], WAITING_REPLY_ID,
        "the unanswered request was dropped: {after}"
    );
}

fn plant_stale_endpoint(host: &ControlHost, thread_id: &str) -> StaleEndpoint {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    fs::write(
        host.state.join("endpoint-1.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "port": port,
            "token": STALE_TOKEN,
            "threadId": thread_id,
            "process": {
                "pid": 1,
                "creationTime": 1,
                "program": r"C:\stale\codex.exe"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    StaleEndpoint { listener, port }
}

fn stale_endpoint_untouched(stale: &StaleEndpoint) {
    match stale.listener.accept() {
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
        other => panic!("the stale control endpoint accepted a successor connection: {other:?}"),
    }
}

fn assert_successor_endpoint(host: &ControlHost, thread_id: &str, stale: &StaleEndpoint) {
    stale_endpoint_untouched(stale);
    let endpoint = receipt_json(&host.state.join("endpoint-1.json"));
    assert_eq!(endpoint["threadId"], thread_id, "{endpoint}");
    assert_eq!(endpoint["port"], host.server().port, "{endpoint}");
    assert_ne!(endpoint["port"], stale.port, "{endpoint}");
    assert_ne!(endpoint["token"], STALE_TOKEN, "{endpoint}");
    assert_ne!(endpoint["process"]["pid"], 1, "{endpoint}");
}

fn write_receipt(host: &ControlHost, receipt: &Value) {
    fs::write(&host.receipt, serde_json::to_vec_pretty(receipt).unwrap()).unwrap();
}

fn resume_receipt(host: &ControlHost, assignment: &str) -> Value {
    let mut receipt = receipt_json(&host.receipt);
    receipt["mode"] = json!("tui");
    receipt["control"]["presentation"] = json!("native-tui");
    receipt["control"]["assignment"] = json!(assignment);
    receipt["control"]["resumeSession"] = json!(CONTROL_THREAD);
    receipt["observation"]["previousSession"] = json!(CONTROL_THREAD);
    receipt["observation"]["session"] = Value::Null;
    receipt
}

fn restart_handoff() -> String {
    format!(
        "Continue the original assignment in this same preserved worktree. This is a fresh conversation; do not reset or replay completed work. Predecessor session {CONTROL_THREAD}.\n\nOriginal assignment:\n{CONTROL_ASSIGNMENT}"
    )
}

fn spawn_hosted(host: &ControlHost) -> ConsoleSession {
    ConsoleSession::spawn(ConsoleSpec::new(host_spec(host))).unwrap()
}

fn wait_for_turn(host: &ControlHost) {
    let until = Instant::now() + Duration::from_secs(25);
    while host.server().requests_for("turn/start").is_empty() && Instant::now() < until {
        thread::sleep(Duration::from_millis(40));
    }
}

fn finish_turn(
    host: &ControlHost,
    session: ConsoleSession,
    thread_id: &str,
    turn: &str,
    final_text: &str,
) {
    host.server().push(json!({
        "method": "item/completed",
        "params": {
            "threadId": thread_id,
            "item": {"id": "m-new", "type": "agentMessage", "text": final_text}
        }
    }));
    host.server().push(json!({
        "method": "turn/completed",
        "params": {
            "threadId": thread_id,
            "turn": {"id": turn, "status": "completed"}
        }
    }));
    let finished = session
        .wait(
            Deadline::after(Duration::from_secs(20)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(finished.outcome.exit_code, 0, "{}", finished.transcript);
    assert!(
        !finished.transcript.contains("conversation=control"),
        "controller output corrupted the frontend surface: {}",
        finished.transcript
    );
}

#[test]
fn exact_session_resume_attaches_the_native_frontend_without_reset_or_replay() {
    let host = ControlHost::new("exact-resume");
    write_receipt(&host, &resume_receipt(&host, CONTINUATION_PROMPT));
    host.server().answer(
        "thread/start",
        control_endpoint::Answer::Result(json!({
            "thread": {"id": FRESH_THREAD, "cwd": host.slot},
            "model": CONTROL_MODEL,
            "modelProvider": CONTROL_PROVIDER,
            "reasoningEffort": CONTROL_EFFORT
        })),
    );
    host.server().answer(
        "thread/resume",
        control_endpoint::Answer::Result(json!({
            "thread": {"id": CONTROL_THREAD, "cwd": host.slot},
            "model": CONTROL_MODEL,
            "modelProvider": CONTROL_PROVIDER,
            "reasoningEffort": CONTROL_EFFORT
        })),
    );
    host.server().answer(
        "turn/start",
        control_endpoint::Answer::Result(
            json!({"turn": {"id": CONTINUATION_TURN, "status": "inProgress"}}),
        ),
    );
    host.server().answer(
        "thread/read",
        control_endpoint::Answer::Result(json!({"thread": {
            "id": CONTROL_THREAD,
            "cwd": host.slot,
            "turns": [
                {
                    "id": "prior-turn",
                    "status": "completed",
                    "items": [{"id": "old", "type": "agentMessage", "text": PRIOR_FINAL}]
                },
                {
                    "id": CONTINUATION_TURN,
                    "status": "completed",
                    "items": [{"id": "new", "type": "agentMessage", "text": CONTINUATION_FINAL}]
                }
            ]
        }})),
    );
    let partial = host.slot.join("partial-work.txt");
    fs::write(&partial, "partial work\n").unwrap();
    let stale = plant_stale_endpoint(&host, CONTROL_THREAD);
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    host.register_frontend(&double);
    let session = spawn_hosted(&host);
    wait_for_turn(&host);
    let turns = host.server().requests_for("turn/start");
    assert_eq!(turns.len(), 1, "one continuation prompt: {turns:?}");
    assert_eq!(turns[0]["params"]["threadId"], CONTROL_THREAD);
    assert_eq!(
        turns[0]["params"]["input"],
        json!([{"type": "text", "text": CONTINUATION_PROMPT}])
    );
    assert!(
        host.server().requests_for("thread/start").is_empty(),
        "resume must not start another conversation: {:?}",
        host.server().requests()
    );
    let resumes = host.server().requests_for("thread/resume");
    assert!(!resumes.is_empty(), "the exact session was not resumed");
    assert!(
        resumes
            .iter()
            .all(|request| request["params"]["threadId"] == CONTROL_THREAD),
        "frontend resume left the recorded session: {resumes:?}"
    );
    let argv = fs::read_to_string(host.home.join("frontend-argv.txt")).unwrap();
    assert!(
        argv.contains("resume") && argv.contains(CONTROL_THREAD),
        "{argv}"
    );
    assert!(!argv.contains(FRESH_THREAD), "{argv}");
    assert!(!argv.contains("--no-alt-screen"), "{argv}");
    assert!(!argv.contains(CONTINUATION_PROMPT), "{argv}");
    finish_turn(
        &host,
        session,
        CONTROL_THREAD,
        CONTINUATION_TURN,
        CONTINUATION_FINAL,
    );
    assert_eq!(
        fs::read_to_string(host.state.join("message-1.txt"))
            .unwrap()
            .trim(),
        CONTINUATION_FINAL
    );
    assert_eq!(fs::read_to_string(&partial).unwrap(), "partial work\n");
    assert_eq!(
        receipt_json(&host.state.join("slot-1.json"))["owner"],
        CONTROL_OWNER
    );
    let observed = receipt_json(&host.receipt);
    assert_eq!(observed["observation"]["session"], CONTROL_THREAD);
    assert_eq!(observed["observation"]["previousSession"], CONTROL_THREAD);
    assert_successor_endpoint(&host, CONTROL_THREAD, &stale);
    assert_frontend_gone(&host, &double);
    assert_backend_released(&host);
}

#[test]
fn restart_starts_a_fresh_session_and_ignores_the_stale_endpoint() {
    let host = ControlHost::new("fresh-restart");
    let handoff = restart_handoff();
    let mut receipt = receipt_json(&host.receipt);
    receipt["mode"] = json!("tui");
    receipt["control"]["presentation"] = json!("native-tui");
    receipt["control"]["assignment"] = json!(handoff);
    receipt["control"]["originalAssignment"] = json!(CONTROL_ASSIGNMENT);
    receipt["observation"]["previousSession"] = json!(CONTROL_THREAD);
    receipt["observation"]["session"] = Value::Null;
    // The continuation of a spawned run keeps its originating lead
    // relationship; its own new session is not that lead.
    let lead_thread = "01a0c719-f4d4-7880-a9d2-1a96ee0f2301";
    let dispatcher_program = std::env::current_exe().unwrap();
    let dispatcher_user = harness_core::process_service::current_user().unwrap();
    let dispatcher = harness_core::process_service::ServiceProcess::observe(
        std::process::id(),
        &dispatcher_program,
        0,
        &dispatcher_user,
    )
    .unwrap()
    .identity();
    receipt["originatingLead"] = json!({
        "schema": 1,
        "threadId": lead_thread,
        "runGeneration": "generation-continuation",
        "dispatcher": {
            "pid": dispatcher.pid,
            "creationTime": dispatcher.creation_time,
            "program": dispatcher_program,
        }
    });
    write_receipt(&host, &receipt);
    host.server().answer(
        "thread/start",
        control_endpoint::Answer::Result(json!({
            "thread": {"id": FRESH_THREAD, "cwd": host.slot},
            "model": CONTROL_MODEL,
            "modelProvider": CONTROL_PROVIDER,
            "reasoningEffort": CONTROL_EFFORT
        })),
    );
    host.server().answer(
        "thread/resume",
        control_endpoint::Answer::Result(json!({
            "thread": {"id": FRESH_THREAD, "cwd": host.slot},
            "model": CONTROL_MODEL,
            "modelProvider": CONTROL_PROVIDER,
            "reasoningEffort": CONTROL_EFFORT
        })),
    );
    host.server().answer(
        "turn/start",
        control_endpoint::Answer::Result(
            json!({"turn": {"id": CONTINUATION_TURN, "status": "inProgress"}}),
        ),
    );
    host.server().answer_sequence(
        "thread/read",
        vec![
            control_endpoint::Answer::Result(json!({"thread": {
                "id": FRESH_THREAD,
                "cwd": host.slot,
                "turns": []
            }})),
            control_endpoint::Answer::Result(json!({"thread": {
                "id": FRESH_THREAD,
                "cwd": host.slot,
                "turns": [{
                    "id": CONTINUATION_TURN,
                    "status": "completed",
                    "items": [{"id": "new", "type": "agentMessage", "text": CONTINUATION_FINAL}]
                }]
            }})),
        ],
    );
    let partial = host.slot.join("partial-work.txt");
    fs::write(&partial, "partial work\n").unwrap();
    let stale = plant_stale_endpoint(&host, CONTROL_THREAD);
    let double = PathBuf::from(env!("CARGO_BIN_EXE_harness-frontend-double"));
    host.register_frontend(&double);
    let session = spawn_hosted(&host);
    wait_for_turn(&host);
    let started = host.server().requests_for("thread/start");
    assert_eq!(
        started.len(),
        1,
        "restart must start one fresh thread: {started:?}"
    );
    let turns = host.server().requests_for("turn/start");
    assert_eq!(turns.len(), 1, "one handoff, not a replay: {turns:?}");
    assert_eq!(turns[0]["params"]["threadId"], FRESH_THREAD);
    assert_eq!(
        turns[0]["params"]["input"][0]["text"], handoff,
        "restart submitted the original assignment instead of the bounded handoff"
    );
    assert_ne!(turns[0]["params"]["input"][0]["text"], CONTROL_ASSIGNMENT);
    let argv = fs::read_to_string(host.home.join("frontend-argv.txt")).unwrap();
    assert!(
        argv.contains("resume") && argv.contains(FRESH_THREAD),
        "{argv}"
    );
    assert!(!argv.contains(CONTROL_THREAD), "{argv}");
    finish_turn(
        &host,
        session,
        FRESH_THREAD,
        CONTINUATION_TURN,
        CONTINUATION_FINAL,
    );
    assert_eq!(
        fs::read_to_string(host.state.join("message-1.txt"))
            .unwrap()
            .trim(),
        CONTINUATION_FINAL
    );
    assert_eq!(fs::read_to_string(&partial).unwrap(), "partial work\n");
    assert_eq!(
        receipt_json(&host.state.join("slot-1.json"))["owner"],
        CONTROL_OWNER
    );
    let observed = receipt_json(&host.receipt);
    assert_eq!(observed["observation"]["session"], FRESH_THREAD);
    assert_eq!(observed["observation"]["previousSession"], CONTROL_THREAD);
    assert_ne!(
        observed["observation"]["session"],
        observed["observation"]["previousSession"]
    );
    assert_eq!(
        observed["originatingLead"]["threadId"], lead_thread,
        "the continuation replaced its originating lead: {observed}"
    );
    assert_eq!(
        observed["originatingLead"]["runGeneration"], "generation-continuation",
        "{observed}"
    );
    assert_ne!(
        observed["observation"]["session"], lead_thread,
        "the successor run was silently reparented to its lead: {observed}"
    );
    assert_successor_endpoint(&host, FRESH_THREAD, &stale);
    assert_frontend_gone(&host, &double);
    assert_backend_released(&host);
}

#[test]
fn exact_session_resume_keeps_the_recorded_session_when_resume_returns_another_thread() {
    let host = ControlHost::new("resume-mismatch");
    write_receipt(&host, &resume_receipt(&host, CONTINUATION_PROMPT));
    host.server().answer(
        "thread/resume",
        control_endpoint::Answer::Result(json!({
            "thread": {"id": FRESH_THREAD, "cwd": host.slot},
            "model": CONTROL_MODEL,
            "modelProvider": CONTROL_PROVIDER,
            "reasoningEffort": CONTROL_EFFORT
        })),
    );
    let partial = host.slot.join("partial-work.txt");
    fs::write(&partial, "partial work\n").unwrap();
    let stale = plant_stale_endpoint(&host, CONTROL_THREAD);
    let out = lead_command()
        .args(["executor", "run", "--file"])
        .arg(&host.receipt)
        .env("CODEX_HOME", &host.home)
        .env("HARNESS_EXECUTOR_FIXTURE_MODE", "render")
        .env_remove("WT_SESSION")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let output = text(&out);
    assert_ne!(out.status.code(), Some(0), "{output}");
    assert!(
        output.contains("another thread") || output.contains("was not resumed"),
        "{output}"
    );
    assert!(
        host.server().requests_for("turn/start").is_empty(),
        "a refused resume submitted the assignment: {:?}",
        host.server().requests()
    );
    assert!(
        host.server().requests_for("thread/start").is_empty(),
        "a refused resume started another conversation: {:?}",
        host.server().requests_for("thread/start")
    );
    let observed = receipt_json(&host.receipt);
    assert!(
        observed["observation"]["session"].is_null(),
        "the wrong thread replaced the recorded session: {observed}"
    );
    assert_eq!(observed["observation"]["previousSession"], CONTROL_THREAD);
    assert!(
        !host.state.join("endpoint-1.json").exists(),
        "a stale or foreign endpoint remained addressable"
    );
    stale_endpoint_untouched(&stale);
    assert_eq!(fs::read_to_string(&partial).unwrap(), "partial work\n");
}

const ACCEPTANCE_ASSIGNMENT: &str =
    "Perform the owned proof command and return its consumed result.";
const ACCEPTANCE_CORRECTION: &str = "Keep the owned proof and do not run the tool again.";
const ACCEPTANCE_RESUME: &str =
    "Continue only the previously authorized task and return its consumed result.";

fn terminate_pid(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return;
        }
        let _ = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
    }
}

struct Evidence {
    path: PathBuf,
}

impl Drop for Evidence {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("native acceptance evidence: {}", self.path.display());
        } else {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

struct AcceptanceRun {
    name: &'static str,
    owner: String,
    session: String,
    source: PathBuf,
    home: PathBuf,
    state: PathBuf,
    receipt: PathBuf,
    slot: PathBuf,
    spawn: Option<Child>,
    resume: Option<Child>,
    responses: Option<native_responses::Responses>,
}

impl Drop for AcceptanceRun {
    fn drop(&mut self) {
        self.stop_processes();
    }
}

impl AcceptanceRun {
    fn launch(root: &Path, name: &'static str, exe: &Path) -> Self {
        let run_root = root.join(name);
        let source = prepare_acceptance_source(&run_root);
        let home = run_root.join("home");
        fs::create_dir_all(&home).unwrap();
        let evidence = home.join("evidence");
        fs::create_dir_all(&evidence).unwrap();
        let responses = native_responses::Responses::with_view_loss(evidence);
        write_acceptance_home(&home, exe, responses.port, &source);
        let owner = format!("acceptance-{name}");
        let stdout = fs::File::create(run_root.join("spawn-stdout.txt")).unwrap();
        let stderr = fs::File::create(run_root.join("spawn-stderr.txt")).unwrap();
        let mut command = lead_command();
        command
            .args([
                "executor",
                "spawn",
                "--source",
                source.to_str().unwrap(),
                "--codex-home",
                home.to_str().unwrap(),
                "--profile",
                "default",
                "--owner",
                &owner,
                "--mode",
                "tui",
                "--exec",
                ACCEPTANCE_ASSIGNMENT,
            ])
            .current_dir(&source)
            .env("CODEX_HOME", &home)
            .env("HARNESS_CONTROL_FIXTURE_KEY", "synthetic-owned-fixture")
            .env_remove("HARNESS_EXECUTOR_FIXTURE_MODE")
            .env_remove("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .env_remove("CODEX_ACCESS_TOKEN")
            .env_remove("OPENAI_BASE_URL")
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr);
        let spawn = command.spawn().expect("executor spawn");
        Self {
            name,
            owner,
            session: String::new(),
            source,
            home,
            state: PathBuf::new(),
            receipt: PathBuf::new(),
            slot: PathBuf::new(),
            spawn: Some(spawn),
            resume: None,
            responses: Some(responses),
        }
    }

    fn wait_until_running(&mut self) {
        let until = Instant::now() + Duration::from_secs(180);
        loop {
            if self.child_exited("spawn") {
                panic!(
                    "{}: executor spawn exited early\n{}",
                    self.name,
                    self.diagnostics()
                );
            }
            if let Some(receipt) = find_receipt(&self.home) {
                self.receipt = receipt;
                self.state = self.receipt.parent().unwrap().to_path_buf();
                let recorded = receipt_json(&self.receipt);
                let state = recorded["observation"]["state"].as_str().unwrap_or("");
                if matches!(state, "failed" | "defect" | "interrupted" | "stopped") {
                    panic!(
                        "{}: run failed before the sequence\n{}",
                        self.name,
                        self.diagnostics()
                    );
                }
                let session = recorded["observation"]["session"].as_str().unwrap_or("");
                self.slot = recorded["slot"]["path"]
                    .as_str()
                    .map(PathBuf::from)
                    .unwrap_or_default();
                if state == "running"
                    && !session.is_empty()
                    && self.slot.join("proof.txt").is_file()
                    && self.state.join("frontend-1.json").is_file()
                    && self.state.join("endpoint-1.json").is_file()
                {
                    self.session = session.to_owned();
                    return;
                }
            }
            assert!(
                Instant::now() < until,
                "{}: native TUI did not reach a running canned assignment\n{}",
                self.name,
                self.diagnostics()
            );
            thread::sleep(Duration::from_millis(200));
        }
    }

    fn session(&self) -> String {
        self.session.clone()
    }

    fn host_alive(&self) -> bool {
        let lease = self.state.join("lease-1.json");
        if !lease.is_file() {
            return false;
        }
        let value = receipt_json(&lease);
        let pid = value["pid"].as_u64().unwrap_or(0) as u32;
        let created = value["created"].as_u64().unwrap_or(0);
        let program = PathBuf::from(value["program"].as_str().unwrap_or(""));
        pid != 0 && !process_gone(pid, created, &program)
    }

    fn child_exited(&mut self, which: &str) -> bool {
        let child = match which {
            "spawn" => self.spawn.as_mut(),
            _ => self.resume.as_mut(),
        };
        child.is_some_and(|child| child.try_wait().unwrap().is_some())
    }

    fn command(&self, args: &[&str]) -> Output {
        let mut command = lead_command();
        command
            .args(args)
            .env("CODEX_HOME", &self.home)
            .env("HARNESS_CONTROL_FIXTURE_KEY", "synthetic-owned-fixture")
            .env_remove("HARNESS_EXECUTOR_FIXTURE_MODE")
            .stdin(Stdio::null());
        command.output().expect("executor command")
    }

    fn address(&self) -> Vec<String> {
        vec![
            "--source".into(),
            self.source.display().to_string(),
            "--codex-home".into(),
            self.home.display().to_string(),
            "--slot".into(),
            "1".into(),
            "--owner".into(),
            self.owner.clone(),
            "--session".into(),
            self.session(),
        ]
    }

    fn watch_while_running(&self) {
        let watched = self.watch(Some("1"));
        let text = text(&watched);
        let recorded = receipt_json(&self.receipt);
        let state = recorded["observation"]["state"].as_str().unwrap_or("");
        assert_eq!(
            watched.status.code(),
            Some(2),
            "{}: a live watch must time out without stopping the run: {text}\n{}",
            self.name,
            self.diagnostics()
        );
        assert_eq!(
            state, "running",
            "{}: watch stopped the run: {text}",
            self.name
        );
        assert!(
            text.contains(&self.session()),
            "{}: watch did not name the session: {text}",
            self.name
        );
    }

    fn watch(&self, timeout: Option<&str>) -> Output {
        let mut args = vec![
            "executor".to_owned(),
            "watch".to_owned(),
            "--source".to_owned(),
            self.source.display().to_string(),
            "--codex-home".to_owned(),
            self.home.display().to_string(),
            "--slot".to_owned(),
            "1".to_owned(),
            "--owner".to_owned(),
            self.owner.clone(),
        ];
        if let Some(timeout) = timeout {
            args.push("--timeout".to_owned());
            args.push(timeout.to_owned());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.command(&refs)
    }

    fn message(&self) {
        let mut args = vec!["executor".into(), "message".into()];
        args.extend(self.address());
        args.extend(["--text".into(), ACCEPTANCE_CORRECTION.into()]);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let sent = self.command(&refs);
        let body = text(&sent);
        assert!(
            sent.status.success() && (body.contains("delivered") || body.contains("queued")),
            "{}: message was not delivered to the live native thread: {body}\n{}",
            self.name,
            self.diagnostics()
        );
    }

    fn stop(&self) {
        let mut args = vec!["executor".into(), "stop".into()];
        args.extend(self.address());
        args.extend(["--timeout".into(), "45".into()]);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let stopped = self.command(&refs);
        let body = text(&stopped);
        assert!(
            stopped.status.success(),
            "{}: executor stop failed: {body}\n{}",
            self.name,
            self.diagnostics()
        );
        assert!(
            body.contains("stopped") || body.contains("interrupted"),
            "{}: stop did not report the run outcome: {body}",
            self.name
        );
    }

    fn close_frontend(&self) {
        let path = self.state.join("frontend-1.json");
        let phases = fs::read_to_string(&path).unwrap_or_default();
        let Some(last) = phases.lines().rev().find(|line| !line.is_empty()) else {
            panic!("{}: no frontend record\n{}", self.name, self.diagnostics());
        };
        let value: Value = serde_json::from_str(last).unwrap();
        let pid = value["pid"].as_u64().unwrap() as u32;
        let created = value["creationTime"].as_u64().unwrap();
        let program = PathBuf::from(value["program"].as_str().unwrap());
        if !process_gone(pid, created, &program) {
            terminate_pid(pid);
        }
        let until = Instant::now() + Duration::from_secs(15);
        while !process_gone(pid, created, &program) && Instant::now() < until {
            thread::sleep(Duration::from_millis(100));
        }
        assert!(
            process_gone(pid, created, &program),
            "{}: manual TUI closure left frontend pid {pid} running",
            self.name
        );
    }

    fn resume(&mut self) {
        let run_root = self.home.parent().unwrap();
        let stdout = fs::File::create(run_root.join("resume-stdout.txt")).unwrap();
        let stderr = fs::File::create(run_root.join("resume-stderr.txt")).unwrap();
        let mut args = vec![
            "executor".to_owned(),
            "resume".to_owned(),
            "--exec".to_owned(),
            ACCEPTANCE_RESUME.to_owned(),
        ];
        args.extend(self.address());
        let mut command = lead_command();
        command
            .args(&args)
            .env("CODEX_HOME", &self.home)
            .env("HARNESS_CONTROL_FIXTURE_KEY", "synthetic-owned-fixture")
            .env_remove("HARNESS_EXECUTOR_FIXTURE_MODE")
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr);
        self.resume = Some(command.spawn().expect("executor resume"));
    }

    fn wait_resumed(&mut self) {
        let until = Instant::now() + Duration::from_secs(180);
        loop {
            if self.child_exited("resume") {
                let status = self.resume.as_mut().unwrap().wait().unwrap();
                assert_eq!(
                    status.code(),
                    Some(0),
                    "{}: resume host did not close successfully\n{}",
                    self.name,
                    self.diagnostics()
                );
                return;
            }
            if self.receipt.is_file() {
                let recorded = receipt_json(&self.receipt);
                let state = recorded["observation"]["state"].as_str().unwrap_or("");
                if matches!(state, "failed" | "defect") {
                    panic!("{}: resumed run failed\n{}", self.name, self.diagnostics());
                }
            }
            assert!(
                Instant::now() < until,
                "{}: exact-session resume did not finish\n{}",
                self.name,
                self.diagnostics()
            );
            thread::sleep(Duration::from_millis(200));
        }
    }

    /// The owned console is a pseudoconsole on this host, so another process
    /// cannot attach and scrape it. The attached native frontend and the event
    /// stream it renders are the readable content.
    fn native_content(&self) -> String {
        let phases = fs::read_to_string(self.state.join("frontend-1.json")).unwrap_or_default();
        let attached = phases
            .lines()
            .rev()
            .find(|line| line.contains("\"attached\""));
        assert!(
            attached.is_some(),
            "{}: native frontend did not attach\n{}",
            self.name,
            self.diagnostics()
        );
        let value: Value = serde_json::from_str(attached.unwrap()).unwrap();
        assert_eq!(value["threadId"].as_str(), Some(self.session.as_str()));
        assert!(
            value["alive"] == true,
            "{}: attached frontend is not alive: {value}",
            self.name
        );
        let titles = visible_window_titles();
        assert!(
            titles.iter().any(|title| title.contains(&self.owner)),
            "{}: native TUI surface is not visible: {titles:?}\n{}",
            self.name,
            self.diagnostics()
        );
        fs::read_to_string(self.state.join("stream-1.jsonl")).unwrap_or_default()
    }

    fn stop_processes(&mut self) {
        for name in ["lease-1.json", "endpoint-1.json"] {
            let path = self.state.join(name);
            if !path.is_file() {
                continue;
            }
            let value = receipt_json(&path);
            let process = if name.starts_with("endpoint") {
                &value["process"]
            } else {
                &value
            };
            if let Some(pid) = process["pid"].as_u64() {
                terminate_pid(pid as u32);
            }
        }
        if let Some(mut child) = self.resume.take() {
            let _ = child.kill();
        }
        if let Some(mut child) = self.spawn.take() {
            let _ = child.kill();
        }
        self.responses.take();
    }

    fn diagnostics(&self) -> String {
        let run_root = self.home.parent().unwrap_or(&self.home);
        let mut parts = Vec::new();
        for name in [
            "spawn-stdout.txt",
            "spawn-stderr.txt",
            "resume-stdout.txt",
            "resume-stderr.txt",
        ] {
            let path = run_root.join(name);
            if path.is_file() {
                let text = fs::read_to_string(&path).unwrap_or_default();
                let tail = text.chars().rev().take(1500).collect::<String>();
                let tail = tail.chars().rev().collect::<String>();
                parts.push(format!("{name}: {tail}"));
            }
        }
        if self.receipt.is_file() {
            let recorded = receipt_json(&self.receipt);
            parts.push(format!(
                "state={} session={} cause={}",
                recorded["observation"]["state"],
                recorded["observation"]["session"],
                recorded["observation"]["cause"]
            ));
        }
        let log = self.state.join("endpoint-1.log");
        if log.is_file() {
            let text = fs::read_to_string(&log).unwrap_or_default();
            let tail = text.chars().rev().take(1500).collect::<String>();
            parts.push(format!(
                "endpoint log: {}",
                tail.chars().rev().collect::<String>()
            ));
        }
        parts.join("\n")
    }
}

fn prepare_acceptance_source(root: &Path) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    let bare = root.join("remote.git");
    let seed = root.join("seed");
    let source = root.join("proj");
    git(
        root,
        &[
            "init",
            "--bare",
            "-q",
            "--initial-branch=main",
            bare.to_str().unwrap(),
        ],
    );
    git(
        root,
        &[
            "init",
            "-q",
            "--initial-branch=main",
            seed.to_str().unwrap(),
        ],
    );
    git(&seed, &["config", "user.email", "executor@example.test"]);
    git(&seed, &["config", "user.name", "Executor"]);
    fs::write(seed.join("README.md"), "seed\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-qm", "seed"]);
    git(&seed, &["remote", "add", "origin", &file_url(&bare)]);
    git(&seed, &["push", "-q", "origin", "main"]);
    git(
        root,
        &["clone", "-q", &file_url(&bare), source.to_str().unwrap()],
    );
    git(&source, &["config", "user.email", "executor@example.test"]);
    git(&source, &["config", "user.name", "Executor"]);
    fs::create_dir_all(source.join("global")).unwrap();
    fs::write(
        source.join("global/orchestration.toml"),
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"default\"\nexecutor_profiles = [\"default\"]\nmax_concurrent_executors = 1\n",
    )
    .unwrap();
    source
}

fn write_acceptance_home(home: &Path, exe: &Path, port: u16, source: &Path) {
    let launch = home.join("harness/bin");
    fs::create_dir_all(&launch).unwrap();
    let launcher = launch.join("codex.exe");
    if fs::hard_link(exe, &launcher).is_err() {
        fs::copy(exe, &launcher).unwrap();
    }
    fs::write(
        home.join("harness/native-launch.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 2,
            "upstream": {"executable": exe}
        }))
        .unwrap(),
    )
    .unwrap();
    let trusted = source.to_string_lossy().to_lowercase();
    fs::write(
        home.join("config.toml"),
        format!(
            "model = \"gpt-6-astra\"\nmodel_reasoning_effort = \"low\"\nmodel_provider = \"control_fixture\"\napproval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\ncli_auth_credentials_store = \"file\"\n[model_providers.control_fixture]\nname = \"Owned observation fixture\"\nbase_url = \"http://127.0.0.1:{port}/v1\"\nwire_api = \"responses\"\nenv_key = \"HARNESS_CONTROL_FIXTURE_KEY\"\nrequires_openai_auth = false\nrequest_max_retries = 0\nstream_max_retries = 0\nsupports_websockets = false\n[analytics]\nenabled = false\n[projects.'{trusted}']\ntrust_level = \"trusted\"\n"
        ),
    )
    .unwrap();
}

fn find_receipt(home: &Path) -> Option<PathBuf> {
    let pool = home.join("harness/executor-pool");
    let entries = fs::read_dir(&pool).ok()?;
    for entry in entries.flatten() {
        let receipt = entry.path().join("spawn-1.json");
        if receipt.is_file() {
            return Some(receipt);
        }
    }
    None
}

fn assert_readable(name: &str, screen: &str) {
    let shown = screen.chars().take(2000).collect::<String>();
    assert!(
        screen.contains(ACCEPTANCE_ASSIGNMENT)
            && (screen.contains("proof.txt") || screen.contains("commandExecution")),
        "{name}: native content did not include the assignment and tool activity: {shown}"
    );
}

fn visible_window_titles() -> Vec<String> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };
    unsafe extern "system" fn collect(window: HWND, parameter: LPARAM) -> i32 {
        let titles = unsafe { &mut *(parameter as *mut Vec<String>) };
        if unsafe { IsWindowVisible(window) } == 0 {
            return 1;
        }
        let length = unsafe { GetWindowTextLengthW(window) };
        if length <= 0 {
            return 1;
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let read = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
        if read > 0 {
            titles.push(String::from_utf16_lossy(&buffer[..read as usize]));
        }
        1
    }
    let mut titles = Vec::new();
    unsafe {
        EnumWindows(Some(collect), &mut titles as *mut Vec<String> as LPARAM);
    }
    titles
}

/// Two isolated runs of the real spawn/watch/message/stop/manual-close/resume
/// sequence. Both use the installed Codex TUI and app-server with canned
/// Responses. Protocol mocks and conversation screenshots are not used.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; two isolated native TUI runs and canned responses"]
fn native_acceptance_runs_the_command_sequence_twice_in_isolation() {
    let exe = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    assert!(exe.is_absolute() && exe.is_file(), "{}", exe.display());
    let version = Command::new(&exe).arg("--version").output().unwrap();
    let version = String::from_utf8_lossy(&version.stdout);
    eprintln!("codex version: {}", version.trim());
    let root = std::env::temp_dir().join(format!("native-acceptance-{}", std::process::id()));
    let _evidence = Evidence { path: root.clone() };
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("codex-version.txt"), version.trim().as_bytes()).unwrap();

    let mut first = AcceptanceRun::launch(&root, "run-a", &exe);
    let mut second = AcceptanceRun::launch(&root, "run-b", &exe);
    first.wait_until_running();
    second.wait_until_running();
    assert_ne!(
        first.session(),
        second.session(),
        "isolated runs shared a session"
    );
    assert_ne!(first.home, second.home);
    let first_screen = first.native_content();
    let second_screen = second.native_content();
    assert_readable(first.name, &first_screen);
    assert_readable(second.name, &second_screen);

    std::thread::scope(|scope| {
        scope.spawn(|| first.watch_while_running());
        scope.spawn(|| second.watch_while_running());
    });
    std::thread::scope(|scope| {
        scope.spawn(|| first.message());
        scope.spawn(|| second.message());
    });
    assert!(second.host_alive(), "neighbor was already gone before stop");
    first.stop();
    first.close_frontend();
    assert!(
        second.host_alive() && !second.child_exited("spawn"),
        "stopping and closing one TUI stopped the neighboring run\n{}",
        second.diagnostics()
    );
    second.stop();
    second.close_frontend();

    first.resume();
    first.wait_resumed();
    second.resume();
    second.wait_resumed();
    for run in [&first, &second] {
        assert_completed(run);
    }
}

fn assert_completed(run: &AcceptanceRun) {
    let proof = fs::read_to_string(run.slot.join("proof.txt")).unwrap_or_default();
    assert_eq!(
        proof, "one",
        "{}: assignment effect was not exactly one write: {proof:?}",
        run.name
    );
    let recorded = receipt_json(&run.receipt);
    let session = recorded["observation"]["session"].as_str().unwrap_or("");
    assert!(
        !session.is_empty(),
        "{}: missing session identity",
        run.name
    );
    assert_eq!(
        recorded["observation"]["state"], "completed",
        "{}: {recorded}",
        run.name
    );
    let result = fs::read_to_string(run.state.join("message-1.txt")).unwrap_or_default();
    assert!(
        result.contains(native_responses::FINAL),
        "{}: persisted result {result:?} does not agree with the canned final",
        run.name
    );
    let watched = run.watch(None);
    let watched_text = text(&watched);
    assert_eq!(
        watched.status.code(),
        Some(0),
        "{}: watch disagreed with completion: {watched_text}",
        run.name
    );
    assert!(
        watched_text.contains(native_responses::FINAL) && watched_text.contains(session),
        "{}: watch result/identity disagreed: {watched_text}",
        run.name
    );
    let endpoint = run.state.join("endpoint-1.json");
    if endpoint.is_file() {
        let value = receipt_json(&endpoint);
        if let Some(thread) = value["threadId"].as_str() {
            assert_eq!(thread, session, "{}: endpoint thread disagrees", run.name);
        }
        if let Some(pid) = value["process"]["pid"].as_u64() {
            let created = value["process"]["creationTime"].as_u64().unwrap_or(0);
            let program = PathBuf::from(value["process"]["program"].as_str().unwrap_or(""));
            assert!(
                process_gone(pid as u32, created, &program),
                "{}: automatic closure left the app-server running",
                run.name
            );
        }
    }
}
