//! Infrastructure adjustment and the decision gate that consumes it.
//! Fixtures here are not actual-model acceptance.
use harness_core::improvement_policy::{
    Basis, ComparisonPolicy, Objective, Overhead, PolicyDecision, RepeatedSelection, StoppingRule,
    evaluate,
};
use harness_core::infrastructure_accounting::{
    MEASUREMENT_LINEAGE, Mechanism, MetricView, adjust, binding_clause, parse_binding,
};
use harness_core::outcome_report::{MATCH_FIELDS, summarize_attempts};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const MINUTE: u64 = 60 * 1_000_000_000;

fn wait_capture(useful_start: u64, useful_end: u64) -> Value {
    json!({
        "window": {"start_ns": 0, "end_ns": 30 * MINUTE},
        "detail_overflow": false,
        "activity_overflow": false,
        "admissions": [{
            "id": "adm-1",
            "class": "unrelated_wait",
            "domain_match": true,
            "start_ns": 0,
            "end_ns": 10 * MINUTE,
            "endpoint_start_ns": 0,
            "endpoint_end_ns": 0,
            "tick_ns": 0,
            "tool_call_id": "tool-1",
            "command_id": "cmd-1",
            "started": true,
            "terminal": "waited_grant"
        }],
        "activity": [
            {
                "id": "tool-1",
                "kind": "command",
                "start_ns": 0,
                "end_ns": 10 * MINUTE,
                "tool_call_id": "tool-1",
                "command_id": "cmd-1"
            },
            {
                "id": "message-1",
                "kind": "message",
                "start_ns": useful_start,
                "end_ns": useful_end
            }
        ],
        "requests": []
    })
}

#[test]
fn thirty_minute_attempt_deducts_only_verified_blocking() {
    let adjusted = adjust(&wait_capture(6 * MINUTE, 10 * MINUTE), Some(30 * MINUTE));
    assert_eq!(adjusted["observed_ns"], 30 * MINUTE);
    assert_eq!(adjusted["deductible_ns"], 6 * MINUTE, "{adjusted}");
    assert_eq!(adjusted["adjusted_ns"], 24 * MINUTE, "{adjusted}");
    assert_eq!(adjusted["observed_seconds"], 1800.0);
    assert_eq!(adjusted["adjusted_seconds"], 1440.0);
    assert_ne!(adjusted["observed_ns"], adjusted["adjusted_ns"]);
    assert!(adjusted["deductible_ns"].as_u64().unwrap() <= 30 * MINUTE);
}

#[test]
fn inherited_and_overlapping_waits_are_counted_once() {
    let mut capture = wait_capture(0, 0);
    capture["activity"][1]["end_ns"] = json!(0);
    capture["admissions"].as_array_mut().unwrap().push(json!({
        "id": "child",
        "parent_id": "adm-1",
        "class": "inherited",
        "domain_match": true
    }));
    capture["admissions"].as_array_mut().unwrap().push(json!({
        "id": "overlap",
        "class": "unrelated_wait",
        "domain_match": true,
        "start_ns": 5 * MINUTE,
        "end_ns": 12 * MINUTE,
        "endpoint_start_ns": 0,
        "endpoint_end_ns": 0,
        "tick_ns": 0,
        "tool_call_id": "tool-1",
        "command_id": "cmd-1",
        "started": true,
        "terminal": "waited_grant"
    }));
    // The second admission extends past the blocked command, so only the
    // covered portion is deductible and the rest is unresolved, not doubled.
    let adjusted = adjust(&capture, Some(30 * MINUTE));
    let deductible = adjusted["deductible_ns"].as_u64().unwrap();
    assert!(deductible <= 10 * MINUTE, "{adjusted}");
    assert!(
        adjusted["unresolved_ns"].as_u64().unwrap() > 0,
        "{adjusted}"
    );
}

