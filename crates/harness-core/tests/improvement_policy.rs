//! Native counterexamples for the predeclared comparison policy. Every case
//! consumes the authoritative outcome accounting
//! (`outcome_report::summarize_attempts`); no case substitutes a candidate
//! claim, a synthetic statistic or a post-hoc threshold.
use harness_core::improvement_policy::{
    Basis, ComparisonPolicy, DeclaredComparison, Objective, Overhead, PerSuccessStatus,
    PolicyDecision, RepeatedSelection, StoppingRule, TradeOff, evaluate,
    refuse_cross_task_speed_claim,
};
use harness_core::outcome_report::{MATCH_FIELDS, summarize_attempts};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn policy() -> ComparisonPolicy {
    ComparisonPolicy {
        schema: 1,
        objective: Objective::Time,
        basis: Basis::Efficiency,
        meaningful_effect_percent: Some(10.0),
        tolerance_percent: 5.0,
        require_acceptance: true,
        task_mix: "one frozen task case".into(),
        stopping: StoppingRule {
            max_attempts_per_arm: 3,
            required_units: 1,
        },
        repeated_selection: RepeatedSelection::Predeclared,
        trade_off: None,
        uncertainty: "unknown evidence stays inconclusive".into(),
        horizon_tasks: 5.0,
        overhead: Overhead {
            implementation_seconds: 10.0,
            evaluation_seconds: 5.0,
            maintenance_seconds_per_task: 0.5,
        },
    }
}

fn declare(policy: &ComparisonPolicy) -> DeclaredComparison {
    policy.declare().expect("declared policy")
}

#[allow(clippy::too_many_arguments)]
fn attempt(
    id: &str,
    arm: &str,
    case: &str,
    start: f64,
    seconds: f64,
    accepted: bool,
    rounds: Option<u64>,
    tools: Option<u64>,
    usage: bool,
) -> Value {
    let matched: BTreeMap<&str, &str> = MATCH_FIELDS.iter().map(|key| (*key, "fixed")).collect();
    let mut native = json!({"started_at": start, "ended_at": start + 2.0, "status": "completed"});
    if let Some(rounds) = rounds {
        native["rounds"] = json!(rounds);
    }
    if let Some(tools) = tools {
        native["tool_operations"] = json!(tools);
    }
    if usage {
        native["usage"] = json!({
            "model": "fixed",
            "input_tokens": 100,
            "cached_input_tokens": 40,
            "output_tokens": 20,
            "reasoning_tokens": 5,
        });
    }
    json!({
        "attempt_id": id,
        "case_id": case,
        "arm": arm,
        "experiment_id": "exp-1",
        "started_at": start,
        "ended_at": start + seconds,
        "discovery_verified": true,
        "observed_model_metadata_verified": true,
        "matched": matched,
        "native_runs": [native],
        "checks": [{
            "id": "acceptance",
            "started_at": start + 2.0,
            "ended_at": start + seconds,
            "required": true,
            "executed": true,
            "passed": accepted,
            "exit_code": if accepted { 0 } else { 1 },
            "evidence": "private/log",
        }],
        "children": [],
        "interventions": [],
        "retry_of": null,
    })
}

fn summarize(rows: &[Value], policy: &ComparisonPolicy) -> Value {
    let rows: Vec<Value> = rows
        .iter()
        .map(|row| {
            let mut row = row.clone();
            row["declaration"] = policy.declaration();
            row
        })
        .collect();
    summarize_attempts(&rows).expect("authoritative summary")
}

/// Declare a subtractive treatment on one authoritative accounting row: the
/// removed burden is recorded on the arm's treatment block, and its actual
/// consumption is recorded separately from invocation counts.
fn subtractive(mut row: Value, status: &str, evidenced: bool) -> Value {
    row["treatment"] = json!({"kind": "subtraction", "removed": "catalogue-skill"});
    row["consumption"] = json!({
        "capability": "catalogue-skill",
        "status": status,
        "evidence": if evidenced { "private/consumption.json" } else { "" },
    });
    row
}

