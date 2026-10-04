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
//!
//! Measurement/attribution bounds and empirical run-to-run variation stay
//! separate: bounds come from the infrastructure accounting of both arms,
//! while variation comes only from complete paired attempts. Requests,
//! rounds, tool operations and repeated readings within one task are dependent
//! observations, never replications. A declared statistical claim binds its
//! method, confidence level and assumptions before results through the same
//! digested [`ComparisonPolicy::uncertainty`] text; this evaluator can apply
//! descriptive bounds and observed complete-pair ranges, but it has no valid
//! statistical basis for a confidence interval over dependent local runs and
//! therefore refuses any declared confidence percentage instead of labeling a
//! measurement bound with one. A declared experiment selection
//! ([`SELECTION_CLAUSE`]) binds the effect path and claim, the required
//! outcome, the experimental unit/method, the applicability rationale, the
//! controls, the projected use/cost, the admissible baseline basis and the
//! stopping/escalation/deferral rules in the same digested text before any
//! result exists: a unit that cannot exercise the declared claim (a fixed
//! command or replay standing in for an agent, a short probe standing in for
//! complete paired implementations, a single operation standing in for a
//! repeated-use sequence, or a size-only shortcut) is refused at declaration,
//! and a declaration changed after results cannot inherit an earlier
//! adoption.
//!
//! The declared corroboration requirement is consumed from the report's
//! corroboration section ([`crate::outcome_report::CorroborationSection`],
//! built from the run-local receipt): a decision may claim a scope that needs
//! more independent units than the recorded complete units only when the
//! declared additional units are selected ready by identity and replay
//! reference. An inconclusive or unavailable selection, a selection made for a
//! different declared requirement, or a missing section leaves the broader
//! claim inconclusive; the consumed section is recorded with the decision, and
//! a changed or missing state cannot inherit an earlier adoption.

use crate::benefit_gate::{DecisionDraft, DecisionOutcome, QualityOutcome};
use crate::outcome_report::{
    CORROBORATION_SCHEMA, CorroborationSection, CorroborationState, corroboration_digest,
};
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

/// Marker of the predeclared statistical-analysis clause inside the policy's
/// uncertainty text. The clause must precede any infrastructure binding so
/// each owner parses its own token without consuming the other's fields.
pub const STATISTICAL_CLAUSE: &str = "statistical-analysis.v1";

/// The analysis method a declared claim is evaluated with. Only a descriptive
/// analysis over complete paired attempts is supportable here: this evaluator
/// cannot verify independence or a sampling model for local agent runs, so it
/// never assigns a statistical confidence level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnalysisMethod {
    ObservedPairsDescriptive,
}

impl AnalysisMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ObservedPairsDescriptive => "observed-pairs-descriptive",
        }
    }
}

/// What evidence a declared claim is scoped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimScope {
    /// The result describes the observed complete paired attempts only.
    Scoped,
    /// The claim asserts repeatable savings. It needs at least two complete
    /// paired attempts whose observed effects clear the declared threshold.
    Repeatable,
}

impl ClaimScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scoped => "scoped",
            Self::Repeatable => "repeatable",
        }
    }
}

/// The predeclared statistical/analysis binding of a comparison, digested and
/// recorded on every attempt before any comparative outcome. A changed claim
/// cannot inherit an earlier adoption, and a percentage this evaluator cannot
/// justify is refused instead of bound.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatisticalClaim {
    pub method: AnalysisMethod,
    /// The declared confidence level. Only `None` is supportable; a numeric
    /// percentage would label a measurement bound without a valid basis.
    pub confidence_percent: Option<f64>,
    pub scope: ClaimScope,
    /// Assumptions recorded before results.
    pub assumptions: String,
}

/// The canonical statistical-analysis clause. Callers may add human
/// uncertainty text before it and append the infrastructure binding after it.
pub fn statistical_clause(claim: &StatisticalClaim) -> String {
    let confidence = match claim.confidence_percent {
        None => "none".to_owned(),
        Some(value) => value.to_string(),
    };
    format!(
        "{STATISTICAL_CLAUSE}; method={}; confidence={confidence}; claim={}; assumptions={}",
        claim.method.as_str(),
        claim.scope.as_str(),
        claim.assumptions
    )
}

/// Parse the predeclared statistical-analysis clause out of the uncertainty
/// text. `Ok(None)` when it is absent: the descriptive behavior over the
/// declared stopping and repeated-selection policy is the older default. A
/// present but unusable clause is an error so it can never silently degrade
/// into a different analysis.
pub fn parse_statistical_claim(
    uncertainty: &str,
) -> Result<Option<StatisticalClaim>, &'static str> {
    let Some(start) = uncertainty.find(STATISTICAL_CLAUSE) else {
        return Ok(None);
    };
    if let Some(infrastructure) = uncertainty.find(crate::infrastructure_accounting::RULE_VERSION)
        && infrastructure < start
    {
        return Err(
            "the statistical-analysis clause must precede the infrastructure binding it qualifies",
        );
    }
    let rest = &uncertainty[start + STATISTICAL_CLAUSE.len()..];
    // The body ends at the next predeclared clause or at the infrastructure
    // binding, so each owner parses its own fields without consuming the
    // other's clause.
    let end = [
        crate::infrastructure_accounting::RULE_VERSION,
        SELECTION_CLAUSE,
    ]
    .into_iter()
    .filter_map(|marker| rest.find(marker))
    .min()
    .unwrap_or(rest.len());
    let body = &rest[..end];
    let mut method = None;
    let mut confidence = false;
    let mut scope = None;
    let mut assumptions: Option<String> = None;
    for segment in body.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let Some((key, value)) = segment.split_once('=') else {
            return Err("statistical-analysis fields must use key=value separated by ';'");
        };
        match (key.trim(), value.trim()) {
            ("method", "observed-pairs-descriptive") => {
                if method.is_some() {
                    return Err("the statistical-analysis method is declared twice");
                }
                method = Some(AnalysisMethod::ObservedPairsDescriptive);
            }
            ("method", _) => {
                return Err(
                    "the declared analysis method has no verifiable statistical basis; only observed-pairs-descriptive is supported",
                );
            }
            ("confidence", "none") => {
                if confidence {
                    return Err("the statistical-analysis confidence is declared twice");
                }
                confidence = true;
            }
            ("confidence", _) => {
                return Err(
                    "no valid statistical basis exists for a confidence level over dependent local runs; declare confidence=none",
                );
            }
            ("claim", scope_value) => {
                if scope.is_some() {
                    return Err("the statistical-analysis claim scope is declared twice");
                }
                scope = Some(match scope_value {
                    "scoped" => ClaimScope::Scoped,
                    "repeatable" => ClaimScope::Repeatable,
                    _ => return Err("the declared claim scope is not scoped or repeatable"),
                });
            }
            ("assumptions", value) => {
                if assumptions.is_some() {
                    return Err("the statistical-analysis assumptions are declared twice");
                }
                if value.is_empty() || value.len() > 256 || value.contains(['\n', '\r']) {
                    return Err("the statistical claim must record its assumptions before results");
                }
                assumptions = Some(value.to_owned());
            }
            _ => return Err("the statistical-analysis clause has an unknown field"),
        }
    }
    match (method, confidence, scope, assumptions) {
        (Some(method), true, Some(scope), Some(assumptions)) => Ok(Some(StatisticalClaim {
            method,
            confidence_percent: None,
            scope,
            assumptions,
        })),
        _ => Err(
            "the statistical-analysis clause is incomplete; declare method, confidence, claim and assumptions before results",
        ),
    }
}

