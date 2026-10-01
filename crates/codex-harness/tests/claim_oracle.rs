//! Candidate-independent claim acceptance (`harness-executor-fixture claim-oracle`).
//!
//! The checker is frozen outside both improvement arms. These checks keep it
//! honest against this checkout: it must resolve a built executable, reject a
//! non-CLI stand-in, and keep its report aligned with the exit code. Whether
//! the current product already holds an in-flight claim is not asserted here.
//!
//! Discrimination against fixed specimens is opt-in, because those builds live
//! outside shared history:
//!
//! - `HARNESS_CLAIM_ORACLE_UNCHANGED` must be rejected, with the one-slot
//!   overlap cases failing because the contender replaced the claim;
//! - `HARNESS_CLAIM_ORACLE_CORRECTED` must pass every case;
//! - `HARNESS_CLAIM_ORACLE_FORGED` must be rejected even if it prints acceptance.
//!
//! `HARNESS_CLAIM_ORACLE_PREPARE_WORKSPACE`, `_CARGO` and `_OWNER` name a
//! source-only checkout and the pinned programs for one real preparation run.
#![cfg(windows)]

use serde_json::Value;
use std::{
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
        .arg("claim-oracle")
        .arg("--workspace")
        .arg(workspace);
    if let Some(executable) = executable {
        command.arg("--exe").arg(executable);
    }
    command.output().unwrap()
}

fn run_claim_oracle(args: &[&str]) -> Output {
    let mut command = Command::new(fixture());
    command.arg("claim-oracle").args(args);
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

fn verdict(report: &Value, id: &str) -> (bool, String) {
    let case = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == id)
        .unwrap_or_else(|| panic!("the report does not name {id}"));
    (
        case["passed"].as_bool().unwrap(),
        case["detail"].as_str().unwrap_or_default().to_owned(),
    )
}

