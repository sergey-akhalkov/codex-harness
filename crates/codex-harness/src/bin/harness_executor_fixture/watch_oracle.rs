//! Candidate-independent native acceptance for the finalized `executor watch`
//! contract, driven through the workspace's own built CLI.
//!
//! `harness-executor-fixture watch-oracle --workspace DIR [--exe PROGRAM]`
//! seeds private native receipts and a live host stand-in, then runs the real
//! `executor watch` entry point of the workspace's `codex-harness` executable
//! against them. The verdict comes from observed exit status, the printed
//! review report and the fidelity of the private receipt after the call -
//! never from candidate source, candidate tests, candidate-produced records
//! or a success marker. The executable must be an ordinary file newer than
//! every Rust source of the workspace, so a missing or skipped build is
//! rejected before any behavior check.
//!
//! Cases:
//! - `built-executable`: ordinary workspace executable, newer than its sources;
//! - `cli-identity`: `--version` and the documented unavailable result for a
//!   receipt that was never written;
//! - `failed-run-stays-failure`: a failed run is not reported as success;
//! - `finalized-run-returns-result-text`/`-json`: an already finalized run
//!   returns success with the recorded bounded result in both output modes;
//! - `pending-finalization-timeout-text`/`-json`: a completed native turn whose
//!   host exit status and result are not yet recorded never returns success,
//!   keeps observing to the declared timeout and leaves the run untouched;
//! - `finalization-during-watch-text`/`-json`: the same call returns the actual
//!   bounded result when the host finalizes the run while it observes;
//! - `finalized-without-result-not-success`: a recorded successful exit
//!   without a retained result is never reported as success.
//!
//! Exit code 0 means every case passed; 1 means the JSON report on stdout
//! names at least one failed or skipped case.

use harness_core::{
    build_identity,
    process_service::{self, ServiceProcess},
};
use serde_json::{Value, json};
use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const USAGE: &str = "\
harness-executor-fixture watch-oracle [--workspace DIRECTORY] [--exe PROGRAM]
  Candidate-independent acceptance for the finalized executor watch contract.
  Runs the workspace's built codex-harness through the real `executor watch`
  entry point against private controlled receipts and a live host stand-in.
  Exit 0 only when every case passed; the JSON report names each case.";

/// Exact native session identity the controlled receipts record.
const SESSION: &str = "01a0c719-f4d4-7880-a9d2-1a96ee0f3f10";
/// The bounded result the controlled finalization writes and watch must return.
const FINAL_RESULT: &str =
    "WATCH_ORACLE_FINALIZED\nremaining: none\nchecks: controlled native finalization";
/// Declared watch timeout of the pending-finalization cases.
const PENDING_TIMEOUT_SECS: u64 = 4;
/// Declared watch timeout of the late-finalization cases.
const FINALIZE_TIMEOUT_SECS: u64 = 20;
/// Declared watch timeout of the finalized-without-result case.
const DEFECT_TIMEOUT_SECS: u64 = 6;
/// How long a late-finalization call observes before the host finalizes.
const FINALIZE_AFTER: Duration = Duration::from_millis(800);
/// Watch poll interval passed to the candidate CLI.
const POLL_MS: &str = "50";
/// Clock tolerance when asserting a call reached its declared timeout.
const TIMEOUT_SLACK_MS: u64 = 500;
/// Session identity of the surrounding caller must not leak to the CLI.
const CLEARED_ENV: [&str; 13] = [
    "HARNESS_EXECUTOR_SESSION",
    "HARNESS_EXECUTOR_RUN",
    "HARNESS_ORIGINATING_LEAD",
    "CODEX_SESSION_ID",
    "CODEX_THREAD_ID",
    "CODEX_CI",
    "WT_SESSION",
    "HARNESS_EXECUTOR_FIXTURE_MODE",
    "HARNESS_EXECUTOR_FIXTURE_SESSION",
    "HARNESS_EXECUTOR_FIXTURE_STARTED",
    "HARNESS_EXECUTOR_FIXTURE_CHILD_MARKER",
    "HARNESS_EXECUTOR_FIXTURE_RELEASE",
    "HARNESS_EXECUTOR_FIXTURE_DELAY_MS",
];