#[test]
fn unknown_endpoint_self_contention_and_absent_trace_are_not_measured_zero() {
    let mut unknown = wait_capture(0, 0);
    unknown["admissions"][0]["endpoint_end_ns"] = Value::Null;
    let unknown = adjust(&unknown, Some(30 * MINUTE));
    assert_eq!(unknown["deductible_ns"], 0);
    assert!(unknown["unresolved_ns"].as_u64().unwrap() > 0, "{unknown}");

    let mut own = wait_capture(0, 0);
    own["admissions"][0]["class"] = json!("self_contention");
    let own = adjust(&own, Some(30 * MINUTE));
    assert_eq!(own["deductible_ns"], 0, "{own}");
    assert_eq!(own["unresolved_ns"], 0, "{own}");

    let absent = adjust(
        &json!({"window": {"start_ns": 0, "end_ns": 30 * MINUTE}, "admissions": [], "activity": [], "requests": []}),
        Some(30 * MINUTE),
    );
    assert_eq!(absent["proven_zero_queue"], false);
    assert_eq!(absent["absent_telemetry"], true);
    assert_eq!(absent["adjusted_low_ns"], 0);
    assert_eq!(absent["adjusted_high_ns"], 30 * MINUTE);

    let zero = adjust(
        &json!({
            "window": {"start_ns": 0, "end_ns": MINUTE},
            "admissions": [{"id": "now", "class": "measured_zero", "domain_match": true}],
            "activity": [],
            "requests": []
        }),
        Some(MINUTE),
    );
    assert_eq!(zero["proven_zero_queue"], true, "{zero}");
    assert_eq!(zero["deductible_ns"], 0);
}

#[test]
fn wait_only_usage_is_excluded_and_mixed_usage_stays() {
    let mut capture = wait_capture(11 * MINUTE, 12 * MINUTE);
    capture["requests"] = json!([
        {
            "id": "wait-1",
            "wait_only": true,
            "start_ns": MINUTE,
            "end_ns": 2 * MINUTE,
            "tool_call_id": "tool-1",
            "command_id": "cmd-1",
            "input_tokens": 10,
            "cached_input_tokens": 4,
            "output_tokens": 2,
            "reasoning_output_tokens": 1,
            "total_tokens": 12
        },
        {
            "id": "mixed-1",
            "wait_only": false,
            "start_ns": 3 * MINUTE,
            "end_ns": 4 * MINUTE,
            "tool_call_id": "tool-1",
            "command_id": "cmd-1",
            "input_tokens": 8,
            "cached_input_tokens": 0,
            "output_tokens": 3,
            "reasoning_output_tokens": 0,
            "total_tokens": 11
        }
    ]);
    let adjusted = adjust(&capture, Some(30 * MINUTE));
    assert_eq!(adjusted["usage"]["raw"]["total_tokens"], 23);
    assert_eq!(
        adjusted["usage"]["excluded"]["total_tokens"], 12,
        "{adjusted}"
    );
    assert_eq!(adjusted["usage"]["adjusted"]["total_tokens"], 11);
    assert_eq!(adjusted["excluded_requests"], json!(["wait-1"]));
    assert_eq!(adjusted["polling_operations"], 1);
}

