use serde_json::{Value, json};
use std::{fs, process::Command};

fn run(path: &std::path::Path, markdown: bool) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    command.current_dir(path.parent().unwrap());
    command.args(["outcome-report", "--input"]).arg(path);
    if markdown {
        command.arg("--markdown");
    }
    command.output().unwrap()
}

fn attempt(id: &str, arm: &str) -> Value {
    let matched: std::collections::BTreeMap<_, _> = harness_core::outcome_report::MATCH_FIELDS
        .iter()
        .map(|k| (*k, "fixed"))
        .collect();
    json!({"attempt_id":id,"case_id":"case|one\nλ","arm":arm,"started_at":0,"ended_at":10,"matched":matched,"discovery_verified":true,
        "native_runs":[{"status":"completed","started_at":0,"ended_at":2}],
        "checks":[{"id":"proof","required":true,"executed":true,"exit_code":0,"passed":true,"started_at":2,"ended_at":10,"evidence":"private/proof"}]})
}

#[test]
fn native_json_markdown_and_incremental_input_preserve_accounting() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("attempts λ.json");
    fs::write(
        &input,
        serde_json::to_vec(&json!([
            attempt("a", "baseline"),
            attempt("b", "candidate")
        ]))
        .unwrap(),
    )
    .unwrap();
    let original = fs::read(&input).unwrap();
    let output = run(&input, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["attempts"].as_array().unwrap().len(), 2);
    assert_eq!(report["comparisons"][0]["comparable"], true);
    assert_eq!(report["benefit_status"], "not_evaluated");
    assert!(report["attempts"][0]["usage"]["total_tokens"].is_null());
    assert_eq!(fs::read(&input).unwrap(), original);
    fs::write(&input, &output.stdout).unwrap();
    let second = run(&input, false);
    assert!(second.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&second.stdout).unwrap(),
        report
    );
    let markdown = run(&input, true);
    assert!(markdown.status.success());
    let text = String::from_utf8(markdown.stdout).unwrap();
    assert!(text.contains("case\\|one λ"));
    assert!(text.contains("10.000"));
    assert!(text.contains("not billing or subscription-quota savings"));
}

#[test]
fn malformed_oversized_and_duplicate_inputs_fail_privately_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.json");
    for bytes in [
        b"private-parser-content".to_vec(),
        vec![b'x'; 8 * 1024 * 1024 + 1],
        serde_json::to_vec(&json!([
            attempt("a", "baseline"),
            attempt("a", "candidate")
        ]))
        .unwrap(),
    ] {
        fs::write(&input, &bytes).unwrap();
        let output = run(&input, false);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-parser-content"));
        assert_eq!(fs::read(&input).unwrap(), bytes);
    }
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["outcome-report", "--markdown", "--markdown"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}