const CASE_BUILD: &str = "built-executable";
const CASE_IDENTITY: &str = "cli-identity";
const CASE_FAILED: &str = "failed-run-stays-failure";
const CASE_FINALIZED_TEXT: &str = "finalized-run-returns-result-text";
const CASE_FINALIZED_JSON: &str = "finalized-run-returns-result-json";
const CASE_PENDING_TEXT: &str = "pending-finalization-timeout-text";
const CASE_PENDING_JSON: &str = "pending-finalization-timeout-json";
const CASE_DURING_TEXT: &str = "finalization-during-watch-text";
const CASE_DURING_JSON: &str = "finalization-during-watch-json";
const CASE_DEFECT: &str = "finalized-without-result-not-success";

const SKIPPED_AFTER_IDENTITY: [&str; 8] = [
    CASE_FAILED,
    CASE_FINALIZED_TEXT,
    CASE_FINALIZED_JSON,
    CASE_PENDING_TEXT,
    CASE_PENDING_JSON,
    CASE_DURING_TEXT,
    CASE_DURING_JSON,
    CASE_DEFECT,
];

pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    let mut workspace: Option<PathBuf> = None;
    let mut executable: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        let key = args[index]
            .to_str()
            .ok_or_else(|| invalid("invalid watch-oracle option"))?;
        match key {
            "--help" => {
                println!("{USAGE}");
                return Ok(0);
            }
            "--workspace" | "--exe" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| invalid(&format!("watch-oracle {key} needs a path")))?;
                if key == "--workspace" {
                    workspace = Some(PathBuf::from(value));
                } else {
                    executable = Some(PathBuf::from(value));
                }
                index += 2;
            }
            _ => return Err(invalid(&format!("invalid watch-oracle option: {key}"))),
        }
    }
    let workspace = match workspace {
        Some(path) => path,
        None => env::current_dir()?,
    };
    let workspace = workspace.canonicalize().map_err(|error| {
        invalid(&format!(
            "watch-oracle workspace {} is not readable: {error}",
            workspace.display()
        ))
    })?;
    if !workspace.is_dir() || !workspace.join("crates").is_dir() {
        return Err(invalid(
            "watch-oracle --workspace must be the root of a codex-harness checkout",
        ));
    }
    let (resolved, mut cases) = match resolve(&workspace, executable.as_deref()) {
        Ok(resolved) => {
            let detail = resolved.detail.clone();
            (Some(resolved), vec![Case::pass(CASE_BUILD, detail)])
        }
        Err(cause) => (None, vec![Case::fail(CASE_BUILD, cause)]),
    };
    if let Some(exe) = resolved.as_ref().map(|resolved| &resolved.path) {
        if let Some(cause) = identity_case(exe) {
            cases.push(Case::fail(CASE_IDENTITY, cause));
            for id in SKIPPED_AFTER_IDENTITY {
                cases.push(Case::skip(
                    id,
                    "not run: the executable did not prove it is this CLI",
                ));
            }
        } else {
            cases.push(Case::pass(
                CASE_IDENTITY,
                "the executable answers --version and the documented unavailable result",
            ));
            cases.push(failed_run_case(exe));
            cases.push(finalized_case(exe, OutputMode::Text));
            cases.push(finalized_case(exe, OutputMode::Json));
            cases.push(pending_case(exe, OutputMode::Text));
            cases.push(pending_case(exe, OutputMode::Json));
            cases.push(during_watch_case(exe, OutputMode::Text));
            cases.push(during_watch_case(exe, OutputMode::Json));
            cases.push(defect_case(exe));
        }
    }
    let passed = cases.iter().all(|case| case.passed());
    let report = json!({
        "schema": 1,
        "kind": "executor-watch-oracle",
        "workspace": workspace.to_string_lossy(),
        "executable": resolved.as_ref().map(|resolved| resolved.path.to_string_lossy().into_owned()),
        "executableSha256": resolved.as_ref().map(|resolved| resolved.sha256.clone()),
        "cases": cases.iter().map(Case::json).collect::<Vec<_>>(),
        "passed": passed,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(io::Error::other)?
    );
    Ok(if passed { 0 } else { 1 })
}

