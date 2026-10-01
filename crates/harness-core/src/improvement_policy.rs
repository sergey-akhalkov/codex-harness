//! The predeclared comparison policy for one improvement experiment.
//!
//! The policy is complete before any result is observed: it fixes the
//! correctness gate, the meaningful effect, the tolerated noncritical
//! variation, the matched unit, the stopping budget and the repeated-selection
//! treatment, and it is digested into the experiment bindings. [`evaluate`]
//! consumes the authoritative outcome accounting
//! ([`crate::outcome_report::summarize_attempts`]) and applies the declared
//! rules; it never substitutes a model outcome, a synthetic timing or a
//! post-hoc threshold. Its result is operational evidence for a board decision
//! published through the benefit gate, not a decision publication itself.

use crate::benefit_gate::{DecisionDraft, DecisionOutcome, QualityOutcome};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

pub const POLICY_SCHEMA: u32 = 1;

fn invalid(detail: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("comparison policy: {detail}"),
    )
}

fn unsupported(detail: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("comparison policy: {detail}"),
    )
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|value| value.is_finite())
}

fn counter(row: &Value, key: &str) -> Option<u64> {
    row.get(key).and_then(Value::as_u64)
}

fn unit_name(unit: &Value) -> String {
    unit.get("unit")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed unit>")
        .to_owned()
}

/// The measured dimension of the declared comparison rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Metric {
    Time,
    Rounds,
    ToolOperations,
    Usage,
}

/// What the comparison decides. `Quality` requires a correctness difference;
/// `Time` and `Resource` require a meaningful measured efficiency effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Objective {
    Quality,
    Time,
    Resource,
}

impl Objective {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quality => "quality",
            Self::Time => "time",
            Self::Resource => "resource",
        }
    }

    /// The outcome-accounting claim class this objective maps to.
    pub fn claim_class(self) -> &'static str {
        match self {
            Self::Quality => "deterministic_quality",
            Self::Time | Self::Resource => "stochastic_savings",
        }
    }

    /// The dimensions that must be measured on both arm results before the
    /// declared claim can be evaluated.
    pub fn required_metrics(self) -> &'static [Metric] {
        match self {
            Self::Quality => &[],
            Self::Time => &[Metric::Time, Metric::Rounds, Metric::ToolOperations],
            Self::Resource => &[
                Metric::Time,
                Metric::Rounds,
                Metric::ToolOperations,
                Metric::Usage,
            ],
        }
    }
}

/// What a comparison result may claim. The default efficiency policy requires
/// a meaningful measured effect; a maintenance-only claim needs its own
/// predeclared, prior-agreed basis and never becomes an efficiency result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Basis {
    Efficiency,
    Maintenance { basis: String },
}

impl Basis {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Efficiency => "efficiency",
            Self::Maintenance { .. } => "maintenance",
        }
    }

    pub fn maintenance_basis(&self) -> Option<&str> {
        match self {
            Self::Efficiency => None,
            Self::Maintenance { basis } => Some(basis),
        }
    }
}

/// How a candidate selected from several observed attempts is treated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepeatedSelection {
    /// Each declared unit was fixed before results.
    Predeclared,
    /// The candidate was chosen after observing gains, so independent
    /// corroboration units are required before adoption.
    BestOf,
}

/// A predeclared trade-off between the primary effect and another measured
/// dimension. It is required before a genuine cost/time exchange can be
/// adopted; no weighted score is invented at evaluation time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TradeOff {
    /// Why the declared exchange is worth it; agreed before results.
    pub basis: String,
    /// Maximum tolerated regression percent on the traded-away dimension.
    pub allowed_regression_percent: f64,
}

/// The declared stopping rule for the experiment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoppingRule {
    pub max_attempts_per_arm: u32,
    /// Independently matched units required for an adoption.
    pub required_units: u32,
}

/// Experiment overhead recorded once, for net-effect statements.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Overhead {
    pub implementation_seconds: f64,
    pub evaluation_seconds: f64,
    pub maintenance_seconds_per_task: f64,
}

/// The complete policy, fixed before any comparative result is observed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComparisonPolicy {
    pub schema: u32,
    pub objective: Objective,
    pub basis: Basis,
    /// Minimum meaningful relative effect in percent for time or resource
    /// objectives under the efficiency basis.
    pub meaningful_effect_percent: Option<f64>,
    /// Tolerated noncritical variation in percent.
    pub tolerance_percent: f64,
    /// The independent acceptance gate is mandatory; this must be true.
    pub require_acceptance: bool,
    /// The declared task mix the result is scoped to.
    pub task_mix: String,
    pub stopping: StoppingRule,
    pub repeated_selection: RepeatedSelection,
    pub trade_off: Option<TradeOff>,
    /// How uncertainty and coverage gaps are handled.
    pub uncertainty: String,
    pub horizon_tasks: f64,
    pub overhead: Overhead,
}

/// A policy fixed and digested before results.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclaredComparison {
    pub policy: ComparisonPolicy,
    pub digest: String,
}

impl ComparisonPolicy {
    pub fn stopping_text(&self) -> String {
        format!(
            "max_attempts_per_arm={} required_units={}",
            self.stopping.max_attempts_per_arm, self.stopping.required_units
        )
    }