#[test]
fn policy_declaration_is_validated_before_results() {
    let declared = declare(&policy());
    assert_eq!(declared, declare(&policy()), "digests must be stable");
    assert!(declared.digest_matches(&declared.digest));

    let mut invalid = policy();
    invalid.require_acceptance = false;
    assert!(
        invalid.declare().is_err(),
        "the correctness gate is mandatory"
    );
    let mut invalid = policy();
    invalid.meaningful_effect_percent = None;
    assert!(invalid.declare().is_err(), "efficiency needs an effect");
    let mut invalid = policy();
    invalid.meaningful_effect_percent = Some(0.0);
    assert!(
        invalid.declare().is_err(),
        "a zero effect is not meaningful"
    );
    let mut invalid = policy();
    invalid.tolerance_percent = -1.0;
    assert!(invalid.declare().is_err());
    let mut invalid = policy();
    invalid.repeated_selection = RepeatedSelection::BestOf;
    invalid.stopping.required_units = 1;
    assert!(
        invalid.declare().is_err(),
        "best-of selection needs independent corroboration"
    );
    let mut invalid = policy();
    invalid.basis = Basis::Maintenance { basis: "  ".into() };
    assert!(
        invalid.declare().is_err(),
        "a maintenance basis is required"
    );
    let mut invalid = policy();
    invalid.horizon_tasks = 0.0;
    assert!(invalid.declare().is_err());
    let mut invalid = policy();
    invalid.task_mix.clear();
    assert!(invalid.declare().is_err());
}

#[test]
fn adoption_requires_a_measured_meaningful_effect() {
    let policy = policy();
    let declared = declare(&policy);
    let report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                85.0,
                true,
                Some(3),
                Some(6),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Adopt);
    assert_eq!(
        evaluation.quality,
        harness_core::improvement_policy::Quality::Unchanged
    );
    assert_eq!(evaluation.matched, 1);
    assert_eq!(evaluation.baseline_seconds, Some(100.0));
    assert_eq!(evaluation.candidate_seconds, Some(85.0));
    assert_eq!(evaluation.per_success.status, PerSuccessStatus::Complete);
    assert!(
        evaluation.coverage.contains("time"),
        "{}",
        evaluation.coverage
    );
    let draft = evaluation
        .decision_draft(
            "sample-task",
            "exp-1",
            "base-sha",
            "cand-sha",
            "acceptance/locator",
        )
        .unwrap();
    let record = draft.record().unwrap();
    assert!(record.starts_with("benefit-gate v2"), "{record}");
    assert!(record.contains("outcome=adopt"), "{record}");
}

