//! The candidate-independent `executor watch` acceptance checker
//! (`harness-executor-fixture watch-oracle`) exercised end to end.
//!
//! The checker is the artifact the real-task oracle freezes outside both
//! improvement arms; these checks keep it honest against this checkout's own
//! CLI: it must resolve only an actually built executable, reject a forged
//! stand-in and keep every positive assertion satisfiable, whatever the
//! watched change has done to the product.
//!
//! The discrimination against the unchanged watch path is proven on fixed
//! specimens, not on the mutable product, so a correct implementation passes
//! this suite and the independent checker:
//!
//! - `HARNESS_WATCH_ORACLE_UNCHANGED` names a retained build of the unchanged
//!   watch path, which the checker must reject, naming its early success;
//! - `HARNESS_WATCH_ORACLE_CORRECTED` names a corrected build, which the
//!   checker must accept in full.
//!
//! Both are supplied explicitly and live outside shared history; when they
//! are absent only the product-neutral checks and the private control
//! evidence apply.
//!
//! The opt-in source-only preparation is covered by the refusal checks below
//! and, when `HARNESS_WATCH_ORACLE_PREPARE_WORKSPACE`,
//! `HARNESS_WATCH_ORACLE_PREPARE_CARGO` and
//! `HARNESS_WATCH_ORACLE_PREPARE_OWNER` name a clean source-only checkout and
//! the pinned programs outside it, by one real prepared acceptance run.
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

/// Run the checker directly with raw arguments, for spellings the typed
/// helper above cannot express.
fn run_watch_oracle(args: &[&str]) -> Output {
    let mut command = Command::new(fixture());
    command.arg("watch-oracle").args(args);
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

/// The checker runs end to end against whatever this checkout builds, and
/// its positive assertions stay satisfiable. The verdict itself is not
/// asserted here: whether the watched change is present is the product's
/// business, and a correct implementation must pass this suite.
#[test]
fn checker_runs_against_the_workspace_cli() {
    let out = checker(&workspace(), Some(&manager()));
    let report = report(&out);
    assert!(
        report["executableSha256"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "{}",
        text(&out)
    );
    assert_eq!(report["cases"].as_array().unwrap().len(), 11);
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
}

/// The retained build of the unchanged watch path, supplied explicitly, is
/// still rejected with the early success named: the frozen discrimination
/// lives on a fixed specimen instead of on the mutable product.
#[test]
fn supplied_unchanged_specimen_is_rejected() {
    let Some(unchanged) = std::env::var_os("HARNESS_WATCH_ORACLE_UNCHANGED") else {
        println!("HARNESS_WATCH_ORACLE_UNCHANGED is not set; private control evidence covers this");
        return;
    };
    let unchanged = PathBuf::from(unchanged);
    let out = checker(&workspace(), Some(&unchanged));
    let report = report(&out);
    assert_eq!(
        report["passed"],
        false,
        "the unchanged watch path must not be accepted: {}",
        text(&out)
    );
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
    for id in [
        "finalized-without-result-defect-text",
        "finalized-without-result-defect-json",
    ] {
        let (passed, detail) = verdict(&report, id);
        assert!(!passed, "{id} unexpectedly passed: {detail}");
        assert!(
            detail.contains("exit 1"),
            "{id} did not require the defect exit: {detail}"
        );
    }
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
}

/// A corrected build, supplied explicitly, passes the unchanged frozen
/// checker in full - the target both arms are measured against.
#[test]
fn supplied_corrected_specimen_is_accepted() {
    let Some(corrected) = std::env::var_os("HARNESS_WATCH_ORACLE_CORRECTED") else {
        println!("HARNESS_WATCH_ORACLE_CORRECTED is not set; private control evidence covers this");
        return;
    };
    let corrected = PathBuf::from(corrected);
    let out = checker(&workspace(), Some(&corrected));
    let report = report(&out);
    assert_eq!(report["passed"], true, "{}", text(&out));
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
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

/// The opt-in preparation is explicit: incomplete or conflicting spellings
/// are refused before anything runs, so a half-configured preparation can
/// never fall back to the default prebuilt path or an ambient compiler.
#[test]
fn preparation_options_are_refused_when_incomplete_or_conflicting() {
    let workspace = workspace().to_string_lossy().into_owned();
    let manager = manager().to_string_lossy().into_owned();
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["--prepare"], "--cargo"),
        (
            vec!["--prepare", "--cargo", manager.as_str()],
            "--resource-owner",
        ),
        (
            vec![
                "--workspace",
                workspace.as_str(),
                "--cargo",
                manager.as_str(),
            ],
            "only valid together with --prepare",
        ),
        (
            vec![
                "--workspace",
                workspace.as_str(),
                "--resource-owner",
                manager.as_str(),
            ],
            "only valid together with --prepare",
        ),
        (
            vec![
                "--workspace",
                workspace.as_str(),
                "--prepare",
                "--cargo",
                manager.as_str(),
                "--resource-owner",
                manager.as_str(),
                "--exe",
                manager.as_str(),
            ],
            "cannot be combined with --exe",
        ),
    ];
    for (args, expected) in cases {
        let out = run_watch_oracle(&args);
        assert_ne!(out.status.code(), Some(0), "{}", text(&out));
        let printed = text(&out);
        assert!(
            printed.contains(expected),
            "expected {expected:?} in: {printed}"
        );
    }
}

/// Preparation accepts only absolute, existing, candidate-external programs:
/// neither PATH lookup nor a candidate-writable program may stand in for the
/// pinned tools.
#[test]
fn preparation_tools_must_be_absolute_ordinary_files_outside_the_checkout() {
    let workspace = workspace();
    let manager = manager();
    let workspace_text = workspace.to_string_lossy().into_owned();
    let manager_text = manager.to_string_lossy().into_owned();
    let missing = workspace.join("no-such-preparation-cargo.exe");
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (
            vec![
                "--workspace",
                workspace_text.as_str(),
                "--prepare",
                "--cargo",
                "cargo",
                "--resource-owner",
                manager_text.as_str(),
            ],
            "absolute",
        ),
        (
            vec![
                "--workspace",
                workspace_text.as_str(),
                "--prepare",
                "--cargo",
                missing.to_str().unwrap(),
                "--resource-owner",
                manager_text.as_str(),
            ],
            "not an existing program",
        ),
        (
            vec![
                "--workspace",
                workspace_text.as_str(),
                "--prepare",
                "--cargo",
                manager_text.as_str(),
                "--resource-owner",
                manager_text.as_str(),
            ],
            "inside the workspace",
        ),
    ];
    for (args, expected) in cases {
        let out = run_watch_oracle(&args);
        assert_ne!(out.status.code(), Some(0), "{}", text(&out));
        let printed = text(&out);
        assert!(
            printed.contains(expected),
            "expected {expected:?} in: {printed}"
        );
    }
}