/// One acceptance case: its verdict and the observed cause when it failed.
struct Case {
    id: &'static str,
    verdict: Verdict,
    detail: String,
}

enum Verdict {
    Passed,
    Failed,
    Skipped,
}

impl Case {
    fn pass(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            verdict: Verdict::Passed,
            detail: detail.into(),
        }
    }

    fn fail(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            verdict: Verdict::Failed,
            detail: detail.into(),
        }
    }

    fn skip(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            verdict: Verdict::Skipped,
            detail: detail.into(),
        }
    }

    fn passed(&self) -> bool {
        matches!(self.verdict, Verdict::Passed)
    }

    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "passed": self.passed(),
            "skipped": matches!(self.verdict, Verdict::Skipped),
            "detail": self.detail,
        })
    }
}

/// The workspace's own built CLI: an ordinary executable no older than any
/// Rust source that could have produced it.
struct Resolved {
    path: PathBuf,
    sha256: String,
    detail: String,
}

fn resolve(workspace: &Path, explicit: Option<&Path>) -> Result<Resolved, String> {
    let candidates: Vec<PathBuf> = match explicit {
        Some(path) => vec![absolute(workspace, path)],
        None => vec![
            workspace
                .join("target")
                .join("debug")
                .join("codex-harness.exe"),
            workspace
                .join("target")
                .join("release")
                .join("codex-harness.exe"),
        ],
    };
    let mut existing: Vec<(PathBuf, SystemTime)> = Vec::new();
    for path in candidates {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        if let Err(error) = build_identity::ordinary(&path) {
            return Err(format!(
                "the candidate executable {} is not an ordinary file: {error}",
                path.display()
            ));
        }
        existing.push((path, metadata.modified().unwrap_or(UNIX_EPOCH)));
    }
    existing.sort_by_key(|entry| entry.1);
    let Some((path, built)) = existing.pop() else {
        return Err(
            "no built executable at target/debug/codex-harness.exe or target/release/codex-harness.exe; \
             the workspace build was not run or did not produce the CLI"
                .to_owned(),
        );
    };
    if let Some((source, changed)) = newest_source(workspace)
        && changed > built
    {
        return Err(format!(
            "the built executable {} is older than {}; the workspace build was skipped after the last source change",
            path.display(),
            source.display()
        ));
    }
    let path = path.canonicalize().map_err(|error| {
        format!(
            "the candidate executable {} is unreadable: {error}",
            path.display()
        )
    })?;
    let sha256 = build_identity::hash_file(&path).map_err(|error| {
        format!(
            "the candidate executable {} is unreadable: {error}",
            path.display()
        )
    })?;
    Ok(Resolved {
        detail: format!("{} sha256 {sha256}", path.display()),
        path,
        sha256,
    })
}

/// The newest Rust source or manifest that a build of the CLI must postdate.
/// Only inputs of the CLI build are considered: crate sources and manifests.
/// A newer integration test, benchmark, example or sibling binary does not
/// make the built executable stale, because the CLI does not compile them.
/// Neighboring workspace crates are not resolved through the dependency
/// graph here; the reported file names the exact cause for a human decision.
fn newest_source(workspace: &Path) -> Option<(PathBuf, SystemTime)> {
    let mut newest: Option<(PathBuf, SystemTime)> = None;
    let mut stack = vec![workspace.join("crates")];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let name = entry.file_name();
                let sibling_binary = name == "bin"
                    && directory.file_name().and_then(|name| name.to_str()) == Some("src");
                if !sibling_binary
                    && !matches!(
                        name.to_str(),
                        Some("target" | ".git" | "node_modules" | "tests" | "benches" | "examples")
                    )
                {
                    stack.push(path);
                }
                continue;
            }
            if !kind.is_file() || !is_source(&path) {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else {
                continue;
            };
            if newest
                .as_ref()
                .is_none_or(|(_, current)| modified > *current)
            {
                newest = Some((path, modified));
            }
        }
    }
    for name in ["Cargo.toml", "Cargo.lock"] {
        let path = workspace.join(name);
        let Ok(modified) = fs::metadata(&path).and_then(|metadata| metadata.modified()) else {
            continue;
        };
        if newest
            .as_ref()
            .is_none_or(|(_, current)| modified > *current)
        {
            newest = Some((path, modified));
        }
    }
    newest
}