#[test]
fn no_effect_or_regression_cannot_pass_the_default_policy() {
    let policy = policy();
    let declared = declare(&policy);
    let no_effect = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &no_effect).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("meaningful threshold")),
        "{:?}",
        evaluation.reasons
    );

    let regression = summarize(
        &[
            attempt(
                "b2",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c2",
                "candidate",
                "case-b",
                200.0,
                120.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &regression).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("regressed the primary time metric")),
        "{:?}",
        evaluation.reasons
    );

    let below_threshold = summarize(
        &[
            attempt(
                "b3",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c3",
                "candidate",
                "case-b",
                200.0,
                97.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &below_threshold).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
}

#[test]
fn failed_attempts_remain_accounted_and_zero_success_is_undefined() {
    let policy = policy();
    let declared = declare(&policy);
    // A retried candidate: the failed attempt stays inside the chain total.
    let mut failed = attempt(
        "c1",
        "candidate",
        "case-b",
        200.0,
        10.0,
        false,
        Some(2),
        Some(3),
        true,
    );
    failed["retry_of"] = Value::Null;
    let mut retry = attempt(
        "c2",
        "candidate",
        "case-b",
        210.0,
        15.0,
        true,
        Some(2),
        Some(3),
        true,
    );
    retry["retry_of"] = json!("c1");
    let report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            failed,
            retry,
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Adopt);
    assert_eq!(evaluation.attempts, 3, "the failed attempt stays counted");
    assert_eq!(
        evaluation.candidate_seconds,
        Some(25.0),
        "the retry chain total includes the failed attempt's time"
    );
    assert_eq!(evaluation.per_success.status, PerSuccessStatus::Complete);
    assert_eq!(
        evaluation.per_success.seconds,
        Some(62.5),
        "per-success cost includes the failed attempt exactly once"
    );
    assert_eq!(evaluation.acceptance_rate, Some(1.0));

    // The candidate never achieves acceptance while the baseline does.
    let report = summarize(
        &[
            attempt(
                "b2",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c3",
                "candidate",
                "case-b",
                200.0,
                40.0,
                false,
                Some(2),
                Some(3),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("independent acceptance failed")),
        "{:?}",
        evaluation.reasons
    );
    assert_eq!(
        evaluation.per_success.status,
        PerSuccessStatus::Complete,
        "the failed attempt stays inside the cost accounting"
    );

    // No task succeeds on either arm: the per-success cost is undefined and
    // no number may be reported.
    let report = summarize(
        &[
            attempt(
                "b3",
                "baseline",
                "case-b",
                0.0,
                100.0,
                false,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c4",
                "candidate",
                "case-b",
                200.0,
                40.0,
                false,
                Some(2),
                Some(3),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(evaluation.decision, PolicyDecision::Adopt);
    assert_eq!(evaluation.per_success.status, PerSuccessStatus::Undefined);
    assert_eq!(evaluation.per_success.seconds, None);
    assert_eq!(evaluation.accepted_tasks, 0);
    assert!(
        evaluation
            .per_success
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("undefined")),
        "{:?}",
        evaluation.per_success
    );
}

#[test]
fn incomplete_metrics_and_unknown_coverage_are_not_a_decision_basis() {
    let policy = policy();
    let declared = declare(&policy);
    // Tool operations are unknown on the candidate: the declared coverage of
    // the "other" dimension is incomplete.
    let report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                80.0,
                true,
                Some(3),
                None,
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("metric coverage is incomplete")),
        "{:?}",
        evaluation.reasons
    );

    // An unverified model identity leaves the comparison evidence incomplete.
    let mut candidate = attempt(
        "c2",
        "candidate",
        "case-b",
        200.0,
        80.0,
        true,
        Some(3),
        Some(6),
        true,
    );
    candidate["observed_model_metadata_verified"] = json!(false);
    let report = summarize(
        &[
            attempt(
                "b2",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            candidate,
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("comparison evidence is incomplete")),
        "{:?}",
        evaluation.reasons
    );

    // A resource objective requires measured usage coverage on both arms.
    let mut resource = policy.clone();
    resource.objective = Objective::Resource;
    let declared_resource = declare(&resource);
    let report = summarize(
        &[
            attempt(
                "b3",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                false,
            ),
            attempt(
                "c3",
                "candidate",
                "case-b",
                200.0,
                80.0,
                true,
                Some(3),
                Some(6),
                false,
            ),
        ],
        &resource,
    );
    let evaluation = evaluate(&declared_resource, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("Usage")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn trade_offs_stopping_and_repeated_selection_need_predeclared_rules() {
    let policy = policy();
    let declared = declare(&policy);
    // Time improves, matched steps double: without a declared trade-off this
    // is not an adoption.
    let report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                80.0,
                true,
                Some(8),
                Some(12),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("trade-off")),
        "{:?}",
        evaluation.reasons
    );

    // The same measurement under a predeclared trade-off is adoptable.
    let mut traded = policy.clone();
    traded.trade_off = Some(TradeOff {
        basis: "halved wall time at doubled tool steps was agreed before results".into(),
        allowed_regression_percent: 250.0,
    });
    let declared_traded = declare(&traded);
    let traded_report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                80.0,
                true,
                Some(8),
                Some(12),
                true,
            ),
        ],
        &traded,
    );
    let evaluation = evaluate(&declared_traded, &traded_report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Adopt);
    assert!(evaluation.trade_off_used);

    // A no-effect result cannot be adopted through a maintenance basis that
    // was never declared, and a declared basis stays separate from efficiency.
    let no_effect = summarize(
        &[
            attempt(
                "b2",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c2",
                "candidate",
                "case-b",
                200.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &no_effect).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);

    let mut maintenance = policy.clone();
    maintenance.basis = Basis::Maintenance {
        basis: "user agreed before results: simpler implementation, no efficiency claim".into(),
    };
    maintenance.meaningful_effect_percent = None;
    let declared_maintenance = declare(&maintenance);
    let report = summarize(
        &[
            attempt(
                "b3",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c3",
                "candidate",
                "case-b",
                200.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
        ],
        &maintenance,
    );
    let evaluation = evaluate(&declared_maintenance, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Adopt);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("maintenance basis")),
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .decision_draft("sample-task", "exp-1", "base-sha", "cand-sha", "acceptance")
            .is_err(),
        "a maintenance adoption is not an efficiency benefit decision"
    );

    // Exceeding the predeclared stopping budget is not a decision basis.
    let mut budget = policy.clone();
    budget.stopping.max_attempts_per_arm = 1;
    let declared_budget = declare(&budget);
    let mut retry = attempt(
        "c5",
        "candidate",
        "case-b",
        210.0,
        15.0,
        true,
        Some(2),
        Some(3),
        true,
    );
    retry["retry_of"] = json!("c4");
    let report = summarize(
        &[
            attempt(
                "b4",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c4",
                "candidate",
                "case-b",
                200.0,
                10.0,
                false,
                Some(2),
                Some(3),
                true,
            ),
            retry,
        ],
        &budget,
    );
    let evaluation = evaluate(&declared_budget, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("stopping budget")),
        "{:?}",
        evaluation.reasons
    );

    // Best-of selection with one corroboration unit is not enough.
    let mut best_of = policy.clone();
    best_of.repeated_selection = RepeatedSelection::BestOf;
    best_of.stopping.required_units = 2;
    let declared_best_of = declare(&best_of);
    let report = summarize(
        &[
            attempt(
                "b5",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c6",
                "candidate",
                "case-b",
                200.0,
                80.0,
                true,
                Some(3),
                Some(6),
                true,
            ),
        ],
        &best_of,
    );
    let evaluation = evaluate(&declared_best_of, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("corroboration")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn unmatched_units_and_cross_task_trends_are_not_comparisons() {
    let policy = policy();
    let declared = declare(&policy);
    // Baseline ran on B, candidate on C: no matched unit exists.
    let report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c1",
                "candidate",
                "case-c",
                200.0,
                80.0,
                true,
                Some(3),
                Some(6),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("no matched")),
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation.scope.contains("not a speed trend"),
        "{}",
        evaluation.scope
    );
    assert!(refuse_cross_task_speed_claim(&["case-b", "case-c"]).is_err());
    assert!(refuse_cross_task_speed_claim(&["case-b"]).is_ok());
    assert!(refuse_cross_task_speed_claim(&["case-b", "case-b"]).is_ok());
}