/// The opt-in preparation of a source-only workspace: when the three
/// variables name a clean checkout and the pinned programs outside it, the
/// checker must build that workspace, run its fixed checks and accept the
/// built CLI in all 11 behavioral cases.
#[test]
fn prepared_source_only_workspace_is_built_and_accepted() {
    let (Some(workspace), Some(cargo), Some(owner)) = (
        std::env::var_os("HARNESS_WATCH_ORACLE_PREPARE_WORKSPACE"),
        std::env::var_os("HARNESS_WATCH_ORACLE_PREPARE_CARGO"),
        std::env::var_os("HARNESS_WATCH_ORACLE_PREPARE_OWNER"),
    ) else {
        println!("preparation variables are not set; private evidence covers this");
        return;
    };
    let out = Command::new(fixture())
        .arg("watch-oracle")
        .arg("--workspace")
        .arg(workspace)
        .arg("--prepare")
        .arg("--cargo")
        .arg(cargo)
        .arg("--resource-owner")
        .arg(owner)
        .output()
        .unwrap();
    let report = report(&out);
    assert_eq!(report["preparation"]["passed"], true, "{}", text(&out));
    assert_eq!(report["preparation"]["executedBy"], "checker");
    assert_eq!(report["preparation"]["steps"].as_array().unwrap().len(), 4);
    assert_eq!(report["cases"].as_array().unwrap().len(), 11);
    assert_eq!(report["passed"], true, "{}", text(&out));
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
}
