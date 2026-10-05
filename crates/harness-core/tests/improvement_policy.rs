//! Native counterexamples for the predeclared comparison policy. Every case
//! consumes the authoritative outcome accounting
//! (`outcome_report::summarize_attempts`); no case substitutes a candidate
//! claim, a synthetic statistic or a post-hoc threshold.
use harness_core::improvement_policy::{
    AnalysisMethod, BASELINE_REUSE_CLAUSE, BaselineReusePlan, Basis, ClaimScope, ComparisonPolicy,
    DeclaredComparison, EffectPath, ExperimentMethod, ExperimentSelection, FaultRule, InitialState,
    LoadRule, MeasurementStatus, NuisanceControlPlan, Objective, OrderRule, Overhead,
    PerSuccessStatus, PolicyDecision, REUSE_IDENTITIES, RealizedFirst, RepeatedSelection,
    RetryRule, SharedState, StatisticalClaim, StoppingRule, TradeOff, VariationStatus,
    baseline_reuse_clause, evaluate, experiment_selection_clause, nuisance_control_clause,
    parse_baseline_reuse, parse_experiment_selection, parse_nuisance_control,
    parse_statistical_claim, refuse_cross_task_speed_claim, statistical_clause,
};
use harness_core::infrastructure_accounting::{Mechanism, MetricView, binding_clause};
use harness_core::outcome_report::{
    CorroborationSection, CorroborationState, MATCH_FIELDS, attach_corroboration,
    corroboration_digest, summarize_attempts,
};
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

fn claim(scope: ClaimScope) -> StatisticalClaim {
    StatisticalClaim {
        method: AnalysisMethod::ObservedPairsDescriptive,
        confidence_percent: None,
        scope,
        assumptions: "complete paired attempts fixed before results".to_owned(),
    }
}

/// Declare the statistical-analysis clause in the policy's uncertainty text.
fn with_claim(mut policy: ComparisonPolicy, scope: ClaimScope) -> ComparisonPolicy {
    policy.uncertainty = format!(
        "unknown evidence stays inconclusive; {}",
        statistical_clause(&claim(scope))
    );
    policy
}

/// Declared policy with both the statistical claim and the work-efficiency
/// infrastructure binding, in the order the owners require.
fn with_claim_and_binding(scope: ClaimScope) -> ComparisonPolicy {
    let mut policy = with_claim(policy(), scope);
    policy.uncertainty = format!(
        "{}; {}",
        statistical_clause(&claim(scope)),
        binding_clause(MetricView::WorkEfficiency, Mechanism::None)
    );
    policy
}

/// Record a precomputed measurement/attribution bound on an attempt. The
/// evaluator consumes exactly this contract; the accounting-side construction
/// of these values is exercised by the infrastructure-accounting tests. The
/// rule identity and the raw/adjusted/excluded identity make the retained
/// evidence reconcilable, as a real reduction would be.
fn bounded(row: &mut Value, low: f64, high: f64) {
    let observed_ns = (high * 1_000_000_000.0).round() as u64;
    let low_ns = (low * 1_000_000_000.0).round() as u64;
    row["infrastructure"] = json!({
        "rule_version": harness_core::infrastructure_accounting::RULE_VERSION,
        "lineage": harness_core::infrastructure_accounting::MEASUREMENT_LINEAGE,
        "eligible_cause": harness_core::infrastructure_accounting::ELIGIBLE_CAUSE,
        "observed_ns": observed_ns,
        "observed_seconds": high,
        "adjusted_ns": observed_ns,
        "adjusted_seconds": high,
        "adjusted_low_ns": low_ns,
        "adjusted_high_ns": observed_ns,
        "adjusted_low_seconds": low,
        "adjusted_high_seconds": high,
        "deductible_ns": 0,
        "deductible_seconds": 0.0,
        "unresolved_ns": observed_ns - low_ns,
        "unresolved_seconds": high - low,
        "coverage": "measured",
    });
}