#[test]
fn shared_costs_and_repeated_edges_count_once_and_drift_is_invalid() {
    let policy = policy();
    let declared = declare(&policy);
    // Two candidate results behind one baseline are repeated selection, not
    // two samples: the unit is excluded instead of double counting.
    let report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                80.0,
                true,
                Some(3),
                Some(6),
                true,
            ),
            attempt(
                "c2",
                "candidate",
                "case-b",
                300.0,
                70.0,
                true,
                Some(3),
                Some(6),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(evaluation.decision, PolicyDecision::Adopt);
    assert_eq!(evaluation.matched, 0, "repeated edges are one sample");

    // Inherited child work and overlapping token categories are consumed from
    // the authoritative accounting exactly once, never summed here.
    let mut candidate = attempt(
        "c3",
        "candidate",
        "case-b",
        200.0,
        80.0,
        true,
        Some(3),
        Some(6),
        true,
    );
    candidate["children"] = json!([{
        "attempt_id": "b2",
        "started_at": 200.0,
        "ended_at": 280.0,
        "usage": {"input_tokens": 100, "cached_input_tokens": 40},
    }]);
    let rows = vec![
        attempt(
            "b2",
            "baseline",
            "case-b",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        candidate,
    ];
    let summary = summarize(&rows, &policy);
    let evaluation = evaluate(&declared, &summary).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Adopt);
    assert_eq!(
        evaluation.per_success.seconds,
        summary["accounting"]["cost_per_accepted_task"]["seconds"].as_f64(),
        "per-success cost is the authoritative value, not a token-category sum"
    );
    assert!(evaluation.coverage.contains("usage"));
    assert_eq!(
        evaluation.attempts, 2,
        "shared child work is not an attempt"
    );

    // A recorded declaration that differs from the predeclared policy
    // invalidates the comparison instead of silently re-scoring it.
    let mut drifted = policy.clone();
    let rows: Vec<Value> = vec![
        attempt(
            "b3",
            "baseline",
            "case-b",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        attempt(
            "c4",
            "candidate",
            "case-b",
            200.0,
            80.0,
            true,
            Some(3),
            Some(6),
            true,
        ),
    ];
    drifted.meaningful_effect_percent = Some(20.0);
    let rows: Vec<Value> = rows
        .into_iter()
        .map(|mut row| {
            row["declaration"] = policy.declaration();
            row
        })
        .collect();
    let summary = summarize_attempts(&rows).unwrap();
    let drifted_declared = declare(&drifted);
    let evaluation = evaluate(&drifted_declared, &summary).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("declaration differs")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn a_mutated_or_serialized_stale_declaration_is_refused() {
    let policy = policy();
    let declared = declare(&policy);
    let report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                80.0,
                true,
                Some(3),
                Some(6),
                true,
            ),
        ],
        &policy,
    );
    assert_eq!(
        evaluate(&declared, &report).unwrap().decision,
        PolicyDecision::Adopt,
        "an untouched declared policy keeps its behavior"
    );

    // Fields mutated after declaration must not ride on the original digest.
    let mut relaxed = declared.clone();
    relaxed.policy.tolerance_percent = 50.0;
    assert!(evaluate(&relaxed, &report).is_err());
    assert!(
        !relaxed.digest_matches(&declared.digest),
        "the stored digest no longer binds the changed policy"
    );
    let mut traded = declared.clone();
    traded.policy.trade_off = Some(TradeOff {
        basis: "added after results".into(),
        allowed_regression_percent: 500.0,
    });
    assert!(evaluate(&traded, &report).is_err());
    let mut basis = declared.clone();
    basis.policy.basis = Basis::Maintenance {
        basis: "claimed after results".into(),
    };
    assert!(evaluate(&basis, &report).is_err());
    let mut invalid = declared.clone();
    invalid.policy.tolerance_percent = -1.0;
    assert!(evaluate(&invalid, &report).is_err());

    // The same holds for a serialized declaration whose fields were edited.
    let mut value = serde_json::to_value(&declared).unwrap();
    value["policy"]["tolerancePercent"] = json!(50.0);
    let serialized: DeclaredComparison = serde_json::from_value(value).unwrap();
    assert_eq!(serialized.digest, declared.digest);
    assert!(evaluate(&serialized, &report).is_err());
}