fn is_source(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("rs")
        || matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some("Cargo.toml" | "Cargo.lock")
        )
}

/// The executable must be the CLI it claims to be: a forged success stub that
/// does not answer the real command line is rejected before any behavior case.
fn identity_case(exe: &Path) -> Option<String> {
    let private = tempfile::tempdir().ok()?;
    let version = match invoke(
        exe,
        private.path(),
        &[OsString::from("--version")],
        Duration::from_secs(30),
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            return Some(format!(
                "could not run {} --version: {error}",
                exe.display()
            ));
        }
    };
    if version.code != Some(0) || !version.output.contains("codex-harness ") {
        return Some(format!(
            "{} --version exited {:?} without identifying the codex-harness CLI: {}",
            exe.display(),
            version.code,
            excerpt(&version.output, 300)
        ));
    }
    let missing = private.path().join("never-written-receipt.json");
    let probe = match invoke(
        exe,
        private.path(),
        &watch_arguments(&missing, 1, OutputMode::Text),
        Duration::from_secs(30),
    ) {
        Ok(outcome) => outcome,
        Err(error) => return Some(format!("could not run executor watch: {error}")),
    };
    if probe.code != Some(2) || !probe.output.contains("receipt") {
        return Some(format!(
            "a receipt that was never written must return the documented unavailable result (2); \
             observed exit {:?}: {}",
            probe.code,
            excerpt(&probe.output, 300)
        ));
    }
    None
}

/// A recorded failure stays a failure: the CLI must not report success.
fn failed_run_case(exe: &Path) -> Case {
    let fixture = match Fixture::start() {
        Ok(fixture) => fixture,
        Err(error) => return Case::fail(CASE_FAILED, format!("could not seed the run: {error}")),
    };
    if let Err(error) = fixture.write("failed", Some(19), Some("fixture failure")) {
        return Case::fail(CASE_FAILED, format!("could not seed the receipt: {error}"));
    }
    let outcome = match invoke(
        exe,
        fixture.directory(),
        &watch_arguments(&fixture.receipt, 4, OutputMode::Text),
        Duration::from_secs(20),
    ) {
        Ok(outcome) => outcome,
        Err(error) => return Case::fail(CASE_FAILED, format!("watch did not run: {error}")),
    };
    if outcome.code != Some(1) {
        return Case::fail(
            CASE_FAILED,
            format!(
                "a failed run must return 1; observed exit {:?} after {:.1}s: {}",
                outcome.code,
                outcome.elapsed.as_secs_f64(),
                excerpt(&outcome.output, 300)
            ),
        );
    }
    Case::pass(CASE_FAILED, "a failed run still returns 1")
}