/// Marker of the predeclared experiment-selection clause inside the policy's
/// uncertainty text. The clause order is: any statistical-analysis clause,
/// then this selection clause, then the infrastructure binding, so every
/// owner parses its own fields without consuming another's.
pub const SELECTION_CLAUSE: &str = "experiment-selection.v1";

/// Bound on one experiment-selection value. The rendered clause also has its
/// own bound so the whole declaration fits the uncertainty text.
const MAX_SELECTION_FIELD_BYTES: usize = 192;
/// Bound on the rendered experiment-selection clause.
const MAX_SELECTION_CLAUSE_BYTES: usize = 640;

/// Where the claimed effect and its decision-relevant regressions arise. The
/// declared path fixes the smallest sufficient experimental unit, and there
/// is no mandatory sequence of cheaper trials before that unit is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EffectPath {
    /// A local build, check, output transformation or other real operation.
    LocalOperation,
    /// Agent search, command selection or diagnosis: the effect propagates
    /// through the agent's choices and cannot be read from a fixed command.
    AgentChoice,
    /// Planning, implementation strategy, delegation or corrections: shorter
    /// work would omit the decision-relevant interactions and outcomes.
    TaskStrategy,
    /// Recurring cache, long-session, repeated-use or recovery behavior: the
    /// relevant sequence and state transitions must be preserved.
    RepeatedUse,
    /// A smaller size, file/skill count or fewer exposed names alone. Refused:
    /// these never establish benefit without a measured outcome.
    SizeOnly,
}

impl EffectPath {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalOperation => "local-operation",
            Self::AgentChoice => "agent-choice",
            Self::TaskStrategy => "task-strategy",
            Self::RepeatedUse => "repeated-use",
            Self::SizeOnly => "size-only",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "local-operation" => Some(Self::LocalOperation),
            "agent-choice" => Some(Self::AgentChoice),
            "task-strategy" => Some(Self::TaskStrategy),
            "repeated-use" => Some(Self::RepeatedUse),
            "size-only" => Some(Self::SizeOnly),
            _ => None,
        }
    }

    /// The smallest sufficient unit for this path, selected directly. A
    /// stronger real method is permitted; no cheaper probe is mandatory
    /// before it.
    pub fn smallest_sufficient(self) -> ExperimentMethod {
        match self {
            Self::LocalOperation => ExperimentMethod::RealOperation,
            Self::AgentChoice => ExperimentMethod::AgentTask,
            Self::TaskStrategy => ExperimentMethod::PairedImplementations,
            Self::RepeatedUse => ExperimentMethod::Sequence,
            Self::SizeOnly => ExperimentMethod::RealOperation,
        }
    }
}

/// The real experimental unit/method a hypothesis declares. The unit must
/// exercise the claimed mechanism; a stronger real method is permitted, an
/// insufficient or mismatched one is refused before dependent work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExperimentMethod {
    /// A bounded retained-input replay of a real operation. It can validate a
    /// local transformation and never stands in for an unexercised agent.
    BoundedReplay,
    /// The real operation through its actual build/check/run cycle.
    RealOperation,
    /// A short real agent task with independently accepted output.
    AgentTask,
    /// Complete paired task implementations through accepted completion.
    PairedImplementations,
    /// The sequence and state transitions of a repeated-use or recovery path.
    Sequence,
}

impl ExperimentMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BoundedReplay => "bounded-replay",
            Self::RealOperation => "real-operation",
            Self::AgentTask => "agent-task",
            Self::PairedImplementations => "paired-implementations",
            Self::Sequence => "sequence",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bounded-replay" => Some(Self::BoundedReplay),
            "real-operation" => Some(Self::RealOperation),
            "agent-task" => Some(Self::AgentTask),
            "paired-implementations" => Some(Self::PairedImplementations),
            "sequence" => Some(Self::Sequence),
            _ => None,
        }
    }

    /// Whether this unit exercises the declared effect path.
    pub fn exercises(self, path: EffectPath) -> bool {
        self.insufficiency(path).is_none()
    }

    /// Why this unit cannot support the declared claim, if it cannot.
    pub fn insufficiency(self, path: EffectPath) -> Option<&'static str> {
        match (path, self) {
            (EffectPath::SizeOnly, _) => Some(
                "fewer lines, files, skills or exposed names never establish benefit; state the measured outcome and the real operation, agent task, paired implementations or sequence that exercises it",
            ),
            (EffectPath::LocalOperation, Self::Sequence) => Some(
                "a repeated-use sequence is selected only for a repeated-use or recovery claim; a local build or output treatment selects a short real operation or a bounded input replay",
            ),
            (EffectPath::AgentChoice, Self::BoundedReplay) => Some(
                "a bounded replay of fixed inputs cannot stand in for an unexercised agent; an agent-choice claim needs a real short agent task",
            ),
            (EffectPath::AgentChoice, Self::RealOperation) => Some(
                "a fixed real operation bypasses the agent's choices; an agent-choice claim needs a real short agent task",
            ),
            (EffectPath::AgentChoice, Self::Sequence) => Some(
                "a repeated-use sequence does not exercise the agent's search, command selection or diagnosis; an agent-choice claim needs a real short agent task",
            ),
            (
                EffectPath::TaskStrategy,
                Self::BoundedReplay | Self::RealOperation | Self::AgentTask,
            ) => Some(
                "planning or implementation strategy effects need complete paired task implementations; shorter work would omit decision-relevant strategy, interactions, corrections or outcomes",
            ),
            (
                EffectPath::RepeatedUse,
                Self::BoundedReplay
                | Self::RealOperation
                | Self::AgentTask
                | Self::PairedImplementations,
            ) => Some(
                "a repeated-use, cache or recovery claim needs the sequence and state transitions of both variants; a single operation or task does not preserve preparation, invalidation, return and state",
            ),
            _ => None,
        }
    }
}

