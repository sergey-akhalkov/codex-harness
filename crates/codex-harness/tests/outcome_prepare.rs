//! Fresh native controlled cases and real copied executable consumers.
#![cfg(windows)]
use harness_core::build_identity::hash_file;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn prepare(case: &str) -> (PathBuf, Value) {
    let cwd = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    command
        .args(["outcome-prepare", "--case", case])
        .current_dir(cwd.path());
    if case == "process" {
        command.args(["--observer", env!("CARGO_BIN_EXE_harness-observe")]);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "passed");
    assert_eq!(report["model_calls"], 0);
    let root = PathBuf::from(report["case_root"].as_str().unwrap());
    assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert!(
        !root.starts_with(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .canonicalize()
                .unwrap()
        )
    );
    assert_eq!(
        report,
        serde_json::from_slice::<Value>(&fs::read(root.join("preparation.json")).unwrap()).unwrap()
    );
    for (file, expected) in report["setup"]["immutable"].as_object().unwrap() {
        assert_eq!(
            hash_file(&root.join(file)).unwrap(),
            expected.as_str().unwrap()
        );
        assert!(
            !["py", "ps1", "mjs", "js", "cs"].contains(
                &Path::new(file)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
            )
        );
    }
    println!("controlled case evidence: {}", root.display());
    (root, report)
}

#[test]
fn all_controlled_cases_are_fresh_and_preserve_the_workload_and_document_contract() {
    for case in ["freshness", "entrypoint", "process", "missing", "negative"] {
        let (root, report) = prepare(case);
        let setup = &report["setup"];
        assert_eq!(setup["case_id"], case);
        assert_eq!(setup["source_state"], "controlled-v3-rust");
        assert!(
            setup["prompt"]
                .as_str()
                .unwrap()
                .contains("Never count a skipped check as passed")
        );
        if ["freshness", "entrypoint", "process"].contains(&case) {
            assert_eq!(
                hash_file(&root.join("case.exe")).unwrap(),
                hash_file(Path::new(env!("CARGO_BIN_EXE_codex-harness"))).unwrap()
            );
        } else {
            assert!(!root.join("case.exe").exists());
        }
        if case == "negative" {
            assert_eq!(
                fs::read_to_string(root.join("README.md")).unwrap(),
                "# Guide\n\nRun verfication; see [details](guide.md).\n"
            );
            assert!(setup["immutable"].get("README.md").is_none());
            assert!(setup["immutable"].get("guide.md").is_some());
        }
        if case == "missing" {
            let verification: Value =
                serde_json::from_slice(&fs::read(root.join("verification.json")).unwrap()).unwrap();
            let missing = Path::new(verification["command"][0].as_str().unwrap());
            assert!(missing.is_absolute());
            assert!(missing.starts_with(&root));
            assert!(!missing.exists());
            assert_eq!(verification["required"], true);
        }
        if case == "freshness" {
            assert!(
                fs::read_to_string(root.join("docs/validation.md"))
                    .unwrap()
                    .contains("source version 1")
            );
            assert!(setup["documents"].get("docs/validation.md").is_some());
            assert!(setup["immutable"].get("docs/validation.md").is_none());
        }
        assert!(!root.join("outcome.json").exists());
        assert!(!root.join("execution-audit.jsonl").exists());
        assert!(!root.join("process-results.json").exists());
    }
}

#[test]
fn copied_manager_runs_the_real_build_and_product_cli_and_never_reuses_an_old_arm() {
    let (root, _) = prepare("entrypoint");
    let elsewhere = tempfile::tempdir().unwrap();
    for (mode, expected) in [("cli", "1\n"), ("build", ""), ("cli", "2\n")] {
        let output = Command::new(root.join("case.exe"))
            .args(["--outcome-case", mode])
            .current_dir(elsewhere.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, expected.as_bytes());
    }
    let events: Vec<Value> = fs::read_to_string(root.join("execution-audit.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["version"], 1);
    assert_eq!(events[1]["entrypoint"], "build");
    assert_eq!(events[2]["version"], 2);
    let (next, _) = prepare("entrypoint");
    assert_ne!(root, next);
    assert_eq!(
        fs::read_to_string(next.join("built.json")).unwrap(),
        "{\"version\":1}\n"
    );
    assert!(!next.join("execution-audit.jsonl").exists());
}

#[test]
fn copied_observer_executes_the_copied_target_with_natural_failure_evidence() {
    let (root, report) = prepare("process");
    assert_eq!(
        hash_file(&root.join("observe.exe")).unwrap(),
        report["setup"]["fixture_executables"]["observe.exe"]
            .as_str()
            .unwrap()
    );
    let output = Command::new(root.join("observe.exe"))
        .arg("--cwd")
        .arg(&root)
        .args(["--timeout", "5", "--"])
        .arg(root.join("case.exe"))
        .args(["--outcome-case", "fail"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let evidence: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(evidence["status"], "exited");
    assert_eq!(evidence["native"]["ProcessExitCode"], 7);
    assert_eq!(evidence["job"]["active_processes"], 0);
    assert!(
        fs::read_to_string(root.join("process-audit.jsonl"))
            .unwrap()
            .contains("\"mode\":\"fail\"")
    );
}

#[test]
fn oversized_case_data_fails_without_changing_the_generated_artifact_or_audit() {
    let (root, _) = prepare("entrypoint");
    let source = root.join("source.json");
    fs::write(&source, vec![b' '; 1025]).unwrap();
    let original = fs::read(root.join("built.json")).unwrap();
    let output = Command::new(root.join("case.exe"))
        .args(["--outcome-case", "build"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(root.join("built.json")).unwrap(), original);
    assert!(!root.join("execution-audit.jsonl").exists());
}

#[test]
fn external_cases_and_implicit_or_irrelevant_observers_are_not_substituted() {
    for args in [
        vec!["--case", "focused"],
        vec!["--case", "second"],
        vec!["--case", "reduction"],
        vec!["--case", "process"],
        vec!["--case", "process", "--observer", "relative.exe"],
        vec![
            "--case",
            "negative",
            "--observer",
            env!("CARGO_BIN_EXE_harness-observe"),
        ],
        vec!["--case", "negative", "--case", "negative"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .arg("outcome-prepare")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}