#[test]
fn overflow_is_not_inactivity_or_full_coverage() {
    let adjusted = adjust(
        &json!({
            "window": {"start_ns": 0, "end_ns": MINUTE},
            "detail_overflow": true,
            "activity_overflow": true,
            "admissions": [],
            "activity": [],
            "requests": []
        }),
        Some(MINUTE),
    );
    assert_eq!(adjusted["detail_overflow"], true);
    assert_ne!(adjusted["coverage"], "measured");
    assert_eq!(adjusted["proven_zero_queue"], false);
    assert!(
        adjusted["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap == "raw_detail_overflow")
    );
}

fn policy(uncertainty: &str) -> ComparisonPolicy {
    ComparisonPolicy {
        schema: 1,
        objective: Objective::Time,
        basis: Basis::Efficiency,
        meaningful_effect_percent: Some(10.0),
        tolerance_percent: 5.0,
        require_acceptance: true,
        task_mix: "one frozen task case".into(),
        stopping: StoppingRule {
            max_attempts_per_arm: 2,
            required_units: 1,
        },
        repeated_selection: RepeatedSelection::Predeclared,
        trade_off: None,
        uncertainty: uncertainty.into(),
        horizon_tasks: 1.0,
        overhead: Overhead {
            implementation_seconds: 0.0,
            evaluation_seconds: 0.0,
            maintenance_seconds_per_task: 0.0,
        },
    }
}

fn row(id: &str, arm: &str, seconds: f64, capture: Value) -> Value {
    let matched: BTreeMap<&str, &str> = MATCH_FIELDS.iter().map(|key| (*key, "fixed")).collect();
    json!({
        "attempt_id": id,
        "case_id": "case",
        "arm": arm,
        "experiment_id": "exp",
        "started_at": 0.0,
        "ended_at": seconds,
        "discovery_verified": true,
        "observed_model_metadata_verified": true,
        "matched": matched,
        "native_runs": [{"started_at": 0.0, "ended_at": seconds, "status": "completed", "rounds": 1, "tool_operations": 1}],
        "checks": [{
            "id": "acceptance",
            "started_at": 1.0,
            "ended_at": seconds,
            "required": true,
            "executed": true,
            "passed": true,
            "exit_code": 0,
            "evidence": "private/log"
        }],
        "children": [],
        "interventions": [],
        "retry_of": null,
        "infrastructure_capture": capture,
        "declaration": policy(&binding_clause(MetricView::WorkEfficiency, Mechanism::None)).declaration(),
    })
}

fn point_capture(wait_ns: u64, total_ns: u64) -> Value {
    json!({
        "window": {"start_ns": 0, "end_ns": total_ns},
        "admissions": [{
            "id": "adm",
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
            "start_ns": 0,
            "end_ns": wait_ns,
            "tool_call_id": "tool-1",
            "command_id": "cmd-1"
        }],
        "requests": []
    })
}

#[test]
fn queue_only_difference_is_not_an_improvement_or_regression() {
    let declared = policy(&binding_clause(MetricView::WorkEfficiency, Mechanism::None))
        .declare()
        .unwrap();
    let baseline = row(
        "base",
        "baseline",
        40.0,
        point_capture(10_000_000_000, 40_000_000_000),
    );
    let candidate = row("cand", "candidate", 30.0, point_capture(0, 30_000_000_000));
    // The candidate capture with a zero-length wait is not an admission. Use a
    // proven immediate grant so the adjusted times match.
    let mut candidate = candidate;
    candidate["infrastructure_capture"] = json!({
        "window": {"start_ns": 0, "end_ns": 30_000_000_000_u64},
        "admissions": [{"id": "now", "class": "measured_zero", "domain_match": true}],
        "activity": [],
        "requests": []
    });
    let report = summarize_attempts(&[baseline, candidate]).unwrap();
    assert_eq!(report["attempts"][0]["elapsed_seconds"], 40.0);
    assert!(
        report["attempts"][0]["infrastructure"]["adjusted_seconds"]
            .as_f64()
            .unwrap()
            < 40.0
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(evaluation.decision, PolicyDecision::Adopt, "{evaluation:?}");
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("external waiting")),
        "{evaluation:?}"
    );
}