/// The declared experiment selection: the mechanism's effect path, the
/// required outcome, the selected experimental unit/method, the applicability
/// rationale, the controls, the projected use and cost, the admissible
/// baseline basis and the stopping/escalation/deferral rules. It is declared
/// before dependent work and never redefined after results.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExperimentSelection {
    pub method: ExperimentMethod,
    pub claim: EffectPath,
    /// The required outcome that would support the claim.
    pub outcome: String,
    /// Why the chosen unit exercises the mechanism and operating conditions.
    pub rationale: String,
    /// The controls and operating conditions the comparison needs.
    pub controls: String,
    /// Projected use and cost of the experiment and of retaining its work.
    pub projection: String,
    /// The admissible baseline basis: what is excluded and what makes it stale.
    pub baseline: String,
    /// Stopping, escalation and deferral rules, declared before results.
    pub stopping: String,
}

impl ExperimentSelection {
    /// Bounded semantic validation shared by the comparison policy and the
    /// grounded intake: the declared unit must exercise the declared effect
    /// path, no size-only shortcut is accepted, and every declared value is a
    /// nonempty bounded single line.
    pub fn validate(&self) -> io::Result<()> {
        match self.problem() {
            Some(problem) => Err(invalid(problem)),
            None => Ok(()),
        }
    }

    /// The first bounded semantic problem, if any. Shared by every owner that
    /// consumes a declared selection; the comparison policy wraps it in its
    /// own error, grounded intake records it as the refusal reason.
    pub fn problem(&self) -> Option<String> {
        if let Some(reason) = self.method.insufficiency(self.claim) {
            return Some(format!(
                "the declared experiment selection cannot support its claim: {reason}"
            ));
        }
        for (name, value) in [
            ("outcome", &self.outcome),
            ("rationale", &self.rationale),
            ("controls", &self.controls),
            ("projection", &self.projection),
            ("baseline", &self.baseline),
            ("stopping", &self.stopping),
        ] {
            if value.trim().is_empty() {
                return Some(format!(
                    "the declared experiment selection field {name} is empty; declare it before dependent work"
                ));
            }
            if value.len() > MAX_SELECTION_FIELD_BYTES || value.contains(['\n', '\r', ';']) {
                return Some(format!(
                    "the declared experiment selection field {name} must be one bounded line of at most {MAX_SELECTION_FIELD_BYTES} bytes without ';'"
                ));
            }
        }
        None
    }
}

/// The canonical experiment-selection clause. It follows any
/// statistical-analysis clause and precedes the infrastructure binding.
pub fn experiment_selection_clause(selection: &ExperimentSelection) -> String {
    format!(
        "{SELECTION_CLAUSE}; method={}; claim={}; outcome={}; rationale={}; controls={}; projection={}; baseline={}; stopping={}",
        selection.method.as_str(),
        selection.claim.as_str(),
        selection.outcome,
        selection.rationale,
        selection.controls,
        selection.projection,
        selection.baseline,
        selection.stopping
    )
}

/// The recorded declaration shape of one experiment selection, attached to
/// every measured attempt before results.
fn selection_declaration(selection: &ExperimentSelection) -> Value {
    json!({
        "method": selection.method.as_str(),
        "claim": selection.claim.as_str(),
        "outcome": selection.outcome,
        "rationale": selection.rationale,
        "controls": selection.controls,
        "projection": selection.projection,
        "baseline": selection.baseline,
        "stopping": selection.stopping,
    })
}

/// Parse the predeclared experiment-selection clause out of the uncertainty
/// text. `Ok(None)` when it is absent: the older default binds no selection.
/// A present but unusable clause is an error so it can never silently degrade
/// into a different experiment.
pub fn parse_experiment_selection(
    uncertainty: &str,
) -> Result<Option<ExperimentSelection>, String> {
    let Some(start) = uncertainty.find(SELECTION_CLAUSE) else {
        return Ok(None);
    };
    if let Some(infrastructure) = uncertainty.find(crate::infrastructure_accounting::RULE_VERSION)
        && infrastructure < start
    {
        return Err(
            "the experiment-selection clause must precede the infrastructure binding it qualifies"
                .to_owned(),
        );
    }
    if let Some(statistical) = uncertainty.find(STATISTICAL_CLAUSE)
        && statistical > start
    {
        return Err(
            "the experiment-selection clause must follow the statistical-analysis clause so each owner parses its own fields"
                .to_owned(),
        );
    }
    let rest = &uncertainty[start + SELECTION_CLAUSE.len()..];
    let body = match rest.find(crate::infrastructure_accounting::RULE_VERSION) {
        Some(end) => &rest[..end],
        None => rest,
    };
    if body.len() > MAX_SELECTION_CLAUSE_BYTES {
        return Err(
            "the experiment-selection clause exceeds its bounded size; keep every declared value within the clause bound"
                .to_owned(),
        );
    }
    let mut method = None;
    let mut claim = None;
    let mut outcome: Option<String> = None;
    let mut rationale: Option<String> = None;
    let mut controls: Option<String> = None;
    let mut projection: Option<String> = None;
    let mut baseline: Option<String> = None;
    let mut stopping: Option<String> = None;
    for segment in body.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let Some((key, value)) = segment.split_once('=') else {
            return Err(
                "experiment-selection fields must use key=value separated by ';'".to_owned(),
            );
        };
        let value = value.trim();
        match key.trim() {
            "method" => {
                if method.is_some() {
                    return Err("the experiment-selection method is declared twice".to_owned());
                }
                method = Some(ExperimentMethod::parse(value).ok_or_else(|| {
                    "the declared experiment method is not bounded-replay, real-operation, agent-task, paired-implementations or sequence".to_owned()
                })?);
            }
            "claim" => {
                if claim.is_some() {
                    return Err("the experiment-selection claim path is declared twice".to_owned());
                }
                claim = Some(EffectPath::parse(value).ok_or_else(|| {
                    "the declared claim path is not local-operation, agent-choice, task-strategy, repeated-use or size-only".to_owned()
                })?);
            }
            "outcome" => set_selection_value("outcome", value, &mut outcome)?,
            "rationale" => set_selection_value("rationale", value, &mut rationale)?,
            "controls" => set_selection_value("controls", value, &mut controls)?,
            "projection" => set_selection_value("projection", value, &mut projection)?,
            "baseline" => set_selection_value("baseline", value, &mut baseline)?,
            "stopping" => set_selection_value("stopping", value, &mut stopping)?,
            _ => {
                return Err(
                    "the experiment-selection clause has an unknown field; a staged or mandatory ladder is not a declared selection"
                        .to_owned(),
                );
            }
        }
    }
    match (
        method, claim, outcome, rationale, controls, projection, baseline, stopping,
    ) {
        (
            Some(method),
            Some(claim),
            Some(outcome),
            Some(rationale),
            Some(controls),
            Some(projection),
            Some(baseline),
            Some(stopping),
        ) => {
            let selection = ExperimentSelection {
                method,
                claim,
                outcome,
                rationale,
                controls,
                projection,
                baseline,
                stopping,
            };
            if let Some(problem) = selection.problem() {
                return Err(problem);
            }
            Ok(Some(selection))
        }
        _ => Err(
            "the experiment-selection clause is incomplete; declare method, claim, outcome, rationale, controls, projection, baseline and stopping before results"
                .to_owned(),
        ),
    }
}