    /// The normalized declaration attached to every measured attempt before
    /// results, in the shape the outcome accounting consumes.
    pub fn declaration(&self) -> Value {
        let declared_effect = match self.objective {
            Objective::Quality => self.meaningful_effect_percent,
            // A maintenance basis declares no efficiency threshold; the
            // accounting still needs an explicit zero so its evidence remains
            // complete rather than ambiguous.
            _ => Some(self.meaningful_effect_percent.unwrap_or(0.0)),
        };
        let mut value = json!({
            "task_mix": self.task_mix,
            "objective": self.objective.as_str(),
            "nuisance": true,
            "stopping": self.stopping_text(),
            "uncertainty": self.uncertainty,
            "horizon_tasks": self.horizon_tasks,
            "costs": {
                "implementation_seconds": self.overhead.implementation_seconds,
                "evaluation_seconds": self.overhead.evaluation_seconds,
                "maintenance_seconds_per_task": self.overhead.maintenance_seconds_per_task,
            },
        });
        if let Some(effect) = declared_effect {
            value["effect_percent"] = json!(effect);
        }
        value
    }

    /// Validate and digest the policy. Everything consequential is declared
    /// here, before any result exists.
    pub fn declare(&self) -> io::Result<DeclaredComparison> {
        if self.schema != POLICY_SCHEMA {
            return Err(invalid("unsupported comparison policy schema"));
        }
        match &self.basis {
            Basis::Efficiency => {
                if self.objective != Objective::Quality
                    && !self
                        .meaningful_effect_percent
                        .is_some_and(|effect| effect.is_finite() && effect > 0.0)
                {
                    return Err(invalid(
                        "an efficiency comparison needs a positive meaningful effect percent",
                    ));
                }
            }
            Basis::Maintenance { basis } => {
                if basis.trim().is_empty() || basis.len() > 1024 || basis.contains(['\n', '\r']) {
                    return Err(invalid(
                        "a maintenance basis must state the pre-agreed reason, before results",
                    ));
                }
            }
        }
        if let Some(effect) = self.meaningful_effect_percent
            && (!effect.is_finite() || effect < 0.0)
        {
            return Err(invalid(
                "meaningful_effect_percent must be a finite non-negative number",
            ));
        }
        if !self.tolerance_percent.is_finite() || self.tolerance_percent < 0.0 {
            return Err(invalid(
                "tolerance_percent must be a finite non-negative number",
            ));
        }
        if !self.require_acceptance {
            return Err(invalid(
                "independent acceptance is mandatory; a policy cannot waive the correctness gate",
            ));
        }
        if self.task_mix.trim().is_empty() || self.task_mix.len() > 512 {
            return Err(invalid("a bounded task mix must be declared"));
        }
        if self.uncertainty.trim().is_empty() || self.uncertainty.len() > 1024 {
            return Err(invalid("an uncertainty policy must be declared"));
        }
        if self.stopping.max_attempts_per_arm == 0 || self.stopping.required_units == 0 {
            return Err(invalid(
                "stopping requires at least one attempt per arm and one independent unit",
            ));
        }
        if self.repeated_selection == RepeatedSelection::BestOf && self.stopping.required_units < 2
        {
            return Err(invalid(
                "a candidate selected from observed gains needs independent corroboration units",
            ));
        }
        if let Some(trade_off) = &self.trade_off {
            if trade_off.basis.trim().is_empty() || trade_off.basis.len() > 1024 {
                return Err(invalid(
                    "a trade-off needs its predeclared basis before results",
                ));
            }
            if !trade_off.allowed_regression_percent.is_finite()
                || trade_off.allowed_regression_percent < 0.0
            {
                return Err(invalid(
                    "the trade-off allowance must be a finite non-negative percent",
                ));
            }
        }
        if !self.horizon_tasks.is_finite() || self.horizon_tasks <= 0.0 {
            return Err(invalid("horizon_tasks must be a finite positive number"));
        }
        for (name, value) in [
            (
                "implementation_seconds",
                self.overhead.implementation_seconds,
            ),
            ("evaluation_seconds", self.overhead.evaluation_seconds),
            (
                "maintenance_seconds_per_task",
                self.overhead.maintenance_seconds_per_task,
            ),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(invalid(format!(
                    "overhead {name} must be a finite non-negative number"
                )));
            }
        }
        if let Err(error) = crate::infrastructure_accounting::parse_binding(&self.uncertainty) {
            return Err(invalid(error));
        }
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(self).map_err(io::Error::other)?)
        );
        Ok(DeclaredComparison {
            policy: self.clone(),
            digest,
        })
    }
}

impl DeclaredComparison {
    /// The policy is bound to its digest, and the digest is recomputed from
    /// the policy itself: a deserialized or mutated declaration whose stored
    /// digest no longer matches its fields is refused before it can produce a
    /// decision or an adoption.
    pub fn verify(&self) -> io::Result<()> {
        let recomputed = self.policy.declare()?;
        if recomputed.digest != self.digest {
            return Err(invalid(
                "the declared policy changed after its declaration; the stored digest no longer binds it",
            ));
        }
        Ok(())
    }

    pub fn digest_matches(&self, digest: &str) -> bool {
        self.verify().is_ok() && self.digest == digest
    }
}

/// The evaluation's operational decision, ready for the board decision owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PolicyDecision {
    Adopt,
    Reject,
    Inconclusive,
}

impl PolicyDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Adopt => "adopt",
            Self::Reject => "reject",
            Self::Inconclusive => "inconclusive",
        }
    }

    fn gate(self) -> DecisionOutcome {
        match self {
            Self::Adopt => DecisionOutcome::Adopt,
            Self::Reject => DecisionOutcome::Reject,
            Self::Inconclusive => DecisionOutcome::Inconclusive,
        }
    }
}

/// Measured corrective quality across the included units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Quality {
    Unchanged,
    Improved,
    Regressed,
    Unmeasurable,
}

impl Quality {
    fn gate(self) -> QualityOutcome {
        match self {
            Self::Unchanged => QualityOutcome::Unchanged,
            Self::Improved => QualityOutcome::Improved,
            Self::Regressed => QualityOutcome::Regressed,
            Self::Unmeasurable => QualityOutcome::Unmeasurable,
        }
    }
}