/// An already finalized run returns success with the recorded bounded result.
fn finalized_case(exe: &Path, mode: OutputMode) -> Case {
    let id = match mode {
        OutputMode::Text => CASE_FINALIZED_TEXT,
        OutputMode::Json => CASE_FINALIZED_JSON,
    };
    let fixture = match Fixture::start() {
        Ok(fixture) => fixture,
        Err(error) => return Case::fail(id, format!("could not seed the run: {error}")),
    };
    if let Err(error) = fixture.finalize() {
        return Case::fail(id, format!("could not seed the finalized run: {error}"));
    }
    let outcome = match invoke(
        exe,
        fixture.directory(),
        &watch_arguments(&fixture.receipt, 6, mode),
        Duration::from_secs(20),
    ) {
        Ok(outcome) => outcome,
        Err(error) => return Case::fail(id, format!("watch did not run: {error}")),
    };
    if outcome.code != Some(0) {
        return Case::fail(
            id,
            format!(
                "a finalized success must return 0; observed exit {:?} after {:.1}s: {}",
                outcome.code,
                outcome.elapsed.as_secs_f64(),
                excerpt(&outcome.output, 300)
            ),
        );
    }
    match mode {
        OutputMode::Text => {
            if !outcome.output.contains(FINAL_RESULT) {
                return Case::fail(
                    id,
                    format!(
                        "the recorded bounded result is missing from the text report: {}",
                        excerpt(&outcome.output, 300)
                    ),
                );
            }
        }
        OutputMode::Json => {
            let report = match parse_report(&outcome) {
                Ok(report) => report,
                Err(cause) => return Case::fail(id, cause),
            };
            if report["exitCode"] != json!(0) || report["session"] != json!(SESSION) {
                return Case::fail(
                    id,
                    format!(
                        "the JSON report must name the finalized exit 0 and session {SESSION}: {}",
                        excerpt(&outcome.output, 300)
                    ),
                );
            }
            if !report["returned"]
                .as_str()
                .is_some_and(|returned| returned.contains(FINAL_RESULT))
            {
                return Case::fail(
                    id,
                    format!(
                        "the recorded bounded result is missing from the JSON report: {}",
                        excerpt(&outcome.output, 300)
                    ),
                );
            }
        }
    }
    Case::pass(id, "the finalized run returned its recorded bounded result")
}

/// The baseline defect: a completed native turn whose host exit status and
/// result are still pending must never be reported as success, must keep
/// observing to the declared timeout and must leave the run untouched.
fn pending_case(exe: &Path, mode: OutputMode) -> Case {
    let id = match mode {
        OutputMode::Text => CASE_PENDING_TEXT,
        OutputMode::Json => CASE_PENDING_JSON,
    };
    let mut fixture = match Fixture::start() {
        Ok(fixture) => fixture,
        Err(error) => return Case::fail(id, format!("could not seed the run: {error}")),
    };
    if let Err(error) = fixture.write("completed", None, None) {
        return Case::fail(id, format!("could not seed the receipt: {error}"));
    }
    let Ok(before) = fs::read(&fixture.receipt) else {
        return Case::fail(id, "could not read the seeded receipt");
    };
    let Ok(listed) = listing(fixture.directory()) else {
        return Case::fail(id, "could not list the private run directory");
    };
    let outcome = match invoke(
        exe,
        fixture.directory(),
        &watch_arguments(&fixture.receipt, PENDING_TIMEOUT_SECS, mode),
        Duration::from_secs(PENDING_TIMEOUT_SECS + 10),
    ) {
        Ok(outcome) => outcome,
        Err(error) => return Case::fail(id, format!("watch did not run: {error}")),
    };
    if !outcome.finished {
        return Case::fail(
            id,
            format!(
                "watch did not return within its declared {PENDING_TIMEOUT_SECS}s timeout and had to be ended"
            ),
        );
    }
    if outcome.code != Some(2) {
        return Case::fail(
            id,
            format!(
                "a completed turn with a pending host exit and no result must not return success; \
                 observed exit {:?} after {:.1}s: {}",
                outcome.code,
                outcome.elapsed.as_secs_f64(),
                excerpt(&outcome.output, 300)
            ),
        );
    }
    let declared = Duration::from_secs(PENDING_TIMEOUT_SECS);
    let slack = Duration::from_millis(TIMEOUT_SLACK_MS);
    if outcome.elapsed + slack < declared {
        return Case::fail(
            id,
            format!(
                "the call returned after {:.1}s instead of reaching the declared {PENDING_TIMEOUT_SECS}s timeout",
                outcome.elapsed.as_secs_f64()
            ),
        );
    }
    match fixture.host.try_wait() {
        Ok(None) => {}
        Ok(Some(status)) => {
            return Case::fail(
                id,
                format!("the timeout stopped the run: its host exited {status}"),
            );
        }
        Err(error) => {
            return Case::fail(id, format!("could not inspect the run's host: {error}"));
        }
    }
    if fs::read(&fixture.receipt).ok().as_deref() != Some(before.as_slice()) {
        return Case::fail(
            id,
            "the timeout mutated the run's receipt instead of preserving it",
        );
    }
    if listing(fixture.directory()).ok().as_deref() != Some(listed.as_slice()) {
        return Case::fail(
            id,
            "the timeout left new files beside the run instead of leaving it untouched",
        );
    }
    if mode == OutputMode::Json {
        let report = match parse_report(&outcome) {
            Ok(report) => report,
            Err(cause) => return Case::fail(id, cause),
        };
        if !report["exitCode"].is_null() || !report["returned"].is_null() {
            return Case::fail(
                id,
                format!(
                    "the timeout report claims an exit or result for a run that recorded neither: {}",
                    excerpt(&outcome.output, 300)
                ),
            );
        }
    }
    Case::pass(
        id,
        format!(
            "stayed non-success for the declared {PENDING_TIMEOUT_SECS}s, preserved the run and its host"
        ),
    )
}