#[test]
fn a_favourable_subset_cannot_offset_a_primary_regression() {
    let policy = policy();
    let declared = declare(&policy);
    // One favourable matched unit reaches the required count, while a larger
    // regression in the same declared task mix makes the aggregate adoption
    // unsupported.
    let report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                80.0,
                true,
                Some(3),
                Some(6),
                true,
            ),
            attempt(
                "b2",
                "baseline",
                "case-c",
                400.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c2",
                "candidate",
                "case-c",
                600.0,
                140.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("regressed the primary time metric")),
        "{:?}",
        evaluation.reasons
    );
    assert_eq!(
        evaluation.matched, 2,
        "both matched units stay in the record"
    );
    assert_eq!(evaluation.attempts, 4, "all attempts remain accounted");
    assert_eq!(evaluation.accepted_tasks, 4);
    assert_eq!(evaluation.per_success.status, PerSuccessStatus::Complete);
    assert_eq!(evaluation.baseline_seconds, Some(200.0));
    assert_eq!(
        evaluation.candidate_seconds,
        Some(220.0),
        "the aggregate regressed even though one unit improved"
    );
    let draft = evaluation
        .decision_draft("sample-task", "exp-1", "base-sha", "cand-sha", "acceptance")
        .unwrap();
    assert!(draft.record().unwrap().contains("outcome=reject"));
}

