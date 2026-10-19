use harness_core::improvement_policy::ComparisonPolicy;
use serde_json::{Value, json};
use std::{fs, process::Command};

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    command.arg("outcome-report");
    command
}

fn policy_json() -> Value {
    json!({
        "schema": 1,
        "objective": "time",
        "basis": "efficiency",
        "meaningfulEffectPercent": 10.0,
        "tolerancePercent": 5.0,
        "requireAcceptance": true,
        "taskMix": "one frozen task case",
        "stopping": {"maxAttemptsPerArm": 3, "requiredUnits": 1},
        "repeatedSelection": "predeclared",
        "tradeOff": Value::Null,
        "uncertainty": "unknown evidence stays inconclusive",
        "horizonTasks": 5.0,
        "overhead": {
            "implementationSeconds": 10.0,
            "evaluationSeconds": 5.0,
            "maintenanceSecondsPerTask": 0.5
        }
    })
}

fn policy_attempt(
    id: &str,
    arm: &str,
    start: f64,
    seconds: f64,
    accepted: bool,
    declaration: &Value,
) -> Value {
    let mut row = attempt(id, arm);
    row["started_at"] = json!(start);
    row["ended_at"] = json!(start + seconds);
    row["observed_model_metadata_verified"] = json!(true);
    row["native_runs"][0]["started_at"] = json!(start);
    row["native_runs"][0]["ended_at"] = json!(start + 2.0);
    row["native_runs"][0]["rounds"] = json!(3);
    row["native_runs"][0]["tool_operations"] = json!(6);
    row["native_runs"][0]["usage"] = json!({
        "model": "fixed",
        "total_tokens": 100,
        "input_tokens": 100,
        "cached_input_tokens": 40,
        "output_tokens": 20,
        "reasoning_tokens": 5,
    });
    row["checks"][0]["started_at"] = json!(start + 2.0);
    row["checks"][0]["ended_at"] = json!(start + seconds);
    row["checks"][0]["passed"] = json!(accepted);
    row["checks"][0]["exit_code"] = json!(if accepted { 0 } else { 1 });
    row["declaration"] = declaration.clone();
    row
}

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
fn acceptance_corrections_batching_and_unknown_counters_survive_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("acceptance.json");
    let mut baseline = attempt("a", "baseline");
    baseline["native_runs"][0]["ended_at"] = json!(2);
    baseline["native_runs"][0]["requests"] = json!(4);
    baseline["native_runs"][0]["rounds"] = json!(3);
    baseline["native_runs"][0]["tool_calls"] = json!(3);
    baseline["native_runs"][0]["tool_operations"] = json!(5);
    baseline["checks"] = json!([
        {"id":"proof","round":0,"required":true,"executed":true,"exit_code":1,"passed":false,
            "started_at":2,"ended_at":4,"evidence":"private/first-check"},
        {"id":"proof","round":1,"required":true,"executed":true,"exit_code":0,"passed":true,
            "started_at":5,"ended_at":10,"evidence":"private/corrected-check"}
    ]);
    let mut candidate = attempt("b", "candidate");
    // One batched outer call performed the same five operations.
    candidate["native_runs"][0]["ended_at"] = json!(2);
    candidate["native_runs"][0]["requests"] = json!(2);
    candidate["native_runs"][0]["rounds"] = json!(2);
    candidate["native_runs"][0]["tool_calls"] = json!(1);
    candidate["native_runs"][0]["tool_operations"] = json!(5);
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
    // Time through the failed check, the correction and the re-check is the
    // enclosing attempt span, counted once.
    assert_eq!(report["attempts"][0]["elapsed_seconds"], 10.0);
    assert_eq!(report["attempts"][0]["status"], "accepted");
    assert_eq!(report["attempts"][0]["checks"].as_array().unwrap().len(), 2);
    // Requests, rounds, outer calls and operations stay distinct, and the
    // lower batched call count alone produces no saving verdict.
    assert_eq!(report["attempts"][0]["requests"], 4);
    assert_eq!(report["attempts"][0]["total_requests"], 4);
    assert_eq!(report["attempts"][0]["total_tool_calls"], 3);
    assert_eq!(report["attempts"][1]["tool_calls"], 1);
    assert_eq!(report["attempts"][1]["tool_operations"], 5);
    assert_eq!(report["units"][0]["positive_effect"], Value::Null);
    assert_eq!(report["units"][0]["net_saving"], Value::Null);
    let markdown = run(&input, true);
    assert!(markdown.status.success());
    let text = String::from_utf8(markdown.stdout).unwrap();
    assert!(text.contains("requests=6"), "{text}");
    assert!(text.contains("tool_calls=4"), "{text}");

    // A counter no run recorded stays unknown instead of becoming zero.
    let partial_input = root.path().join("partial.json");
    fs::write(
        &partial_input,
        serde_json::to_vec(&json!([attempt("c", "baseline")])).unwrap(),
    )
    .unwrap();
    let output = run(&partial_input, false);
    assert!(output.status.success());
    let partial: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(partial["attempts"][0]["requests"].is_null());
    assert!(partial["attempts"][0]["tool_calls"].is_null());
    assert!(partial["attempts"][0]["tool_operations"].is_null());
    assert_eq!(
        partial["variation"][0]["within_run_events"]["unknown_counters"]["tool_operations"],
        1
    );
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