/// Per-success cost derived from the authoritative accounting. It is never
/// computed by summing usage categories: overlapping token categories and
/// shared work are counted once by that owner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PerSuccess {
    pub status: PerSuccessStatus,
    pub seconds: Option<f64>,
    pub accepted_tasks: u64,
    pub tasks: u64,
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PerSuccessStatus {
    Complete,
    Incomplete,
    Undefined,
}

/// Operational evaluation evidence. Publishing the decision stays with the
/// benefit-gate owner; this record only states what was measured and decided.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyEvaluation {
    pub schema: u32,
    pub policy_digest: String,
    pub decision: PolicyDecision,
    pub basis: Basis,
    pub quality: Quality,
    pub matched: u64,
    pub baseline_seconds: Option<f64>,
    pub candidate_seconds: Option<f64>,
    pub tolerance_percent: f64,
    pub coverage: String,
    pub scope: String,
    pub reasons: Vec<String>,
    pub per_success: PerSuccess,
    pub attempts: u64,
    pub tasks: u64,
    pub accepted_tasks: u64,
    pub acceptance_rate: Option<f64>,
    pub trade_off_used: bool,
}

struct UnitFacts {
    name: String,
    case_id: String,
    baseline_seconds: Option<f64>,
    candidate_seconds: Option<f64>,
    baseline_steps: Option<u64>,
    candidate_steps: Option<u64>,
    usage_measured: bool,
    baseline_accepted: bool,
    candidate_accepted: bool,
    positive_effect: bool,
    evidence_complete: bool,
    does_not_repay: bool,
    limitations: Vec<String>,
}

fn unit_limitations(unit: &Value) -> Vec<String> {
    unit.get("limitations")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn arm_result<'a>(
    results: &BTreeMap<&'a str, &'a Value>,
    unit: &Value,
    key: &str,
) -> Option<&'a Value> {
    unit.get(key)
        .and_then(Value::as_str)
        .and_then(|id| results.get(id).copied())
}

fn accepted(row: &Value) -> bool {
    row.get("status").and_then(Value::as_str) == Some("accepted")
}

fn steps(row: &Value) -> Option<u64> {
    let rounds = counter(row, "total_rounds")?;
    let tools = counter(row, "total_tool_operations")?;
    rounds.checked_add(tools)
}

fn usage_measured(row: &Value) -> bool {
    row.get("usage")
        .and_then(Value::as_object)
        .is_some_and(|usage| {
            usage.get("status").and_then(Value::as_str) == Some("per_run")
                && usage.get("unknown_runs").is_none()
        })
}

fn percent_regression(baseline: f64, candidate: f64) -> Option<f64> {
    (baseline > 0.0).then(|| (candidate - baseline) / baseline * 100.0)
}