/// The host finalizes the same run while the call observes: the result must
/// reach the caller with the exact run identity.
fn during_watch_case(exe: &Path, mode: OutputMode) -> Case {
    let id = match mode {
        OutputMode::Text => CASE_DURING_TEXT,
        OutputMode::Json => CASE_DURING_JSON,
    };
    let fixture = match Fixture::start() {
        Ok(fixture) => fixture,
        Err(error) => return Case::fail(id, format!("could not seed the run: {error}")),
    };
    if let Err(error) = fixture.write("completed", None, None) {
        return Case::fail(id, format!("could not seed the receipt: {error}"));
    }
    let arguments = watch_arguments(&fixture.receipt, FINALIZE_TIMEOUT_SECS, mode);
    let started = Instant::now();
    let child = match spawn(exe, fixture.directory(), &arguments) {
        Ok(child) => child,
        Err(error) => return Case::fail(id, format!("watch did not start: {error}")),
    };
    thread::sleep(FINALIZE_AFTER);
    if let Err(error) = fixture.finalize() {
        return Case::fail(
            id,
            format!("could not finalize the run during the watch: {error}"),
        );
    }
    let outcome = match wait(
        child,
        started,
        Duration::from_secs(FINALIZE_TIMEOUT_SECS + 10),
    ) {
        Ok(outcome) => outcome,
        Err(error) => return Case::fail(id, format!("watch did not run: {error}")),
    };
    if !outcome.finished {
        return Case::fail(
            id,
            format!(
                "watch did not return within its declared {FINALIZE_TIMEOUT_SECS}s timeout after finalization"
            ),
        );
    }
    if outcome.code != Some(0) {
        return Case::fail(
            id,
            format!(
                "finalization during the watch must return success; observed exit {:?} after {:.1}s: {}",
                outcome.code,
                outcome.elapsed.as_secs_f64(),
                excerpt(&outcome.output, 300)
            ),
        );
    }
    if outcome.elapsed < FINALIZE_AFTER {
        return Case::fail(
            id,
            format!(
                "the call returned after {:.1}s, before the host finalized at {:.1}s",
                outcome.elapsed.as_secs_f64(),
                FINALIZE_AFTER.as_secs_f64()
            ),
        );
    }
    match mode {
        OutputMode::Text => {
            if !outcome.output.contains(FINAL_RESULT) {
                return Case::fail(
                    id,
                    format!(
                        "the text report does not carry the finalized bounded result: {}",
                        excerpt(&outcome.output, 300)
                    ),
                );
            }
        }
        OutputMode::Json => {
            let report = match parse_report(&outcome) {
                Ok(report) => report,
                Err(cause) => return Case::fail(id, cause),
            };
            if report["exitCode"] != json!(0) || report["session"] != json!(SESSION) {
                return Case::fail(
                    id,
                    format!(
                        "the JSON report must name the finalized exit 0 and session {SESSION}: {}",
                        excerpt(&outcome.output, 300)
                    ),
                );
            }
            if !report["returned"]
                .as_str()
                .is_some_and(|returned| returned.contains(FINAL_RESULT))
            {
                return Case::fail(
                    id,
                    format!(
                        "the JSON report does not carry the finalized bounded result: {}",
                        excerpt(&outcome.output, 300)
                    ),
                );
            }
        }
    }
    Case::pass(id, "the same call returned the finalized bounded result")
}

