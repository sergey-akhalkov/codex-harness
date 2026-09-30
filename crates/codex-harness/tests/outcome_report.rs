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

fn faster(mut row: Value, end: f64) -> Value {
    row["ended_at"] = json!(end);
    row["checks"][0]["ended_at"] = json!(end);
    row
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
    assert_eq!(report["independent_units"], 1);
    assert_eq!(report["comparisons"][0]["independent"], true);
    assert_eq!(report["accounting"]["accepted_tasks"], 2);
    assert_eq!(
        report["accounting"]["cost_per_accepted_task"]["status"],
        "complete"
    );
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
fn declared_units_classification_and_accounting_survive_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("declared.json");
    let declaration = json!({
        "task_mix": "focused cases",
        "objective": "time",
        "effect_percent": 5.0,
        "nuisance": ["fixed order"],
        "stopping": "single attempt",
        "uncertainty": "none beyond elapsed",
        "horizon_tasks": 10.0,
        "costs": {
            "implementation_seconds": 1.0,
            "evaluation_seconds": 1.0,
            "maintenance_seconds_per_task": 0.0
        }
    });
    let mut baseline = attempt("a", "baseline");
    baseline["experiment_id"] = json!("exp-1");
    baseline["pair_id"] = json!("pair-1");
    baseline["declaration"] = declaration.clone();
    baseline["observed_model_metadata_verified"] = json!(true);
    let mut candidate = faster(attempt("b", "candidate"), 8.0);
    candidate["experiment_id"] = json!("exp-1");
    candidate["pair_id"] = json!("pair-1");
    candidate["declaration"] = declaration;
    candidate["observed_model_metadata_verified"] = json!(true);
    fs::write(
        &input,
        serde_json::to_vec(&json!([baseline, candidate])).unwrap(),
    )
    .unwrap();

    let output = run(&input, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["independent_units"], 1);
    let unit = &report["units"][0];
    assert_eq!(unit["unit"], "exp-1/case|one\nλ/pair-1");
    assert_eq!(unit["claim_class"], "stochastic_savings");
    assert_eq!(unit["one_to_one"], true);
    assert_eq!(unit["evidence_complete"], true);
    assert_eq!(unit["positive_effect"], true);
    assert_eq!(unit["net_saving"]["verdict"], "net_saving");
    assert_eq!(unit["net_saving"]["seconds"], 18.0);
    assert_eq!(report["accounting"]["accepted_tasks"], 2);
    assert_eq!(report["accounting"]["acceptance_rate"], 1.0);
    assert_eq!(
        report["accounting"]["cost_per_accepted_task"]["status"],
        "complete"
    );

    let markdown = run(&input, true);
    assert!(markdown.status.success());
    let text = String::from_utf8(markdown.stdout).unwrap();
    assert!(text.contains("stochastic_savings"), "{text}");
    assert!(text.contains("accounting attempts=2 tasks=2"), "{text}");
}

#[test]
fn cartesian_units_blank_identities_and_zero_acceptance_stay_honest() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("cartesian.json");
    let rows = vec![
        attempt("b1", "baseline"),
        attempt("b2", "baseline"),
        attempt("c1", "candidate"),
        attempt("c2", "candidate"),
        attempt("c3", "candidate"),
    ];
    fs::write(&input, serde_json::to_vec(&rows).unwrap()).unwrap();
    let output = run(&input, false);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["comparisons"].as_array().unwrap().len(), 6);
    assert_eq!(report["independent_units"], 1);
    assert!(
        report["comparisons"]
            .as_array()
            .unwrap()
            .iter()
            .all(|pair| pair["independent"] == false)
    );
    assert_eq!(report["accounting"]["tasks"], 5);

    // Blank placeholder identities cannot make the pair comparable.
    let mut blank_left = attempt("d", "baseline");
    let mut blank_right = attempt("e", "candidate");
    blank_left["matched"]["input_identity"] = json!("  ");
    blank_right["matched"]["input_identity"] = json!("  ");
    fs::write(
        &input,
        serde_json::to_vec(&json!([blank_left, blank_right])).unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["comparisons"][0]["comparable"], false);
    assert!(
        report["comparisons"][0]["excluded_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("unknown:input_identity"))
    );

    // Zero accepted tasks leave the per-task ratio undefined.
    let mut failed = attempt("f", "baseline");
    failed["checks"][0]["passed"] = json!(false);
    failed["checks"][0]["exit_code"] = json!(1);
    fs::write(&input, serde_json::to_vec(&json!([failed])).unwrap()).unwrap();
    let output = run(&input, false);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["accounting"]["accepted_tasks"], 0);
    assert_eq!(report["accounting"]["acceptance_rate"], 0.0);
    assert_eq!(
        report["accounting"]["cost_per_accepted_task"]["status"],
        "undefined"
    );
}

#[test]
fn dangling_inherited_child_reference_is_rejected_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.json");
    let mut parent = attempt("a", "baseline");
    parent["children"] = json!([
        {"attempt_id": "missing-child", "started_at": 0, "ended_at": 10}
    ]);
    let bytes = serde_json::to_vec(&json!([parent, attempt("b", "candidate")])).unwrap();
    fs::write(&input, &bytes).unwrap();
    let output = run(&input, false);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("missing-child"));
    assert_eq!(fs::read(&input).unwrap(), bytes);
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