/// Evaluate the declared policy against the authoritative outcome summary.
pub fn evaluate(declared: &DeclaredComparison, report: &Value) -> io::Result<PolicyEvaluation> {
    declared.verify()?;
    let policy = &declared.policy;
    if report.get("schema_version").and_then(Value::as_u64) != Some(2) {
        return Err(invalid(
            "the outcome summary is not the authoritative schema-2 report",
        ));
    }
    let attempts = report
        .get("attempts")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("the outcome summary lacks attempts"))?;
    let units = report
        .get("units")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("the outcome summary lacks units"))?;
    let accounting = report
        .get("accounting")
        .ok_or_else(|| invalid("the outcome summary lacks accounting"))?;
    let results: BTreeMap<&str, &Value> = attempts
        .iter()
        .filter_map(|row| {
            row.get("attempt_id")
                .and_then(Value::as_str)
                .map(|id| (id, row))
        })
        .collect();

    let mut reasons: Vec<String> = Vec::new();
    let mut recorded_limitations: Vec<String> = Vec::new();
    let mut included: Vec<&Value> = Vec::new();
    for unit in units {
        let name = unit_name(unit);
        let claim = unit
            .get("claim_class")
            .and_then(Value::as_str)
            .unwrap_or("");
        if claim != policy.objective.claim_class() {
            recorded_limitations.push(format!(
                "unit {name}: claim class '{claim}' does not match the declared {} objective",
                policy.objective.as_str()
            ));
            continue;
        }
        let comparable = unit
            .get("comparable_pairs")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let one_to_one = unit
            .get("one_to_one")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !one_to_one || comparable == 0 {
            recorded_limitations.push(format!(
                "unit {name}: repeated or unmatched attempt edges count once and are not an independent sample"
            ));
            continue;
        }
        included.push(unit);
    }

    // A recorded declaration that differs from the predeclared policy
    // invalidates comparability; the policy was fixed before results.
    let mut drift: Vec<String> = Vec::new();
    for unit in &included {
        let name = unit_name(unit);
        let Some(declared_value) = unit.get("declared").and_then(Value::as_object) else {
            drift.push(format!("unit {name}: no declaration was recorded"));
            continue;
        };
        let field = |key: &str| declared_value.get(key).and_then(Value::as_str);
        if field("objective") != Some(policy.objective.as_str()) {
            drift.push(format!("unit {name}: declared objective differs"));
        }
        if field("task_mix") != Some(policy.task_mix.as_str()) {
            drift.push(format!("unit {name}: declared task mix differs"));
        }
        if field("stopping") != Some(policy.stopping_text().as_str()) {
            drift.push(format!("unit {name}: declared stopping rule differs"));
        }
        if field("uncertainty") != Some(policy.uncertainty.as_str()) {
            drift.push(format!("unit {name}: declared uncertainty policy differs"));
        }
        let declared_effect = declared_value.get("effect_percent").and_then(number);
        let expected_effect = match policy.objective {
            Objective::Quality => policy.meaningful_effect_percent,
            _ => Some(policy.meaningful_effect_percent.unwrap_or(0.0)),
        };
        match (declared_effect, expected_effect) {
            (Some(a), Some(b)) if a == b => {}
            (None, None) => {}
            _ => drift.push(format!("unit {name}: declared meaningful effect differs")),
        }
    }

    let mut attempts_per_arm: BTreeMap<String, u64> = BTreeMap::new();
    for row in attempts {
        if let Some(arm) = row.get("arm").and_then(Value::as_str) {
            *attempts_per_arm.entry(arm.to_owned()).or_default() += 1;
        }
    }
    let over_budget: Vec<String> = attempts_per_arm
        .iter()
        .filter(|(_, count)| **count > u64::from(policy.stopping.max_attempts_per_arm))
        .map(|(arm, count)| format!("arm {arm} recorded {count} attempts"))
        .collect();
    // Failed task results stay in the accounting; a candidate task that never
    // achieved independent acceptance while the baseline did cannot support
    // adoption, whatever its measured duration. A task is the latest attempt
    // of its retry chain, exactly as the accounting counts tips.
    let referenced: BTreeSet<&str> = attempts
        .iter()
        .filter_map(|row| row.get("retry_of").and_then(Value::as_str))
        .filter(|id| !id.is_empty())
        .collect();
    let is_tip = |row: &Value| {
        row.get("attempt_id")
            .and_then(Value::as_str)
            .is_some_and(|id| !referenced.contains(id))
    };
    let failed_candidate_tasks: Vec<String> = attempts
        .iter()
        .filter(|row| {
            row.get("arm").and_then(Value::as_str) == Some("candidate")
                && is_tip(row)
                && row.get("status").and_then(Value::as_str) != Some("accepted")
        })
        .filter(|row| {
            let case = row.get("case_id").and_then(Value::as_str).unwrap_or("");
            attempts.iter().any(|other| {
                other.get("arm").and_then(Value::as_str) == Some("baseline")
                    && other.get("case_id").and_then(Value::as_str) == Some(case)
                    && is_tip(other)
                    && other.get("status").and_then(Value::as_str) == Some("accepted")
            })
        })
        .map(|row| {
            row.get("attempt_id")
                .and_then(Value::as_str)
                .unwrap_or("<unnamed>")
                .to_owned()
        })
        .collect();

    let mut facts: Vec<UnitFacts> = Vec::new();
    let mut missing_results = false;
    for unit in &included {
        let name = unit_name(unit);
        let baseline_row = arm_result(&results, unit, "baseline_result");
        let candidate_row = arm_result(&results, unit, "candidate_result");
        if baseline_row.is_none() || candidate_row.is_none() {
            missing_results = true;
        }
        let usages = match (baseline_row, candidate_row) {
            (Some(a), Some(b)) => usage_measured(a) && usage_measured(b),
            _ => false,
        };
        facts.push(UnitFacts {
            case_id: unit
                .get("case_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            baseline_seconds: number(&unit["effect"]["baseline_seconds"]),
            candidate_seconds: number(&unit["effect"]["candidate_seconds"]),
            baseline_steps: baseline_row.and_then(steps),
            candidate_steps: candidate_row.and_then(steps),
            usage_measured: usages,
            baseline_accepted: baseline_row.is_some_and(accepted),
            candidate_accepted: candidate_row.is_some_and(accepted),
            positive_effect: unit.get("positive_effect") == Some(&Value::Bool(true)),
            evidence_complete: unit.get("evidence_complete") == Some(&Value::Bool(true)),
            does_not_repay: unit
                .get("net_saving")
                .and_then(|saving| saving.get("verdict"))
                .and_then(Value::as_str)
                == Some("does_not_repay"),
            limitations: unit_limitations(unit),
            name,
        });
    }

    let matched: u64 = included
        .iter()
        .map(|unit| {
            unit.get("comparable_pairs")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        })
        .sum();
    let mut baseline_seconds = (facts.iter().all(|fact| fact.baseline_seconds.is_some())
        && !facts.is_empty())
    .then(|| {
        facts
            .iter()
            .filter_map(|fact| fact.baseline_seconds)
            .sum::<f64>()
    });
    let mut candidate_seconds = (facts.iter().all(|fact| fact.candidate_seconds.is_some())
        && !facts.is_empty())
    .then(|| {
        facts
            .iter()
            .filter_map(|fact| fact.candidate_seconds)
            .sum::<f64>()
    });

    let attempts_total = accounting
        .get("attempts")
        .and_then(Value::as_u64)
        .unwrap_or(attempts.len() as u64);
    let tasks = accounting.get("tasks").and_then(Value::as_u64).unwrap_or(0);
    let accepted_tasks = accounting
        .get("accepted_tasks")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let acceptance_rate = accounting.get("acceptance_rate").and_then(number);
    let cost = accounting.get("cost_per_accepted_task");
    let per_success = match cost
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str)
    {
        Some("complete") => PerSuccess {
            status: PerSuccessStatus::Complete,
            seconds: cost.and_then(|value| number(&value["seconds"])),
            accepted_tasks,
            tasks,
            reason: None,
        },
        Some("undefined") => PerSuccess {
            status: PerSuccessStatus::Undefined,
            seconds: None,
            accepted_tasks,
            tasks,
            reason: Some("no accepted task: cost per success is undefined".to_owned()),
        },
        _ => PerSuccess {
            status: PerSuccessStatus::Incomplete,
            seconds: None,
            accepted_tasks,
            tasks,
            reason: Some(
                "some attempt costs are unknown; no per-success cost is reported".to_owned(),
            ),
        },
    };

    // Correctness across included units.
    let regressed_units: Vec<&UnitFacts> = facts
        .iter()
        .filter(|fact| fact.baseline_accepted && !fact.candidate_accepted)
        .collect();
    let quality = if missing_results {
        Quality::Unmeasurable
    } else if !regressed_units.is_empty() {
        Quality::Regressed
    } else if facts
        .iter()
        .any(|fact| fact.candidate_accepted && !fact.baseline_accepted)
    {
        Quality::Improved
    } else if facts.is_empty() {
        Quality::Unmeasurable
    } else {
        Quality::Unchanged
    };

    let cases: Vec<String> = {
        let mut sorted: Vec<String> = facts
            .iter()
            .map(|fact| fact.case_id.clone())
            .filter(|case| !case.is_empty())
            .collect();
        sorted.sort();
        sorted.dedup();
        sorted
    };
    let scope = format!(
        "matched pairs on {}; each task is compared within itself and absolute durations across different tasks are not a speed trend",
        if cases.is_empty() {
            "no task".to_owned()
        } else {
            cases.join(", ")
        }
    );

    // Required measured coverage for the declared objective.
    let mut coverage_parts: Vec<&str> = Vec::new();
    if !facts.is_empty() {
        if facts
            .iter()
            .all(|fact| fact.baseline_seconds.is_some() && fact.candidate_seconds.is_some())
        {
            coverage_parts.push("time");
        }
        if facts.iter().all(|fact| fact.baseline_steps.is_some()) {
            coverage_parts.push("rounds+tool_ops");
        }
        if facts.iter().all(|fact| fact.usage_measured) {
            coverage_parts.push("usage");
        }
    }
    let mut coverage = if coverage_parts.is_empty() {
        "none".to_owned()
    } else {
        coverage_parts.join("+")
    };
    let mut missing_coverage: Vec<String> = Vec::new();
    for metric in policy.objective.required_metrics() {
        let measured = match metric {
            Metric::Time => facts
                .iter()
                .all(|fact| fact.baseline_seconds.is_some() && fact.candidate_seconds.is_some()),
            Metric::Rounds | Metric::ToolOperations => facts
                .iter()
                .all(|fact| fact.baseline_steps.is_some() && fact.candidate_steps.is_some()),
            Metric::Usage => facts.iter().all(|fact| fact.usage_measured),
        };
        if !measured {
            missing_coverage.push(format!("{metric:?}"));
        }
    }

    // Positive primary units per objective.
    let positive_units = facts
        .iter()
        .filter(|fact| match policy.objective {
            Objective::Quality => fact.positive_effect && fact.evidence_complete,
            Objective::Time => fact.positive_effect && fact.evidence_complete,
            Objective::Resource => {
                fact.evidence_complete
                    && fact.baseline_steps.zip(fact.candidate_steps).is_some_and(
                        |(baseline, candidate)| {
                            let reduction = -percent_regression(baseline as f64, candidate as f64)
                                .unwrap_or(0.0);
                            reduction >= policy.meaningful_effect_percent.unwrap_or(f64::INFINITY)
                        },
                    )
            }
        })
        .count();
    let required_units = policy.stopping.required_units as usize;

    // Noncritical variation on the other measured dimension.
    let time_regressions: Vec<(String, f64)> = facts
        .iter()
        .filter_map(|fact| {
            let baseline = fact.baseline_seconds?;
            let candidate = fact.candidate_seconds?;
            percent_regression(baseline, candidate).map(|percent| (fact.name.clone(), percent))
        })
        .filter(|(_, percent)| *percent > policy.tolerance_percent)
        .collect();
    let steps_regressions: Vec<(String, f64)> = facts
        .iter()
        .filter_map(|fact| {
            let baseline = fact.baseline_steps?;
            let candidate = fact.candidate_steps?;
            percent_regression(baseline as f64, candidate as f64)
                .map(|percent| (fact.name.clone(), percent))
        })
        .filter(|(_, percent)| *percent > policy.tolerance_percent)
        .collect();
    let worst_time = time_regressions
        .iter()
        .map(|(_, percent)| *percent)
        .fold(f64::NEG_INFINITY, f64::max);
    let worst_steps = steps_regressions
        .iter()
        .map(|(_, percent)| *percent)
        .fold(f64::NEG_INFINITY, f64::max);
    let mut trade_off_used = false;
    let covers = |worst: f64| {
        policy
            .trade_off
            .as_ref()
            .is_some_and(|trade_off| worst <= trade_off.allowed_regression_percent)
    };

    let mut decision = PolicyDecision::Inconclusive;
    if !drift.is_empty() {
        reasons.push(
            "the recorded attempt declaration differs from the predeclared policy; the comparison is invalid"
                .to_owned(),
        );
        reasons.extend(drift.iter().take(4).cloned());
    } else if !failed_candidate_tasks.is_empty() {
        reasons.push(format!(
            "independent acceptance failed on {} candidate task result(s) ({}); correctness and benefit remain separate decisions",
            failed_candidate_tasks.len(),
            failed_candidate_tasks.join(", ")
        ));
        decision = PolicyDecision::Reject;
    } else if included.is_empty() {
        reasons.push(
            "no matched, independently accepted unit was recorded for the declared policy"
                .to_owned(),
        );
    } else if !over_budget.is_empty() {
        reasons.push(format!(
            "the predeclared stopping budget was exceeded ({}); results beyond it are not a decision basis",
            over_budget.join(", ")
        ));
    } else if missing_results {
        reasons.push(
            "acceptance evidence is incomplete on one or both arms of an included unit".to_owned(),
        );
    } else if !regressed_units.is_empty() {
        reasons.push(format!(
            "independent acceptance failed on {} matched unit(s): {}",
            regressed_units.len(),
            regressed_units
                .iter()
                .map(|fact| fact.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        decision = PolicyDecision::Reject;
    } else if accepted_tasks == 0 {
        reasons.push(
            "no accepted task on either arm: the cost per success is undefined and no effect can be adopted"
                .to_owned(),
        );
    } else if policy.objective != Objective::Quality
        && per_success.status != PerSuccessStatus::Complete
    {
        reasons.push(format!(
            "the measured cost accounting is {}; a per-success effect cannot be stated",
            match per_success.status {
                PerSuccessStatus::Undefined => "undefined",
                _ => "incomplete",
            }
        ));
    } else {
        let incomplete: Vec<&UnitFacts> = facts
            .iter()
            .filter(|fact| !fact.evidence_complete)
            .collect();
        if !incomplete.is_empty() {
            reasons.push(format!(
                "comparison evidence is incomplete for {} unit(s); unknown evidence is not a decision basis",
                incomplete.len()
            ));
            for fact in &incomplete {
                for limitation in fact.limitations.iter().take(2) {
                    reasons.push(format!("unit {}: {limitation}", fact.name));
                }
            }
        } else if !missing_coverage.is_empty() {
            reasons.push(format!(
                "the declared metric coverage is incomplete ({}); unmeasured dimensions cannot support adoption",
                missing_coverage.join(", ")
            ));
        } else if policy.objective == Objective::Time && worst_time > policy.tolerance_percent {
            // A favourable subset never offsets a materially regressed primary
            // metric in the same declared task mix; the aggregate claim would
            // be unsupported even though the required count of positive units
            // was recorded.
            reasons.push(format!(
                "the candidate materially regressed the primary time metric on at least one matched unit (worst +{worst_time:.1}% beyond the {:.1}% tolerance); a favourable unit does not offset it",
                policy.tolerance_percent
            ));
            decision = PolicyDecision::Reject;
        } else if policy.objective == Objective::Resource && worst_steps > policy.tolerance_percent
        {
            reasons.push(format!(
                "the candidate materially regressed matched steps on at least one matched unit (worst +{worst_steps:.1}% beyond the {:.1}% tolerance); a favourable unit does not offset it",
                policy.tolerance_percent
            ));
            decision = PolicyDecision::Reject;
        } else {
            let maintenance = policy.basis.maintenance_basis().map(str::to_owned);
            if facts.len() < required_units {
                reasons.push(format!(
                    "the predeclared corroboration was not recorded (needed {required_units} independent unit(s), recorded {})",
                    facts.len()
                ));
            } else if positive_units < required_units {
                if let Some(basis) = &maintenance {
                    // A maintenance-only basis never covers a material
                    // regression of a measured dimension: an exchange must be
                    // admitted by the same predeclared trade-off policy the
                    // efficiency path uses, never a bypass.
                    let mut blocked = false;
                    for (regressions, worst) in [
                        (&time_regressions, worst_time),
                        (&steps_regressions, worst_steps),
                    ] {
                        if regressions.is_empty() {
                            continue;
                        }
                        if covers(worst) {
                            trade_off_used = true;
                        } else {
                            blocked = true;
                            reasons.push(format!(
                                "the maintenance basis does not cover a material regression in a measured dimension (worst +{worst:.1}% beyond the {:.1}% tolerance); it needs the predeclared trade-off policy",
                                policy.tolerance_percent
                            ));
                        }
                    }
                    if blocked {
                        decision = PolicyDecision::Reject;
                    } else {
                        if trade_off_used {
                            reasons.push(format!(
                                "adopted under the predeclared trade-off: {}",
                                policy
                                    .trade_off
                                    .as_ref()
                                    .map(|trade_off| trade_off.basis.as_str())
                                    .unwrap_or("")
                            ));
                        }
                        reasons.push(format!(
                            "no meaningful efficiency effect was measured; adoption rests on the predeclared maintenance basis: {basis}"
                        ));
                        decision = PolicyDecision::Adopt;
                    }
                } else {
                    reasons.push(
                        "the recorded effect does not meet the predeclared meaningful threshold; reduced size or an unmeasured benefit is not an efficiency effect"
                            .to_owned(),
                    );
                    decision = PolicyDecision::Reject;
                }
            } else {
                // A genuine exchange between dimensions needs its own
                // predeclared policy, never a score invented at evaluation.
                let mut blocked = false;
                if policy.objective == Objective::Time && !steps_regressions.is_empty() {
                    if covers(worst_steps) {
                        trade_off_used = true;
                        reasons.push(format!(
                            "adopted under the predeclared trade-off: {}",
                            policy
                                .trade_off
                                .as_ref()
                                .map(|trade_off| trade_off.basis.as_str())
                                .unwrap_or("")
                        ));
                    } else {
                        reasons.push(
                            "time improved but matched step operations regressed beyond the declared tolerance; a genuine trade-off needs its predeclared policy"
                                .to_owned(),
                        );
                        blocked = true;
                    }
                }
                if policy.objective == Objective::Resource
                    && !time_regressions.is_empty()
                    && !blocked
                {
                    if covers(worst_time) {
                        trade_off_used = true;
                        reasons.push(format!(
                            "adopted under the predeclared trade-off: {}",
                            policy
                                .trade_off
                                .as_ref()
                                .map(|trade_off| trade_off.basis.as_str())
                                .unwrap_or("")
                        ));
                    } else {
                        reasons.push(
                            "the resource effect comes with a time regression beyond tolerance; declare the trade-off policy before results"
                                .to_owned(),
                        );
                        blocked = true;
                    }
                }
                if !blocked && facts.iter().any(|fact| fact.does_not_repay) {
                    reasons.push(
                        "the declared per-task effect does not repay implementation and evaluation within the declared horizon"
                            .to_owned(),
                    );
                    blocked = true;
                }
                decision = if blocked {
                    PolicyDecision::Reject
                } else {
                    PolicyDecision::Adopt
                };
            }
        }
    }

    if decision == PolicyDecision::Adopt
        && policy.objective != Objective::Quality
        && per_success.status != PerSuccessStatus::Complete
    {
        // Cannot happen through the ordering above, but never emit an
        // efficiency adoption without a defined per-success figure.
        decision = PolicyDecision::Inconclusive;
        reasons
            .push("the per-success cost is undefined; no efficiency adoption is stated".to_owned());
    }

    for fact in &facts {
        for limitation in &fact.limitations {
            recorded_limitations.push(format!("unit {}: {limitation}", fact.name));
        }
    }
    if !missing_coverage.is_empty() {
        recorded_limitations.push(format!(
            "unmeasured declared dimensions: {}",
            missing_coverage.join(", ")
        ));
    }
    if let Some(basis) = policy.basis.maintenance_basis() {
        recorded_limitations.push(format!(
            "maintenance basis declared before results: {basis}"
        ));
    }

    apply_infrastructure_gate(
        policy,
        report,
        &mut decision,
        &mut reasons,
        &mut baseline_seconds,
        &mut candidate_seconds,
        &mut coverage,
    );

    Ok(PolicyEvaluation {
        schema: POLICY_SCHEMA,
        policy_digest: declared.digest.clone(),
        decision,
        basis: policy.basis.clone(),
        quality,
        matched,
        baseline_seconds,
        candidate_seconds,
        tolerance_percent: policy.tolerance_percent,
        coverage,
        scope,
        reasons,
        per_success,
        attempts: attempts_total,
        tasks,
        accepted_tasks,
        acceptance_rate,
        trade_off_used,
    })
}

fn apply_infrastructure_gate(
    policy: &ComparisonPolicy,
    report: &Value,
    decision: &mut PolicyDecision,
    reasons: &mut Vec<String>,
    baseline_seconds: &mut Option<f64>,
    candidate_seconds: &mut Option<f64>,
    coverage: &mut String,
) {
    let binding = match crate::infrastructure_accounting::parse_binding(&policy.uncertainty) {
        Ok(Some(binding)) => binding,
        Ok(None) => return,
        Err(error) => {
            *decision = PolicyDecision::Inconclusive;
            reasons.push(format!("the infrastructure binding is not usable: {error}"));
            return;
        }
    };
    if binding.mechanism.owns_queue() {
        if binding.view == crate::infrastructure_accounting::MetricView::WorkEfficiency {
            *decision = PolicyDecision::Inconclusive;
            reasons.push(
                "the treatment changes admission, scheduling or waiting; evaluate that operational effect under controlled load instead of subtracting it"
                    .to_owned(),
            );
        } else {
            reasons.push(
                "operational view retains the waiting change; it is not subtracted from either arm"
                    .to_owned(),
            );
        }
        return;
    }
    if binding.view == crate::infrastructure_accounting::MetricView::Operational {
        reasons.push(
            "operational view retains observed waiting and cost; adjusted figures are not the decision metric"
                .to_owned(),
        );
        return;
    }
    let Some(threshold) = policy.meaningful_effect_percent else {
        return;
    };
    let units = report
        .get("units")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let attempts = report
        .get("attempts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if attempts
        .iter()
        .any(crate::infrastructure_accounting::failed_before_start)
        && *decision == PolicyDecision::Reject
    {
        *decision = PolicyDecision::Inconclusive;
        reasons.push(
            "a queue timeout or cancellation before the command started is a failed infrastructure attempt, not an incorrect model solution"
                .to_owned(),
        );
        return;
    }
    let row = |id: Option<&str>| {
        id.and_then(|id| {
            attempts
                .iter()
                .find(|row| row.get("attempt_id").and_then(Value::as_str) == Some(id))
        })
    };
    let mut missing = false;
    let mut straddles = false;
    let mut below = true;
    let mut above = true;
    let mut failed_infra = false;
    let mut saw = false;
    for unit in units.iter().filter(|unit| {
        unit.get("one_to_one").and_then(Value::as_bool) == Some(true)
            && unit
                .get("comparable_pairs")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                > 0
    }) {
        saw = true;
        let baseline = row(unit.get("baseline_result").and_then(Value::as_str));
        let candidate = row(unit.get("candidate_result").and_then(Value::as_str));
        if candidate.is_some_and(crate::infrastructure_accounting::failed_before_start) {
            failed_infra = true;
        }
        let Some((baseline_low, baseline_high)) =
            baseline.and_then(crate::infrastructure_accounting::arm_bounds)
        else {
            missing = true;
            continue;
        };
        let Some((candidate_low, candidate_high)) =
            candidate.and_then(crate::infrastructure_accounting::arm_bounds)
        else {
            missing = true;
            continue;
        };
        let Some((min, max)) = crate::infrastructure_accounting::reduction_range(
            baseline_low,
            baseline_high,
            candidate_low,
            candidate_high,
        ) else {
            missing = true;
            continue;
        };
        if min < threshold && max >= threshold {
            straddles = true;
        }
        if max >= threshold {
            below = false;
        }
        if min < threshold {
            above = false;
        }
        if let Some(point) = baseline.and_then(|row| {
            row.get("infrastructure")
                .and_then(|value| value.get("adjusted_seconds"))
                .and_then(Value::as_f64)
        }) {
            *baseline_seconds = Some(point);
        }
        if let Some(point) = candidate.and_then(|row| {
            row.get("infrastructure")
                .and_then(|value| value.get("adjusted_seconds"))
                .and_then(Value::as_f64)
        }) {
            *candidate_seconds = Some(point);
        }
    }
    if !saw || missing || straddles {
        *decision = PolicyDecision::Inconclusive;
        reasons.push(
            "infrastructure attribution gaps could move the declared effect across its threshold; raw and adjusted evidence are retained and the result is inconclusive"
                .to_owned(),
        );
        coverage.push_str("; infrastructure-gap");
        return;
    }
    if failed_infra && *decision == PolicyDecision::Reject {
        *decision = PolicyDecision::Inconclusive;
        reasons.push(
            "a queue timeout or cancellation before the command started is a failed infrastructure attempt, not an incorrect model solution"
                .to_owned(),
        );
        return;
    }
    if above {
        let acceptance_reject = reasons
            .iter()
            .any(|reason| reason.contains("independent acceptance failed"));
        let time_reject = reasons.iter().any(|reason| {
            reason.contains("materially regressed the primary time metric")
                || reason.contains("does not meet the predeclared meaningful threshold")
        });
        if *decision == PolicyDecision::Reject && time_reject && !acceptance_reject {
            *decision = PolicyDecision::Adopt;
            reasons.push(
                "raw time regression does not survive the adjusted range; external waiting is not a model regression"
                    .to_owned(),
            );
        }
        reasons.push(
            "the adjusted range stays above the declared effect; external waiting alone is not the measured improvement"
                .to_owned(),
        );
        return;
    }
    if below {
        if *decision == PolicyDecision::Adopt {
            *decision = PolicyDecision::Reject;
        }
        reasons.push(
            "adjusted bounds stay below the declared effect; external waiting is not a model or harness improvement"
                .to_owned(),
        );
    }
}

impl PolicyEvaluation {
    /// The bounded human-readable decision reason; the board record keeps the
    /// compact token form and carries this text in its bounded detail.
    fn reason_text(&self) -> String {
        if self.reasons.is_empty() {
            self.decision.as_str().to_owned()
        } else {
            self.reasons.join("; ")
        }
    }

    /// Build the decision record for the board owner. The caller supplies the
    /// hypothesis, experiment, exact revisions and acceptance reference; this
    /// method refuses to fabricate numbers the accounting did not produce and
    /// refuses to present a maintenance-basis adoption as an efficiency
    /// benefit decision.
    pub fn decision_draft(
        &self,
        item: &str,
        experiment: &str,
        baseline_revision: &str,
        candidate_revision: &str,
        acceptance: &str,
    ) -> io::Result<DecisionDraft> {
        let (Some(baseline_seconds), Some(candidate_seconds)) =
            (self.baseline_seconds, self.candidate_seconds)
        else {
            return Err(invalid(
                "the recorded comparison lacks the arm seconds a decision record requires",
            ));
        };
        if self.matched == 0 || baseline_seconds <= 0.0 {
            return Err(invalid(
                "the recorded comparison lacks matched units or a positive baseline; no decision record is justified",
            ));
        }
        if self.decision == PolicyDecision::Adopt
            && self.basis.maintenance_basis().is_some()
            && self
                .reasons
                .iter()
                .any(|reason| reason.contains("no meaningful efficiency effect was measured"))
        {
            return Err(invalid(
                "a maintenance-basis adoption is not an efficiency benefit decision; record the predeclared basis through the decision owner",
            ));
        }
        let reason_text = self.reason_text();
        let record_token = token_text(&reason_text, 160);
        let detail = (!reason_text.is_empty()
            && reason_text.len() <= 512
            && !reason_text.contains(['\n', '\r']))
        .then_some(reason_text);
        Ok(DecisionDraft {
            item: item.to_owned(),
            experiment: experiment.to_owned(),
            outcome: self.decision.gate(),
            quality: self.quality.gate(),
            matched: self.matched,
            tolerance_percent: self.tolerance_percent,
            baseline_seconds,
            candidate_seconds,
            baseline_arm: "baseline".to_owned(),
            candidate_arm: "candidate".to_owned(),
            accounting: token_text(
                &format!(
                    "attempts:{} tasks:{} accepted:{} per_success:{}",
                    self.attempts,
                    self.tasks,
                    self.accepted_tasks,
                    match self.per_success.status {
                        PerSuccessStatus::Complete => "complete",
                        PerSuccessStatus::Incomplete => "incomplete",
                        PerSuccessStatus::Undefined => "undefined",
                    }
                ),
                160,
            ),
            baseline_revision: baseline_revision.to_owned(),
            candidate_revision: candidate_revision.to_owned(),
            acceptance: acceptance.to_owned(),
            coverage: token_text(&self.coverage, 160),
            scope: token_text(&self.scope, 160),
            reason: record_token,
            detail,
        })
    }
}

/// Reduce free prose to the token form the board record accepts, keeping the
/// stable field boundary: unsupported characters become `_`. Shared with the
/// comparison controller so a published decision and the record the
/// activation owner re-derives from the retained evaluation render identically.
pub fn board_token(value: &str, limit: usize) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || "._-:@+/\\,;~#".contains(character) {
                character
            } else {
                '_'
            }
        })
        .take(limit)
        .collect()
}

fn token_text(value: &str, limit: usize) -> String {
    board_token(value, limit)
}

/// Refuse a speed-trend claim built from absolute durations of different
/// tasks. Repeated measurements of the same task may be pooled; different
/// tasks are not a longitudinal improvement measurement.
pub fn refuse_cross_task_speed_claim(cases: &[&str]) -> io::Result<()> {
    if cases.len() <= 1 {
        return Ok(());
    }
    let first = cases[0].trim();
    if first.is_empty() {
        return Err(invalid("a task identity is required for a duration claim"));
    }
    if cases.iter().all(|case| case.trim() == first) {
        return Ok(());
    }
    Err(unsupported(
        "absolute durations from different tasks are not a comparable speed trend; compare baseline and candidate within each task",
    ))
}