/// A recorded successful exit without a retained result is not success: the
/// host is gone, so no later finalization can supply the missing result.
fn defect_case(exe: &Path) -> Case {
    let mut fixture = match Fixture::start() {
        Ok(fixture) => fixture,
        Err(error) => return Case::fail(CASE_DEFECT, format!("could not seed the run: {error}")),
    };
    if let Err(error) = fixture.write("completed", Some(0), None) {
        return Case::fail(CASE_DEFECT, format!("could not seed the receipt: {error}"));
    }
    if let Err(error) = fixture.end_host() {
        return Case::fail(
            CASE_DEFECT,
            format!("could not end the run's host: {error}"),
        );
    }
    let outcome = match invoke(
        exe,
        fixture.directory(),
        &watch_arguments(&fixture.receipt, DEFECT_TIMEOUT_SECS, OutputMode::Text),
        Duration::from_secs(DEFECT_TIMEOUT_SECS + 10),
    ) {
        Ok(outcome) => outcome,
        Err(error) => return Case::fail(CASE_DEFECT, format!("watch did not run: {error}")),
    };
    if !outcome.finished {
        return Case::fail(
            CASE_DEFECT,
            format!("watch did not return within its declared {DEFECT_TIMEOUT_SECS}s timeout"),
        );
    }
    if outcome.code == Some(0) {
        return Case::fail(
            CASE_DEFECT,
            format!(
                "a recorded exit 0 without a readable result must not be reported as success: {}",
                excerpt(&outcome.output, 300)
            ),
        );
    }
    Case::pass(
        CASE_DEFECT,
        format!(
            "reported exit {:?} instead of success for a finalized run without a result",
            outcome.code
        ),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Text,
    Json,
}

fn watch_arguments(receipt: &Path, timeout_seconds: u64, mode: OutputMode) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = vec![
        "executor".into(),
        "watch".into(),
        "--receipt".into(),
        receipt.as_os_str().to_owned(),
        "--timeout".into(),
        timeout_seconds.to_string().into(),
        "--poll".into(),
        POLL_MS.into(),
    ];
    if mode == OutputMode::Json {
        arguments.push("--json".into());
    }
    arguments
}

fn parse_report(outcome: &WatchOutcome) -> Result<Value, String> {
    serde_json::from_str::<Value>(outcome.output.trim()).map_err(|error| {
        format!(
            "the JSON watch report is not readable JSON: {error}: {}",
            excerpt(&outcome.output, 300)
        )
    })
}

/// One controlled run's private directory: the receipt, the result the host
/// writes on finalization, the release file that ends the host stand-in and
/// the live host process whose identity the receipt records. The stand-in is
/// this frozen checker's own binary, never the candidate executable: only the
/// observed `executor watch` call may depend on what is under test.
struct Fixture {
    directory: tempfile::TempDir,
    receipt: PathBuf,
    result: PathBuf,
    detail: PathBuf,
    release: PathBuf,
    host: Child,
    identity: Value,
    launcher: PathBuf,
}