#[test]
fn complete_pair_variation_is_not_manufactured_from_within_run_events() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("variation.json");
    let paired = |id: &str, arm: &str, pair_id: &str, end: f64, events: u64| {
        let mut row = faster(attempt(id, arm), end);
        row["experiment_id"] = json!("exp-1");
        row["pair_id"] = json!(pair_id);
        row["native_runs"][0]["rounds"] = json!(events);
        row["native_runs"][0]["tool_operations"] = json!(events);
        row
    };

    // One complete pair whose arms each carry a thousand dependent events:
    // variation stays unmeasured and the events stay events.
    fs::write(
        &input,
        serde_json::to_vec(&json!([
            paired("b1", "baseline", "p1", 10.0, 500),
            paired("c1", "candidate", "p1", 8.0, 500)
        ]))
        .unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let group = &report["variation"][0];
    assert_eq!(group["experiment_id"], "exp-1");
    assert_eq!(group["complete_pairs"], 1);
    assert_eq!(group["run_variation"], "unmeasured");
    assert!(group["observed_elapsed_effect_percent"].is_null());
    assert_eq!(group["within_run_events"]["attempts"], 2);
    assert_eq!(group["within_run_events"]["native_runs"], 2);
    assert_eq!(group["within_run_events"]["rounds"], 1000);
    assert_eq!(group["within_run_events"]["tool_operations"], 1000);
    assert!(
        group["basis"]
            .as_str()
            .unwrap()
            .contains("not replications")
    );

    // Two complete pairs expose a descriptive observed range only.
    fs::write(
        &input,
        serde_json::to_vec(&json!([
            paired("b1", "baseline", "p1", 10.0, 1),
            paired("c1", "candidate", "p1", 8.0, 1),
            paired("b2", "baseline", "p2", 10.0, 1),
            paired("c2", "candidate", "p2", 9.5, 1)
        ]))
        .unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let group = &report["variation"][0];
    assert_eq!(group["complete_pairs"], 2);
    assert_eq!(group["run_variation"], "observed-pairs");
    assert_eq!(group["observed_elapsed_effect_percent"], json!([5.0, 20.0]));
    assert_eq!(group["within_run_events"]["attempts"], 4);
    assert_eq!(report["units"].as_array().unwrap().len(), 2);
    // A pair identity scopes comparability: edges crossing the two declared
    // pairs are listed but excluded, never counted as evidence for either.
    assert!(
        report["comparisons"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|pair| pair["excluded_reasons"]
                .as_array()
                .cloned()
                .unwrap_or_default())
            .any(|reason| reason == json!("different_declared_unit")),
        "{}",
        report["comparisons"]
    );

    let markdown = run(&input, true);
    assert!(markdown.status.success());
    let text = String::from_utf8(markdown.stdout).unwrap();
    assert!(text.contains("variation experiment=exp-1"), "{text}");
    assert!(text.contains("complete_pairs=2"), "{text}");
}

#[test]
fn subtractive_consumption_and_retained_checks_survive_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("subtractive.json");
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
    let subtractive = |mut row: Value, status: &str| {
        row["experiment_id"] = json!("exp-1");
        row["pair_id"] = json!("pair-1");
        row["declaration"] = declaration.clone();
        row["observed_model_metadata_verified"] = json!(true);
        row["treatment"] = json!({"kind": "subtraction", "removed": "catalogue-skill"});
        row["consumption"] = json!({
            "capability": "catalogue-skill",
            "status": status,
            "evidence": "private/consumption.json"
        });
        row
    };
    let baseline = subtractive(attempt("a", "baseline"), "consumed");
    let mut candidate = subtractive(faster(attempt("b", "candidate"), 8.0), "absent");

    // The burden was consumed before removal and the candidate is observed
    // not to consume it: the unit keeps the recorded applicability and can
    // support its scoped saving.
    fs::write(
        &input,
        serde_json::to_vec(&json!([baseline.clone(), candidate.clone()])).unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let unit = &report["units"][0];
    assert_eq!(unit["subtractive"], true);
    assert_eq!(unit["removed_burden"], "catalogue-skill");
    assert_eq!(unit["applicability"], "exercised");
    assert_eq!(unit["retained_checks"], true);
    assert_eq!(unit["evidence_complete"], true);
    assert_eq!(unit["positive_effect"], true);
    assert_eq!(unit["net_saving"]["verdict"], "net_saving");

    // Stale context after removal: the candidate arm still consumes the
    // removed burden, so the intended context treatment is not established
    // and a smaller source tree cannot support a saving.
    candidate["consumption"]["status"] = json!("consumed");
    fs::write(
        &input,
        serde_json::to_vec(&json!([baseline.clone(), candidate.clone()])).unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let unit = &report["units"][0];
    assert_eq!(unit["applicability"], "unknown");
    assert_eq!(unit["evidence_complete"], false);
    assert_eq!(unit["positive_effect"], false);
    assert!(
        unit["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|limitation| limitation
                .as_str()
                .unwrap_or("")
                .contains("still records consumption")),
        "{:?}",
        unit["limitations"]
    );

    // A replaced acceptance check leaves the candidate's own record passing
    // but removes required coverage relative to the baseline; the lower check
    // cost cannot become an accounted saving.
    candidate["consumption"]["status"] = json!("absent");
    candidate["checks"][0]["id"] = json!("quick-check");
    fs::write(
        &input,
        serde_json::to_vec(&json!([baseline.clone(), candidate.clone()])).unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let unit = &report["units"][0];
    assert_eq!(unit["retained_checks"], false);
    assert_eq!(unit["evidence_complete"], false);

    // Deleting the acceptance check outright leaves the candidate without
    // independent acceptance: the pair has no comparable, independently
    // accepted result and cannot become a decision basis.
    candidate["checks"] = json!([]);
    fs::write(
        &input,
        serde_json::to_vec(&json!([baseline, candidate])).unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["attempts"][1]["status"], "incomplete");
    assert_eq!(report["accounting"]["accepted_tasks"], 1);
    assert_eq!(report["comparisons"][0]["comparable"], false);
    assert!(
        report["comparisons"][0]["excluded_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("outcome_incomplete")),
        "{:?}",
        report["comparisons"][0]["excluded_reasons"]
    );
}

/// The declared corroboration receipt is consumed into the report with its
/// selection status and identity/replay references; the markdown keeps the
/// status, the units, the exclusions and the exact reason visible.
#[test]
fn corroboration_status_and_evidence_references_survive_the_report() {
    let rows = vec![
        faster(attempt("a", "baseline"), 10.0),
        faster(attempt("b", "candidate"), 8.0),
    ];
    let report = harness_core::outcome_report::summarize_attempts(&rows).unwrap();
    let receipt = json!({
        "schema": 1,
        "status": "selected",
        "required_units": 1,
        "selection": {
            "schema": 1,
            "requiredUnits": 1,
            "status": "ready",
            "units": [{
                "owner": "card-prior",
                "caseId": "case-prior",
                "experiment": "exp-prior",
                "mechanism": "bounded-output",
                "conditions": "local-tool-runs",
                "revision": "rev-prior",
                "treeSha256": "c".repeat(64)
            }],
            "excluded": [{
                "owner": "card-other",
                "caseId": "case-other",
                "reason": "notApplicable"
            }]
        },
        "reason": Value::Null
    });
    let report = harness_core::outcome_report::attach_corroboration(&report, &receipt).unwrap();
    let section = &report["corroboration"];
    assert_eq!(section["schema"], 1);
    assert_eq!(section["status"], "ready");
    assert_eq!(section["ready"], true);
    assert_eq!(section["requiredUnits"], 1);
    assert_eq!(section["units"][0]["owner"], "card-prior");
    assert_eq!(section["units"][0]["caseId"], "case-prior");
    assert_eq!(section["units"][0]["experiment"], "exp-prior");
    assert_eq!(section["units"][0]["mechanism"], "bounded-output");
    assert_eq!(section["units"][0]["conditions"], "local-tool-runs");
    assert_eq!(section["units"][0]["revision"], "rev-prior");
    assert_eq!(section["units"][0]["treeSha256"], "c".repeat(64));
    assert!(section["units"][0].get("answer").is_none());
    assert_eq!(section["excluded"][0]["reason"], "notApplicable");
    let digest = section["digest"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    let bound: harness_core::outcome_report::CorroborationSection =
        serde_json::from_value(section.clone()).unwrap();
    assert_eq!(
        harness_core::outcome_report::corroboration_digest(&bound).unwrap(),
        digest
    );
    // A substituted reference no longer matches the binding digest.
    let mut substituted = bound.clone();
    substituted.units[0].case_id = "case-substituted".to_owned();
    assert_ne!(
        harness_core::outcome_report::corroboration_digest(&substituted).unwrap(),
        digest
    );

    let text = harness_core::outcome_report::concise_report(&report).unwrap();
    assert!(
        text.contains("corroboration: status=ready required_units=1 units=1 excluded=1"),
        "{text}"
    );
    assert!(text.contains("case=case-prior"), "{text}");
    assert!(text.contains("tree_sha256="), "{text}");
    assert!(
        text.contains(
            "corroboration excluded: owner=card-other case=case-other reason=not applicable"
        ),
        "{text}"
    );

    // An inconclusive selection and an unavailable selection stay explicit
    // with their exact reason and exclusions, never replaced by a summary.
    let inconclusive = json!({
        "schema": 1,
        "status": "selected",
        "required_units": 1,
        "selection": {
            "schema": 1,
            "requiredUnits": 1,
            "status": {"inconclusive": "fewer applicable independent replayable retained tasks than the declared corroboration requirement (required 1, admissible 0); the broader claim remains unsupported"},
            "units": [],
            "excluded": [{"owner": "card-other", "caseId": "case-other", "reason": "notApplicable"}]
        },
        "reason": Value::Null
    });
    let changed =
        harness_core::outcome_report::attach_corroboration(&report, &inconclusive).unwrap();
    assert_eq!(changed["corroboration"]["status"], "inconclusive");
    assert_eq!(changed["corroboration"]["ready"], false);
    assert!(
        changed["corroboration"]["units"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let text = harness_core::outcome_report::concise_report(&changed).unwrap();
    assert!(
        text.contains("corroboration: status=inconclusive"),
        "{text}"
    );
    assert!(text.contains("corroboration reason: "), "{text}");

    let unavailable = json!({
        "schema": 1,
        "status": "unavailable",
        "required_units": 1,
        "selection": Value::Null,
        "reason": "retained-task discovery through the board is unavailable"
    });
    let report =
        harness_core::outcome_report::attach_corroboration(&changed, &unavailable).unwrap();
    assert_eq!(report["corroboration"]["status"], "unavailable");
    assert!(
        report["corroboration"]["reason"]
            .as_str()
            .unwrap()
            .contains("retained-task discovery")
    );
    let text = harness_core::outcome_report::concise_report(&report).unwrap();
    assert!(text.contains("corroboration: status=unavailable"), "{text}");
}

/// A receipt whose unit list contradicts its own declared requirement, whose
/// fields are unknown, or whose unavailable status lacks its reason is refused
/// without producing a section; no unit is fabricated or summarized.
#[test]
fn invalid_corroboration_receipts_are_refused_without_a_section() {
    let rows = vec![attempt("a", "baseline"), attempt("b", "candidate")];
    let report = harness_core::outcome_report::summarize_attempts(&rows).unwrap();
    let unit = || {
        json!({
            "owner": "card", "caseId": "case", "experiment": "exp",
            "mechanism": "mechanism", "conditions": "conditions",
            "revision": "rev", "treeSha256": "tree"
        })
    };
    let cases = [
        // Ready with fewer units than its own declared requirement.
        json!({"schema": 1, "status": "selected", "required_units": 2,
            "selection": {"schema": 1, "requiredUnits": 2, "status": "ready",
                "units": [unit()], "excluded": []},
            "reason": Value::Null}),
        // Ready with a blank identity reference.
        json!({"schema": 1, "status": "selected", "required_units": 1,
            "selection": {"schema": 1, "requiredUnits": 1, "status": "ready",
                "units": [{"owner": "  ", "caseId": "case", "experiment": "exp",
                    "mechanism": "mechanism", "conditions": "conditions",
                    "revision": "rev", "treeSha256": "tree"}],
                "excluded": []},
            "reason": Value::Null}),
        // Unknown receipt field: an assumed summary is not evidence.
        json!({"schema": 1, "status": "unavailable", "required_units": 1,
            "selection": Value::Null, "reason": "not performed",
            "summary": "assumed saving"}),
        // Unavailable without its exact reason.
        json!({"schema": 1, "status": "unavailable", "required_units": 1,
            "selection": Value::Null, "reason": Value::Null}),
        // A selection whose declared requirement disagrees with the receipt.
        json!({"schema": 1, "status": "selected", "required_units": 1,
            "selection": {"schema": 1, "requiredUnits": 2, "status": "ready",
                "units": [unit(), unit()], "excluded": []},
            "reason": Value::Null}),
    ];
    for receipt in cases {
        assert!(
            harness_core::outcome_report::attach_corroboration(&report, &receipt).is_err(),
            "{receipt}"
        );
    }
    assert!(report.get("corroboration").is_none());
}

/// A retained capture with one verified unrelated external wait of
/// `wait_seconds` inside a 10-second attempt and the given requests.
fn cli_wait_capture(wait_seconds: u64, requests: Value) -> Value {
    let wait_ns = wait_seconds * 1_000_000_000;
    json!({
        "window": {"start_ns": 0, "end_ns": 10_000_000_000_u64},
        "admissions": [{
            "id": "adm-1",
            "class": "unrelated_wait",
            "domain_match": true,
            "start_ns": 0,
            "end_ns": wait_ns,
            "endpoint_start_ns": 0,
            "endpoint_end_ns": 0,
            "tick_ns": 0,
            "tool_call_id": "tool-1",
            "command_id": "cmd-1",
            "started": true,
            "terminal": "waited_grant"
        }],
        "activity": [{
            "id": "tool-1",
            "kind": "command",
            "placement": "exact",
            "start_ns": 0,
            "end_ns": wait_ns,
            "tool_call_id": "tool-1",
            "command_id": "cmd-1"
        }],
        "requests": requests
    })
}

fn cli_immediate_capture() -> Value {
    json!({
        "window": {"start_ns": 0, "end_ns": 10_000_000_000_u64},
        "admissions": [{"id": "now", "class": "measured_zero", "domain_match": true}],
        "activity": [],
        "requests": []
    })
}

fn cli_wait_request(id: &str) -> Value {
    json!({
        "id": id,
        "wait_only": true,
        "start_ns": 1_000_000_000_u64,
        "end_ns": 2_000_000_000_u64,
        "tool_call_id": "tool-1",
        "command_id": "cmd-1",
        "input_tokens": 12,
        "cached_input_tokens": 0,
        "output_tokens": 0,
        "reasoning_output_tokens": 0,
        "total_tokens": 12
    })
}

/// The retained native trace replays into the same adjusted view through the
/// real CLI entry point, the raw/adjusted/excluded totals reconcile, every
/// exclusion carries its rule and evidence, and duplicate request usage is
/// rejected instead of creating a saving.
#[test]
fn infrastructure_attribution_replays_reconciles_and_rejects_duplicates() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("attribution.json");
    let rows = |requests: Value| {
        let mut baseline = attempt("base", "baseline");
        baseline["infrastructure_capture"] = cli_immediate_capture();
        let mut candidate = attempt("cand", "candidate");
        candidate["infrastructure_capture"] = cli_wait_capture(5, requests);
        json!([baseline, candidate])
    };
    fs::write(
        &input,
        serde_json::to_vec(&rows(json!([cli_wait_request("wait-1")]))).unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let attribution = report["attempts"][1]["attribution"].clone();
    assert_eq!(attribution["replay"], "reproduced", "{attribution}");
    assert_eq!(
        attribution["rule_version"], "infrastructure-attribution.v1",
        "{attribution}"
    );
    assert_eq!(
        attribution["reconciliation"]["status"], "consistent",
        "{attribution}"
    );
    assert_eq!(
        attribution["reconciliation"]["elapsed"]["excluded_seconds"], 5.0,
        "{attribution}"
    );
    assert_eq!(
        attribution["reconciliation"]["tokens"]["raw_total_tokens"], 12,
        "{attribution}"
    );
    assert_eq!(
        attribution["reconciliation"]["tokens"]["excluded_total_tokens"], 12,
        "{attribution}"
    );
    assert!(
        attribution["exclusions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == "deducted"
                && entry["metric"] == "tokens"
                && entry["requests"] == json!(["wait-1"])
                && entry["rule"] == "infrastructure-attribution.v1"
                && !entry["evidence"].as_str().unwrap_or("").is_empty()),
        "{attribution}"
    );

    // Resuming from the retained report reproduces the same deductions: the
    // second pass is the same deterministic reduction, not a new measurement.
    let resumed_input = root.path().join("resumed.json");
    fs::write(&resumed_input, serde_json::to_vec(&report).unwrap()).unwrap();
    let output = run(&resumed_input, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let resumed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(resumed["attempts"][1]["attribution"], attribution);

    // A duplicated request identity is one request, not two savings: the
    // exclusion does not happen, the adjusted total keeps the recorded usage,
    // and the degraded coverage is explicit.
    fs::write(
        &input,
        serde_json::to_vec(&rows(json!([
            cli_wait_request("wait-1"),
            cli_wait_request("wait-1")
        ])))
        .unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(output.status.success());
    let duplicate: Value = serde_json::from_slice(&output.stdout).unwrap();
    let attribution = duplicate["attempts"][1]["attribution"].clone();
    assert_eq!(
        attribution["duplicates"],
        json!(["wait-1"]),
        "{attribution}"
    );
    assert_eq!(
        attribution["reconciliation"]["tokens"]["status"], "degraded",
        "{attribution}"
    );
    assert_eq!(
        attribution["reconciliation"]["tokens"]["excluded_total_tokens"], 0,
        "{attribution}"
    );
    assert_eq!(
        attribution["reconciliation"]["tokens"]["adjusted_total_tokens"], 12,
        "{attribution}"
    );
    assert!(
        attribution["exclusions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == "unresolved"
                && entry["metric"] == "tokens"
                && entry["reason"]
                    .as_str()
                    .is_some_and(|reason| reason.contains("duplicate request identity"))),
        "{attribution}"
    );
}

/// One model-free attempt row: the declared method executes no model call, so
/// its model metrics are inapplicable rather than a measured zero, and its
/// measured work is the operation's own duration, exit and declared inputs.
fn operation_attempt(
    id: &str,
    arm: &str,
    method: &str,
    start: f64,
    end: f64,
    accepted: bool,
) -> Value {
    let matched: std::collections::BTreeMap<_, _> = harness_core::outcome_report::MATCH_FIELDS
        .iter()
        .map(|k| (*k, "fixed"))
        .collect();
    json!({
        "attempt_id": id,
        "case_id": "operation-case",
        "arm": arm,
        "method": method,
        "model_calls": 0,
        "model_metrics": "inapplicable",
        "started_at": start,
        "ended_at": end,
        "matched": matched,
        "native_runs": [{
            "status": "completed",
            "started_at": start,
            "ended_at": start + 2.0,
            "elapsed_seconds": 2.0,
            "exit_code": 0,
            "evidence": "private/operation-receipt.json",
        }],
        "checks": [{
            "id": "independent-acceptance",
            "required": true,
            "executed": true,
            "exit_code": if accepted { 0 } else { 1 },
            "passed": accepted,
            "started_at": start + 2.0,
            "ended_at": end,
            "evidence": "private/oracle.json",
        }],
    })
}

#[test]
fn model_free_units_are_comparable_and_name_the_method_distinction() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("model-free.json");
    let declaration = json!({
        "task_mix": "one operation case",
        "objective": "time",
        "effect_percent": 5.0,
        "nuisance": ["declared order"],
        "stopping": "one declared pair",
        "uncertainty": "unknown evidence stays inconclusive",
    });
    let mut baseline = operation_attempt("b", "baseline", "real-operation", 0.0, 100.0, true);
    let mut candidate = operation_attempt("c", "candidate", "real-operation", 200.0, 270.0, true);
    for row in [&mut baseline, &mut candidate] {
        row["declaration"] = declaration.clone();
    }
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
    assert_eq!(report["comparisons"][0]["comparable"], true, "{report}");
    let unit = &report["units"][0];
    assert_eq!(unit["method"], "real-operation", "{unit}");
    assert_eq!(unit["model_metrics"], "inapplicable", "{unit}");
    assert_eq!(unit["operation_work"], true, "{unit}");
    assert_eq!(unit["comparable_pairs"], 1, "{unit}");
    assert_eq!(unit["evidence_complete"], true, "{unit}");
    // Inapplicable stays non-numeric: no model counter becomes zero.
    assert_eq!(
        report["attempts"][0]["total_rounds"],
        Value::Null,
        "{report}"
    );
    assert_eq!(
        report["attempts"][0]["total_tool_operations"],
        Value::Null,
        "{report}"
    );
    assert_eq!(
        report["attempts"][0]["usage"]["status"], "unknown",
        "{report}"
    );
    // The rendered report keeps the declared unit visible.
    let markdown = run(&input, true);
    assert!(markdown.status.success());
    let text = String::from_utf8(markdown.stdout).unwrap();
    assert!(text.contains("operation-case"), "{text}");

    // Two different declared methods never form one comparable pair.
    let input = root.path().join("mixed-methods.json");
    fs::write(
        &input,
        serde_json::to_vec(&json!([
            operation_attempt("b2", "baseline", "real-operation", 0.0, 100.0, true),
            operation_attempt("c2", "candidate", "bounded-replay", 200.0, 270.0, true),
        ]))
        .unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["comparisons"][0]["comparable"], false, "{report}");
    assert!(
        report["comparisons"][0]["excluded_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("different_method")),
        "{}",
        report["comparisons"][0]
    );
    assert_eq!(report["units"].as_array().unwrap().len(), 0, "{report}");
}

// ---------------------------------------------------------------------------
// Retained baseline reuse.
// ---------------------------------------------------------------------------

/// One retained-baseline reuse record: the retained execution identity, when
/// it ran, its age, the retained trace, coverage and uncertainty, the
/// predeclared selection and the conditions it was measured under.
fn retained_reuse() -> Value {
    json!({
        "of": "retained-execution-1",
        "executed_at": 0.0,
        "age_seconds": 1200.0,
        "trace": "private/retained/trace.jsonl",
        "coverage": "time+method:real-operation; complete-pairs:1",
        "uncertainty": "single-pair variation remains unmeasured",
        "selection": {"at": 150.0, "basis": "declared before candidate results"},
        "conditions": {"cache": "owned-cold", "load": "recorded-idle"},
    })
}

#[test]
fn a_reused_operation_baseline_keeps_its_original_cost_and_inapplicable_metrics() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("reused-baseline.json");
    // The baseline result is the retained execution (executed at 0); only the
    // candidate operation runs now. The reuse was selected before the
    // candidate started, so the original cost is accounted once and no fresh
    // baseline execution is claimed.
    let mut baseline = operation_attempt("b", "baseline", "real-operation", 0.0, 100.0, true);
    baseline["reuse"] = retained_reuse();
    let mut candidate = operation_attempt("c", "candidate", "real-operation", 200.0, 270.0, true);
    candidate["conditions"] = json!({"cache": "owned-cold", "load": "recorded-idle"});
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
    assert_eq!(report["comparisons"][0]["comparable"], true, "{report}");
    let unit = &report["units"][0];
    assert_eq!(unit["baseline_reused"], true, "{unit}");
    assert_eq!(
        unit["baseline_executed_now"], false,
        "retained evidence is never presented as a fresh execution: {unit}"
    );
    assert_eq!(unit["reuse"]["of"], "retained-execution-1", "{unit}");
    assert_eq!(unit["reuse"]["age_seconds"], 1200.0, "{unit}");
    assert_eq!(
        unit["reuse"]["original_seconds"], 100.0,
        "the retained execution's original cost is preserved: {unit}"
    );
    assert_eq!(unit["reuse"]["model_metrics"], "inapplicable", "{unit}");
    assert_eq!(unit["reuse"]["qualification"], "inapplicable", "{unit}");
    assert_eq!(
        unit["model_metrics"], "inapplicable",
        "a method without model execution keeps its model metrics inapplicable: {unit}"
    );
    assert_eq!(unit["operation_work"], true, "{unit}");
    assert_eq!(
        report["accounting"]["reused_baseline_attempts"], 1,
        "{report}"
    );
    assert_eq!(
        report["accounting"]["reuse_refused_attempts"], 0,
        "{report}"
    );
    assert_eq!(
        report["attempts"][0]["reuse_evidence"]["trace"], "private/retained/trace.jsonl",
        "{report}"
    );
    assert!(
        report["attempts"][0].get("reuse_refused").is_none(),
        "{report}"
    );
    // The rendered report keeps the retained baseline's status visible: its
    // original cost is accounted once, and no model metric becomes zero.
    let markdown = run(&input, true);
    assert!(markdown.status.success());
    let text = String::from_utf8(markdown.stdout).unwrap();
    assert!(text.contains("retained evidence"), "{text}");
}

#[test]
fn a_reuse_without_a_retained_trace_is_refused_with_its_exact_reason() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("missing-trace.json");
    let mut baseline = operation_attempt("b", "baseline", "real-operation", 0.0, 100.0, true);
    let mut reuse = retained_reuse();
    reuse["trace"] = json!("");
    baseline["reuse"] = reuse;
    let mut candidate = operation_attempt("c", "candidate", "real-operation", 200.0, 270.0, true);
    candidate["conditions"] = json!({"cache": "owned-cold", "load": "recorded-idle"});
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
    assert_eq!(
        report["comparisons"][0]["comparable"], false,
        "an unverifiable reuse supplies no comparable edge: {report}"
    );
    let refused = report["attempts"][0]["reuse_refused"][0]
        .as_str()
        .unwrap_or_default();
    assert!(refused.contains("trace is missing"), "{refused}");
    assert_eq!(
        report["accounting"]["reuse_refused_attempts"], 1,
        "{report}"
    );
    assert!(report["units"].as_array().unwrap().is_empty(), "{report}");
    let markdown = run(&input, true);
    assert!(markdown.status.success());
    let text = String::from_utf8(markdown.stdout).unwrap();
    assert!(text.contains("trace is missing"), "{text}");
}

#[test]
fn the_retained_trace_identity_refuses_missing_or_changed_bytes() {
    use harness_core::rollout_reader::RetainedTrace;

    let root = tempfile::tempdir().unwrap();
    let trace_path = root.path().join("retained-trace.jsonl");
    fs::write(&trace_path, "{\"type\":\"session_meta\"}\n").unwrap();
    let trace = RetainedTrace::capture(&trace_path).unwrap();
    trace
        .verify()
        .expect("the unchanged retained trace verifies");

    // A trace that changed after capture is a different generation: the
    // retained execution cannot be reused from it.
    fs::write(
        &trace_path,
        "{\"type\":\"session_meta\"}\n{\"type\":\"turn_context\"}\n",
    )
    .unwrap();
    let reason = trace.verify().unwrap_err();
    assert!(reason.contains("changed since it was captured"), "{reason}");

    // A missing trace cannot verify the retained execution at all.
    fs::remove_file(&trace_path).unwrap();
    let reason = trace.verify().unwrap_err();
    assert!(reason.contains("is missing"), "{reason}");
    let reason = RetainedTrace::capture(&trace_path).unwrap_err().to_string();
    assert!(reason.contains("missing or unreadable"), "{reason}");
}

fn qualification_run(
    session: &str,
    input: u64,
    cached: u64,
    output: u64,
    reasoning: u64,
    responses: u64,
) -> Value {
    json!({
        "session": session,
        "status": "completed",
        "started_at": 0.0,
        "ended_at": 2.0,
        "usage": {
            "input_tokens": input,
            "cached_input_tokens": cached,
            "output_tokens": output,
            "reasoning_output_tokens": reasoning,
            "total_tokens": input + output,
            "response_count": responses,
        },
    })
}

fn qualification_aggregate(runs: &[Value]) -> Value {
    let mut aggregate = serde_json::Map::new();
    for key in [
        "input_tokens",
        "cached_input_tokens",
        "output_tokens",
        "reasoning_output_tokens",
        "total_tokens",
        "response_count",
    ] {
        let total = runs.iter().try_fold(0_u64, |total, run| {
            total.checked_add(run["usage"][key].as_u64()?)
        });
        if let Some(total) = total {
            aggregate.insert(key.to_owned(), json!(total));
        }
    }
    Value::Object(aggregate)
}

fn qualification_row(
    id: &str,
    accepted: bool,
    review_passed: bool,
    runs: Vec<Value>,
    aggregate: Value,
) -> Value {
    json!({
        "attempt_id": id,
        "case_id": "native-control",
        "unit": "run/native-control",
        "qualification_scope": "ordinary control observation; no comparison margins, reflection qualification or benefit verdict",
        "arm": "baseline",
        "started_at": 0.0,
        "execution_started_at": 0.0,
        "ended_at": 3.0,
        "accepted": accepted,
        "native_runs": runs,
        "usage": aggregate,
        "checks": [
            {"id":"independent-acceptance","round":0,"required":true,"executed":true,"passed":true,"started_at":2.0,"ended_at":3.0,"evidence":"private/acceptance","evidence_sha256":"aaaa"},
            {"id":"independent-semantic-review","round":0,"required":true,"executed":true,"passed":review_passed,"evidence":"private/review","evidence_sha256":"bbbb"}
        ],
        "children": [],
        "interventions": [],
        "retry_of": null
    })
}

#[test]
fn native_qualification_wrapper_is_consumed_without_losing_checks_or_usage() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("native-qualification.json");
    let runs = vec![
        qualification_run("session-a", 100, 40, 20, 10, 2),
        qualification_run("session-b", 50, 10, 5, 2, 1),
    ];
    let aggregate = qualification_aggregate(&runs);
    let accepted = qualification_row("native-a", true, true, runs.clone(), aggregate.clone());
    fs::write(
        &input,
        serde_json::to_vec(&json!({"attempts":[accepted]})).unwrap(),
    )
    .unwrap();
    let original = fs::read(&input).unwrap();

    let output = run(&input, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let attempt = &report["attempts"][0];
    assert_eq!(attempt["status"], "accepted", "{report}");
    assert_eq!(attempt["correct"], true, "{report}");
    assert_eq!(attempt["accepted"], true, "{report}");
    assert_eq!(attempt["usage"]["status"], "per_run", "{report}");
    assert_eq!(attempt["usage"]["runs"].as_array().unwrap().len(), 2);
    let total: u64 = attempt["usage"]["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["total_tokens"].as_u64().unwrap())
        .sum();
    assert_eq!(total, 175);
    assert_eq!(attempt["usage"]["aggregate_verified"], true);
    assert!(attempt["usage"].get("recorded_aggregate").is_none());
    assert!(
        attempt["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|check| check.get("exit_code").is_none())
    );
    assert!(
        attempt["excluded_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("no_opposite_arm"))
    );
    assert!(report["comparisons"].as_array().unwrap().is_empty());
    assert_eq!(report["benefit_status"], "not_evaluated");
    assert_eq!(fs::read(&input).unwrap(), original);

    // The normalized report remains consumable by the same CLI.
    fs::write(&input, &output.stdout).unwrap();
    let second = run(&input, false);
    assert!(second.status.success());
    let normalized: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(normalized["attempts"][0]["status"], "accepted");
    assert_eq!(normalized["attempts"][0]["usage"]["status"], "per_run");
    assert_eq!(
        normalized["attempts"][0]["usage"]["runs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // An explicit failed acceptance stays failed and retains the failed check.
    let failed = qualification_row("native-b", false, false, runs.clone(), aggregate.clone());
    fs::write(
        &input,
        serde_json::to_vec(&json!({"attempts":[failed]})).unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["attempts"][0]["status"], "failed", "{report}");
    assert_eq!(report["attempts"][0]["correct"], false);
    assert_eq!(report["attempts"][0]["accepted"], false);
    assert_eq!(report["attempts"][0]["checks"][1]["passed"], false);

    // A contradictory aggregate is retained but not presented as measured.
    let mut contradictory = aggregate;
    contradictory["total_tokens"] = json!(999);
    let row = qualification_row("native-c", true, true, runs, contradictory);
    fs::write(
        &input,
        serde_json::to_vec(&json!({"attempts":[row]})).unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["attempts"][0]["usage"]["status"], "unknown");
    assert_eq!(
        report["attempts"][0]["usage"]["recorded_aggregate"]["total_tokens"],
        999
    );
    assert!(
        report["attempts"][0]["excluded_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("usage_aggregate_mismatch"))
    );

    // A missing per-run counter must not hide a contradiction in another
    // fully known counter.
    let incomplete_runs = vec![
        json!({"session":"session-c","status":"completed","usage":{"total_tokens":10}}),
        json!({"session":"session-d","status":"completed","usage":{"total_tokens":10}}),
        json!({"session":"session-e","status":"completed","usage":{"total_tokens":10}}),
    ];
    let row = qualification_row(
        "native-d",
        true,
        true,
        incomplete_runs,
        json!({"input_tokens":100,"total_tokens":999}),
    );
    fs::write(
        &input,
        serde_json::to_vec(&json!({"attempts":[row]})).unwrap(),
    )
    .unwrap();
    let output = run(&input, false);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let usage = &report["attempts"][0]["usage"];
    assert_eq!(usage["status"], "unknown", "{report}");
    assert_eq!(usage["runs"].as_array().unwrap().len(), 3);
    assert_eq!(usage["recorded_aggregate"]["total_tokens"], 999);
    assert_eq!(usage["aggregate_verified"], false);
    assert!(
        usage["contradictions"]
            .as_array()
            .unwrap()
            .contains(&json!("usage_aggregate_mismatch"))
    );
    assert!(
        report["attempts"][0]["excluded_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("usage_aggregate_mismatch"))
    );
}

#[test]
fn declared_policy_evaluates_retained_attempts_through_the_native_cli() {
    let root = tempfile::tempdir().unwrap();
    let policy_path = root.path().join("policy.json");
    let declaration_path = root.path().join("declaration.json");
    let input_path = root.path().join("attempts.json");
    let policy_json = policy_json();
    fs::write(
        &policy_path,
        serde_json::to_vec_pretty(&policy_json).unwrap(),
    )
    .unwrap();
    let policy: ComparisonPolicy = serde_json::from_value(policy_json.clone()).unwrap();
    let declaration = policy.declaration();
    let baseline = policy_attempt("b1", "baseline", 0.0, 100.0, true, &declaration);
    let candidate = policy_attempt("c1", "candidate", 200.0, 80.0, true, &declaration);
    fs::write(
        &input_path,
        serde_json::to_vec(&json!([baseline, candidate])).unwrap(),
    )
    .unwrap();
    let policy_bytes = fs::read(&policy_path).unwrap();
    let input_bytes = fs::read(&input_path).unwrap();

    let declared_output = cli()
        .arg("--declare-policy")
        .arg(&policy_path)
        .output()
        .unwrap();
    assert!(
        declared_output.status.success(),
        "{}",
        String::from_utf8_lossy(&declared_output.stderr)
    );
    assert!(declared_output.stderr.is_empty());
    let declared: Value = serde_json::from_slice(&declared_output.stdout).unwrap();
    assert_eq!(declared["policy"], policy_json);
    let digest = declared["digest"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    assert!(
        digest
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    );
    assert_eq!(fs::read(&policy_path).unwrap(), policy_bytes);
    fs::write(&declaration_path, &declared_output.stdout).unwrap();
    let declaration_bytes = fs::read(&declaration_path).unwrap();

    let evaluated = cli()
        .arg("--input")
        .arg(&input_path)
        .arg("--policy")
        .arg(&declaration_path)
        .output()
        .unwrap();
    assert!(
        evaluated.status.success(),
        "{}",
        String::from_utf8_lossy(&evaluated.stderr)
    );
    assert!(evaluated.stderr.is_empty());
    let evaluation: Value = serde_json::from_slice(&evaluated.stdout).unwrap();
    assert_eq!(evaluation["policyDigest"], declared["digest"]);
    assert_eq!(evaluation["decision"], "adopt");
    assert_eq!(evaluation["matched"], 1);
    assert_eq!(evaluation["baselineSeconds"], 100.0);
    assert_eq!(evaluation["candidateSeconds"], 80.0);
    assert!(evaluation.get("schema_version").is_none());
    assert_eq!(fs::read(&input_path).unwrap(), input_bytes);
    assert_eq!(fs::read(&declaration_path).unwrap(), declaration_bytes);
    assert_eq!(fs::read(&policy_path).unwrap(), policy_bytes);

    // The normal report mode stays a summary and never evaluates supplied data.
    let normal = cli().arg("--input").arg(&input_path).output().unwrap();
    assert!(normal.status.success());
    let summary: Value = serde_json::from_slice(&normal.stdout).unwrap();
    assert_eq!(summary["schema_version"], 2);
    assert_eq!(summary["benefit_status"], "not_evaluated");
    assert!(summary.get("policyDigest").is_none());

    // A slower candidate is a supported negative result, not an adoption.
    let regression = json!([
        policy_attempt("b2", "baseline", 0.0, 100.0, true, &declaration),
        policy_attempt("c2", "candidate", 200.0, 120.0, true, &declaration),
    ]);
    fs::write(&input_path, serde_json::to_vec(&regression).unwrap()).unwrap();
    let rejected = cli()
        .arg("--input")
        .arg(&input_path)
        .arg("--policy")
        .arg(&declaration_path)
        .output()
        .unwrap();
    assert!(
        rejected.status.success(),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    let evaluation: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(evaluation["decision"], "reject", "{evaluation}");
}

#[test]
fn policy_entry_refuses_ambiguous_missing_malformed_oversized_and_tampered_inputs() {
    let root = tempfile::tempdir().unwrap();
    let policy_path = root.path().join("policy.json");
    let declaration_path = root.path().join("declaration.json");
    let input_path = root.path().join("attempts.json");
    let policy_json = policy_json();
    fs::write(&policy_path, serde_json::to_vec(&policy_json).unwrap()).unwrap();
    let policy: ComparisonPolicy = serde_json::from_value(policy_json).unwrap();
    let declaration = policy.declaration();
    fs::write(
        &input_path,
        serde_json::to_vec(&json!([
            policy_attempt("b", "baseline", 0.0, 100.0, true, &declaration),
            policy_attempt("c", "candidate", 200.0, 80.0, true, &declaration),
        ]))
        .unwrap(),
    )
    .unwrap();
    let declared_output = cli()
        .arg("--declare-policy")
        .arg(&policy_path)
        .output()
        .unwrap();
    assert!(declared_output.status.success());
    fs::write(&declaration_path, &declared_output.stdout).unwrap();
    let policy_bytes = fs::read(&policy_path).unwrap();
    let declaration_bytes = fs::read(&declaration_path).unwrap();
    let input_bytes = fs::read(&input_path).unwrap();

    let refused = |output: std::process::Output| {
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).is_empty());
    };

    refused(
        cli()
            .arg("--declare-policy")
            .arg(&policy_path)
            .arg("--input")
            .arg(&input_path)
            .output()
            .unwrap(),
    );
    refused(
        cli()
            .arg("--declare-policy")
            .arg(&policy_path)
            .arg("--policy")
            .arg(&declaration_path)
            .output()
            .unwrap(),
    );
    refused(
        cli()
            .arg("--declare-policy")
            .arg(&policy_path)
            .arg("--markdown")
            .output()
            .unwrap(),
    );
    refused(
        cli()
            .arg("--input")
            .arg(&input_path)
            .arg("--policy")
            .arg(&declaration_path)
            .arg("--markdown")
            .output()
            .unwrap(),
    );
    refused(
        cli()
            .arg("--policy")
            .arg(&declaration_path)
            .output()
            .unwrap(),
    );
    refused(
        cli()
            .arg("--input")
            .arg(&input_path)
            .arg("--input")
            .arg(&input_path)
            .output()
            .unwrap(),
    );
    refused(
        cli()
            .arg("--input")
            .arg(&input_path)
            .arg("--policy")
            .arg(&declaration_path)
            .arg("--policy")
            .arg(&declaration_path)
            .output()
            .unwrap(),
    );
    refused(
        cli()
            .arg("--declare-policy")
            .arg(&policy_path)
            .arg("--declare-policy")
            .arg(&policy_path)
            .output()
            .unwrap(),
    );
    refused(cli().arg("--policy").output().unwrap());
    refused(cli().arg("--input").arg("--markdown").output().unwrap());

    let missing = root.path().join("missing.json");
    refused(
        cli()
            .arg("--declare-policy")
            .arg(&missing)
            .output()
            .unwrap(),
    );
    refused(
        cli()
            .arg("--input")
            .arg(&missing)
            .arg("--policy")
            .arg(&declaration_path)
            .output()
            .unwrap(),
    );
    refused(
        cli()
            .arg("--input")
            .arg(&input_path)
            .arg("--policy")
            .arg(&missing)
            .output()
            .unwrap(),
    );

    let malformed_policy = root.path().join("malformed-policy.json");
    let malformed_declaration = root.path().join("malformed-declaration.json");
    let malformed_input = root.path().join("malformed-input.json");
    let oversized_policy = root.path().join("oversized-policy.json");
    let oversized_input = root.path().join("oversized-input.json");
    for (path, bytes) in [
        (&malformed_policy, b"private-parser-content".to_vec()),
        (&malformed_declaration, b"{}".to_vec()),
        (&malformed_input, b"{}".to_vec()),
        (&oversized_policy, vec![b'x'; 8 * 1024 * 1024 + 1]),
        (&oversized_input, vec![b'x'; 8 * 1024 * 1024 + 1]),
    ] {
        fs::write(path, &bytes).unwrap();
        let mut command = cli();
        if path == &malformed_policy || path == &oversized_policy {
            command.arg("--declare-policy").arg(path);
        } else if path == &malformed_input || path == &oversized_input {
            command
                .arg("--input")
                .arg(path)
                .arg("--policy")
                .arg(&declaration_path);
        } else {
            command
                .arg("--input")
                .arg(&input_path)
                .arg("--policy")
                .arg(path);
        }
        let output = command.output().unwrap();
        refused(output);
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    let mut tampered: Value = serde_json::from_slice(&declared_output.stdout).unwrap();
    tampered["policy"]["tolerancePercent"] = json!(6.0);
    let tampered_path = root.path().join("tampered.json");
    fs::write(&tampered_path, serde_json::to_vec(&tampered).unwrap()).unwrap();
    let tampered_bytes = fs::read(&tampered_path).unwrap();
    let output = cli()
        .arg("--input")
        .arg(&input_path)
        .arg("--policy")
        .arg(&tampered_path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("digest"));
    assert_eq!(fs::read(&tampered_path).unwrap(), tampered_bytes);

    assert_eq!(fs::read(&policy_path).unwrap(), policy_bytes);
    assert_eq!(fs::read(&declaration_path).unwrap(), declaration_bytes);
    assert_eq!(fs::read(&input_path).unwrap(), input_bytes);
}

#[test]
fn missing_decisive_evidence_never_adopts_through_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let policy_path = root.path().join("policy.json");
    let declaration_path = root.path().join("declaration.json");
    let input_path = root.path().join("attempts.json");
    let policy_json = policy_json();
    fs::write(&policy_path, serde_json::to_vec(&policy_json).unwrap()).unwrap();
    let policy: ComparisonPolicy = serde_json::from_value(policy_json).unwrap();
    let declaration = policy.declaration();
    fs::write(
        &declaration_path,
        serde_json::to_vec(&policy.declare().unwrap()).unwrap(),
    )
    .unwrap();
    let mut candidate = policy_attempt("c", "candidate", 200.0, 80.0, true, &declaration);
    candidate["native_runs"][0]
        .as_object_mut()
        .unwrap()
        .remove("tool_operations");
    let bytes = serde_json::to_vec(&json!([
        policy_attempt("b", "baseline", 0.0, 100.0, true, &declaration),
        candidate,
    ]))
    .unwrap();
    fs::write(&input_path, &bytes).unwrap();

    let output = cli()
        .arg("--input")
        .arg(&input_path)
        .arg("--policy")
        .arg(&declaration_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let evaluation: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(evaluation["decision"], "inconclusive", "{evaluation}");
    assert_ne!(evaluation["decision"], "adopt");
    assert!(
        evaluation["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| {
                reason
                    .as_str()
                    .is_some_and(|reason| reason.contains("metric coverage is incomplete"))
            }),
        "{evaluation}"
    );
    assert_eq!(fs::read(&input_path).unwrap(), bytes);
}

#[test]
fn composed_schema_two_report_preserves_corroboration_for_evaluation() {
    let root = tempfile::tempdir().unwrap();
    let policy_path = root.path().join("policy.json");
    let declaration_path = root.path().join("declaration.json");
    let input_path = root.path().join("report.json");
    let mut policy_json = policy_json();
    policy_json["stopping"]["requiredUnits"] = json!(2);
    fs::write(&policy_path, serde_json::to_vec(&policy_json).unwrap()).unwrap();
    let policy: ComparisonPolicy = serde_json::from_value(policy_json).unwrap();
    let declaration = policy.declaration();
    fs::write(
        &declaration_path,
        serde_json::to_vec(&policy.declare().unwrap()).unwrap(),
    )
    .unwrap();
    let rows = vec![
        policy_attempt("b", "baseline", 0.0, 100.0, true, &declaration),
        policy_attempt("c", "candidate", 200.0, 80.0, true, &declaration),
    ];
    let report = harness_core::outcome_report::summarize_attempts(&rows).unwrap();
    let receipt = json!({
        "schema": 1,
        "status": "selected",
        "required_units": 1,
        "selection": {
            "schema": 1,
            "requiredUnits": 1,
            "status": "ready",
            "units": [{
                "owner": "card-prior",
                "caseId": "case-prior",
                "experiment": "exp-prior",
                "mechanism": "bounded-output",
                "conditions": "local-tool-runs",
                "revision": "rev-prior",
                "treeSha256": "c".repeat(64)
            }],
            "excluded": [{
                "owner": "card-other",
                "caseId": "case-other",
                "reason": "notApplicable"
            }]
        },
        "reason": Value::Null
    });
    let report = harness_core::outcome_report::attach_corroboration(&report, &receipt).unwrap();
    assert_eq!(report["corroboration"]["status"], "ready");
    let corroboration_digest = report["corroboration"]["digest"].clone();
    fs::write(&input_path, serde_json::to_vec(&report).unwrap()).unwrap();
    let report_bytes = fs::read(&input_path).unwrap();

    let output = cli()
        .arg("--input")
        .arg(&input_path)
        .arg("--policy")
        .arg(&declaration_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let evaluation: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(evaluation["decision"], "adopt", "{evaluation}");
    assert_eq!(
        evaluation["corroboration"]["status"], "ready",
        "{evaluation}"
    );
    assert_eq!(evaluation["corroboration"]["ready"], true);
    assert_eq!(evaluation["corroboration"]["digest"], corroboration_digest);
    assert_eq!(
        evaluation["corroboration"]["excluded"][0]["owner"],
        "card-other"
    );
    assert_eq!(
        evaluation["corroboration"]["excluded"][0]["caseId"],
        "case-other"
    );
    assert_eq!(
        evaluation["corroboration"]["excluded"][0]["reason"],
        "notApplicable"
    );
    assert_eq!(fs::read(&input_path).unwrap(), report_bytes);
}

#[test]
fn schema_two_report_attempts_are_renormalized_before_evaluation() {
    let root = tempfile::tempdir().unwrap();
    let policy_path = root.path().join("policy.json");
    let declaration_path = root.path().join("declaration.json");
    let input_path = root.path().join("report.json");
    let policy_json = policy_json();
    fs::write(&policy_path, serde_json::to_vec(&policy_json).unwrap()).unwrap();
    let policy: ComparisonPolicy = serde_json::from_value(policy_json).unwrap();
    let declaration = policy.declaration();
    fs::write(
        &declaration_path,
        serde_json::to_vec(&policy.declare().unwrap()).unwrap(),
    )
    .unwrap();
    let rows = vec![
        policy_attempt("b", "baseline", 0.0, 100.0, true, &declaration),
        policy_attempt("c", "candidate", 200.0, 80.0, true, &declaration),
    ];
    let mut report = harness_core::outcome_report::summarize_attempts(&rows).unwrap();
    assert_eq!(
        report["units"][0]["effect"]["candidate_seconds"],
        json!(80.0)
    );
    assert_eq!(report["units"][0]["positive_effect"], json!(true));

    // Change the retained candidate attempt itself, while leaving every
    // derived positive field and status from the earlier serialization in
    // place. A schema number alone cannot make those fields authoritative.
    report["attempts"][1]["ended_at"] = json!(320.0);
    report["attempts"][1]["checks"][0]["ended_at"] = json!(320.0);
    assert_eq!(report["attempts"][1]["elapsed_seconds"], json!(80.0));
    assert_eq!(report["attempts"][1]["total_result_seconds"], json!(80.0));
    assert_eq!(report["attempts"][1]["status"], json!("accepted"));
    assert_eq!(
        report["units"][0]["effect"]["candidate_seconds"],
        json!(80.0)
    );
    fs::write(&input_path, serde_json::to_vec(&report).unwrap()).unwrap();

    let output = cli()
        .arg("--input")
        .arg(&input_path)
        .arg("--policy")
        .arg(&declaration_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let evaluation: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(evaluation["decision"], "reject", "{evaluation}");
}

#[test]
fn schema_two_report_attempt_bound_cannot_be_bypassed_by_derived_fields() {
    let root = tempfile::tempdir().unwrap();
    let policy_path = root.path().join("policy.json");
    let declaration_path = root.path().join("declaration.json");
    let input_path = root.path().join("report.json");
    let policy_json = policy_json();
    fs::write(&policy_path, serde_json::to_vec(&policy_json).unwrap()).unwrap();
    let policy: ComparisonPolicy = serde_json::from_value(policy_json).unwrap();
    let declaration = policy.declaration();
    fs::write(
        &declaration_path,
        serde_json::to_vec(&policy.declare().unwrap()).unwrap(),
    )
    .unwrap();
    let rows = vec![
        policy_attempt("b", "baseline", 0.0, 100.0, true, &declaration),
        policy_attempt("c", "candidate", 200.0, 80.0, true, &declaration),
    ];
    let mut report = harness_core::outcome_report::summarize_attempts(&rows).unwrap();
    assert_eq!(report["attempts"].as_array().unwrap().len(), 2);
    let mut attempts = report["attempts"].as_array().unwrap().clone();
    attempts.resize(1025, attempts[0].clone());
    report["attempts"] = Value::Array(attempts);
    assert_eq!(report["accounting"]["attempts"], json!(2));
    fs::write(&input_path, serde_json::to_vec(&report).unwrap()).unwrap();

    let output = cli()
        .arg("--input")
        .arg(&input_path)
        .arg("--policy")
        .arg(&declaration_path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).is_empty());
}
