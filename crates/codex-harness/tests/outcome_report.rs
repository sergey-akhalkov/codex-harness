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