fn set_selection_value(
    name: &'static str,
    value: &str,
    slot: &mut Option<String>,
) -> Result<(), String> {
    if slot.is_some() {
        return Err(match name {
            "outcome" => "the experiment-selection outcome is declared twice",
            "rationale" => "the experiment-selection rationale is declared twice",
            "controls" => "the experiment-selection controls are declared twice",
            "projection" => "the experiment-selection projection is declared twice",
            "baseline" => "the experiment-selection baseline is declared twice",
            _ => "the experiment-selection stopping rule is declared twice",
        }
        .to_owned());
    }
    *slot = Some(value.to_owned());
    Ok(())
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

    /// The basis text recorded on every measured attempt before results. A
    /// maintenance-only result is adopted only when the recorded declaration
    /// carries the same pre-agreed basis, never a reason reconstructed after
    /// seeing the outcome.
    pub fn declaration_text(&self) -> String {
        match self {
            Self::Efficiency => "efficiency".to_owned(),
            Self::Maintenance { basis } => format!("maintenance: {basis}"),
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
            "basis": self.basis.declaration_text(),
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
        // The declared experiment selection is recorded with every measured
        // attempt before results, so a policy or declaration changed after
        // results cannot inherit an earlier adoption.
        if let Ok(Some(selection)) = parse_experiment_selection(&self.uncertainty) {
            value["selection"] = selection_declaration(&selection);
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
        let statistical = parse_statistical_claim(&self.uncertainty).map_err(invalid)?;
        if statistical
            .as_ref()
            .is_some_and(|claim| claim.scope == ClaimScope::Repeatable)
            && self.stopping.required_units < 2
        {
            return Err(invalid(
                "a repeatable claim needs at least two complete paired units declared before results",
            ));
        }
        // A present selection clause is parsed and validated by its own owner:
        // the declared unit must exercise the declared claim path and no
        // size-only shortcut is accepted before any result exists.
        let _ = parse_experiment_selection(&self.uncertainty).map_err(invalid)?;
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

/// Whether a usable attribution bound exists for the evaluated units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MeasurementStatus {
    /// At least one included unit carries a usable attribution range.
    Bounded,
    /// No usable bound was recorded. Absent evidence is not a point estimate.
    Absent,
}

/// Measurement/attribution bounds applicable to the decision, retained
/// separately from empirical variation across complete paired attempts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MeasurementBounds {
    pub status: MeasurementStatus,
    /// Union of the per-unit supported effect ranges from the attributed arm
    /// bounds. Not a confidence interval and never a substitute for variation.
    pub effect_percent: Option<[f64; 2]>,
    /// Worst admissible time regression percent across the included bounds.
    /// `None` when no usable bound exists; absent evidence is not zero.
    pub worst_time_regression_percent: Option<f64>,
    /// Evidence identity of the bounds.
    pub evidence: String,
    /// Assumptions carried with the bounds.
    pub assumptions: String,
}

/// Whether run-to-run variation is observable from the recorded evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VariationStatus {
    /// At least two complete paired attempts yield a comparable decision
    /// metric, so an observed descriptive range exists.
    Observed,
    /// Fewer than two complete, comparable paired attempts were recorded.
    /// The variance is unmeasured, never assumed zero.
    Unmeasured,
}

/// Empirical variation across complete paired attempts. Requests, rounds,
/// tool operations and repeated readings within one task are dependent
/// observations and are not replications.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VariationRecord {
    /// Complete one-to-one baseline/candidate attempts; the experimental unit.
    pub complete_pairs: u64,
    pub status: VariationStatus,
    /// Observed decision-metric effect range across the complete pairs. `None`
    /// when unmeasured; a descriptive range, not a confidence interval.
    pub observed_effect_percent: Option<[f64; 2]>,
    /// The declared claim scope this evidence is evaluated against.
    pub claim: ClaimScope,
    /// Bounded basis, including what is not counted.
    pub basis: String,
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
    /// Attribution bounds used by the decision, separated from variation.
    #[serde(default)]
    pub measurement: Option<MeasurementBounds>,
    /// Run-to-run variation across complete paired attempts.
    #[serde(default)]
    pub variation: Option<VariationRecord>,
    /// The predeclared statistical claim, when one was bound.
    #[serde(default)]
    pub statistical_claim: Option<StatisticalClaim>,
    /// The declared corroboration requirement consumed at decision time: the
    /// selection status over the additional independent retained units, their
    /// identity and replay references, the exact exclusions and the digest
    /// binding the consumed section. `None` for a declared scope that needs no
    /// additional units or when the report carries no bound selection.
    #[serde(default)]
    pub corroboration: Option<CorroborationSection>,
}

struct UnitFacts {
    name: String,
    case_id: String,
    baseline_seconds: Option<f64>,
    candidate_seconds: Option<f64>,
    /// Attributed arm ranges for this unit, when the accounting produced them.
    /// They are measurement bounds, not run-to-run variation.
    baseline_bounds: Option<(f64, f64)>,
    candidate_bounds: Option<(f64, f64)>,
    baseline_steps: Option<u64>,
    candidate_steps: Option<u64>,
    usage_measured: bool,
    baseline_accepted: bool,
    candidate_accepted: bool,
    positive_effect: bool,
    evidence_complete: bool,
    does_not_repay: bool,
    /// Adjusted time range could move the declared threshold. Not a point estimate.
    time_uncertain: bool,
    /// Candidate token increase over baseline, when both totals were observed.
    token_regression: Option<f64>,
    /// Subtractive applicability recorded by the accounting, when declared.
    subtractive: Option<SubtractiveFacts>,
    limitations: Vec<String>,
}

impl UnitFacts {
    /// Best/worst admissible time regression percent from this unit's
    /// attribution bounds. `None` when either arm has no usable bound.
    fn time_regression_range(&self) -> Option<(f64, f64)> {
        let (baseline_low, baseline_high) = self.baseline_bounds?;
        let (candidate_low, candidate_high) = self.candidate_bounds?;
        if baseline_low <= 0.0 || baseline_high <= 0.0 {
            return None;
        }
        let best = percent_regression(baseline_high, candidate_low)?;
        let worst = percent_regression(baseline_low, candidate_high)?;
        Some((best.min(worst), best.max(worst)))
    }
}