fn pair(mut row: Value, pair_id: &str) -> Value {
    row["pair_id"] = json!(pair_id);
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

// ---------------------------------------------------------------------------
// Retained baseline reuse.
// ---------------------------------------------------------------------------

/// Declare the baseline-reuse clause in the policy's uncertainty text. The
/// clause must follow the clauses owned by the other parsers.
fn with_reuse(scope: ClaimScope, max_age_seconds: f64) -> ComparisonPolicy {
    let mut policy = with_claim(policy(), scope);
    policy.uncertainty = format!(
        "{}; {}",
        statistical_clause(&claim(scope)),
        baseline_reuse_clause(&BaselineReusePlan { max_age_seconds })
    );
    policy
}

/// One retained-baseline reuse record as the evidence owner normalizes it:
/// the retained execution identity, when it ran, its age, the retained trace,
/// coverage and uncertainty, the predeclared selection and the conditions it
/// was measured under.
fn retained(row: Value, executed_at: f64, selected_at: f64, age_seconds: f64) -> Value {
    let mut row = row;
    row["reuse"] = json!({
        "of": "retained-execution-1",
        "executed_at": executed_at,
        "age_seconds": age_seconds,
        "trace": "private/retained/trace.jsonl",
        "coverage": "time+method:real-operation; complete-pairs:1",
        "uncertainty": "single-pair variation remains unmeasured",
        "selection": {"at": selected_at, "basis": "declared before candidate results"},
        "conditions": {"cache": "owned-cold", "load": "recorded-idle"},
        "qualification": "verified",
    });
    row
}

/// The current arm's actually observed conditions, which a reused baseline
/// must still match.
fn conditions(row: Value, cache: &str, load: &str) -> Value {
    let mut row = row;
    row["conditions"] = json!({"cache": cache, "load": load});
    row
}

#[test]
fn a_baseline_reuse_policy_is_frozen_before_results() {
    let plan = BaselineReusePlan {
        max_age_seconds: 3600.0,
    };
    let clause = baseline_reuse_clause(&plan);
    assert!(clause.contains(REUSE_IDENTITIES), "{clause}");
    let declared = declare(&with_reuse(ClaimScope::Scoped, 3600.0));
    assert_eq!(
        parse_baseline_reuse(&declared.policy.uncertainty).unwrap(),
        Some(plan),
        "the frozen reuse policy round-trips through its clause"
    );
    let declaration = declared.policy.declaration();
    assert_eq!(declaration["baseline_reuse"]["max_age_seconds"], 3600.0);
    assert_eq!(declaration["baseline_reuse"]["trace"], "required");
    assert_eq!(
        declaration["baseline_reuse"]["identities"],
        REUSE_IDENTITIES
    );

    // A waived identity, an optional trace, an unbounded age or an unrelated
    // model qualification cannot be declared before results.
    let base = format!(
        "unknown evidence stays inconclusive; {BASELINE_REUSE_CLAUSE}; identities={REUSE_IDENTITIES}; trace=required; qualification=model-context; max-age-seconds=3600"
    );
    assert!(parse_baseline_reuse(&base).unwrap().is_some());
    for broken in [
        base.replace(REUSE_IDENTITIES, "input+runtime"),
        base.replace("trace=required", "trace=optional"),
        base.replace("qualification=model-context", "qualification=none"),
        base.replace("max-age-seconds=3600", "max-age-seconds=0"),
        base.replace("max-age-seconds=3600", "max-age-seconds=999999999999"),
        base.replace("max-age-seconds=3600", "max-age-seconds=soon"),
    ] {
        assert!(parse_baseline_reuse(&broken).is_err(), "{broken}");
    }
    let mut invalid = policy();
    invalid.uncertainty = base.replace(REUSE_IDENTITIES, "input+runtime");
    assert!(
        invalid.declare().is_err(),
        "a waived reuse identity is refused at declaration"
    );

    // The clause must follow the nuisance-control clause so each owner parses
    // its own fields, and the declared policy validates the whole text.
    let ordered = format!(
        "unknown evidence stays inconclusive; {}; {}",
        nuisance_control_clause(&nuisance_plan()),
        clause
    );
    assert!(parse_nuisance_control(&ordered).unwrap().is_some());
    assert_eq!(parse_baseline_reuse(&ordered).unwrap(), Some(plan));
    let mut accepted = policy();
    accepted.uncertainty = ordered;
    assert!(accepted.declare().is_ok());
    let misordered = format!(
        "unknown evidence stays inconclusive; {clause}; {}",
        nuisance_control_clause(&nuisance_plan())
    );
    assert!(parse_baseline_reuse(&misordered).is_err());
}

#[test]
fn a_valid_retained_operation_baseline_is_reused_without_a_fresh_baseline_run() {
    let policy = with_reuse(ClaimScope::Scoped, 3600.0);
    let declared = declare(&policy);
    let baseline = retained(
        attempt(
            "rb",
            "baseline",
            "case-r",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        0.0,
        500.0,
        1200.0,
    );
    let candidate = conditions(
        attempt(
            "rc",
            "candidate",
            "case-r",
            1000.0,
            50.0,
            true,
            Some(3),
            Some(5),
            true,
        ),
        "owned-cold",
        "recorded-idle",
    );
    let report = summarize(&[baseline, candidate], &policy);
    let unit = &report["units"][0];
    assert_eq!(unit["baseline_reused"], true, "{unit}");
    assert_eq!(
        unit["baseline_executed_now"], false,
        "the retained baseline is never presented as a fresh run: {unit}"
    );
    assert_eq!(unit["reuse"]["of"], "retained-execution-1", "{unit}");
    assert_eq!(unit["reuse"]["age_seconds"], 1200.0, "{unit}");
    assert_eq!(
        unit["reuse"]["original_seconds"], 100.0,
        "the retained execution's original cost is preserved: {unit}"
    );
    assert_eq!(unit["reuse"]["model_metrics"], "measured", "{unit}");
    assert_eq!(
        report["accounting"]["reused_baseline_attempts"], 1,
        "{report}"
    );
    assert_eq!(
        report["accounting"]["reuse_refused_attempts"], 0,
        "{report}"
    );

    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    assert_eq!(evaluation.baseline_seconds, Some(100.0));
    assert_eq!(evaluation.candidate_seconds, Some(50.0));
    assert_eq!(evaluation.reused_baselines.len(), 1, "{evaluation:?}");
    let reuse = &evaluation.reused_baselines[0];
    assert_eq!(reuse.unit, unit["unit"]);
    assert_eq!(reuse.attempt, "retained-execution-1");
    assert_eq!(reuse.executed_at, 0.0);
    assert_eq!(reuse.selected_at, 500.0);
    assert_eq!(reuse.age_seconds, 1200.0);
    assert_eq!(reuse.original_seconds, Some(100.0));
    assert!(reuse.trace.contains("trace.jsonl"), "{reuse:?}");
    assert!(reuse.coverage.contains("complete-pairs:1"), "{reuse:?}");
    assert!(reuse.uncertainty.contains("unmeasured"), "{reuse:?}");
    assert_eq!(reuse.model_metrics, "measured");
    assert_eq!(reuse.qualification, "verified");
    assert!(
        evaluation
            .coverage
            .contains("baseline-reuse:1 retained attempt(s)"),
        "{}",
        evaluation.coverage
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("retained baseline result(s)")),
        "the decision names the consumed retained baseline: {:?}",
        evaluation.reasons
    );
}

#[test]
fn stale_retained_source_identity_refuses_baseline_reuse_with_the_exact_fact() {
    let policy = with_reuse(ClaimScope::Scoped, 3600.0);
    let declared = declare(&policy);
    let mut baseline = retained(
        attempt(
            "rb",
            "baseline",
            "case-r",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        0.0,
        500.0,
        1200.0,
    );
    baseline["matched"]["source_state"] = json!("retained-frozen-tree");
    let mut candidate = conditions(
        attempt(
            "rc",
            "candidate",
            "case-r",
            1000.0,
            50.0,
            true,
            Some(3),
            Some(5),
            true,
        ),
        "owned-cold",
        "recorded-idle",
    );
    candidate["matched"]["source_state"] = json!("current-tree");

    let report = summarize(&[baseline, candidate], &policy);
    assert!(
        report["units"].as_array().unwrap().is_empty(),
        "a stale retained baseline supplies no comparable unit: {report}"
    );
    let pair = &report["comparisons"][0];
    assert!(
        pair["excluded_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason.as_str().is_some_and(
                |text| text.contains("reuse-refused") && text.contains("source_state")
            )),
        "{pair}"
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("source_state") && reason.contains("fresh control")),
        "{:?}",
        evaluation.reasons
    );

    // The same refusal covers the other required identity groups: a changed
    // runtime identity is named exactly instead of being averaged over.
    let mut baseline = retained(
        attempt(
            "rb2",
            "baseline",
            "case-r",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        0.0,
        500.0,
        1200.0,
    );
    baseline["matched"]["runtime"] = json!("retained-runtime");
    let mut candidate = conditions(
        attempt(
            "rc2",
            "candidate",
            "case-r",
            1000.0,
            50.0,
            true,
            Some(3),
            Some(5),
            true,
        ),
        "owned-cold",
        "recorded-idle",
    );
    candidate["matched"]["runtime"] = json!("current-runtime");
    let report = summarize(&[baseline, candidate], &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("different runtime") && reason.contains("fresh control")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn changed_or_unverified_retained_conditions_refuse_baseline_reuse() {
    let policy = with_reuse(ClaimScope::Scoped, 3600.0);
    let declared = declare(&policy);
    let pair = |candidate: Value| {
        let baseline = retained(
            attempt(
                "rb",
                "baseline",
                "case-r",
                0.0,
                100.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
            0.0,
            500.0,
            1200.0,
        );
        summarize(&[baseline, candidate], &policy)
    };
    let reason_of = |report: &Value| -> Vec<String> {
        report["comparisons"][0]["excluded_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };

    // The current arm records a different cache condition: the retained
    // baseline was measured under another state and cannot be reused.
    let candidate = conditions(
        attempt(
            "rc",
            "candidate",
            "case-r",
            1000.0,
            50.0,
            true,
            Some(3),
            Some(5),
            true,
        ),
        "warm",
        "recorded-idle",
    );
    let report = pair(candidate);
    assert!(report["units"].as_array().unwrap().is_empty(), "{report}");
    let reasons = reason_of(&report);
    assert!(
        reasons.iter().any(|reason| reason.contains("reuse-refused")
            && reason.contains("cache")
            && reason.contains("warm")),
        "{reasons:?}"
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("cache") && reason.contains("fresh control")),
        "{:?}",
        evaluation.reasons
    );

    // The current comparison records no conditions at all: unchanged
    // conditions cannot be verified, so the retained baseline is refused.
    let candidate = attempt(
        "rc",
        "candidate",
        "case-r",
        1000.0,
        50.0,
        true,
        Some(3),
        Some(5),
        true,
    );
    let report = pair(candidate);
    let reasons = reason_of(&report);
    assert!(
        reasons.iter().any(|reason| reason.contains("reuse-refused")
            && reason.contains("does not record its cache condition")),
        "{reasons:?}"
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("cannot be verified")),
        "{:?}",
        evaluation.reasons
    );

    // An undeclared condition cannot be absorbed silently either: a recorded
    // condition the reuse does not verify refuses the reuse.
    let mut baseline = retained(
        attempt(
            "rb",
            "baseline",
            "case-r",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        0.0,
        500.0,
        1200.0,
    );
    baseline["reuse"]["conditions"] =
        json!({"cache": "owned-cold", "load": "recorded-idle", "gpu": "shared"});
    let candidate = conditions(
        attempt(
            "rc",
            "candidate",
            "case-r",
            1000.0,
            50.0,
            true,
            Some(3),
            Some(5),
            true,
        ),
        "owned-cold",
        "recorded-idle",
    );
    let report = summarize(&[baseline, candidate], &policy);
    assert!(
        report["attempts"][0]["reuse_refused"][0]
            .as_str()
            .is_some_and(|reason| reason.contains("unknown condition 'gpu'")),
        "{report}"
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("unknown condition 'gpu'")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn a_missing_retained_trace_refuses_baseline_reuse() {
    let policy = with_reuse(ClaimScope::Scoped, 3600.0);
    let declared = declare(&policy);
    let mut baseline = retained(
        attempt(
            "rb",
            "baseline",
            "case-r",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        0.0,
        500.0,
        1200.0,
    );
    baseline["reuse"]["trace"] = json!("");
    let candidate = conditions(
        attempt(
            "rc",
            "candidate",
            "case-r",
            1000.0,
            50.0,
            true,
            Some(3),
            Some(5),
            true,
        ),
        "owned-cold",
        "recorded-idle",
    );
    let report = summarize(&[baseline, candidate], &policy);
    assert!(
        report["attempts"][0]["reuse_refused"][0]
            .as_str()
            .is_some_and(|reason| reason.contains("trace is missing")),
        "{report}"
    );
    assert_eq!(
        report["accounting"]["reuse_refused_attempts"], 1,
        "{report}"
    );
    assert!(report["units"].as_array().unwrap().is_empty(), "{report}");
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("trace is missing")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn baseline_selection_after_the_candidate_result_is_refused() {
    let policy = with_reuse(ClaimScope::Scoped, 3600.0);
    let declared = declare(&policy);
    // The retained execution and conditions are valid and comparable; only
    // the recorded selection happens after the candidate arm already ran.
    let baseline = retained(
        attempt(
            "rb",
            "baseline",
            "case-r",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        0.0,
        1100.0,
        1200.0,
    );
    let candidate = conditions(
        attempt(
            "rc",
            "candidate",
            "case-r",
            1000.0,
            50.0,
            true,
            Some(3),
            Some(5),
            true,
        ),
        "owned-cold",
        "recorded-idle",
    );
    let report = summarize(&[baseline, candidate], &policy);
    assert_eq!(
        report["units"][0]["baseline_reused"], true,
        "the evidence owner records the retained facts; the policy decides their admissibility: {report}"
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("post-result baseline selection")
                && reason.contains("1100.0")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn retained_evidence_beyond_the_declared_age_bound_is_refused() {
    let policy = with_reuse(ClaimScope::Scoped, 600.0);
    let declared = declare(&policy);
    let baseline = retained(
        attempt(
            "rb",
            "baseline",
            "case-r",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        0.0,
        500.0,
        1200.0,
    );
    let candidate = conditions(
        attempt(
            "rc",
            "candidate",
            "case-r",
            1000.0,
            50.0,
            true,
            Some(3),
            Some(5),
            true,
        ),
        "owned-cold",
        "recorded-idle",
    );
    let report = summarize(&[baseline, candidate], &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation.reasons.iter().any(|reason| reason
            .contains("1200.0 s old")
            && reason.contains("600.0 s reuse bound")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn reuse_without_a_predeclared_clause_cannot_support_a_decision() {
    let policy = policy();
    let declared = declare(&policy);
    let baseline = retained(
        attempt(
            "rb",
            "baseline",
            "case-r",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        0.0,
        500.0,
        1200.0,
    );
    let candidate = conditions(
        attempt(
            "rc",
            "candidate",
            "case-r",
            1000.0,
            50.0,
            true,
            Some(3),
            Some(5),
            true,
        ),
        "owned-cold",
        "recorded-idle",
    );
    let report = summarize(&[baseline, candidate], &policy);
    assert_eq!(report["units"][0]["baseline_reused"], true);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("without a predeclared baseline-reuse clause")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn a_model_free_reused_baseline_keeps_its_model_metrics_inapplicable() {
    let policy = with_reuse(ClaimScope::Scoped, 3600.0);
    let declared = declare(&policy);
    let mut baseline = retained(
        operation_attempt("ob", "baseline", "case-m", 0.0, 100.0, true),
        0.0,
        500.0,
        1200.0,
    );
    // A method without model execution requires no model qualification: the
    // evidence owner normalizes the applicability itself instead of demanding
    // unrelated model evidence.
    baseline["reuse"]
        .as_object_mut()
        .unwrap()
        .remove("qualification");
    let candidate = conditions(
        operation_attempt("oc", "candidate", "case-m", 1000.0, 70.0, true),
        "owned-cold",
        "recorded-idle",
    );
    let report = summarize(&[baseline, candidate], &policy);
    assert_eq!(
        report["accounting"]["reuse_refused_attempts"], 0,
        "{report}"
    );
    let unit = &report["units"][0];
    assert_eq!(unit["baseline_reused"], true, "{unit}");
    assert_eq!(unit["model_metrics"], "inapplicable", "{unit}");
    assert_eq!(unit["reuse"]["model_metrics"], "inapplicable", "{unit}");
    assert_eq!(unit["reuse"]["qualification"], "inapplicable", "{unit}");

    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    assert_eq!(evaluation.reused_baselines.len(), 1);
    let reuse = &evaluation.reused_baselines[0];
    assert_eq!(reuse.model_metrics, "inapplicable");
    assert_eq!(reuse.qualification, "inapplicable");
    assert!(
        evaluation.coverage.contains("model-metrics:inapplicable"),
        "{}",
        evaluation.coverage
    );
    assert!(
        evaluation
            .coverage
            .contains("baseline-reuse:1 retained attempt(s)"),
        "{}",
        evaluation.coverage
    );
    // Inapplicable stays non-numeric: no model metric or model saving is
    // fabricated for the retained evidence.
    let serialized = serde_json::to_string(&evaluation).unwrap();
    assert!(
        !serialized.contains("model_saving") && !serialized.contains("model_tokens"),
        "{serialized}"
    );
}

#[test]
fn a_model_dependent_reuse_needs_its_runtime_qualification_and_a_retained_baseline_arm() {
    // A model-dependent reuse without the verified qualification and context
    // isolation is refused by the evidence owner with the exact reason.
    let policy = with_reuse(ClaimScope::Scoped, 3600.0);
    let mut baseline = retained(
        attempt(
            "rb",
            "baseline",
            "case-r",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        0.0,
        500.0,
        1200.0,
    );
    baseline["reuse"]
        .as_object_mut()
        .unwrap()
        .remove("qualification");
    let candidate = conditions(
        attempt(
            "rc",
            "candidate",
            "case-r",
            1000.0,
            50.0,
            true,
            Some(3),
            Some(5),
            true,
        ),
        "owned-cold",
        "recorded-idle",
    );
    let report = summarize(&[baseline, candidate], &policy);
    assert!(
        report["attempts"][0]["reuse_refused"][0]
            .as_str()
            .is_some_and(|reason| reason.contains("model-dependent reuse requires")),
        "{report}"
    );

    // Only a retained baseline may be reused: a reused candidate would
    // replace the measured treatment.
    let mut candidate = conditions(
        attempt(
            "rc",
            "candidate",
            "case-r",
            1000.0,
            50.0,
            true,
            Some(3),
            Some(5),
            true,
        ),
        "owned-cold",
        "recorded-idle",
    );
    candidate["reuse"] = json!({
        "of": "retained-execution-2",
        "executed_at": 200.0,
        "age_seconds": 100.0,
        "trace": "private/retained/candidate.jsonl",
        "coverage": "time+method:agent-task",
        "uncertainty": "unmeasured",
        "selection": {"at": 300.0, "basis": "declared"},
        "conditions": {"cache": "owned-cold", "load": "recorded-idle"},
        "qualification": "verified",
    });
    let baseline = retained(
        attempt(
            "rb",
            "baseline",
            "case-r",
            0.0,
            100.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
        0.0,
        500.0,
        1200.0,
    );
    let report = summarize(&[baseline, candidate], &policy);
    assert!(
        report["attempts"][1]["reuse_refused"][0]
            .as_str()
            .is_some_and(|reason| reason.contains("only a retained baseline may be reused")),
        "{report}"
    );
}
// ---------------------------------------------------------------------------
// Frozen nuisance-control plan for paired same-task runs.
// ---------------------------------------------------------------------------

fn nuisance_plan() -> NuisanceControlPlan {
    NuisanceControlPlan {
        initial: InitialState::OwnedCold,
        recipe: None,
        shared: SharedState::Unobserved,
        order: OrderRule::Fixed,
        seed: None,
        pairs: None,
        load: LoadRule::Recorded,
        faults: FaultRule::ObservedEffect,
        retries: RetryRule::PolicyStopping,
    }
}

fn with_nuisance(mut policy: ComparisonPolicy) -> ComparisonPolicy {
    policy.uncertainty = format!(
        "unknown evidence stays inconclusive; {}",
        nuisance_control_clause(&nuisance_plan())
    );
    policy
}

/// Attach the controller's observed nuisance record to one authoritative arm
/// row, exactly as a measured arm of a plan-bound comparison carries it.
fn nuisance_record(mut row: Value, realized: &str) -> Value {
    let plan = nuisance_plan();
    row["nuisance"] = json!({
        "schema": 1,
        "plan": plan.declaration(),
        "order": {
            "planned": realized,
            "realized": realized,
            "exposure": "the single pair runs one order; its exposure to time/order drift stays retained",
        },
        "initial": {
            "declared": "owned-cold",
            "observed": "empty-or-absent-owned-state",
            "violated": false,
            "condition_source": "observed-owned-state",
        },
        "load": {
            "rule": "recorded",
            "observed": {
                "unrelated_wait": false,
                "self_contention": false,
                "admission_evidence": "unobserved",
            },
            "uncontrolled": true,
        },
        "faults": [],
    });
    row
}

fn plan_pair(baseline_seconds: f64, candidate_seconds: f64) -> Vec<Value> {
    vec![
        nuisance_record(
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                baseline_seconds,
                true,
                Some(4),
                Some(6),
                true,
            ),
            "baseline-first",
        ),
        nuisance_record(
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                candidate_seconds,
                true,
                Some(3),
                Some(6),
                true,
            ),
            "baseline-first",
        ),
    ]
}

#[test]
fn a_nuisance_plan_is_frozen_and_cannot_loosen_its_conditions() {
    let plan = nuisance_plan();
    assert_eq!(plan.realized_first(), Some(RealizedFirst::BaselineFirst));
    let clause = nuisance_control_clause(&plan);
    let policy = with_nuisance(policy());
    let declared = declare(&policy);
    assert_eq!(
        parse_nuisance_control(&declared.policy.uncertainty)
            .unwrap()
            .unwrap(),
        plan
    );
    // The plan is part of the digested declaration: a changed plan cannot
    // inherit an earlier adoption.
    let mut changed = policy.clone();
    changed.uncertainty = format!(
        "unknown evidence stays inconclusive; {}",
        nuisance_control_clause(&NuisanceControlPlan {
            order: OrderRule::Randomized,
            seed: Some(0),
            ..plan.clone()
        })
    );
    assert_ne!(declare(&changed).digest, declared.digest);
    // A recorded attempt carries the frozen plan before results.
    let declaration = declared.policy.declaration();
    assert_eq!(declaration["nuisance_plan"]["shared"], json!("unobserved"));
    assert_eq!(
        declaration["nuisance_plan"]["realized_first"],
        json!("baseline-first")
    );
    assert_eq!(
        declaration["nuisance_plan"]["faults"],
        json!("observed-effect")
    );
    assert_eq!(
        declaration["nuisance_plan"]["retries"],
        json!("policy-stopping")
    );

    // Every loosened or invented declaration is refused before results.
    let refusal = |body: &str, needle: &str| {
        let mut candidate = policy.clone();
        candidate.uncertainty = format!("unknown evidence stays inconclusive; {body}");
        let error = candidate
            .declare()
            .expect_err(&format!("{body} must be refused"))
            .to_string();
        assert!(error.contains(needle), "{body}: {error}");
    };
    refusal(
        &clause.replace("shared=unobserved", "shared=observed"),
        "not observed",
    );
    refusal(
        &clause.replace("shared=unobserved", "shared=reset"),
        "not observed",
    );
    refusal(
        &clause.replace("load=recorded", "load=utilization-adjusted"),
        "utilization",
    );
    refusal(
        &clause.replace("faults=observed-effect", "faults=deduct-all"),
        "observed effect",
    );
    refusal(
        &clause.replace("retries=policy-stopping", "retries=best-of"),
        "stopping rule",
    );
    refusal(
        &clause.replace("; retries=policy-stopping", ""),
        "incomplete",
    );
    refusal(&clause.replace("order=fixed", "order=randomized"), "seed");
    refusal(
        &clause.replace("order=fixed", "order=balanced"),
        "repetition count",
    );
    refusal(
        &clause.replace("order=fixed", "order=fixed; seed=7"),
        "neither a seed",
    );
    refusal(
        &clause.replace("initial=owned-cold", "initial=owned-prepared"),
        "recipe token",
    );
    refusal(
        &clause.replace(
            "initial=owned-cold",
            "initial=owned-cold; recipe=owner-install/v1",
        ),
        "only with an owned-prepared",
    );
    refusal(&format!("{clause}; undeclared=value"), "unknown field");
    // Clause order stays fixed: nuisance-control follows the selection clause
    // and precedes the infrastructure binding.
    let selection = experiment_selection_clause(&ExperimentSelection {
        method: ExperimentMethod::RealOperation,
        claim: EffectPath::LocalOperation,
        outcome: "the declared build cycle is measured on both arms".to_owned(),
        rationale: "the unit exercises the claimed local mechanism".to_owned(),
        controls: "frozen inputs with the declared nuisance-control plan".to_owned(),
        projection: "one bounded experiment".to_owned(),
        baseline: "the accepted revision excluding the candidate edit".to_owned(),
        stopping: "stop after the declared attempts".to_owned(),
    });
    let mut ordered = policy.clone();
    ordered.uncertainty = format!("{selection}; {}", nuisance_control_clause(&plan));
    assert!(
        ordered.declare().is_ok(),
        "selection then nuisance is valid: {:?}",
        ordered.declare().err()
    );
    let mut misordered = policy.clone();
    misordered.uncertainty = format!("{}; {selection}", nuisance_control_clause(&plan));
    assert!(
        misordered.declare().is_err(),
        "the nuisance clause must follow the selection clause"
    );
    let mut after_binding = policy.clone();
    after_binding.uncertainty = format!(
        "{}; {}",
        binding_clause(MetricView::WorkEfficiency, Mechanism::None),
        nuisance_control_clause(&plan)
    );
    assert!(
        after_binding.declare().is_err(),
        "the nuisance clause must precede the infrastructure binding"
    );
}

#[test]
fn a_warmed_second_arm_or_missing_observation_withholds_the_verdict() {
    let policy = with_nuisance(policy());
    let declared = declare(&policy);

    // A complete, plan-bound pair may adopt: the plan adds no unavailable
    // identity prerequisite and does not weaken the acceptance gate.
    let compliant = summarize(&plan_pair(100.0, 85.0), &policy);
    let evaluation = evaluate(&declared, &compliant).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation.coverage.contains("nuisance-shared-unobserved"),
        "{}",
        evaluation.coverage
    );

    // The second arm started warmer than declared: the comparison is refused
    // until corrected conditions are exercised, and the retained attempt is
    // preserved.
    let mut rows = plan_pair(100.0, 80.0);
    rows[1]["nuisance"]["initial"]["observed"] = json!("non-empty-owned-scratch");
    rows[1]["nuisance"]["initial"]["violated"] = json!(true);
    let report = summarize(&rows, &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("violated the declared owned-cold initial state")),
        "{:?}",
        evaluation.reasons
    );
    assert_eq!(report["attempts"].as_array().unwrap().len(), 2);

    // A settled arm without the observed record cannot prove its declared
    // conditions; the evidence gap withholds the verdict.
    let mut rows = plan_pair(100.0, 80.0);
    rows[1].as_object_mut().unwrap().remove("nuisance");
    let report = summarize(&rows, &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("does not record its plan")),
        "{:?}",
        evaluation.reasons
    );

    // A realized order that differs from the predeclared rule is refused.
    let mut rows = plan_pair(100.0, 80.0);
    rows[0]["nuisance"]["order"]["realized"] = json!("candidate-first");
    let report = summarize(&rows, &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("realized arm order")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn a_changed_trajectory_fault_is_not_repaired_by_elapsed_subtraction() {
    let policy = with_nuisance(policy());
    let declared = declare(&policy);
    let mut rows = plan_pair(100.0, 70.0);
    rows[1]["nuisance"]["faults"] = json!([{
        "class": "changed-trajectory",
        "detail": "a lost response was retried with changed context and work performed",
    }]);
    let report = summarize(&rows, &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("changed or unverified the response, context or work")),
        "{:?}",
        evaluation.reasons
    );
    // The original attempt, its usage and its failures stay recorded: no
    // hypothetical fault-free trajectory replaces them.
    let attempts = report["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    assert!(attempts[1]["native_runs"].is_array());
    assert_eq!(
        report["units"][0]["candidate_result"].as_str(),
        Some("c1"),
        "{report}"
    );

    let mut rows = plan_pair(100.0, 70.0);
    rows[0]["nuisance"]["faults"] = json!([{
        "class": "work-started-unverified",
        "detail": "the observed conversation could not verify the declared work",
    }]);
    let report = summarize(&rows, &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
}

#[test]
fn recorded_background_contention_withholds_an_unbounded_verdict() {
    let plain = with_nuisance(policy());
    let declared = declare(&plain);

    // Comparable recorded contention stays visible without inventing a
    // correction; the declared effect still decides.
    let mut rows = plan_pair(100.0, 85.0);
    for row in &mut rows {
        row["nuisance"]["load"]["observed"]["unrelated_wait"] = json!(true);
        row["nuisance"]["load"]["observed"]["admission_evidence"] = json!("recorded");
    }
    let report = summarize(&rows, &plain);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert!(
        evaluation
            .coverage
            .contains("nuisance-comparable-recorded-load"),
        "{}",
        evaluation.coverage
    );

    // Contention measured over exactly one arm without a declared
    // work-efficiency binding cannot be waved away by a utilization guess.
    let mut rows = plan_pair(100.0, 85.0);
    rows[0]["nuisance"]["load"]["observed"]["unrelated_wait"] = json!(true);
    rows[0]["nuisance"]["load"]["observed"]["admission_evidence"] = json!("recorded");
    rows[1]["nuisance"]["load"]["observed"]["admission_evidence"] = json!("recorded");
    let report = summarize(&rows, &plain);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation.reasons.iter().any(|reason| reason
            .contains("unrelated external heavy command was measured over exactly one arm")),
        "{:?}",
        evaluation.reasons
    );
    // The raw evidence and the recorded load stay in the report.
    assert!(report["attempts"][0]["nuisance"]["load"].is_object());

    // With the declared work-efficiency binding the existing blocking
    // adjustment bounds the eligible wait; the nuisance gate does not add a
    // second, invented correction of its own.
    let mut bound = with_nuisance(policy());
    bound.uncertainty = format!(
        "{}; {}",
        bound.uncertainty,
        binding_clause(MetricView::WorkEfficiency, Mechanism::None)
    );
    let declared = declare(&bound);
    let mut rows = plan_pair(100.0, 85.0);
    rows[0]["nuisance"]["load"]["observed"]["unrelated_wait"] = json!(true);
    for row in &mut rows {
        row["nuisance"]["load"]["observed"]["admission_evidence"] = json!("recorded");
    }
    let report = summarize(&rows, &bound);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert!(
        evaluation
            .coverage
            .contains("nuisance-asymmetric-load-adjusted"),
        "{}",
        evaluation.coverage
    );
    assert!(
        !evaluation.reasons.iter().any(|reason| reason
            .contains("unrelated external heavy command was measured over exactly one arm")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn a_nuisance_plan_cannot_weaken_acceptance_identity_or_metric_gates() {
    // The correctness gate and the declared experiment selection stay in
    // force with a plan declared.
    let mut invalid = with_nuisance(policy());
    invalid.require_acceptance = false;
    assert!(invalid.declare().is_err());
    let mut invalid = with_nuisance(policy());
    invalid.uncertainty = format!(
        "unknown evidence stays inconclusive; {}; {}",
        experiment_selection_clause(&ExperimentSelection {
            method: ExperimentMethod::BoundedReplay,
            claim: EffectPath::TaskStrategy,
            outcome: "a smaller proxy stands in for the task".to_owned(),
            rationale: "the declared plan cannot bypass the selection gate".to_owned(),
            controls: "none".to_owned(),
            projection: "one replay".to_owned(),
            baseline: "the accepted revision".to_owned(),
            stopping: "one attempt".to_owned(),
        }),
        nuisance_control_clause(&nuisance_plan())
    );
    assert!(
        invalid.declare().is_err(),
        "a proxy unit cannot replace the required full-task comparison"
    );

    // A faster candidate that fails independent acceptance is not adopted,
    // whatever the plan records; proxy metrics never replace acceptance.
    let policy = with_nuisance(policy());
    let declared = declare(&policy);
    let mut rows = plan_pair(100.0, 60.0);
    rows[1]["checks"][0]["passed"] = json!(false);
    rows[1]["checks"][0]["exit_code"] = json!(1);
    let report = summarize(&rows, &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(evaluation.decision, PolicyDecision::Adopt);
}

/// One authoritative model-free attempt: the declared method executes no
/// model call, so its model metrics are recorded as inapplicable, never as a
/// measured zero, and its measured work is the operation's own duration,
/// exit and declared inputs.
fn operation_attempt(
    id: &str,
    arm: &str,
    case: &str,
    start: f64,
    seconds: f64,
    accepted: bool,
) -> Value {
    let matched: BTreeMap<&str, &str> = MATCH_FIELDS.iter().map(|key| (*key, "fixed")).collect();
    json!({
        "attempt_id": id,
        "case_id": case,
        "arm": arm,
        "experiment_id": "exp-1",
        "method": "real-operation",
        "model_calls": 0,
        "model_metrics": "inapplicable",
        "started_at": start,
        "ended_at": start + seconds,
        "execution_started_at": start,
        "matched": matched,
        "native_runs": [{
            "role": "operation",
            "method": "real-operation",
            "status": "completed",
            "started_at": start,
            "ended_at": start + 2.0,
            "elapsed_seconds": 2.0,
            "exit_code": 0,
            "model_calls": 0,
            "model_metrics": "inapplicable",
            "evidence": "private/operation-receipt.json",
        }],
        "checks": [{
            "id": "independent-acceptance",
            "round": 0,
            "started_at": start + 2.0,
            "ended_at": start + seconds,
            "required": true,
            "executed": true,
            "passed": accepted,
            "exit_code": if accepted { 0 } else { 1 },
            "evidence": "private/oracle.json",
        }],
        "children": [],
        "interventions": [],
        "retry_of": null,
    })
}

#[test]
fn a_model_free_operation_pair_reaches_adoption_with_inapplicable_metrics() {
    let policy = policy();
    let declared = declare(&policy);
    let report = summarize(
        &[
            operation_attempt("ob", "baseline", "operation-case", 0.0, 100.0, true),
            operation_attempt("oc", "candidate", "operation-case", 200.0, 70.0, true),
        ],
        &policy,
    );
    let unit = &report["units"][0];
    assert_eq!(unit["model_metrics"], "inapplicable", "{unit}");
    assert_eq!(unit["method"], "real-operation", "{unit}");
    assert_eq!(unit["operation_work"], true, "{unit}");
    assert_eq!(unit["comparable_pairs"], 1, "{unit}");
    assert_eq!(unit["evidence_complete"], true, "{unit}");
    assert_eq!(unit["positive_effect"], true, "{unit}");
    // Inapplicable stays non-numeric: no model counter is invented anywhere.
    let row = &report["attempts"][0];
    assert!(
        row["total_rounds"].is_null()
            && row["total_tool_operations"].is_null()
            && row["total_requests"].is_null(),
        "{row}"
    );
    assert_eq!(row["usage"]["status"], "unknown", "{row}");

    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation.coverage.contains("method:real-operation"),
        "{}",
        evaluation.coverage
    );
    assert!(
        evaluation.coverage.contains("model-metrics:inapplicable"),
        "{}",
        evaluation.coverage
    );
    assert!(
        evaluation
            .coverage
            .contains("operation-work:duration+exit+inputs"),
        "{}",
        evaluation.coverage
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("model-free method")),
        "the decision record names the method distinction: {:?}",
        evaluation.reasons
    );
    let variation = evaluation.variation.as_ref().expect("variation record");
    assert!(
        variation
            .basis
            .contains("inapplicable rather than a measured zero"),
        "{}",
        variation.basis
    );
    // The decision record consumes exactly the matched operation pair.
    let draft = evaluation
        .decision_draft("item-1", "exp-1", "base-rev", "cand-rev", "acceptance.json")
        .expect("the adopt decision record is justified");
    assert_eq!(draft.matched, 1);
    assert!(
        draft.coverage.contains("model-metrics:inapplicable"),
        "{}",
        draft.coverage
    );
}

#[test]
fn a_model_method_unit_without_identity_or_rounds_still_fails_as_before() {
    let policy = with_claim(policy(), ClaimScope::Scoped);
    let declared = declare(&policy);

    // The model identity is unknown on both arms: the pair stays incomparable
    // exactly as before the model-free method became first-class.
    let mut baseline = attempt(
        "mb", "baseline", "case-m", 0.0, 100.0, true, None, None, false,
    );
    let mut candidate = attempt(
        "mc",
        "candidate",
        "case-m",
        200.0,
        70.0,
        true,
        None,
        None,
        false,
    );
    for row in [&mut baseline, &mut candidate] {
        row["matched"].as_object_mut().unwrap().remove("model");
    }
    let report = summarize(&[baseline, candidate], &policy);
    assert_eq!(report["comparisons"][0]["comparable"], false);
    assert!(
        report["comparisons"][0]["excluded_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("unknown:model")),
        "{}",
        report["comparisons"][0]
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    assert_eq!(
        evaluation.coverage,
        "none; complete-pairs:0; variation:unmeasured"
    );

    // A comparable model-method pair without measured rounds or tool
    // operations keeps the declared metric coverage gate.
    let report = summarize(
        &[
            attempt(
                "rb", "baseline", "case-r", 0.0, 100.0, true, None, None, true,
            ),
            attempt(
                "rc",
                "candidate",
                "case-r",
                200.0,
                70.0,
                true,
                None,
                None,
                true,
            ),
        ],
        &policy,
    );
    assert_eq!(report["comparisons"][0]["comparable"], true);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("metric coverage is incomplete")),
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation.coverage.contains("time"),
        "{}",
        evaluation.coverage
    );
    assert!(
        !evaluation.coverage.contains("operation-work"),
        "{}",
        evaluation.coverage
    );
}

#[test]
fn inapplicable_model_metrics_never_become_a_measured_zero_or_a_shortcut() {
    let policy = policy();
    let declared = declare(&policy);

    // A record that claims inapplicable model metrics while measuring model
    // work is not model-free: the model identity and completeness gates keep
    // applying to it.
    let mut baseline = operation_attempt("sb", "baseline", "case-s", 0.0, 100.0, true);
    let mut candidate = operation_attempt("sc", "candidate", "case-s", 200.0, 70.0, true);
    for row in [&mut baseline, &mut candidate] {
        row["native_runs"][0]["rounds"] = json!(2);
        row["matched"].as_object_mut().unwrap().remove("model");
        row["observed_model_metadata_verified"] = json!(false);
    }
    let report = summarize(&[baseline, candidate], &policy);
    assert_eq!(report["comparisons"][0]["comparable"], false);
    assert!(
        report["comparisons"][0]["excluded_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("unknown:model")),
        "{}",
        report["comparisons"][0]
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );

    // A model-free pair without the operation's own measured work (a recorded
    // exit and duration on the retained execution) cannot form a comparable
    // unit, whatever its elapsed difference.
    let mut baseline = operation_attempt("wb", "baseline", "case-w", 0.0, 100.0, true);
    let mut candidate = operation_attempt("wc", "candidate", "case-w", 200.0, 70.0, true);
    for row in [&mut baseline, &mut candidate] {
        row["native_runs"][0]
            .as_object_mut()
            .unwrap()
            .remove("exit_code");
    }
    let report = summarize(&[baseline, candidate], &policy);
    assert_eq!(report["comparisons"][0]["comparable"], false);
    assert!(
        report["comparisons"][0]["excluded_reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("operation_work_unverified")),
        "{}",
        report["comparisons"][0]
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
}

/// Declared policy bound to the work-efficiency infrastructure view with the
/// given treatment mechanism.
fn with_binding(mechanism: Mechanism) -> ComparisonPolicy {
    let mut policy = policy();
    policy.uncertainty = format!(
        "unknown evidence stays inconclusive; {}",
        binding_clause(MetricView::WorkEfficiency, mechanism)
    );
    policy
}

fn captured(row: Value, capture: Value) -> Value {
    let mut row = row;
    row["infrastructure_capture"] = capture;
    row
}

/// A retained native capture with one verified unrelated external wait of
/// `wait_seconds` inside an attempt of `total_seconds`, with its matching
/// correlated command activity and the given model requests.
fn wait_capture(wait_seconds: u64, total_seconds: u64, requests: Value) -> Value {
    let wait_ns = wait_seconds * 1_000_000_000;
    json!({
        "window": {"start_ns": 0, "end_ns": total_seconds * 1_000_000_000_u64},
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

/// A fully observed immediate grant: proven zero queue delay.
fn immediate_capture(total_seconds: u64, requests: Value) -> Value {
    json!({
        "window": {"start_ns": 0, "end_ns": total_seconds * 1_000_000_000_u64},
        "admissions": [{"id": "now", "class": "measured_zero", "domain_match": true}],
        "activity": [],
        "requests": requests
    })
}

fn wait_only_request(id: &str, total_tokens: u64) -> Value {
    json!({
        "id": id,
        "wait_only": true,
        "start_ns": 1_000_000_000_u64,
        "end_ns": 2_000_000_000_u64,
        "tool_call_id": "tool-1",
        "command_id": "cmd-1",
        "input_tokens": total_tokens,
        "cached_input_tokens": 0,
        "output_tokens": 0,
        "reasoning_output_tokens": 0,
        "total_tokens": total_tokens
    })
}

fn task_request(id: &str, total_tokens: u64, start_ns: u64) -> Value {
    json!({
        "id": id,
        "wait_only": false,
        "start_ns": start_ns,
        "end_ns": start_ns + 1_000_000_000,
        "input_tokens": total_tokens,
        "cached_input_tokens": 0,
        "output_tokens": 0,
        "reasoning_output_tokens": 0,
        "total_tokens": total_tokens
    })
}

#[test]
fn infrastructure_evidence_replays_and_reconciles_across_resume() {
    let policy = with_binding(Mechanism::None);
    let declared = declare(&policy);
    let report = summarize(
        &[
            captured(
                attempt(
                    "b1",
                    "baseline",
                    "case-e",
                    0.0,
                    40.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                immediate_capture(40, json!([task_request("task-b", 10, 1_000_000_000)])),
            ),
            captured(
                attempt(
                    "c1",
                    "candidate",
                    "case-e",
                    100.0,
                    40.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                wait_capture(10, 40, json!([wait_only_request("wait-1", 12)])),
            ),
        ],
        &policy,
    );
    let attribution = report["attempts"][1]["attribution"].clone();
    assert_eq!(attribution["replay"], "reproduced", "{attribution}");
    assert_eq!(
        attribution["rule_version"],
        harness_core::infrastructure_accounting::RULE_VERSION
    );
    assert_eq!(
        attribution["reconciliation"]["status"], "consistent",
        "{attribution}"
    );
    assert_eq!(
        attribution["reconciliation"]["elapsed"]["excluded_seconds"], 10.0,
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
    assert_eq!(
        attribution["reconciliation"]["tokens"]["adjusted_total_tokens"], 0,
        "{attribution}"
    );
    // Every exclusion carries its rule, reason and evidence reference.
    let exclusions = attribution["exclusions"].as_array().unwrap();
    assert!(
        exclusions.iter().any(|entry| {
            entry["kind"] == "deducted"
                && entry["metric"] == "elapsed"
                && entry["rule"] == harness_core::infrastructure_accounting::RULE_VERSION
                && entry["reason"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty())
                && entry["evidence"]
                    .as_str()
                    .is_some_and(|evidence| !evidence.is_empty())
        }),
        "{exclusions:?}"
    );
    assert!(
        exclusions.iter().any(|entry| {
            entry["kind"] == "deducted"
                && entry["metric"] == "tokens"
                && entry["requests"] == json!(["wait-1"])
                && entry["reason"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty())
        }),
        "{exclusions:?}"
    );
    // Replay is deterministic: re-summarizing the retained attempts (the
    // resume path) reproduces the same deductions, exclusions and totals with
    // no model call and no operator label.
    let attempts = report["attempts"].as_array().unwrap().clone();
    let resumed = summarize_attempts(&attempts).unwrap();
    assert_eq!(resumed["attempts"][1]["attribution"], attribution);
    // The explicit replay entry reduces the retained native trace into exactly
    // the retained adjusted view, every time.
    let replayed = harness_core::outcome_report::replay_attempt(&report["attempts"][1])
        .expect("the retained capture replays");
    assert_eq!(replayed, report["attempts"][1]["infrastructure"]);
    assert_eq!(
        replayed,
        harness_core::outcome_report::replay_attempt(&report["attempts"][1]).unwrap()
    );
    // The durable evaluation binds the rule identity, declared view and
    // reconciled totals to the decision.
    let evaluation = evaluate(&declared, &report).unwrap();
    let evidence = evaluation
        .attribution
        .as_ref()
        .expect("attribution is bound to the evaluation");
    assert_eq!(
        evidence.rule_version.as_deref(),
        Some("infrastructure-attribution.v1")
    );
    assert_eq!(evidence.view, "work-efficiency");
    assert_eq!(evidence.replay, "reproduced");
    assert_eq!(evidence.reconciliation, "consistent");
    assert_eq!(evidence.elapsed_excluded_seconds, Some(10.0));
    assert_eq!(evidence.tokens_raw_total, Some(22));
    assert_eq!(evidence.tokens_excluded_total, Some(12));
    assert_eq!(evidence.tokens_adjusted_total, Some(10));
    assert!(
        evidence.exclusions.iter().any(|entry| {
            entry.kind == "deducted"
                && entry.metric == "elapsed"
                && entry.attempt == "c1"
                && entry.arm == "candidate"
        }),
        "{:?}",
        evidence.exclusions
    );
}

#[test]
fn queue_only_false_gain_and_regression_stay_inconclusive() {
    let policy = with_binding(Mechanism::None);
    let declared = declare(&policy);
    // False gain: the baseline, not the candidate, sat in the external queue.
    let false_gain = summarize(
        &[
            captured(
                attempt(
                    "b1",
                    "baseline",
                    "case-q",
                    0.0,
                    40.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                wait_capture(10, 40, json!([])),
            ),
            captured(
                attempt(
                    "c1",
                    "candidate",
                    "case-q",
                    100.0,
                    30.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                immediate_capture(30, json!([])),
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &false_gain).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("external waiting")),
        "{:?}",
        evaluation.reasons
    );
    assert!(evaluation.coverage.contains("raw-only-wait"));
    // False regression: the candidate's extra observed time is a measured
    // external wait and the adjusted view shows no material regression.
    let false_regression = summarize(
        &[
            captured(
                attempt(
                    "b2",
                    "baseline",
                    "case-q",
                    0.0,
                    30.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                immediate_capture(30, json!([])),
            ),
            captured(
                attempt(
                    "c2",
                    "candidate",
                    "case-q",
                    100.0,
                    40.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                wait_capture(10, 40, json!([])),
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &false_regression).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    // Counterexample: a measured regression beyond tolerance on the adjusted
    // view stays a rejection rather than becoming inconclusive.
    let genuine = summarize(
        &[
            captured(
                attempt(
                    "b3",
                    "baseline",
                    "case-q",
                    0.0,
                    30.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                immediate_capture(30, json!([])),
            ),
            captured(
                attempt(
                    "c3",
                    "candidate",
                    "case-q",
                    100.0,
                    40.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                immediate_capture(40, json!([])),
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &genuine).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Reject,
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn an_unreconciled_unreplayable_or_stale_rule_view_cannot_inherit_an_adoption() {
    let policy = with_binding(Mechanism::None);
    let declared = declare(&policy);
    let mut baseline = attempt(
        "b1",
        "baseline",
        "case-r",
        0.0,
        100.0,
        true,
        Some(4),
        Some(6),
        false,
    );
    let mut candidate = attempt(
        "c1",
        "candidate",
        "case-r",
        100.0,
        80.0,
        true,
        Some(3),
        Some(6),
        false,
    );
    bounded(&mut baseline, 100.0, 100.0);
    bounded(&mut candidate, 80.0, 80.0);
    let report = summarize(&[baseline.clone(), candidate.clone()], &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );

    // A retained total that no longer reconciles is an accounting gap, not a
    // larger deduction.
    let mut gapped = candidate.clone();
    gapped["infrastructure"]["adjusted_ns"] = json!(70_000_000_000_u64);
    gapped["infrastructure"]["adjusted_high_ns"] = json!(70_000_000_000_u64);
    gapped["infrastructure"]["adjusted_high_seconds"] = json!(70.0);
    let gapped = summarize(&[baseline.clone(), gapped], &policy);
    assert_eq!(
        gapped["attempts"][1]["attribution"]["reconciliation"]["status"], "gap",
        "{}",
        gapped["attempts"][1]["attribution"]
    );
    let evaluation = evaluate(&declared, &gapped).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("do not reconcile")),
        "{:?}",
        evaluation.reasons
    );

    // Evidence produced under a different attribution rule/version cannot be
    // consumed under the predeclared rule.
    let mut stale = candidate.clone();
    stale["infrastructure"]["rule_version"] = json!("infrastructure-attribution.v2");
    let stale = summarize(&[baseline.clone(), stale], &policy);
    let evaluation = evaluate(&declared, &stale).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("changed rule")),
        "{:?}",
        evaluation.reasons
    );

    // A retained adjusted view that no longer reproduces from its retained
    // native trace is contaminated and cannot decide the comparison.
    let report = summarize(
        &[
            captured(
                attempt(
                    "b4",
                    "baseline",
                    "case-r",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                immediate_capture(100, json!([])),
            ),
            captured(
                attempt(
                    "c4",
                    "candidate",
                    "case-r",
                    100.0,
                    40.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                wait_capture(10, 40, json!([])),
            ),
        ],
        &policy,
    );
    let mut attempts = report["attempts"].as_array().unwrap().clone();
    attempts[1]["infrastructure"]["deductible_ns"] = json!(0);
    attempts[1]["infrastructure"]["adjusted_ns"] = json!(40_000_000_000_u64);
    attempts[1]["infrastructure"]["adjusted_high_ns"] = json!(40_000_000_000_u64);
    attempts[1]["infrastructure"]["adjusted_low_ns"] = json!(40_000_000_000_u64);
    attempts[1]["infrastructure"]["adjusted_seconds"] = json!(40.0);
    attempts[1]["infrastructure"]["adjusted_low_seconds"] = json!(40.0);
    attempts[1]["infrastructure"]["adjusted_high_seconds"] = json!(40.0);
    let resumed = summarize_attempts(&attempts).unwrap();
    assert_eq!(
        resumed["attempts"][1]["attribution"]["replay"], "mismatch",
        "{}",
        resumed["attempts"][1]["attribution"]
    );
    let evaluation = evaluate(&declared, &resumed).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("does not reproduce")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn duplicate_usage_is_rejected_instead_of_creating_a_saving() {
    let policy = with_binding(Mechanism::None);
    let declared = declare(&policy);
    let clean = summarize(
        &[
            captured(
                attempt(
                    "b1",
                    "baseline",
                    "case-u",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                immediate_capture(100, json!([task_request("task-b", 10, 1_000_000_000)])),
            ),
            captured(
                attempt(
                    "c1",
                    "candidate",
                    "case-u",
                    100.0,
                    40.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                wait_capture(10, 40, json!([wait_only_request("wait-1", 12)])),
            ),
        ],
        &policy,
    );
    assert_eq!(
        clean["attempts"][1]["attribution"]["reconciliation"]["tokens"]["excluded_total_tokens"],
        12,
        "{}",
        clean["attempts"][1]["attribution"]
    );
    let evaluation = evaluate(&declared, &clean).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );

    // The same wait-only request recorded twice is one request, not two
    // savings: it is not excluded, the adjusted usage keeps the full total,
    // and the degraded coverage is recorded instead.
    let duplicate = summarize(
        &[
            captured(
                attempt(
                    "b2",
                    "baseline",
                    "case-u",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                immediate_capture(100, json!([task_request("task-b", 10, 1_000_000_000)])),
            ),
            captured(
                attempt(
                    "c2",
                    "candidate",
                    "case-u",
                    100.0,
                    40.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                wait_capture(
                    10,
                    40,
                    json!([
                        wait_only_request("wait-1", 12),
                        wait_only_request("wait-1", 12)
                    ]),
                ),
            ),
        ],
        &policy,
    );
    let attribution = duplicate["attempts"][1]["attribution"].clone();
    assert_eq!(attribution["duplicates"], json!(["wait-1"]));
    assert_eq!(
        attribution["reconciliation"]["tokens"]["status"], "degraded",
        "{attribution}"
    );
    assert_eq!(
        attribution["reconciliation"]["tokens"]["raw_total_tokens"], 12,
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
    let evaluation = evaluate(&declared, &duplicate).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Reject,
        "duplicate usage cannot produce the exclusion saving: {:?}",
        evaluation.reasons
    );
    let evidence = evaluation.attribution.as_ref().unwrap();
    assert_eq!(evidence.reconciliation, "degraded");
    assert_eq!(evidence.duplicates, vec!["wait-1".to_owned()]);
}

#[test]
fn build_demand_treatment_keeps_its_effect_and_waiting_treatment_is_operational() {
    let rows = [
        captured(
            attempt(
                "b1",
                "baseline",
                "case-c",
                0.0,
                40.0,
                true,
                Some(4),
                Some(6),
                false,
            ),
            immediate_capture(40, json!([])),
        ),
        captured(
            attempt(
                "c1",
                "candidate",
                "case-c",
                100.0,
                20.0,
                true,
                Some(4),
                Some(6),
                false,
            ),
            immediate_capture(20, json!([])),
        ),
    ];
    // A cache/build-demand treatment changes real work, not external waits:
    // the reduced work stays in the adjusted view and supports the decision.
    let policy = with_binding(Mechanism::Cache);
    let declared = declare(&policy);
    let report = summarize(&rows, &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    let evidence = evaluation.attribution.as_ref().unwrap();
    assert_eq!(evidence.mechanism, "cache");
    assert_eq!(evidence.elapsed_excluded_seconds, Some(0.0));
    assert_eq!(evaluation.baseline_seconds, Some(40.0));
    assert_eq!(evaluation.candidate_seconds, Some(20.0));

    // A treatment that changes waiting itself is evaluated under controlled
    // load; it is never subtracted from its own measurement.
    let policy = with_binding(Mechanism::Waiting);
    let declared = declare(&policy);
    let report = summarize(&rows, &policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("controlled load")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn downstream_context_after_an_external_wait_cannot_be_erased() {
    let policy = with_binding(Mechanism::None);
    let declared = declare(&policy);
    let report = summarize(
        &[
            captured(
                attempt(
                    "b1",
                    "baseline",
                    "case-d",
                    0.0,
                    40.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                immediate_capture(40, json!([task_request("task-b", 10, 1_000_000_000)])),
            ),
            captured(
                attempt(
                    "c1",
                    "candidate",
                    "case-d",
                    100.0,
                    20.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                wait_capture(
                    5,
                    20,
                    json!([
                        wait_only_request("wait-1", 20),
                        task_request("task-c", 40, 10_000_000_000)
                    ]),
                ),
            ),
        ],
        &policy,
    );
    let attempt = &report["attempts"][1];
    // The measured wait interval alone is subtracted from elapsed time.
    assert_eq!(attempt["infrastructure"]["adjusted_seconds"], 15.0);
    // The later mixed-request usage caused by the waiting context stays in the
    // adjusted total; only the contained wait-only request is excluded.
    let reconciliation = &attempt["attribution"]["reconciliation"];
    assert_eq!(reconciliation["tokens"]["excluded_total_tokens"], 20);
    assert_eq!(reconciliation["tokens"]["adjusted_total_tokens"], 40);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Reject,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("excluded wait usage is not a waiver")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn a_verdict_changing_coverage_gap_stays_visible_with_its_reasons() {
    let policy = with_binding(Mechanism::None);
    let declared = declare(&policy);
    let mut gap = wait_capture(10, 40, json!([]));
    gap["admissions"][0]["endpoint_end_ns"] = Value::Null;
    let report = summarize(
        &[
            captured(
                attempt(
                    "b1",
                    "baseline",
                    "case-g",
                    0.0,
                    40.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                gap,
            ),
            captured(
                attempt(
                    "c1",
                    "candidate",
                    "case-g",
                    100.0,
                    30.0,
                    true,
                    Some(4),
                    Some(6),
                    false,
                ),
                immediate_capture(30, json!([])),
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation.coverage.contains("infrastructure-gap"),
        "{}",
        evaluation.coverage
    );
    let evidence = evaluation.attribution.as_ref().unwrap();
    assert!(
        evidence.exclusions.iter().any(|entry| {
            entry.kind == "unresolved"
                && entry.reason.contains("endpoint_unknown")
                && entry.rule.as_deref()
                    == Some(harness_core::infrastructure_accounting::RULE_VERSION)
                && !entry.evidence.is_empty()
        }),
        "{:?}",
        evidence.exclusions
    );
}

/// The driver's run-local corroboration receipt, exactly as the corroboration
/// selector writes it: schema 1, `selected` or `unavailable`, the additional
/// units the declared scope requires, and (for a selection) the identity-only
/// `CorroborationSelection`.
fn corroboration_receipt(
    status: &str,
    required_units: u64,
    selection: Value,
    reason: Value,
) -> Value {
    json!({
        "schema": 1,
        "status": status,
        "required_units": required_units,
        "selection": selection,
        "reason": reason,
    })
}

/// One retained prior real task selected for corroboration, by identity and
/// replay references only.
fn corroboration_unit(case: &str, revision: &str, tree: &str) -> Value {
    json!({
        "owner": format!("card-{case}"),
        "caseId": case,
        "experiment": "exp-prior",
        "mechanism": "bounded-output",
        "conditions": "local-tool-runs",
        "revision": revision,
        "treeSha256": tree,
    })
}

/// A declared policy whose scope needs one additional independent retained
/// unit beyond the run's own declared plan unit.
fn broader_scope_policy() -> ComparisonPolicy {
    let mut policy = policy();
    policy.stopping.required_units = 2;
    policy.repeated_selection = RepeatedSelection::BestOf;
    policy.overhead = Overhead {
        implementation_seconds: 0.0,
        evaluation_seconds: 0.0,
        maintenance_seconds_per_task: 0.0,
    };
    policy
}

/// One favorable complete pair on the run's own declared plan unit: 100s
/// baseline against 70s candidate, above the 10% declared effect.
fn favorable_pair(case: &str) -> Vec<Value> {
    vec![
        attempt(
            "b1",
            "baseline",
            case,
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
            case,
            200.0,
            70.0,
            true,
            Some(4),
            Some(6),
            true,
        ),
    ]
}

#[test]
fn a_broader_claim_needs_the_declared_corroboration_selection() {
    let policy = broader_scope_policy();
    let declared = declare(&policy);
    let report = summarize(&favorable_pair("case-b"), &policy);

    // One recorded unit without any corroboration state: the declared scope
    // needs two independent units, and no summary replaces the selection.
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(evaluation.corroboration.is_none());
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("no corroboration selection")),
        "{:?}",
        evaluation.reasons
    );

    // A ready selection of the declared additional retained unit supports the
    // broader claim, and the decision carries its identity and replay
    // references.
    let tree = "c".repeat(64);
    let receipt = corroboration_receipt(
        "selected",
        1,
        json!({
            "schema": 1,
            "requiredUnits": 1,
            "status": "ready",
            "units": [corroboration_unit("case-c", "rev-c", &tree)],
            "excluded": [],
        }),
        Value::Null,
    );
    let report = attach_corroboration(&report, &receipt).unwrap();
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    let evidence = evaluation
        .corroboration
        .as_ref()
        .expect("consumed corroboration section");
    assert_eq!(evidence.status, CorroborationState::Ready);
    assert_eq!(evidence.required_units, 1);
    assert_eq!(evidence.units.len(), 1);
    assert_eq!(evidence.units[0].owner, "card-case-c");
    assert_eq!(evidence.units[0].case_id, "case-c");
    assert_eq!(evidence.units[0].experiment, "exp-prior");
    assert_eq!(evidence.units[0].mechanism, "bounded-output");
    assert_eq!(evidence.units[0].conditions, "local-tool-runs");
    assert_eq!(evidence.units[0].revision, "rev-c");
    assert_eq!(evidence.units[0].tree_sha256, tree);
    assert_eq!(evidence.digest.len(), 64);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("ready corroboration selection")
                && reason.contains("case-c")),
        "{:?}",
        evaluation.reasons
    );

    // A recorded effect below the declared threshold does not become an
    // adoption on the strength of a selected, not yet measured unit, and it is
    // not a rejection either: the broader claim stays inconclusive.
    let weak = summarize(
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
                95.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
        ],
        &policy,
    );
    let weak = attach_corroboration(&weak, &receipt).unwrap();
    let evaluation = evaluate(&declared, &weak).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("partially measured set")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn insufficient_corroboration_units_leave_the_broader_claim_inconclusive() {
    let policy = broader_scope_policy();
    let declared = declare(&policy);
    let report = summarize(&favorable_pair("case-b"), &policy);

    // An inconclusive selection keeps its exact reason and excluded
    // candidates; no unit is fabricated and no adoption follows.
    let receipt = corroboration_receipt(
        "selected",
        1,
        json!({
            "schema": 1,
            "requiredUnits": 1,
            "status": {"inconclusive": "fewer applicable independent replayable retained tasks than the declared corroboration requirement (required 1, admissible 0); the broader claim remains unsupported, and a workload that does not exercise the mechanism is not evidence against it"},
            "units": [],
            "excluded": [
                {"owner": "card-case-x", "caseId": "case-x", "reason": "notApplicable"},
                {"owner": "card-case-b", "caseId": "case-b", "reason": "alreadyUsed"},
            ],
        }),
        Value::Null,
    );
    let report = attach_corroboration(&report, &receipt).unwrap();
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation.reasons.iter().any(|reason| reason
            .contains("corroboration selection is inconclusive")
            && reason.contains("broader claim remains unsupported")),
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("case-x") && reason.contains("not applicable")),
        "{:?}",
        evaluation.reasons
    );
    let evidence = evaluation
        .corroboration
        .as_ref()
        .expect("consumed corroboration section");
    assert_eq!(evidence.status, CorroborationState::Inconclusive);
    assert_eq!(evidence.required_units, 1);
    assert!(evidence.units.is_empty(), "no unit may be fabricated");
    assert_eq!(evidence.excluded.len(), 2);
    assert_eq!(evidence.excluded[0].case_id, "case-x");
    assert!(
        evidence
            .reason
            .as_deref()
            .unwrap_or("")
            .contains("broader claim remains unsupported")
    );

    // An unavailable selection records why it was not performed.
    let unavailable = corroboration_receipt(
        "unavailable",
        1,
        Value::Null,
        json!("retained-task discovery through the board is unavailable"),
    );
    let report = attach_corroboration(&report, &unavailable).unwrap();
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation.reasons.iter().any(|reason| reason
            .contains("the corroboration selection was not performed: retained-task discovery")),
        "{:?}",
        evaluation.reasons
    );
    assert_eq!(
        evaluation.corroboration.as_ref().unwrap().status,
        CorroborationState::Unavailable
    );

    // A selection claiming readiness with fewer units than its own declared
    // requirement is fabricated: the evaluator refuses to consume it even
    // when its digest is recomputed, and no unit is invented to fill the gap.
    let mut forged = attach_corroboration(
        &report,
        &corroboration_receipt(
            "selected",
            1,
            json!({
                "schema": 1,
                "requiredUnits": 1,
                "status": "ready",
                "units": [corroboration_unit("case-c", "rev-c", &"c".repeat(64))],
                "excluded": [],
            }),
            Value::Null,
        ),
    )
    .unwrap();
    let mut section: CorroborationSection =
        serde_json::from_value(forged["corroboration"].clone()).unwrap();
    section.units.clear();
    section.digest = corroboration_digest(&section).unwrap();
    forged["corroboration"] = serde_json::to_value(&section).unwrap();
    let evaluation = evaluate(&declared, &forged).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("still fall short of the declared 2 independent unit(s)")),
        "{:?}",
        evaluation.reasons
    );
}

#[test]
fn changed_or_missing_corroboration_state_cannot_inherit_an_adoption() {
    let policy = broader_scope_policy();
    let declared = declare(&policy);
    let base = summarize(&favorable_pair("case-b"), &policy);
    let ready = |case: &str, tree: &str| {
        attach_corroboration(
            &base,
            &corroboration_receipt(
                "selected",
                1,
                json!({
                    "schema": 1,
                    "requiredUnits": 1,
                    "status": "ready",
                    "units": [corroboration_unit(case, "rev-prior", tree)],
                    "excluded": [],
                }),
                Value::Null,
            ),
        )
        .unwrap()
    };
    let adopted = evaluate(&declared, &ready("case-c", &"c".repeat(64))).unwrap();
    assert_eq!(adopted.decision, PolicyDecision::Adopt);
    let first = adopted.corroboration.clone().expect("consumed section");
    assert_eq!(first.units[0].case_id, "case-c");

    // Resume/repeated evaluation against the same recorded attempts without
    // the corroboration state: the earlier adoption is not inherited and the
    // broader claim stays inconclusive with an exact reason.
    let resumed = evaluate(&declared, &base).unwrap();
    assert_eq!(resumed.decision, PolicyDecision::Inconclusive);
    assert!(resumed.corroboration.is_none());
    assert!(
        resumed
            .reasons
            .iter()
            .any(|reason| reason.contains("no corroboration selection")),
        "{:?}",
        resumed.reasons
    );

    // A changed selection that still supports the count is re-established
    // under its own exact evidence: the recorded references and binding digest
    // change with the state, so the earlier adoption's evidence is not
    // inherited.
    let second = evaluate(&declared, &ready("case-d", &"d".repeat(64))).unwrap();
    assert_eq!(second.decision, PolicyDecision::Adopt);
    let evidence = second.corroboration.clone().expect("consumed section");
    assert_eq!(evidence.units[0].case_id, "case-d");
    assert_eq!(evidence.units[0].tree_sha256, "d".repeat(64));
    assert_ne!(evidence.digest, first.digest);

    // A selection made for a different declared requirement cannot carry the
    // adoption: the requirement mapping is checked against the declared
    // policy, not against the receipt's own claim.
    let mismatched = evaluate(
        &declared,
        &attach_corroboration(
            &base,
            &corroboration_receipt(
                "selected",
                2,
                json!({
                    "schema": 1,
                    "requiredUnits": 2,
                    "status": "ready",
                    "units": [
                        corroboration_unit("case-c", "rev-c", &"c".repeat(64)),
                        corroboration_unit("case-d", "rev-d", &"d".repeat(64)),
                    ],
                    "excluded": [],
                }),
                Value::Null,
            ),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(mismatched.decision, PolicyDecision::Inconclusive);
    assert!(
        mismatched.reasons.iter().any(|reason| reason
            .contains("a changed corroboration state cannot support the broader claim")),
        "{:?}",
        mismatched.reasons
    );

    // A section whose stored digest no longer binds its content (the unit was
    // substituted after the selection) cannot support the claim either.
    let mut tampered = ready("case-c", &"c".repeat(64));
    tampered["corroboration"]["units"][0]["caseId"] = json!("case-substituted");
    let evaluation = evaluate(&declared, &tampered).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("does not bind its own content")),
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation.corroboration.is_none(),
        "an unbound section is not consumed"
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

#[test]
fn statistical_claims_bind_method_confidence_and_assumptions_before_results() {
    let scoped = claim(ClaimScope::Scoped);
    assert_eq!(
        parse_statistical_claim(&statistical_clause(&scoped)).unwrap(),
        Some(scoped.clone())
    );
    assert_eq!(parse_statistical_claim("no clause here").unwrap(), None);

    // A confidence percentage has no valid basis over dependent local runs and
    // is refused at declaration, before any result exists.
    let mut unsupported = policy();
    unsupported.uncertainty =
        "statistical-analysis.v1; method=observed-pairs-descriptive; confidence=95; claim=scoped; assumptions=fixed-order"
            .to_owned();
    assert!(unsupported.declare().is_err());

    // An analysis method this evaluator cannot verify is refused, not bound.
    let mut method = policy();
    method.uncertainty =
        "statistical-analysis.v1; method=paired-normal; confidence=none; claim=scoped; assumptions=fixed-order"
            .to_owned();
    assert!(method.declare().is_err());

    // A present clause must record its assumptions.
    let mut assumptions = policy();
    assumptions.uncertainty =
        "statistical-analysis.v1; method=observed-pairs-descriptive; confidence=none; claim=scoped; assumptions="
            .to_owned();
    assert!(assumptions.declare().is_err());

    // The clause must precede the infrastructure binding it qualifies.
    let mut order = policy();
    order.uncertainty = format!(
        "{}; {}",
        binding_clause(MetricView::WorkEfficiency, Mechanism::None),
        statistical_clause(&scoped)
    );
    assert!(order.declare().is_err());

    // A repeatable claim needs at least two complete paired units declared
    // before results; the scoped and repeatable analyses are different
    // declarations and carry different digests.
    let mut repeatable = with_claim(policy(), ClaimScope::Repeatable);
    assert!(repeatable.declare().is_err());
    repeatable.stopping.required_units = 2;
    let declared = declare(&repeatable);
    assert!(declared.digest_matches(&declared.digest));
    assert_ne!(
        declared.digest,
        declare(&with_claim(policy(), ClaimScope::Scoped)).digest
    );
}

#[test]
fn one_pair_and_dependent_within_run_events_do_not_become_variance_or_confidence() {
    let policy = with_claim(policy(), ClaimScope::Scoped);
    let declared = declare(&policy);
    // One complete pair whose arms each contain thousands of dependent events:
    // rounds and tool operations are not independent replications, so the
    // result stays a single experimental unit with unmeasured variation.
    let report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-b",
                0.0,
                100.0,
                true,
                Some(2000),
                Some(3000),
                true,
            ),
            attempt(
                "c1",
                "candidate",
                "case-b",
                200.0,
                80.0,
                true,
                Some(2000),
                Some(3000),
                true,
            ),
        ],
        &policy,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Adopt);
    let variation = evaluation.variation.as_ref().expect("variation record");
    assert_eq!(variation.complete_pairs, 1);
    assert_eq!(variation.status, VariationStatus::Unmeasured);
    assert_eq!(
        variation.observed_effect_percent, None,
        "one pair is not a variance estimate and must not become zero"
    );
    assert_eq!(variation.claim, ClaimScope::Scoped);
    assert!(variation.basis.contains("not replications"));
    assert!(variation.basis.contains("never assumed zero"));
    assert!(variation.basis.contains("not evidence of equivalence"));
    assert!(
        variation.basis.contains("API-observed"),
        "the existing model-identity limits stay visible in the claim: {}",
        variation.basis
    );
    assert_eq!(
        evaluation
            .measurement
            .as_ref()
            .expect("measurement record")
            .status,
        MeasurementStatus::Absent
    );
    let claim = evaluation
        .statistical_claim
        .as_ref()
        .expect("declared claim");
    assert_eq!(claim.confidence_percent, None);
    assert_eq!(claim.scope, ClaimScope::Scoped);
    assert!(evaluation.coverage.contains("complete-pairs:1"));
    assert!(evaluation.coverage.contains("variation:unmeasured"));
    assert!(evaluation.scope.contains("unmeasured"));
    assert!(
        !evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("95%") || reason.contains("confidence interval")),
        "no bound may be labeled with an unsupported confidence level: {:?}",
        evaluation.reasons
    );
}

#[test]
fn a_single_pair_or_crossing_pair_range_cannot_establish_repeatable_savings() {
    let mut repeatable = policy();
    repeatable.stopping.required_units = 2;
    repeatable.overhead = Overhead {
        implementation_seconds: 0.0,
        evaluation_seconds: 0.0,
        maintenance_seconds_per_task: 0.0,
    };
    let repeatable = with_claim(repeatable, ClaimScope::Repeatable);
    let declared = declare(&repeatable);

    // One recorded pair cannot establish the declared repeatable claim.
    let report = summarize(
        &[
            attempt(
                "b1",
                "baseline",
                "case-r",
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
                "case-r",
                200.0,
                80.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
        ],
        &repeatable,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    let variation = evaluation.variation.as_ref().expect("variation record");
    assert_eq!(variation.complete_pairs, 1);
    assert_eq!(variation.claim, ClaimScope::Repeatable);
    assert_eq!(variation.status, VariationStatus::Unmeasured);

    // Two complete pairs that both clear the declared threshold support the
    // repeatable claim with an observed descriptive range.
    let report = summarize(
        &[
            pair(
                attempt(
                    "b2",
                    "baseline",
                    "case-r",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p1",
            ),
            pair(
                attempt(
                    "c2",
                    "candidate",
                    "case-r",
                    200.0,
                    80.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p1",
            ),
            pair(
                attempt(
                    "b3",
                    "baseline",
                    "case-r",
                    400.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p2",
            ),
            pair(
                attempt(
                    "c3",
                    "candidate",
                    "case-r",
                    600.0,
                    85.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p2",
            ),
        ],
        &repeatable,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?} units={}",
        evaluation.reasons,
        report["units"]
    );
    let variation = evaluation.variation.as_ref().expect("variation record");
    assert_eq!(variation.complete_pairs, 2);
    assert_eq!(variation.status, VariationStatus::Observed);
    assert_eq!(variation.observed_effect_percent, Some([15.0, 20.0]));
    assert!(evaluation.scope.contains("observed effect range"));

    // A third pair below the declared threshold makes the repeatable claim
    // inconclusive even though the required count of positive units exists:
    // the best pairs never stand in for the declared repeatability.
    let report = summarize(
        &[
            pair(
                attempt(
                    "b4",
                    "baseline",
                    "case-r",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p1",
            ),
            pair(
                attempt(
                    "c4",
                    "candidate",
                    "case-r",
                    200.0,
                    80.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p1",
            ),
            pair(
                attempt(
                    "b5",
                    "baseline",
                    "case-r",
                    400.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p2",
            ),
            pair(
                attempt(
                    "c5",
                    "candidate",
                    "case-r",
                    600.0,
                    85.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p2",
            ),
            pair(
                attempt(
                    "b6",
                    "baseline",
                    "case-r",
                    800.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p3",
            ),
            pair(
                attempt(
                    "c6",
                    "candidate",
                    "case-r",
                    1000.0,
                    98.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p3",
            ),
        ],
        &repeatable,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("repeatable claim is not supported")),
        "{:?}",
        evaluation.reasons
    );
    assert_eq!(
        evaluation
            .variation
            .as_ref()
            .and_then(|variation| variation.observed_effect_percent),
        Some([2.0, 20.0])
    );
}

#[test]
fn threshold_crossing_bounds_stay_inconclusive_across_decision_metrics() {
    // Primary time effect: a supported range of -10..10% crosses the declared
    // 10% threshold, so a favorable midpoint cannot authorize adoption.
    let binding_policy = with_claim_and_binding(ClaimScope::Scoped);
    let declared = declare(&binding_policy);
    let mut baseline = attempt(
        "b1",
        "baseline",
        "case-b",
        0.0,
        40.0,
        true,
        Some(4),
        Some(6),
        true,
    );
    let mut candidate = attempt(
        "c1",
        "candidate",
        "case-b",
        100.0,
        44.0,
        true,
        Some(4),
        Some(6),
        true,
    );
    bounded(&mut baseline, 40.0, 40.0);
    bounded(&mut candidate, 36.0, 44.0);
    let report = summarize(&[baseline.clone(), candidate.clone()], &binding_policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    let measurement = evaluation.measurement.as_ref().expect("measurement record");
    assert_eq!(measurement.status, MeasurementStatus::Bounded);
    assert_eq!(measurement.effect_percent, Some([-10.0, 10.0]));
    assert_eq!(measurement.worst_time_regression_percent, Some(10.0));
    assert!(
        measurement
            .evidence
            .contains("infrastructure-attribution.v1")
    );

    // A supported range wholly beyond the threshold may report the scoped
    // result while the variation stays separately unmeasured.
    let mut baseline = attempt(
        "b2",
        "baseline",
        "case-b",
        0.0,
        40.0,
        true,
        Some(4),
        Some(6),
        true,
    );
    let mut candidate = attempt(
        "c2",
        "candidate",
        "case-b",
        100.0,
        20.0,
        true,
        Some(4),
        Some(6),
        true,
    );
    bounded(&mut baseline, 40.0, 40.0);
    bounded(&mut candidate, 20.0, 20.0);
    let report = summarize(&[baseline, candidate], &binding_policy);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    assert_eq!(
        evaluation
            .measurement
            .as_ref()
            .and_then(|measurement| measurement.effect_percent),
        Some([50.0, 50.0])
    );
    assert_eq!(
        evaluation
            .variation
            .as_ref()
            .map(|variation| variation.status),
        Some(VariationStatus::Unmeasured)
    );

    // Acceptance is a decision metric too: a candidate acceptance failure on
    // one complete pair is not offset by a quality improvement on another.
    let mut quality = policy();
    quality.objective = Objective::Quality;
    let quality = with_claim(quality, ClaimScope::Scoped);
    let declared = declare(&quality);
    let report = summarize(
        &[
            pair(
                attempt(
                    "q1",
                    "baseline",
                    "case-q",
                    0.0,
                    100.0,
                    false,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p1",
            ),
            pair(
                attempt(
                    "q2",
                    "candidate",
                    "case-q",
                    200.0,
                    80.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p1",
            ),
            pair(
                attempt(
                    "q3",
                    "baseline",
                    "case-q",
                    400.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p2",
            ),
            pair(
                attempt(
                    "q4",
                    "candidate",
                    "case-q",
                    600.0,
                    80.0,
                    false,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p2",
            ),
        ],
        &quality,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Reject,
        "{:?}",
        evaluation.reasons
    );
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
fn resource_verdicts_hold_across_time_bounds_and_declared_trade_offs() {
    // The non-primary time dimension is bound-sensitive: a point regression
    // under the tolerance whose admissible range crosses it cannot settle a
    // resource verdict.
    let mut resource = policy();
    resource.objective = Objective::Resource;
    resource.overhead = Overhead {
        implementation_seconds: 0.0,
        evaluation_seconds: 0.0,
        maintenance_seconds_per_task: 0.0,
    };
    resource.trade_off = Some(TradeOff {
        basis: "declared before results: bounded time exchange".into(),
        allowed_regression_percent: 25.0,
    });
    let resource = with_claim(resource, ClaimScope::Scoped);
    let declared = declare(&resource);
    let mut baseline = attempt(
        "rb",
        "baseline",
        "case-r",
        0.0,
        40.0,
        true,
        Some(50),
        Some(50),
        true,
    );
    let mut candidate = attempt(
        "rc",
        "candidate",
        "case-r",
        100.0,
        39.5,
        true,
        Some(25),
        Some(25),
        true,
    );
    bounded(&mut baseline, 30.0, 40.0);
    bounded(&mut candidate, 39.5, 39.5);
    let report = summarize(&[baseline.clone(), candidate.clone()], &resource);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("non-primary time regression")),
        "{:?}",
        evaluation.reasons
    );

    // A predeclared trade-off that covers the whole admissible time range
    // admits the same resource effect, with the exchange recorded.
    let mut covered = policy();
    covered.objective = Objective::Resource;
    covered.overhead = Overhead {
        implementation_seconds: 0.0,
        evaluation_seconds: 0.0,
        maintenance_seconds_per_task: 0.0,
    };
    covered.trade_off = Some(TradeOff {
        basis: "declared before results: bounded time exchange".into(),
        allowed_regression_percent: 35.0,
    });
    let covered = with_claim(covered, ClaimScope::Scoped);
    let declared = declare(&covered);
    let report = summarize(&[baseline, candidate], &covered);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    assert!(evaluation.trade_off_used);
}

#[test]
fn changed_analysis_on_resume_cannot_inherit_an_adoption() {
    let scoped = with_claim(policy(), ClaimScope::Scoped);
    let declared_scoped = declare(&scoped);
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
                Some(4),
                Some(6),
                true,
            ),
        ],
        &scoped,
    );
    assert_eq!(
        evaluate(&declared_scoped, &report).unwrap().decision,
        PolicyDecision::Adopt
    );

    // The same retained evidence resumed under a changed analysis (repeatable
    // instead of scoped) is not the declared comparison and cannot inherit the
    // earlier adoption.
    let mut repeatable = policy();
    repeatable.stopping.required_units = 2;
    let repeatable = with_claim(repeatable, ClaimScope::Repeatable);
    let declared_repeatable = declare(&repeatable);
    assert_ne!(declared_scoped.digest, declared_repeatable.digest);
    let resumed = evaluate(&declared_repeatable, &report).unwrap();
    assert_eq!(resumed.decision, PolicyDecision::Inconclusive);
    assert!(
        resumed
            .reasons
            .iter()
            .any(|reason| reason.contains("differs from the predeclared policy")),
        "{:?}",
        resumed.reasons
    );
}

#[test]
fn repeated_selection_and_stopping_stay_bound_to_the_predeclared_rule() {
    // A best-of candidate selected from observed gains needs the declared
    // corroboration units; one favorable pair is not independent confirmation.
    let mut best_of = policy();
    best_of.stopping.required_units = 2;
    best_of.repeated_selection = RepeatedSelection::BestOf;
    let best_of = with_claim(best_of, ClaimScope::Scoped);
    let declared = declare(&best_of);
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
                70.0,
                true,
                Some(4),
                Some(6),
                true,
            ),
        ],
        &best_of,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(evaluation.decision, PolicyDecision::Inconclusive);
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("corroboration")),
        "{:?}",
        evaluation.reasons
    );

    // A favorable best pair beside a below-threshold pair is not the declared
    // corroboration; both pairs stay retained and no adoption follows.
    let report = summarize(
        &[
            pair(
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
                "p1",
            ),
            pair(
                attempt(
                    "c2",
                    "candidate",
                    "case-b",
                    200.0,
                    70.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p1",
            ),
            pair(
                attempt(
                    "b3",
                    "baseline",
                    "case-b",
                    400.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p2",
            ),
            pair(
                attempt(
                    "c3",
                    "candidate",
                    "case-b",
                    600.0,
                    95.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                "p2",
            ),
        ],
        &best_of,
    );
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_ne!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    assert_eq!(
        evaluation
            .variation
            .as_ref()
            .map(|variation| variation.complete_pairs),
        Some(2),
        "{:?} units={}",
        evaluation.reasons,
        report["units"]
    );
}

/// One valid experiment-selection declaration for a declared method and
/// claim path. Counterexamples replace only the field under test.
fn selection(method: ExperimentMethod, claim: EffectPath) -> ExperimentSelection {
    ExperimentSelection {
        method,
        claim,
        outcome: "the declared outcome measured through the real unit".to_owned(),
        rationale: "the chosen unit exercises the claimed mechanism".to_owned(),
        controls: "frozen inputs and the accepted baseline conditions".to_owned(),
        projection: "one bounded experiment with the retention cost staying bounded".to_owned(),
        baseline: "the accepted revision excluding the candidate edit".to_owned(),
        stopping:
            "stop after the declared attempts and escalate only for a named missing observation"
                .to_owned(),
    }
}

/// The policy with its uncertainty text carrying the predeclared selection.
fn with_selection(
    mut policy: ComparisonPolicy,
    method: ExperimentMethod,
    claim: EffectPath,
) -> ComparisonPolicy {
    policy.uncertainty = format!(
        "unknown evidence stays inconclusive; {}",
        experiment_selection_clause(&selection(method, claim))
    );
    policy
}

#[test]
fn the_predeclared_selection_binds_the_smallest_sufficient_unit() {
    // The declared effect path fixes the smallest sufficient unit, selected
    // directly with no cheaper trial before it.
    assert_eq!(
        EffectPath::LocalOperation.smallest_sufficient(),
        ExperimentMethod::RealOperation
    );
    assert_eq!(
        EffectPath::AgentChoice.smallest_sufficient(),
        ExperimentMethod::AgentTask
    );
    assert_eq!(
        EffectPath::TaskStrategy.smallest_sufficient(),
        ExperimentMethod::PairedImplementations
    );
    assert_eq!(
        EffectPath::RepeatedUse.smallest_sufficient(),
        ExperimentMethod::Sequence
    );

    // A local build/output treatment selects the short real operation, and a
    // bounded input replay of the same operation stays eligible.
    assert!(ExperimentMethod::RealOperation.exercises(EffectPath::LocalOperation));
    assert!(ExperimentMethod::BoundedReplay.exercises(EffectPath::LocalOperation));

    // Agent-choice effects require a real agent: a fixed operation bypasses
    // the choices and a retained replay cannot stand in for the agent.
    let fixed = selection(ExperimentMethod::RealOperation, EffectPath::AgentChoice);
    assert!(
        fixed
            .problem()
            .unwrap()
            .contains("bypasses the agent's choices"),
        "{:?}",
        fixed.problem()
    );
    let replay = selection(ExperimentMethod::BoundedReplay, EffectPath::AgentChoice);
    assert!(
        replay
            .problem()
            .unwrap()
            .contains("cannot stand in for an unexercised agent"),
        "{:?}",
        replay.problem()
    );
    assert!(
        selection(ExperimentMethod::AgentTask, EffectPath::AgentChoice)
            .problem()
            .is_none()
    );

    // A broad strategy claim selects complete paired implementations when
    // shorter work would lose the interactions; the direct selection needs no
    // earlier stage.
    let short = selection(ExperimentMethod::AgentTask, EffectPath::TaskStrategy);
    assert!(
        short
            .problem()
            .unwrap()
            .contains("complete paired task implementations"),
        "{:?}",
        short.problem()
    );
    assert!(
        selection(
            ExperimentMethod::PairedImplementations,
            EffectPath::TaskStrategy
        )
        .problem()
        .is_none()
    );

    // A repeated-use claim preserves the sequence and state; the sequence
    // unit belongs to that path and is not a generic stronger method.
    let single = selection(ExperimentMethod::RealOperation, EffectPath::RepeatedUse);
    assert!(
        single
            .problem()
            .unwrap()
            .contains("sequence and state transitions"),
        "{:?}",
        single.problem()
    );
    assert!(
        selection(ExperimentMethod::Sequence, EffectPath::RepeatedUse)
            .problem()
            .is_none()
    );
    assert!(
        selection(ExperimentMethod::Sequence, EffectPath::LocalOperation)
            .problem()
            .unwrap()
            .contains("only for a repeated-use or recovery claim"),
        "{:?}",
        selection(ExperimentMethod::Sequence, EffectPath::LocalOperation).problem()
    );

    // Fewer lines, files, skills or exposed names never establish benefit.
    let size_only = selection(ExperimentMethod::RealOperation, EffectPath::SizeOnly);
    assert!(
        size_only
            .problem()
            .unwrap()
            .contains("fewer lines, files, skills or exposed names never establish benefit"),
        "{:?}",
        size_only.problem()
    );

    // Empty required values are refused before results.
    let mut empty = selection(ExperimentMethod::RealOperation, EffectPath::LocalOperation);
    empty.outcome = String::new();
    assert!(empty.problem().unwrap().contains("outcome is empty"));
}

#[test]
fn the_selection_clause_round_trips_and_refuses_staged_ladders() {
    let declared = declare(&with_selection(
        policy(),
        ExperimentMethod::AgentTask,
        EffectPath::AgentChoice,
    ));
    let parsed = parse_experiment_selection(&declared.policy.uncertainty)
        .unwrap()
        .expect("the declared clause parses");
    assert_eq!(parsed.method, ExperimentMethod::AgentTask);
    assert_eq!(parsed.claim, EffectPath::AgentChoice);
    assert_eq!(
        declared.policy.declaration()["selection"]["method"],
        "agent-task"
    );
    assert_eq!(
        declared.policy.declaration()["selection"]["claim"],
        "agent-choice"
    );

    // The clause coexists with the statistical claim and the infrastructure
    // binding in the order each owner parses: statistical, selection, binding.
    let mut ordered = policy();
    ordered.uncertainty = format!(
        "{}; {} {}",
        statistical_clause(&claim(ClaimScope::Scoped)),
        experiment_selection_clause(&selection(
            ExperimentMethod::PairedImplementations,
            EffectPath::TaskStrategy
        )),
        binding_clause(MetricView::WorkEfficiency, Mechanism::None)
    );
    let ordered = declare(&ordered);
    assert!(
        parse_statistical_claim(&ordered.policy.uncertainty)
            .unwrap()
            .is_some()
    );
    assert!(
        harness_core::infrastructure_accounting::parse_binding(&ordered.policy.uncertainty)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        parse_experiment_selection(&ordered.policy.uncertainty)
            .unwrap()
            .unwrap()
            .method,
        ExperimentMethod::PairedImplementations
    );

    // A staged ladder is not a declared selection: the clause has no such
    // field, and an unknown method token is refused instead of defaulted.
    let ladder = "unknown evidence stays inconclusive; experiment-selection.v1; method=paired-implementations; claim=task-strategy; outcome=o; rationale=r; controls=c; projection=p; baseline=b; stopping=s; ladder=operation-then-agent-then-paired";
    assert!(
        parse_experiment_selection(ladder)
            .unwrap_err()
            .contains("unknown field"),
        "{}",
        parse_experiment_selection(ladder).unwrap_err()
    );
    let staged = "experiment-selection.v1; method=probe-ladder; claim=task-strategy; outcome=o; rationale=r; controls=c; projection=p; baseline=b; stopping=s";
    assert!(
        parse_experiment_selection(staged)
            .unwrap_err()
            .contains("method"),
        "{}",
        parse_experiment_selection(staged).unwrap_err()
    );
    // An incomplete clause is refused rather than silently defaulted.
    assert!(
        parse_experiment_selection("experiment-selection.v1; method=agent-task")
            .unwrap_err()
            .contains("incomplete")
    );
    // A present but unusable declaration blocks the policy itself.
    let mut broken = policy();
    broken.uncertainty = format!("{staged}; extra=x");
    assert!(broken.declare().is_err());
}

#[test]
fn a_changed_experiment_selection_cannot_inherit_an_adoption() {
    let bound = with_selection(
        policy(),
        ExperimentMethod::RealOperation,
        EffectPath::LocalOperation,
    );
    let declared = declare(&bound);
    let rows = [
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
    ];

    // The measured pair adopted under the frozen selection.
    let report = summarize(&rows, &bound);
    let evaluation = evaluate(&declared, &report).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );

    // The same pair measured before the selection was declared cannot be
    // paired with the later plan.
    let stale = summarize(&rows, &policy());
    let evaluation = evaluate(&declared, &stale).unwrap();
    assert_eq!(
        evaluation.decision,
        PolicyDecision::Inconclusive,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("declaration differs")),
        "{:?}",
        evaluation.reasons
    );

    // A selection changed after its declaration no longer matches the digest.
    let mut changed = declared.clone();
    changed.policy.uncertainty = format!(
        "unknown evidence stays inconclusive; {}",
        experiment_selection_clause(&selection(
            ExperimentMethod::AgentTask,
            EffectPath::AgentChoice
        ))
    );
    assert!(evaluate(&changed, &report).is_err());

    // A recorded declaration that carries a different selection than the
    // predeclared policy cannot decide an adoption: the selection clause is
    // part of the digested uncertainty text recorded before results.
    let mut tampered_declaration = bound.declaration();
    tampered_declaration["uncertainty"] = json!(changed.policy.uncertainty);
    let tampered: Vec<Value> = rows
        .iter()
        .cloned()
        .map(|mut row| {
            row["declaration"] = tampered_declaration.clone();
            row
        })
        .collect();
    let tampered_report = summarize_attempts(&tampered).unwrap();
    let evaluation = evaluate(&declared, &tampered_report).unwrap();
    assert_ne!(
        evaluation.decision,
        PolicyDecision::Adopt,
        "{:?}",
        evaluation.reasons
    );
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("declaration differs")),
        "{:?}",
        evaluation.reasons
    );
}

/// The raised experiment-selection and nuisance-control field bounds admit
/// honest single-line detail up to 512 bytes; oversized, multiline or
/// separator-carrying values still refuse and name the new bound.
#[test]
fn raised_field_bounds_admit_bounded_detail_and_refuse_beyond_them() {
    let mut at_bound = selection(ExperimentMethod::RealOperation, EffectPath::LocalOperation);
    at_bound.rationale = "r".repeat(512);
    assert!(at_bound.problem().is_none(), "{:?}", at_bound.problem());

    let mut over = selection(ExperimentMethod::RealOperation, EffectPath::LocalOperation);
    over.outcome = "o".repeat(513);
    let problem = over.problem().unwrap();
    assert!(
        problem.contains("outcome") && problem.contains("at most 512 bytes without ';'"),
        "{problem}"
    );

    let mut multiline = selection(ExperimentMethod::RealOperation, EffectPath::LocalOperation);
    multiline.controls = "one\ntwo".to_owned();
    let problem = multiline.problem().unwrap();
    assert!(
        problem.contains("controls") && problem.contains("must be one bounded line"),
        "{problem}"
    );

    let mut separated = selection(ExperimentMethod::RealOperation, EffectPath::LocalOperation);
    separated.stopping = "stop after the attempts; escalate for a missing observation".to_owned();
    let problem = separated.problem().unwrap();
    assert!(
        problem.contains("stopping") && problem.contains("without ';'"),
        "{problem}"
    );

    // The owned-prepared recipe carries the same bounded single-line rule.
    let prepared = |recipe: String| {
        let mut plan = nuisance_plan();
        plan.initial = InitialState::OwnedPrepared;
        plan.recipe = Some(recipe);
        plan
    };
    let at_bound = prepared("r".repeat(512));
    assert!(at_bound.problem().is_none(), "{:?}", at_bound.problem());
    let problem = prepared("r".repeat(513)).problem().unwrap();
    assert!(
        problem.contains("at most 512 bytes without ';'"),
        "{problem}"
    );
    let problem = prepared("one\ntwo".to_owned()).problem().unwrap();
    assert!(problem.contains("must be one bounded line"), "{problem}");
    let problem = prepared("one;two".to_owned()).problem().unwrap();
    assert!(problem.contains("without ';'"), "{problem}");
}
