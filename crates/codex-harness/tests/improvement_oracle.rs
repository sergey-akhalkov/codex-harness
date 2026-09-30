//! The native oracle runs an independent checker; a task's success file cannot
//! replace execution or authorize a changed acceptance input.
#![cfg(windows)]
use harness_core::build_identity::hash_file;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

struct Case {
    _root: tempfile::TempDir,
    workspace: PathBuf,
    request: PathBuf,
    checker: PathBuf,
    expected: PathBuf,
    request_sha256: String,
}

impl Case {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("candidate");
        fs::create_dir(&workspace).unwrap();
        let checker = root.path().join("independent.exe");
        fs::copy(env!("CARGO_BIN_EXE_harness-improvement-fixture"), &checker).unwrap();
        let expected = root.path().join("expected.txt");
        fs::write(&expected, "correct\n").unwrap();
        let request = root.path().join("oracle-request.json");
        let value = json!({
            "schema":1,"kind":"real-task","case_root":workspace,
            "task_contract_sha256":"a".repeat(64),"timeout_seconds":10,
            "oracle":{"program":checker,"program_sha256":hash_file(&checker).unwrap(),
                "arguments":["check","{workspace}",expected],
                "inputs":{expected.to_str().unwrap():hash_file(&expected).unwrap()}}
        });
        fs::write(&request, serde_json::to_vec(&value).unwrap()).unwrap();
        let request_sha256 = hash_file(&request).unwrap();
        Self {
            _root: root,
            workspace,
            request,
            checker,
            expected,
            request_sha256,
        }
    }

    fn run(&self) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["outcome-oracle", "--request"])
            .arg(&self.request)
            .args(["--request-sha256", &self.request_sha256])
            .output()
            .unwrap()
    }
}

#[test]
fn actual_checker_accepts_correct_result_and_rejects_wrong_missing_or_forged_success() {
    let case = Case::new();
    for result in [None, Some("incorrect\n"), Some("correct\n")] {
        if let Some(result) = result {
            fs::write(case.workspace.join("answer.txt"), result).unwrap();
        }
        fs::write(
            case.workspace.join("outcome.json"),
            "{\"passed\":true,\"exit_code\":0}",
        )
        .unwrap();
        let output = case.run();
        let record: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)));
        let correct = result == Some("correct\n");
        assert_eq!(output.status.success(), correct, "{record}");
        assert_eq!(record["passed"], correct);
        assert_eq!(record["checker_executed"], true);
        assert_eq!(record["model_calls"], 0);
        assert_eq!(record["runs"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn changed_or_candidate_owned_oracle_cannot_report_success() {
    let case = Case::new();
    fs::write(case.workspace.join("answer.txt"), "correct\n").unwrap();
    fs::write(&case.expected, "changed\n").unwrap();
    let output = case.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("frozen oracle input changed"));
    fs::write(&case.expected, "correct\n").unwrap();
    fs::write(&case.checker, "replaced checker").unwrap();
    assert!(!case.run().status.success());

    let mut case = Case::new();
    let mutable = case.workspace.join("candidate-checker.exe");
    fs::copy(&case.checker, &mutable).unwrap();
    let mut request: Value = serde_json::from_slice(&fs::read(&case.request).unwrap()).unwrap();
    request["oracle"]["program"] = json!(mutable);
    fs::write(&case.request, serde_json::to_vec(&request).unwrap()).unwrap();
    let output = case.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("frozen oracle request changed"));
    // A newly frozen host request must still reject a candidate-owned checker.
    case.request_sha256 = hash_file(&case.request).unwrap();
    let output = case.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside the candidate checkout"));
}
