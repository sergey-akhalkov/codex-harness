//! The candidate-independent `executor watch` acceptance checker
//! (`harness-executor-fixture watch-oracle`) exercised end to end.
//!
//! The checker is the artifact the real-task oracle freezes outside both
//! improvement arms; these checks keep it honest against this checkout's own
//! CLI: it must resolve only an actually built executable, reject a forged
//! stand-in, keep its success assertions satisfiable, and name the early
//! success of the unchanged watch path as a failure. The unchanged path is
//! what this checkout still ships, so the discrimination assertion belongs
//! here until the watched change itself lands; the frozen experiment reruns
//! the checker against the retained baseline build.
#![cfg(windows)]

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"))
}

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn checker(workspace: &Path, executable: Option<&Path>) -> Output {
    let mut command = Command::new(fixture());
    command
        .arg("watch-oracle")
        .arg("--workspace")
        .arg(workspace);
    if let Some(executable) = executable {
        command.arg("--exe").arg(executable);
    }
    command.output().unwrap()
}

fn report(out: &Output) -> Value {
    let output = text(out);
    let start = output.find('{').unwrap_or_else(|| {
        panic!("the checker printed no report: {output}");
    });
    serde_json::from_str(&output[start..]).unwrap_or_else(|error| {
        panic!("the checker report is not JSON ({error}): {output}");
    })
}

fn case<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == id)
        .unwrap_or_else(|| panic!("the report does not name the case {id}"))
}

fn verdict(report: &Value, id: &str) -> (bool, String) {
    let case = case(report, id);
    (
        case["passed"].as_bool().unwrap(),
        case["detail"].as_str().unwrap_or_default().to_owned(),
    )
}

/// The unchanged watch path trusts a `completed` receipt before the host has
/// recorded its exit status and result. The checker must name that early
/// success as a failure while its positive assertions stay satisfiable.
#[test]
fn frozen_checker_rejects_the_unchanged_early_success() {
    let out = checker(&workspace(), Some(&manager()));
    let report = report(&out);
    assert_eq!(
        report["passed"],
        false,
        "the unchanged watch path must not be accepted: {}",
        text(&out)
    );
    assert!(
        report["executableSha256"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "{}",
        text(&out)
    );
    // The checker's own positive assertions are satisfiable: identity, a
    // recorded failure and an already finalized run are all accepted.
    for id in [
        "built-executable",
        "cli-identity",
        "failed-run-stays-failure",
        "finalized-run-returns-result-text",
        "finalized-run-returns-result-json",
    ] {
        let (passed, detail) = verdict(&report, id);
        assert!(passed, "{id} failed against the real CLI: {detail}");
    }
    // The pending-finalization discrimination: a `completed` receipt whose
    // host exit status is not recorded and whose result is missing must not
    // return success. The unchanged path returns exit 0 immediately; the
    // checker must report exactly that observation.
    for id in [
        "pending-finalization-timeout-text",
        "pending-finalization-timeout-json",
    ] {
        let (passed, detail) = verdict(&report, id);
        assert!(!passed, "{id} unexpectedly passed: {detail}");
        assert!(
            detail.contains("must not return success") && detail.contains("Some(0)"),
            "{id} does not name the observed early success: {detail}"
        );
    }
    let (during, detail) = verdict(&report, "finalization-during-watch-text");
    assert!(
        !during && detail.contains("does not carry the finalized bounded result"),
        "finalization-during-watch-text did not report the early return: {detail}"
    );
    let (defect, detail) = verdict(&report, "finalized-without-result-not-success");
    assert!(
        !defect && detail.contains("must not be reported as success"),
        "finalized-without-result-not-success did not report the false success: {detail}"
    );
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert_eq!(report["cases"].as_array().unwrap().len(), 10);
}

/// A workspace whose CLI was never built is rejected before any behavior
/// check, so a candidate cannot pass by skipping its build.
#[test]
fn checker_rejects_a_missing_build() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("crates")).unwrap();
    let out = checker(root.path(), None);
    let report = report(&out);
    assert_eq!(report["passed"], false);
    assert!(report["executable"].is_null());
    let (passed, detail) = verdict(&report, "built-executable");
    assert!(!passed, "{detail}");
    assert!(detail.contains("no built executable"), "{detail}");
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
}

/// A build left behind by a later source change is a skipped build, not an
/// implementation to judge.
#[test]
fn checker_rejects_a_skipped_build() {
    let root = tempfile::tempdir().unwrap();
    let built = root.path().join("target").join("debug");
    fs::create_dir_all(&built).unwrap();
    fs::copy(manager(), built.join("codex-harness.exe")).unwrap();
    fs::create_dir_all(root.path().join("crates").join("probe").join("src")).unwrap();
    fs::write(
        root.path()
            .join("crates")
            .join("probe")
            .join("src")
            .join("lib.rs"),
        "pub fn probe() {}\n",
    )
    .unwrap();
    let out = checker(root.path(), None);
    let report = report(&out);
    assert_eq!(report["passed"], false);
    let (passed, detail) = verdict(&report, "built-executable");
    assert!(!passed, "{detail}");
    assert!(detail.contains("older than"), "{detail}");
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
}

/// An executable that is not this CLI - here the launcher double, which
/// always emits a fabricated success stream and exits 0 - is rejected before
/// any behavior case can be satisfied by its output.
#[test]
fn checker_rejects_an_executable_that_is_not_the_cli() {
    let out = checker(&workspace(), Some(&fixture()));
    let report = report(&out);
    assert_eq!(report["passed"], false);
    let (passed, detail) = verdict(&report, "cli-identity");
    assert!(!passed, "{detail}");
    assert!(
        detail.contains("without identifying the codex-harness CLI"),
        "{detail}"
    );
    assert_eq!(case(&report, "failed-run-stays-failure")["skipped"], true);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
}

/// Adding the checker must not change any documented fixture mode, including
/// the mode contract the existing observation checks depend on.
#[test]
fn fixture_modes_stay_compatible() {
    let error = Command::new(fixture())
        .env("HARNESS_EXECUTOR_FIXTURE_MODE", "error")
        .output()
        .unwrap();
    assert_eq!(error.status.code(), Some(1), "{}", text(&error));
    assert!(text(&error).contains("turn.failed"), "{}", text(&error));
    let slow = Command::new(fixture())
        .env("HARNESS_EXECUTOR_FIXTURE_MODE", "slow")
        .env("HARNESS_EXECUTOR_FIXTURE_DELAY_MS", "50")
        .output()
        .unwrap();
    assert_eq!(slow.status.code(), Some(0), "{}", text(&slow));
    assert!(
        text(&slow).contains("FIXTURE_OUTCOME_DONE"),
        "{}",
        text(&slow)
    );
    let help = Command::new(fixture())
        .args(["watch-oracle", "--help"])
        .output()
        .unwrap();
    assert_eq!(help.status.code(), Some(0), "{}", text(&help));
    assert!(
        text(&help).contains("Candidate-independent acceptance"),
        "{}",
        text(&help)
    );
    let invalid = Command::new(fixture())
        .args(["watch-oracle", "--bogus"])
        .output()
        .unwrap();
    assert_ne!(invalid.status.code(), Some(0), "{}", text(&invalid));
}