#[test]
fn checker_runs_against_the_workspace_cli() {
    let out = checker(&workspace(), Some(&manager()));
    let report = report(&out);
    assert_eq!(report["kind"], "executor-claim-oracle");
    assert!(
        report["executableSha256"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "{}",
        text(&out)
    );
    assert_eq!(report["cases"].as_array().unwrap().len(), 11);
    for id in ["built-executable", "cli-identity"] {
        let (passed, detail) = verdict(&report, id);
        assert!(passed, "{id} failed against the real CLI: {detail}");
    }
    let expect = if report["passed"] == true { 0 } else { 1 };
    assert_eq!(out.status.code(), Some(expect), "{}", text(&out));
}

#[test]
fn a_non_cli_executable_is_rejected_without_accepting_printed_success() {
    let out = checker(&workspace(), Some(&fixture()));
    let report = report(&out);
    assert_eq!(report["passed"], false, "{}", text(&out));
    let (identity, detail) = verdict(&report, "cli-identity");
    assert!(!identity, "{detail}");
    assert!(detail.contains("not the codex-harness CLI"), "{detail}");
    for id in [
        "one-slot-different-owner",
        "one-slot-same-owner",
        "two-slot-distinct",
    ] {
        let (passed, detail) = verdict(&report, id);
        assert!(!passed, "{id} was accepted for a non-CLI: {detail}");
        assert!(
            detail.contains("skipped after identity failure"),
            "{id}: {detail}"
        );
    }
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
}

#[test]
fn preparation_options_are_refused_when_incomplete_or_conflicting() {
    let missing_cargo = run_claim_oracle(&["--prepare", "--workspace", "."]);
    assert!(
        text(&missing_cargo).contains("--prepare needs --cargo"),
        "{}",
        text(&missing_cargo)
    );
    let cargo_without_prepare = run_claim_oracle(&["--cargo", r"C:\cargo.exe"]);
    assert!(
        text(&cargo_without_prepare).contains("only valid together with --prepare"),
        "{}",
        text(&cargo_without_prepare)
    );
    let both = run_claim_oracle(&[
        "--prepare",
        "--exe",
        r"C:\codex-harness.exe",
        "--cargo",
        r"C:\cargo.exe",
        "--resource-owner",
        r"C:\owner.exe",
    ]);
    assert!(
        text(&both).contains("cannot be combined with --exe"),
        "{}",
        text(&both)
    );
}

#[test]
fn supplied_unchanged_specimen_is_rejected() {
    let Some(unchanged) = std::env::var_os("HARNESS_CLAIM_ORACLE_UNCHANGED") else {
        println!("HARNESS_CLAIM_ORACLE_UNCHANGED is not set; private control evidence covers this");
        return;
    };
    let out = checker(&workspace(), Some(Path::new(&unchanged)));
    let report = report(&out);
    assert_eq!(report["passed"], false, "{}", text(&out));
    for id in ["one-slot-different-owner", "one-slot-same-owner"] {
        let (passed, detail) = verdict(&report, id);
        assert!(!passed, "{id} unexpectedly passed: {detail}");
        assert!(
            detail.contains("replaced the in-flight claim")
                || detail.contains("post-claim fetches"),
            "{id} does not name the stolen claim: {detail}"
        );
    }
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
}

#[test]
fn supplied_corrected_specimen_is_accepted() {
    let Some(corrected) = std::env::var_os("HARNESS_CLAIM_ORACLE_CORRECTED") else {
        println!("HARNESS_CLAIM_ORACLE_CORRECTED is not set; private control evidence covers this");
        return;
    };
    let out = checker(&workspace(), Some(Path::new(&corrected)));
    let report = report(&out);
    assert_eq!(report["passed"], true, "{}", text(&out));
    assert_eq!(report["cases"].as_array().unwrap().len(), 11);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
}

#[test]
fn supplied_forged_success_is_rejected() {
    let Some(forged) = std::env::var_os("HARNESS_CLAIM_ORACLE_FORGED") else {
        println!(
            "HARNESS_CLAIM_ORACLE_FORGED is not set; the non-CLI check covers a local stand-in"
        );
        return;
    };
    let out = checker(&workspace(), Some(Path::new(&forged)));
    let report = report(&out);
    assert_eq!(report["passed"], false, "{}", text(&out));
    assert_ne!(out.status.code(), Some(0), "{}", text(&out));
}

#[test]
fn supplied_wrong_turn_input_is_rejected() {
    let Some(executable) = std::env::var_os("HARNESS_CLAIM_ORACLE_WRONG_TURN") else {
        println!(
            "HARNESS_CLAIM_ORACLE_WRONG_TURN is not set; private control evidence covers this"
        );
        return;
    };
    let out = checker(&workspace(), Some(Path::new(&executable)));
    let report = report(&out);
    assert_eq!(report["passed"], false, "{}", text(&out));
    let rejected = report["cases"].as_array().unwrap().iter().any(|case| {
        !case["passed"].as_bool().unwrap_or(true)
            && case["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("did not carry the expected assignment"))
    });
    assert!(
        rejected,
        "a plausible receipt with the wrong turn input was not rejected: {}",
        text(&out)
    );
    assert_ne!(out.status.code(), Some(0), "{}", text(&out));
}

#[test]
fn supplied_overflowing_handoff_is_rejected() {
    let Some(executable) = std::env::var_os("HARNESS_CLAIM_ORACLE_OVERFLOW") else {
        println!("HARNESS_CLAIM_ORACLE_OVERFLOW is not set; private control evidence covers this");
        return;
    };
    let out = checker(&workspace(), Some(Path::new(&executable)));
    let report = report(&out);
    assert_eq!(report["passed"], false, "{}", text(&out));
    let rejected = report["cases"].as_array().unwrap().iter().any(|case| {
        let detail = case["detail"].as_str().unwrap_or_default();
        !case["passed"].as_bool().unwrap_or(true)
            && detail.contains("truncation is not success")
            && detail.contains("despite exit")
    });
    assert!(
        rejected,
        "a successful handoff whose output exceeded the retained bound was not rejected: {}",
        text(&out)
    );
    assert_ne!(out.status.code(), Some(0), "{}", text(&out));
}

#[test]
fn prepared_source_only_workspace_is_built_and_checked() {
    let (Some(workspace), Some(cargo), Some(owner)) = (
        std::env::var_os("HARNESS_CLAIM_ORACLE_PREPARE_WORKSPACE"),
        std::env::var_os("HARNESS_CLAIM_ORACLE_PREPARE_CARGO"),
        std::env::var_os("HARNESS_CLAIM_ORACLE_PREPARE_OWNER"),
    ) else {
        println!("preparation variables are not set; private evidence covers this");
        return;
    };
    let out = Command::new(fixture())
        .arg("claim-oracle")
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
    assert_eq!(
        report["preparation"]["executedBy"],
        "checker",
        "{}",
        text(&out)
    );
    assert_eq!(report["preparation"]["passed"], true, "{}", text(&out));
    let steps = report["preparation"]["steps"].as_array().unwrap();
    assert!(
        steps
            .iter()
            .any(|step| step["id"] == "native-build" && step["exitCode"] == 0),
        "preparation did not compile: {}",
        text(&out)
    );
}

#[test]
fn missing_expected_capture_fails_and_termination_is_bounded() {
    let out = Command::new(fixture())
        .arg("claim-oracle-cleanup")
        .output()
        .unwrap();
    let report = report(&out);
    assert_eq!(
        report["kind"],
        "executor-claim-oracle-cleanup",
        "{}",
        text(&out)
    );
    assert_eq!(report["passed"], true, "{}", text(&out));
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(
        report["missingCapture"]["accepted"],
        false,
        "{}",
        text(&out)
    );
    assert_eq!(report["missingCapture"]["exitCode"], 0, "{}", text(&out));
    assert!(
        report["missingCapture"]["elapsedMs"].as_u64().unwrap_or(0) >= 4_500,
        "missing piped capture was accepted before the observation bound: {}",
        text(&out)
    );
    assert_eq!(report["delayedCapture"]["accepted"], true, "{}", text(&out));
    assert!(
        (350..5_000).contains(&report["delayedCapture"]["elapsedMs"].as_u64().unwrap_or(0)),
        "delayed capture was not waited out past the old empty-success window: {}",
        text(&out)
    );
    assert_eq!(report["nullCapture"]["accepted"], true, "{}", text(&out));
    assert!(
        report["nullCapture"]["elapsedMs"]
            .as_u64()
            .unwrap_or(u64::MAX)
            < 1_500,
        "null-stream capture waited as though output was required: {}",
        text(&out)
    );
    assert_eq!(report["termination"]["succeeded"], false, "{}", text(&out));
    assert_eq!(report["termination"]["bounded"], true, "{}", text(&out));
    assert_eq!(
        report["termination"]["retainedOriginal"],
        true,
        "{}",
        text(&out)
    );
    assert_eq!(report["termination"]["treeKillOk"], true, "{}", text(&out));
    assert_eq!(
        report["termination"]["treeKillBounded"],
        true,
        "{}",
        text(&out)
    );
    let error = report["termination"]["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("observation exceeded the caller deadline before termination"),
        "original error was dropped: {error}"
    );
    assert!(
        error.contains("termination helper exceeded"),
        "cleanup error was dropped: {error}"
    );
    assert!(
        !error.contains("taskkill /F /T /PID"),
        "recovery advised killing a pid: {error}"
    );
}