impl Fixture {
    fn start() -> io::Result<Self> {
        let directory = tempfile::tempdir()?;
        let host_program = env::current_exe()?;
        let launcher = host_program.canonicalize()?;
        let release = directory.path().join("release");
        let child = Command::new(&launcher)
            .env("HARNESS_EXECUTOR_FIXTURE_MODE", "child-hold")
            .env("HARNESS_EXECUTOR_FIXTURE_RELEASE", &release)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let user = process_service::current_user()?;
        let identity = match ServiceProcess::observe(child.id(), &launcher, 0, &user) {
            Ok(process) => process.identity(),
            Err(error) => {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let result = directory.path().join("message-1.txt");
        let detail = directory.path().join("stream-1.jsonl");
        fs::write(&detail, b"")?;
        Ok(Self {
            receipt: directory.path().join("spawn-1.json"),
            directory,
            result,
            detail,
            release,
            host: child,
            identity: json!({
                "pid": identity.pid,
                "created": identity.creation_time,
                "program": launcher.to_string_lossy(),
            }),
            launcher,
        })
    }

    fn directory(&self) -> &Path {
        self.directory.path()
    }

    /// Records the lifecycle the real host would have persisted: coverage,
    /// identity, the live host and the still-missing result of the run.
    fn write(&self, state: &str, exit_code: Option<i32>, cause: Option<&str>) -> io::Result<()> {
        let value = json!({
            "schema": 1,
            "launcher": self.launcher.to_string_lossy(),
            "mode": "exec",
            "args": [],
            "visible": false,
            "host": "windows-terminal-tab",
            "slot": Value::Null,
            "observation": {
                "schema": 1,
                "coverage": "native",
                "reason": Value::Null,
                "state": state,
                "session": SESSION,
                "previousSession": Value::Null,
                "exitCode": exit_code,
                "events": 3,
                "messages": 1,
                "toolCalls": 0,
                "malformed": 0,
                "cause": cause,
                "host": self.identity,
                "result": self.result.to_string_lossy(),
                "detail": self.detail.to_string_lossy(),
                "updatedMs": now_ms(),
            },
        });
        write_atomic(&self.receipt, &serde_json::to_vec_pretty(&value)?)
    }

    /// The host persists the bounded final result and then the successful
    /// exit - the order the real host uses.
    fn finalize(&self) -> io::Result<()> {
        write_atomic(&self.result, FINAL_RESULT.as_bytes())?;
        self.write("completed", Some(0), None)
    }

    /// Ends the host stand-in and reaps it, so `host_ended` sees a gone host.
    fn end_host(&mut self) -> io::Result<()> {
        let _ = fs::write(&self.release, b"release");
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.host.try_wait()?.is_some() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = self.host.kill();
        let _ = self.host.wait();
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.end_host();
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension("oracle-temporary");
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}

/// The private directory's observable contents: names, sizes and kinds, so a
/// call that quietly stopped, replayed or rewrote the run is visible.
fn listing(directory: &Path) -> io::Result<Vec<String>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        entries.push(format!(
            "{}:{}:{}",
            entry.file_name().to_string_lossy(),
            metadata.len(),
            if metadata.is_dir() { "dir" } else { "file" }
        ));
    }
    entries.sort();
    Ok(entries)
}

struct WatchOutcome {
    code: Option<i32>,
    output: String,
    elapsed: Duration,
    finished: bool,
}

fn invoke(
    exe: &Path,
    cwd: &Path,
    arguments: &[OsString],
    watchdog: Duration,
) -> io::Result<WatchOutcome> {
    let started = Instant::now();
    let child = spawn(exe, cwd, arguments)?;
    wait(child, started, watchdog)
}

fn spawn(exe: &Path, cwd: &Path, arguments: &[OsString]) -> io::Result<Child> {
    let mut command = Command::new(exe);
    command
        .args(arguments)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in CLEARED_ENV {
        command.env_remove(name);
    }
    command.spawn()
}

fn wait(mut child: Child, started: Instant, watchdog: Duration) -> io::Result<WatchOutcome> {
    let mut finished = false;
    loop {
        if child.try_wait()?.is_some() {
            finished = true;
            break;
        }
        if started.elapsed() >= watchdog {
            let _ = child.kill();
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let elapsed = started.elapsed();
    let output = child.wait_with_output()?;
    let mut watched = String::from_utf8_lossy(&output.stdout).into_owned();
    let errors = String::from_utf8_lossy(&output.stderr);
    if !errors.trim().is_empty() {
        watched.push_str("\n[stderr] ");
        watched.push_str(&errors);
    }
    Ok(WatchOutcome {
        code: output.status.code(),
        output: watched,
        elapsed,
        finished,
    })
}

fn absolute(workspace: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

fn excerpt(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    let mut excerpt: String = trimmed.chars().take(limit).collect();
    if trimmed.chars().count() > limit {
        excerpt.push('…');
    }
    excerpt.replace('\n', "\\n")
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