#[test]
fn a_verdict_changing_gap_is_inconclusive_and_a_bound_can_settle() {
    let declared = policy(&binding_clause(MetricView::WorkEfficiency, Mechanism::None))
        .declare()
        .unwrap();
    let mut gap = point_capture(10_000_000_000, 40_000_000_000);
    gap["admissions"][0]["endpoint_end_ns"] = Value::Null;
    let report = summarize_attempts(&[
        row("base", "baseline", 40.0, gap),
        row("cand", "candidate", 30.0, point_capture(0, 30_000_000_000)),
    ])
    .unwrap();
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{evaluation:?}"
    );

    let settled = policy(&binding_clause(MetricView::WorkEfficiency, Mechanism::None))
        .declare()
        .unwrap();
    let mut baseline = row(
        "base",
        "baseline",
        40.0,
        json!({
            "window": {"start_ns": 0, "end_ns": 40_000_000_000_u64},
            "admissions": [{"id": "now", "class": "measured_zero", "domain_match": true}],
            "activity": [],
            "requests": []
        }),
    );
    let mut candidate = row(
        "cand",
        "candidate",
        20.0,
        json!({
            "window": {"start_ns": 0, "end_ns": 20_000_000_000_u64},
            "admissions": [{"id": "now", "class": "measured_zero", "domain_match": true}],
            "activity": [],
            "requests": []
        }),
    );
    baseline["declaration"] = settled.policy.declaration();
    candidate["declaration"] = settled.policy.declaration();
    let report = summarize_attempts(&[baseline, candidate]).unwrap();
    let evaluation = evaluate(&settled, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Adopt, "{evaluation:?}");
}

#[test]
fn changed_binding_cannot_inherit_a_decision_and_waiting_is_not_subtracted() {
    let original = policy(&binding_clause(MetricView::WorkEfficiency, Mechanism::None))
        .declare()
        .unwrap();
    let changed = policy(&binding_clause(MetricView::Operational, Mechanism::None))
        .declare()
        .unwrap();
    assert_ne!(original.digest, changed.digest);
    assert!(
        parse_binding(&original.policy.uncertainty)
            .unwrap()
            .is_some()
    );
    let waiting = policy(&binding_clause(
        MetricView::WorkEfficiency,
        Mechanism::Waiting,
    ))
    .declare()
    .unwrap();
    let report = summarize_attempts(&[
        row(
            "base",
            "baseline",
            40.0,
            point_capture(10_000_000_000, 40_000_000_000),
        ),
        row("cand", "candidate", 30.0, point_capture(0, 30_000_000_000)),
    ])
    .unwrap();
    let mut report = report;
    report["attempts"][0]["declaration"] = waiting.policy.declaration();
    report["attempts"][1]["declaration"] = waiting.policy.declaration();
    report["units"][0]["declared"]["uncertainty"] = json!(waiting.policy.uncertainty);
    let evaluation = evaluate(&waiting, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{evaluation:?}"
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("controlled load")),
        "{evaluation:?}"
    );
    assert_eq!(MEASUREMENT_LINEAGE, "infrastructure-attribution.v1");
}

#[test]
fn queue_timeout_is_not_an_incorrect_solution() {
    let declared = policy(&binding_clause(MetricView::WorkEfficiency, Mechanism::None))
        .declare()
        .unwrap();
    let mut candidate = row(
        "cand",
        "candidate",
        30.0,
        json!({
            "window": {"start_ns": 0, "end_ns": 30_000_000_000_u64},
            "admissions": [{
                "id": "adm",
                "class": "failed",
                "domain_match": true,
                "started": false,
                "terminal": "timeout"
            }],
            "activity": [],
            "requests": []
        }),
    );
    candidate["checks"][0]["passed"] = json!(false);
    candidate["checks"][0]["exit_code"] = json!(1);
    candidate["native_runs"][0]["status"] = json!("failed");
    let baseline = row(
        "base",
        "baseline",
        30.0,
        json!({
            "window": {"start_ns": 0, "end_ns": 30_000_000_000_u64},
            "admissions": [{"id": "now", "class": "measured_zero", "domain_match": true}],
            "activity": [],
            "requests": []
        }),
    );
    let report = summarize_attempts(&[baseline, candidate]).unwrap();
    assert_eq!(report["attempts"][1]["elapsed_seconds"], 30.0);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{evaluation:?}"
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("not an incorrect model solution")),
        "{evaluation:?}"
    );
}