#[test]
fn maintenance_basis_does_not_cover_material_secondary_regression() {
    let mut maintenance = policy();
    maintenance.basis = Basis::Maintenance {
        basis: "user agreed before results: simpler implementation, no efficiency claim".into(),
    };
    maintenance.meaningful_effect_percent = None;
    let declared_maintenance = declare(&maintenance);

    // Time objective: no meaningful time gain and 40% more steps.
    let report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                100.0,
                true,
                Some(6),
                Some(8),
                true,
            ),
        ],
        &maintenance,
    );
    let evaluation = evaluate(&declared_maintenance, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("does not cover a material regression")),
        "{:?}",
        evaluation.reasons
    );
    assert!(!evaluation.trade_off_used);

    // The same measurement under a predeclared trade-off runs through the
    // explicit policy path instead of a bypass.
    let mut traded = maintenance.clone();
    traded.trade_off = Some(TradeOff {
        basis: "predeclared: fewer steps may cost extra operations".into(),
        allowed_regression_percent: 50.0,
    });
    let declared_traded = declare(&traded);
    let evaluation = evaluate(&declared_traded, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Adopt);
    assert!(evaluation.trade_off_used);

    // A regression inside the declared tolerance stays admissible.
    let within = summarize(
        &[
            attempt(
                "b2",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(40),
                Some(60),
                true,
            ),
            attempt(
                "c2",
                "candidate",
                "case-b",
                200.0,
                100.0,
                true,
                Some(41),
                Some(62),
                true,
            ),
        ],
        &maintenance,
    );
    let evaluation = evaluate(&declared_maintenance, &within).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Adopt);
    assert!(!evaluation.trade_off_used);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("maintenance basis")),
        "{:?}",
        evaluation.reasons
    );

    // Symmetric resource objective: no meaningful step gain, 40% more time.
    let mut resource = policy();
    resource.objective = Objective::Resource;
    resource.basis = Basis::Maintenance {
        basis: "agreed before results".into(),
    };
    resource.meaningful_effect_percent = None;
    let declared_resource = declare(&resource);
    let report = summarize(
        &[
            attempt(
                "b3",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            attempt(
                "c3",
                "candidate",
                "case-b",
                200.0,
                140.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
        ],
        &resource,
    );
    let evaluation = evaluate(&declared_resource, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("does not cover a material regression")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn subtractive_treatments_require_actual_consumption_evidence() {
    let policy = policy();
    let declared = declare(&policy);

    // Stale context after removal: the candidate is faster, but it still
    // records consumption of the removed burden, so the intended context
    // treatment was never established.
    let report = summarize(
        &[
            subtractive(
                attempt(
                    "b1",
                    "baseline",
                    "case-b",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "consumed",
                true,
            ),
            subtractive(
                attempt(
                    "c1",
                    "candidate",
                    "case-b",
                    200.0,
                    80.0,
                    true,
                    Some(3),
                    Some(6),
                    true,
                ),
                "consumed",
                true,
            ),
        ],
        &policy,
    );
    let unit = &report["units"][0];
    assert_eq!(unit["subtractive"], true);
    assert_eq!(unit["removed_burden"], "catalogue-skill");
    assert_eq!(unit["applicability"], "unknown");
    assert_eq!(unit["evidence_complete"], false);
    assert_eq!(unit["positive_effect"], false);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(evaluation.decision, PolicyDecision::Adopt);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("not established")),
        "{:?}",
        evaluation.reasons
    );

    // Inapplicable workload: neither arm ever consumes the removed burden, so
    // an apparently faster pair cannot establish a saving and broader
    // usefulness stays unresolved rather than rejected.
    let report = summarize(
        &[
            subtractive(
                attempt(
                    "b2",
                    "baseline",
                    "case-b",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "absent",
                true,
            ),
            subtractive(
                attempt(
                    "c2",
                    "candidate",
                    "case-b",
                    200.0,
                    80.0,
                    true,
                    Some(3),
                    Some(6),
                    true,
                ),
                "absent",
                true,
            ),
        ],
        &policy,
    );
    assert_eq!(report["units"][0]["applicability"], "not_exercised");
    assert_eq!(report["units"][0]["evidence_complete"], false);
    assert_eq!(report["units"][0]["positive_effect"], false);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("never exercised")),
        "{:?}",
        evaluation.reasons
    );

    // Zero invocations without a consumption record: invocation counts are
    // not consumption, and a smaller source tree is not measured context.
    let report = summarize(
        &[
            {
                let mut row = attempt(
                    "b3",
                    "baseline",
                    "case-b",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(0),
                    true,
                );
                row["treatment"] = json!({"kind": "subtraction", "removed": "catalogue-skill"});
                row
            },
            {
                let mut row = attempt(
                    "c3",
                    "candidate",
                    "case-b",
                    200.0,
                    80.0,
                    true,
                    Some(3),
                    Some(0),
                    true,
                );
                row["treatment"] = json!({"kind": "subtraction", "removed": "catalogue-skill"});
                row
            },
        ],
        &policy,
    );
    assert_eq!(report["units"][0]["applicability"], "unknown");
    assert!(
        unit_limitations(&report["units"][0])
            .iter()
            .any(|limitation| limitation.contains("zero invocations")),
        "{:?}",
        report["units"][0]["limitations"]
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(evaluation.decision, PolicyDecision::Adopt);

    // Positive control: the burden was consumed before removal and the
    // candidate is observed not to consume it, with retained checks, so the
    // same accounting can support the scoped saving.
    let report = summarize(
        &[
            subtractive(
                attempt(
                    "b4",
                    "baseline",
                    "case-b",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "consumed",
                true,
            ),
            subtractive(
                attempt(
                    "c4",
                    "candidate",
                    "case-b",
                    200.0,
                    80.0,
                    true,
                    Some(3),
                    Some(6),
                    true,
                ),
                "absent",
                true,
            ),
        ],
        &policy,
    );
    assert_eq!(report["units"][0]["applicability"], "exercised");
    assert_eq!(report["units"][0]["evidence_complete"], true);
    assert_eq!(report["units"][0]["positive_effect"], true);
    assert_eq!(
        evaluate(&declared, &report).unwrap().decision,
        PolicyDecision::Adopt
    );
}

fn unit_limitations(unit: &Value) -> Vec<String> {
    unit["limitations"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn deleted_acceptance_coverage_cannot_support_a_subtractive_adoption() {
    let policy = policy();
    let declared = declare(&policy);
    let baseline = subtractive(
        attempt(
            "b1",
            "baseline",
            "case-b",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        "consumed",
        true,
    );

    // The candidate replaces the baseline's required check with a weaker one
    // that passes: its own record looks accepted and faster, but it records
    // fewer required checks than the baseline.
    let mut candidate = subtractive(
        attempt(
            "c1",
            "candidate",
            "case-b",
            200.0,
            80.0,
            true,
            Some(3),
            Some(6),
            true,
        ),
        "absent",
        true,
    );
    candidate["checks"][0]["id"] = json!("quick-check");
    let report = summarize(&[baseline.clone(), candidate], &policy);
    assert_eq!(report["units"][0]["retained_checks"], false);
    assert_eq!(report["units"][0]["evidence_complete"], false);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("fewer required checks")),
        "{:?}",
        evaluation.reasons
    );

    // Deleting the acceptance check outright leaves the candidate without
    // independent acceptance, and the faster attempt cannot adopt.
    let mut candidate = subtractive(
        attempt(
            "c2",
            "candidate",
            "case-b",
            200.0,
            40.0,
            true,
            Some(2),
            Some(3),
            true,
        ),
        "absent",
        true,
    );
    candidate["checks"] = json!([]);
    let report = summarize(&[baseline, candidate], &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("independent acceptance failed")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn subtractive_acceptance_and_repayment_gates_apply_to_removals() {
    let policy = policy();
    let declared = declare(&policy);
    let baseline = subtractive(
        attempt(
            "b1",
            "baseline",
            "case-b",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        "consumed",
        true,
    );

    // A removed wrapper with a supported indirect caller: much faster, but
    // the retained-behavior acceptance fails.
    let candidate = subtractive(
        attempt(
            "c1",
            "candidate",
            "case-b",
            200.0,
            40.0,
            false,
            Some(2),
            Some(3),
            true,
        ),
        "absent",
        true,
    );
    let report = summarize(&[baseline.clone(), candidate], &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("independent acceptance failed")),
        "{:?}",
        evaluation.reasons
    );

    // Fewer automated steps that shift work into a slower manual fallback do
    // not repay the declared per-task maintenance over the use horizon.
    let mut resource = policy.clone();
    resource.objective = Objective::Resource;
    resource.meaningful_effect_percent = Some(10.0);
    resource.overhead.maintenance_seconds_per_task = 10.0;
    resource.horizon_tasks = 5.0;
    let declared_resource = declare(&resource);
    let report = summarize(
        &[
            subtractive(
                attempt(
                    "b2",
                    "baseline",
                    "case-b",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(10),
                    true,
                ),
                "consumed",
                true,
            ),
            subtractive(
                attempt(
                    "c2",
                    "candidate",
                    "case-b",
                    200.0,
                    100.0,
                    true,
                    Some(2),
                    Some(5),
                    true,
                ),
                "absent",
                true,
            ),
        ],
        &resource,
    );
    assert_eq!(
        report["units"][0]["net_saving"]["verdict"],
        "does_not_repay"
    );
    let evaluation = evaluate(&declared_resource, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("does not repay")),
        "{:?}",
        evaluation.reasons
    );

    // A subtractive efficiency result must record how the per-task effect
    // repays the declared use horizon; without that statement the same
    // faster pair cannot be adopted on an unstated net effect.
    let rows = [
        subtractive(
            attempt(
                "b3",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            "consumed",
            true,
        ),
        subtractive(
            attempt(
                "c3",
                "candidate",
                "case-b",
                200.0,
                80.0,
                true,
                Some(3),
                Some(6),
                true,
            ),
            "absent",
            true,
        ),
    ];
    let mut declaration = policy.declaration();
    declaration.as_object_mut().unwrap().remove("costs");
    let recorded: Vec<Value> = rows
        .iter()
        .cloned()
        .map(|mut row| {
            row["declaration"] = declaration.clone();
            row
        })
        .collect();
    let report = summarize_attempts(&recorded).unwrap();
    assert!(report["units"][0]["net_saving"].is_null());
    assert_eq!(report["units"][0]["evidence_complete"], true);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("no repayment")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn maintenance_only_subtraction_requires_the_preagreed_recorded_basis() {
    let mut maintenance = policy();
    maintenance.basis = Basis::Maintenance {
        basis: "user agreed before results: maintainability only, no efficiency claim".into(),
    };
    maintenance.meaningful_effect_percent = None;
    let declared = declare(&maintenance);
    let rows = [
        subtractive(
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            "consumed",
            true,
        ),
        subtractive(
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            "absent",
            true,
        ),
    ];
    let report = summarize(&rows, &maintenance);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Adopt);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("maintenance basis")),
        "{:?}",
        evaluation.reasons
    );

    // Evidence recorded without the pre-agreed basis (or with another one)
    // cannot become a post-hoc maintenance exemption.
    let mut declaration = maintenance.declaration();
    declaration.as_object_mut().unwrap().remove("basis");
    let recorded: Vec<Value> = rows
        .iter()
        .cloned()
        .map(|mut row| {
            row["declaration"] = declaration.clone();
            row
        })
        .collect();
    let report = summarize_attempts(&recorded).unwrap();
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(evaluation.decision, PolicyDecision::Adopt);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("basis")),
        "{:?}",
        evaluation.reasons
    );

    // The same no-effect removal under the default efficiency policy cannot
    // pass: a maintenance-only claim needs the separate predeclared basis.
    let efficient = declare(&policy());
    let report = summarize(&rows, &policy());
    let evaluation = evaluate(&efficient, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("meaningful threshold")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn size_only_subtractive_results_cannot_pass_the_default_policy() {
    let policy = policy();
    let declared = declare(&policy);
    let mut baseline = subtractive(
        attempt(
            "b1",
            "baseline",
            "case-b",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        "consumed",
        true,
    );
    let mut candidate = subtractive(
        attempt(
            "c1",
            "candidate",
            "case-b",
            200.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        "absent",
        true,
    );
    baseline["size"] = json!({"source_files": 120, "bytes": 90000});
    candidate["size"] = json!({"source_files": 20, "bytes": 15000});
    let report = summarize(&[baseline, candidate], &policy);
    assert_eq!(report["units"][0]["applicability"], "exercised");
    assert_eq!(report["units"][0]["positive_effect"], false);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Reject);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("reduced size or an unmeasured benefit")),
        "{:?}",
        evaluation.reasons
    );
}