/// What the authoritative accounting recorded for a subtractive unit: the
/// removed burden and whether both arms establish actual consumption before
/// removal with the baseline's required checks retained.
struct SubtractiveFacts {
    removed: String,
    applicability: String,
    retained_checks: bool,
    /// The unit records its net effect over the declared use horizon (the
    /// declared implementation, evaluation and recurring maintenance or
    /// manual-fallback cost included).
    repayment: bool,
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

struct SelectedTime {
    baseline_seconds: Option<f64>,
    candidate_seconds: Option<f64>,
    positive_effect: bool,
    uncertain: bool,
    token_regression: Option<f64>,
}

fn observed_tokens(row: &Value) -> Option<u64> {
    let usage = row.get("infrastructure")?.get("usage")?;
    let incomplete = usage
        .get("incomplete")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let side = if incomplete { "raw" } else { "adjusted" };
    usage
        .get(side)
        .and_then(|value| value.get("total_tokens"))
        .and_then(Value::as_u64)
}

/// Work-efficiency time is selected before the decision tree. Other objectives
/// keep their own primary metric. `None` means this policy does not substitute
/// adjusted time.
fn selected_time_view(
    policy: &ComparisonPolicy,
    baseline: Option<&Value>,
    candidate: Option<&Value>,
) -> Option<SelectedTime> {
    let binding = crate::infrastructure_accounting::parse_binding(&policy.uncertainty).ok()??;
    if binding.view != crate::infrastructure_accounting::MetricView::WorkEfficiency
        || binding.mechanism.owns_queue()
        || policy.objective != Objective::Time
    {
        return None;
    }
    let threshold = policy.meaningful_effect_percent.unwrap_or(0.0);
    let token_regression = baseline.zip(candidate).and_then(|(baseline, candidate)| {
        percent_regression(
            observed_tokens(baseline)? as f64,
            observed_tokens(candidate)? as f64,
        )
    });
    let Some((baseline_low, baseline_high)) =
        baseline.and_then(crate::infrastructure_accounting::arm_bounds)
    else {
        return Some(SelectedTime {
            baseline_seconds: None,
            candidate_seconds: None,
            positive_effect: false,
            uncertain: true,
            token_regression,
        });
    };
    let Some((candidate_low, candidate_high)) =
        candidate.and_then(crate::infrastructure_accounting::arm_bounds)
    else {
        return Some(SelectedTime {
            baseline_seconds: None,
            candidate_seconds: None,
            positive_effect: false,
            uncertain: true,
            token_regression,
        });
    };
    let Some((min_reduction, max_reduction)) = crate::infrastructure_accounting::reduction_range(
        baseline_low,
        baseline_high,
        candidate_low,
        candidate_high,
    ) else {
        return Some(SelectedTime {
            baseline_seconds: None,
            candidate_seconds: None,
            positive_effect: false,
            uncertain: true,
            token_regression,
        });
    };
    let worst_regression = percent_regression(baseline_low, candidate_high).unwrap_or(0.0);
    let best_regression = percent_regression(baseline_high, candidate_low).unwrap_or(0.0);
    let straddles_effect = min_reduction < threshold && max_reduction >= threshold;
    let straddles_tolerance =
        worst_regression > policy.tolerance_percent && best_regression <= policy.tolerance_percent;
    let uncertain = straddles_effect || straddles_tolerance;
    Some(SelectedTime {
        baseline_seconds: Some(baseline_high),
        candidate_seconds: Some(candidate_high),
        positive_effect: !uncertain && min_reduction >= threshold && min_reduction > 0.0,
        uncertain,
        token_regression,
    })
}

/// The declared corroboration requirement consumed from the report: whether
/// the selection covers the units the recorded evidence still lacks, and the
/// bound section the decision reports when the report carries one.
struct CorroborationConsumption {
    /// The self-consistent bound section the report carried; `None` when the
    /// report carries none or the section does not bind its own content.
    evidence: Option<CorroborationSection>,
    /// True when a ready selection matches the declared requirement and covers
    /// the additional units beyond the recorded complete units.
    supports: bool,
}

/// Consume the declared corroboration requirement from the report. The
/// declared stopping count is the number of independent units an adoption
/// needs; the run's own declared plan unit is recorded in the report, and the
/// declared additional units (`required_units - 1`, the driver's requirement
/// mapping) must be selected from retained prior real tasks. A missing,
/// unbound, unavailable, inconclusive, mismatched or short selection leaves
/// the broader claim unsupported with its exact reasons; units are never
/// fabricated and a summary never replaces a selection.
fn consume_corroboration(
    policy: &ComparisonPolicy,
    report: &Value,
    recorded_units: usize,
    reasons: &mut Vec<String>,
) -> io::Result<CorroborationConsumption> {
    let required_total = policy.stopping.required_units as usize;
    if required_total <= 1 {
        return Ok(CorroborationConsumption {
            evidence: None,
            supports: false,
        });
    }
    let needed = required_total.saturating_sub(recorded_units);
    let Some(section_value) = report.get("corroboration") else {
        if needed > 0 {
            reasons.push(format!(
                "the declared scope needs {required_total} independent unit(s) and {recorded_units} complete unit(s) were recorded; no corroboration selection of the additional retained units is recorded, so the broader claim stays unsupported and no summary replaces absent evidence"
            ));
        }
        return Ok(CorroborationConsumption {
            evidence: None,
            supports: false,
        });
    };
    let section: CorroborationSection = serde_json::from_value(section_value.clone())
        .map_err(|_| invalid("the report carries an invalid corroboration section"))?;
    if section.schema != CORROBORATION_SCHEMA {
        return Err(invalid(
            "the report corroboration section has an unsupported schema",
        ));
    }
    match corroboration_digest(&section) {
        Ok(bound) if bound == section.digest => {}
        _ => {
            if needed > 0 {
                reasons.push(
                    "the corroboration section does not bind its own content; a changed corroboration state cannot support the broader claim"
                        .to_owned(),
                );
            }
            return Ok(CorroborationConsumption {
                evidence: None,
                supports: false,
            });
        }
    }
    if needed == 0 {
        // The recorded units meet the declared count; the selection state is
        // reported with the decision but nothing is required of it.
        return Ok(CorroborationConsumption {
            evidence: Some(section),
            supports: false,
        });
    }
    let declared_additional = (required_total - 1) as u32;
    let supports = match section.status {
        CorroborationState::Unavailable => {
            reasons.push(format!(
                "the declared scope needs {required_total} independent unit(s) with {recorded_units} complete unit(s) recorded; the corroboration selection was not performed: {}",
                section.reason.as_deref().unwrap_or("no reason recorded")
            ));
            false
        }
        CorroborationState::Inconclusive => {
            reasons.push(format!(
                "the declared scope needs {required_total} independent unit(s) with {recorded_units} complete unit(s) recorded; the corroboration selection is inconclusive: {}",
                section.reason.as_deref().unwrap_or("no reason recorded")
            ));
            for excluded in section.excluded.iter().take(4) {
                reasons.push(format!(
                    "corroboration unit {} ({}) was excluded: {}",
                    excluded.case_id,
                    excluded.owner,
                    exclusion_text(&excluded.reason)
                ));
            }
            if section.excluded.len() > 4 {
                reasons.push(format!(
                    "{} more corroboration candidate exclusion(s) stay recorded with their exact reasons in the evaluation",
                    section.excluded.len() - 4
                ));
            }
            false
        }
        CorroborationState::Ready => {
            let incomplete = section.units.iter().any(|unit| {
                [
                    &unit.owner,
                    &unit.case_id,
                    &unit.experiment,
                    &unit.mechanism,
                    &unit.conditions,
                    &unit.revision,
                    &unit.tree_sha256,
                ]
                .iter()
                .any(|value| value.trim().is_empty())
            });
            if section.required_units != declared_additional {
                reasons.push(format!(
                    "the corroboration selection was made for {} additional unit(s) but the declared scope requires {declared_additional} beyond the run's own declared plan unit; a changed corroboration state cannot support the broader claim",
                    section.required_units
                ));
                false
            } else if incomplete {
                reasons.push(
                    "the corroboration selection records a unit without complete identity and replay references; a selection cannot stand in for the retained unit's identity"
                        .to_owned(),
                );
                false
            } else if section.units.len() < needed {
                reasons.push(format!(
                    "the corroboration selection is ready with {} additional unit(s); the recorded {recorded_units} complete unit(s) plus the selection still fall short of the declared {required_total} independent unit(s), so the broader claim stays unsupported",
                    section.units.len()
                ));
                false
            } else {
                reasons.push(format!(
                    "the declared scope needs {required_total} independent unit(s); {recorded_units} complete unit(s) were recorded and the ready corroboration selection supplies the remaining {needed} additional retained unit(s) by identity: {}",
                    section
                        .units
                        .iter()
                        .map(|unit| format!("{} ({})", unit.case_id, unit.owner))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                true
            }
        }
    };
    Ok(CorroborationConsumption {
        evidence: Some(section),
        supports,
    })
}

/// The selector exclusion reason in the words of the decision record.
fn exclusion_text(reason: &crate::improvement_experiment::ExclusionReason) -> String {
    use crate::improvement_experiment::ExclusionReason;
    match reason {
        ExclusionReason::NotApplicable => {
            "not applicable to the declared mechanism and conditions".to_owned()
        }
        ExclusionReason::AlreadyUsed => {
            "already part of the declared plan or not independent of an earlier unit".to_owned()
        }
        ExclusionReason::NotReplayable { detail } => {
            format!("the retained copy no longer verifies as pristine: {detail}")
        }
    }
}

/// Evaluate the declared policy against the authoritative outcome summary.
pub fn evaluate(declared: &DeclaredComparison, report: &Value) -> io::Result<PolicyEvaluation> {
    declared.verify()?;
    let policy = &declared.policy;
    let statistical_claim = parse_statistical_claim(&policy.uncertainty).map_err(invalid)?;
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
        // A maintenance-only result is adopted only under the basis recorded
        // before results. Evidence recorded without it (or with another
        // basis) cannot become a post-hoc maintenance exemption; evidence
        // from before this field existed keeps the default efficiency reading.
        match declared_value.get("basis").and_then(Value::as_str) {
            Some(basis) if basis == policy.basis.declaration_text() => {}
            None if policy.basis.maintenance_basis().is_none() => {}
            _ => drift.push(format!(
                "unit {name}: the recorded basis is missing or differs from the predeclared {} basis",
                policy.basis.as_str()
            )),
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
        let selected = selected_time_view(policy, baseline_row, candidate_row);
        let mut baseline_seconds = number(&unit["effect"]["baseline_seconds"]);
        let mut candidate_seconds = number(&unit["effect"]["candidate_seconds"]);
        let mut positive_effect = unit.get("positive_effect") == Some(&Value::Bool(true));
        let mut time_uncertain = false;
        let token_regression = selected.as_ref().and_then(|view| view.token_regression);
        if let Some(view) = selected.as_ref() {
            baseline_seconds = view.baseline_seconds;
            candidate_seconds = view.candidate_seconds;
            positive_effect = view.positive_effect;
            time_uncertain = view.uncertain;
        }
        facts.push(UnitFacts {
            case_id: unit
                .get("case_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            baseline_seconds,
            candidate_seconds,
            baseline_bounds: baseline_row.and_then(crate::infrastructure_accounting::arm_bounds),
            candidate_bounds: candidate_row.and_then(crate::infrastructure_accounting::arm_bounds),
            baseline_steps: baseline_row.and_then(steps),
            candidate_steps: candidate_row.and_then(steps),
            usage_measured: usages,
            baseline_accepted: baseline_row.is_some_and(accepted),
            candidate_accepted: candidate_row.is_some_and(accepted),
            positive_effect,
            evidence_complete: unit.get("evidence_complete") == Some(&Value::Bool(true)),
            does_not_repay: unit
                .get("net_saving")
                .and_then(|saving| saving.get("verdict"))
                .and_then(Value::as_str)
                == Some("does_not_repay"),
            time_uncertain,
            token_regression,
            subtractive: (unit.get("subtractive") == Some(&Value::Bool(true))).then(|| {
                SubtractiveFacts {
                    removed: unit
                        .get("removed_burden")
                        .and_then(Value::as_str)
                        .unwrap_or("(unnamed)")
                        .to_owned(),
                    applicability: unit
                        .get("applicability")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_owned(),
                    retained_checks: unit.get("retained_checks") == Some(&Value::Bool(true)),
                    repayment: unit
                        .get("net_saving")
                        .is_some_and(|saving| !saving.is_null()),
                }
            }),
            limitations: unit_limitations(unit),
            name,
        });
    }

    // Measurement bounds and empirical variation are retained separately.
    // Bounds come from the attribution accounting of each arm; variation comes
    // only from complete paired attempts. Requests, rounds, tool operations
    // and repeated readings within a task are dependent observations, and a
    // single pair leaves run-to-run variation unmeasured rather than zero.
    let (time_bound_best, time_bound_worst) = facts
        .iter()
        .filter_map(UnitFacts::time_regression_range)
        .fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(best, worst), (unit_best, unit_worst)| (best.min(unit_best), worst.max(unit_worst)),
        );
    let time_bounds_present = time_bound_best.is_finite() && time_bound_worst.is_finite();
    let bound_effect_ranges: Vec<(f64, f64)> = facts
        .iter()
        .filter_map(|fact| {
            let (baseline_low, baseline_high) = fact.baseline_bounds?;
            let (candidate_low, candidate_high) = fact.candidate_bounds?;
            crate::infrastructure_accounting::reduction_range(
                baseline_low,
                baseline_high,
                candidate_low,
                candidate_high,
            )
        })
        .collect();
    let measurement = MeasurementBounds {
        status: if time_bounds_present {
            MeasurementStatus::Bounded
        } else {
            MeasurementStatus::Absent
        },
        effect_percent: (!bound_effect_ranges.is_empty()).then(|| {
            [
                bound_effect_ranges
                    .iter()
                    .map(|(low, _)| *low)
                    .fold(f64::INFINITY, f64::min),
                bound_effect_ranges
                    .iter()
                    .map(|(_, high)| *high)
                    .fold(f64::NEG_INFINITY, f64::max),
            ]
        }),
        worst_time_regression_percent: time_bounds_present.then_some(time_bound_worst),
        evidence: "infrastructure-attribution.v1 arm bounds (host QPC activity, native queue evidence and applicable activity brackets); steps and tokens are exact observed totals".to_owned(),
        assumptions: "only unrelated-external-blocking is deducted; unresolved evidence widens the range and never becomes zero; this range is an attribution bound, not a confidence interval".to_owned(),
    };
    let pair_effects: Vec<f64> = facts
        .iter()
        .filter_map(|fact| match policy.objective {
            Objective::Quality => None,
            Objective::Time => {
                let baseline = fact.baseline_seconds?;
                let candidate = fact.candidate_seconds?;
                (baseline > 0.0).then(|| (baseline - candidate) / baseline * 100.0)
            }
            Objective::Resource => {
                let baseline = fact.baseline_steps? as f64;
                let candidate = fact.candidate_steps? as f64;
                (baseline > 0.0).then(|| (baseline - candidate) / baseline * 100.0)
            }
        })
        .collect();
    let variation_status = if facts.len() >= 2
        && (policy.objective == Objective::Quality || pair_effects.len() == facts.len())
    {
        VariationStatus::Observed
    } else {
        VariationStatus::Unmeasured
    };
    let variation = VariationRecord {
        complete_pairs: facts.len() as u64,
        status: variation_status,
        observed_effect_percent: (variation_status == VariationStatus::Observed
            && !pair_effects.is_empty())
        .then(|| {
            [
                pair_effects.iter().copied().fold(f64::INFINITY, f64::min),
                pair_effects
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max),
            ]
        }),
        claim: statistical_claim
            .as_ref()
            .map_or(ClaimScope::Scoped, |claim| claim.scope),
        basis: format!(
            "{} complete paired unit(s); requests, rounds, tool operations and repeated readings within a task are dependent observations and are not replications; {}; model identity remains API-observed with unavailable weight hashes or hardware retained as limits; no statistical confidence level is assigned, and an undetected difference is not evidence of equivalence",
            facts.len(),
            match variation_status {
                VariationStatus::Observed =>
                    "the observed range is descriptive of these units only",
                VariationStatus::Unmeasured =>
                    "run-to-run variation is unmeasured (never assumed zero)",
            }
        ),
    };

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
    let variation_scope = match (variation.status, variation.observed_effect_percent) {
        (VariationStatus::Observed, Some(range)) => format!(
            "{} complete paired unit(s) with an observed effect range of {:.1}..{:.1}% (descriptive, not a confidence interval)",
            variation.complete_pairs, range[0], range[1],
        ),
        (VariationStatus::Observed, None) => format!(
            "{} complete paired unit(s) with recorded per-unit outcomes and no numeric effect range",
            variation.complete_pairs
        ),
        (VariationStatus::Unmeasured, _) => format!(
            "{} complete paired unit(s); run-to-run variation is unmeasured and is not assumed zero",
            variation.complete_pairs
        ),
    };
    let scope = format!(
        "matched pairs on {}; each task is compared within itself and absolute durations across different tasks are not a speed trend; {variation_scope}",
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
    coverage.push_str(&format!("; complete-pairs:{}", variation.complete_pairs));
    coverage.push_str(match variation.status {
        VariationStatus::Observed => "; variation:observed-range",
        VariationStatus::Unmeasured => "; variation:unmeasured",
    });
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

    let mut corroboration: Option<CorroborationSection> = None;
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
        let token_regressed = facts.iter().any(|fact| {
            fact.token_regression
                .is_some_and(|percent| percent > policy.tolerance_percent)
        });
        if facts.iter().any(|fact| fact.time_uncertain) {
            reasons.push(
                "infrastructure attribution gaps could move the declared effect across its threshold; raw and adjusted evidence are retained and the result is inconclusive"
                    .to_owned(),
            );
            decision = PolicyDecision::Inconclusive;
        } else if let Some(fact) = facts.iter().find(|fact| {
            fact.subtractive.as_ref().is_some_and(|subtractive| {
                !subtractive.retained_checks
                    || subtractive.applicability != "exercised"
                    || (!subtractive.repayment
                        && policy.basis.maintenance_basis().is_none()
                        && policy.objective != Objective::Quality)
            })
        }) {
            let subtractive = fact
                .subtractive
                .as_ref()
                .expect("the matched fact declares a subtractive treatment");
            if !subtractive.retained_checks {
                reasons.push(format!(
                    "the subtractive candidate on unit {} records fewer required checks than the baseline for the removed burden {}; removing the check that would expose a regression cannot support adoption",
                    fact.name, subtractive.removed
                ));
                decision = PolicyDecision::Reject;
            } else if subtractive.applicability != "exercised" {
                reasons.push(match subtractive.applicability.as_str() {
                    "not_exercised" => format!(
                        "the workload never exercised the removed burden {} (unit {}); removing an unconsumed burden cannot establish a useful saving, and broader usefulness stays unresolved rather than rejected",
                        subtractive.removed, fact.name
                    ),
                    _ => format!(
                        "actual consumption of the removed burden {} is not established on unit {}; the intended context treatment is unproven and cannot support a saving",
                        subtractive.removed, fact.name
                    ),
                });
                for limitation in fact.limitations.iter().take(2) {
                    reasons.push(format!("unit {}: {limitation}", fact.name));
                }
                decision = PolicyDecision::Inconclusive;
            } else {
                reasons.push(format!(
                    "the subtractive result on unit {} records no repayment over the declared use horizon; the declared manual/fallback and maintenance cost must be included before adoption",
                    fact.name
                ));
                decision = PolicyDecision::Inconclusive;
            }
        } else if !incomplete.is_empty() {
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
            // The declared unit count is met by the recorded complete units or
            // by the declared corroboration selection covering the units the
            // recorded evidence still lacks; a broader claim without enough
            // corroborating units stays inconclusive with the selection's
            // exact reasons.
            let consumed = consume_corroboration(policy, report, facts.len(), &mut reasons)?;
            let sufficient_units = facts.len() >= required_units || consumed.supports;
            let recorded_target = if consumed.supports {
                facts.len()
            } else {
                required_units
            };
            corroboration = consumed.evidence;
            if !sufficient_units {
                // consume_corroboration recorded the exact reasons.
            } else if consumed.supports && positive_units < recorded_target {
                // The count is met only with selected, not yet measured,
                // corroboration units: a partially measured set that does not
                // show the declared effect on every recorded unit cannot carry
                // the broader claim, and partial measurement is not a
                // rejection of it either.
                reasons.push(format!(
                    "the recorded {recorded_target} complete unit(s) do not all show the declared effect while the declared {required_units} independent unit(s) rely on selected corroboration units; a partially measured set stays inconclusive for the broader claim"
                ));
            } else if positive_units < recorded_target {
                if let Some(basis) = &maintenance {
                    // A maintenance-only basis never covers a material
                    // regression of a measured dimension: an exchange must be
                    // admitted by the same predeclared trade-off policy the
                    // efficiency path uses, never a bypass.
                    let mut blocked = false;
                    let mut bounds_uncertain = false;
                    for (regressions, worst, bounded) in [
                        (&time_regressions, worst_time, time_bounds_present),
                        (&steps_regressions, worst_steps, false),
                    ] {
                        if regressions.is_empty() {
                            continue;
                        }
                        // A bound-sensitive dimension is evaluated over its
                        // admissible range: an exchange that only covers the
                        // observed point does not cover the supported range.
                        let admissible = if bounded {
                            worst.max(time_bound_worst)
                        } else {
                            worst
                        };
                        if covers(admissible) {
                            trade_off_used = true;
                        } else if bounded
                            && time_bound_best <= policy.tolerance_percent
                            && time_bound_worst > policy.tolerance_percent
                            && worst <= policy.tolerance_percent
                        {
                            bounds_uncertain = true;
                            reasons.push(
                                "admissible attribution bounds straddle the declared time tolerance; the maintenance result does not hold across the supported range and stays inconclusive"
                                    .to_owned(),
                            );
                        } else {
                            blocked = true;
                            reasons.push(format!(
                                "the maintenance basis does not cover a material regression in a measured dimension (worst +{admissible:.1}% beyond the {:.1}% tolerance); it needs the predeclared trade-off policy",
                                policy.tolerance_percent
                            ));
                        }
                    }
                    if blocked {
                        decision = PolicyDecision::Reject;
                    } else if bounds_uncertain {
                        decision = PolicyDecision::Inconclusive;
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
                if policy.objective == Objective::Time && token_regressed && !blocked {
                    let worst_tokens = facts
                        .iter()
                        .filter_map(|fact| fact.token_regression)
                        .fold(f64::NEG_INFINITY, f64::max);
                    if covers(worst_tokens) {
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
                            "adjusted time improved but measured tokens regressed beyond the declared tolerance; excluded wait usage is not a waiver"
                                .to_owned(),
                        );
                        blocked = true;
                    }
                }
                let mut bounds_uncertain = false;
                if policy.objective == Objective::Resource && !blocked {
                    let admissible_worst = if time_bounds_present {
                        worst_time.max(time_bound_worst)
                    } else {
                        worst_time
                    };
                    if !time_regressions.is_empty() || admissible_worst > policy.tolerance_percent {
                        if covers(admissible_worst) {
                            trade_off_used = true;
                            reasons.push(format!(
                                "adopted under the predeclared trade-off: {}",
                                policy
                                    .trade_off
                                    .as_ref()
                                    .map(|trade_off| trade_off.basis.as_str())
                                    .unwrap_or("")
                            ));
                        } else if time_bounds_present
                            && time_bound_best <= policy.tolerance_percent
                            && time_bound_worst > policy.tolerance_percent
                            && worst_time <= policy.tolerance_percent
                        {
                            bounds_uncertain = true;
                            reasons.push(
                                "admissible attribution bounds can move the non-primary time regression across the declared tolerance; the resource verdict does not hold across the supported range and stays inconclusive"
                                    .to_owned(),
                            );
                        } else {
                            reasons.push(
                                "the resource effect comes with a time regression beyond tolerance; declare the trade-off policy before results"
                                    .to_owned(),
                            );
                            blocked = true;
                        }
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
                } else if bounds_uncertain {
                    PolicyDecision::Inconclusive
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

    // A declared repeatable claim is evaluated across the observed complete
    // pairs. One pair, or a pair whose observed effect sits below the declared
    // threshold, cannot establish repeatable savings; the adoption is kept
    // inconclusive for that scope instead of extrapolating from the best pair.
    // This never softens an acceptance rejection.
    if decision == PolicyDecision::Adopt && variation.claim == ClaimScope::Repeatable {
        let supported = match policy.objective {
            Objective::Quality => variation.status == VariationStatus::Observed,
            Objective::Time | Objective::Resource => {
                variation.observed_effect_percent.is_some_and(|range| {
                    variation.status == VariationStatus::Observed
                        && range[0] >= policy.meaningful_effect_percent.unwrap_or(0.0)
                })
            }
        };
        if !supported {
            decision = PolicyDecision::Inconclusive;
            reasons.push(
                "the declared repeatable claim is not supported across the observed complete-pair range; one pair or a pair below the declared threshold cannot establish repeatable savings, so the adoption stays inconclusive for that scope"
                    .to_owned(),
            );
        }
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
        measurement: Some(measurement),
        variation: Some(variation),
        statistical_claim,
        corroboration,
    })
}

fn apply_infrastructure_gate(
    policy: &ComparisonPolicy,
    report: &Value,
    decision: &mut PolicyDecision,
    reasons: &mut Vec<String>,
    _baseline_seconds: &mut Option<f64>,
    _candidate_seconds: &mut Option<f64>,
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
    if policy.objective != Objective::Time {
        reasons.push(
            "the declared primary metric is not adjusted time; a favorable time range cannot waive acceptance, resource or trade-off gates"
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
    }
    if !saw || missing || straddles {
        // A rejected acceptance, resource or trade-off gate stays rejected.
        // Uncertainty does not soften it, and it does not become adoption.
        if *decision != PolicyDecision::Reject {
            *decision = PolicyDecision::Inconclusive;
        }
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
        // The decision tree already evaluated acceptance, resource, token and
        // trade-off gates on the selected view. A cleared time range does not
        // waive any of them and does not rewrite a rejection into adoption.
        if *decision == PolicyDecision::Adopt {
            reasons.push(
                "the adjusted range stays above the declared effect; external waiting alone is not the measured improvement"
                    .to_owned(),
            );
        }
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
