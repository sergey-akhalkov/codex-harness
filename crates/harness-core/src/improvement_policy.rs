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
//!
//! A retained baseline measurement may replace a fresh execution of the old
//! variant only under a predeclared baseline-reuse clause
//! ([`BASELINE_REUSE_CLAUSE`]): the evidence owner must have verified the
//! retained input/runtime/acceptance/metric/condition identity, a retained
//! trace reference, and the recorded age, coverage, uncertainty and
//! predeclared selection. [`evaluate`] refuses a reuse whose age exceeds the
//! declared bound, whose selection postdates the candidate outcome, or whose
//! recorded facts contradict the frozen policy; a model-dependent reuse
//! additionally needs the selected runtime qualification and context
//! isolation, while a method without model execution keeps its model metrics
//! inapplicable and requires none. An admitted reuse is recorded with its
//! retained identity, age, coverage, uncertainty and original cost instead of
//! being presented as a fresh measurement.
//!
//! When the policy declares the infrastructure binding, the adjusted decision
//! is additionally bound to the retained attribution evidence of every
//! included unit attempt: the rule/version and measurement lineage must match
//! the predeclared rule, the adjusted view must reproduce from its retained
//! native trace, and the raw/adjusted/excluded totals must reconcile. A
//! mismatch, an unreconciled or duplicated usage total, or evidence produced
//! under a different rule makes the result inconclusive instead of letting a
//! changed analysis inherit the comparison; the bound identity, reconciled
//! totals and every exclusion reason are recorded with the decision, and an
//! observed difference that exists only in the excluded external waiting is
//! neither a measured gain nor a measured regression.

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

/// Bound of the digested uncertainty text. It has to carry the bounded
/// statistical-analysis, experiment-selection, nuisance-control and
/// infrastructure-binding clauses together.
const MAX_UNCERTAINTY_BYTES: usize = 2048;

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
        BASELINE_REUSE_CLAUSE,
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
    // The selection body ends at the next clause that belongs to another
    // owner: the nuisance-control clause or the infrastructure binding.
    let end = [
        crate::infrastructure_accounting::RULE_VERSION,
        NUISANCE_CLAUSE,
        BASELINE_REUSE_CLAUSE,
    ]
    .iter()
    .filter_map(|marker| rest.find(marker))
    .min()
    .unwrap_or(rest.len());
    let body = &rest[..end];
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

/// Marker of the predeclared nuisance-control clause inside the policy's
/// uncertainty text. Clause order is: any statistical-analysis clause, then
/// the experiment-selection clause, then this nuisance-control clause, then
/// the infrastructure binding, so every owner parses its own fields without
/// consuming another's.
pub const NUISANCE_CLAUSE: &str = "nuisance-control.v1";

/// Bound on one optional nuisance-control value.
const MAX_NUISANCE_FIELD_BYTES: usize = 192;
/// Bound on the rendered nuisance-control clause.
const MAX_NUISANCE_CLAUSE_BYTES: usize = 512;
/// Bound on the declared balanced schedule's repetition count.
const MAX_NUISANCE_PAIRS: u32 = 1024;

/// The declared initial owned cache/warm-up state of both measured arms.
/// Only owned state is reset or prepared: shared OS, compiler and inference
/// caches stay outside the controller's writes and are disclosed separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InitialState {
    /// Every owned working area the measured operation can write starts empty
    /// or absent; no warm-up is applied.
    OwnedCold,
    /// Both arms start from the same declared prepared owned recipe, named by
    /// the bounded `recipe` token; the recipe's preparation cost stays
    /// accounted under the declared operating mode.
    OwnedPrepared,
    /// A treatment that deliberately inherits shared or previously prepared
    /// state; the state is disclosed and only owned state stays reset.
    InheritDisclosed,
}

impl InitialState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OwnedCold => "owned-cold",
            Self::OwnedPrepared => "owned-prepared",
            Self::InheritDisclosed => "inherit-disclosed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "owned-cold" => Some(Self::OwnedCold),
            "owned-prepared" => Some(Self::OwnedPrepared),
            "inherit-disclosed" => Some(Self::InheritDisclosed),
            _ => None,
        }
    }
}

/// The explicit disclosure of shared cache state. This controller observes
/// owned state only; OS, shared-compiler and inference caches are never read,
/// reset or proven equivalent, and a configured reset command alone is not
/// evidence of equal initial conditions. The only truthful declaration is
/// therefore that the shared state stays unobserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SharedState {
    /// Shared cache state is explicitly uncontrolled and unobserved; only
    /// owned state is verified, and residual shared-cache variation remains a
    /// disclosed limit of the comparison.
    Unobserved,
}

impl SharedState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unobserved => "unobserved",
        }
    }
}

/// The arm-order rule the plan fixes before any result. This sequential
/// controller physically realizes one order per pair; a declared rule whose
/// realization differs is refused by preflight instead of being silently
/// substituted, and a single pair's exposure to time/order drift is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrderRule {
    /// The order is predetermined and fully determined by the plan itself.
    Fixed,
    /// The first arm is derived from the declared seed before results.
    Randomized,
    /// A balanced schedule across the declared repetition count, fixed before
    /// results; the first pair's order is the schedule's first order.
    Balanced,
}

impl OrderRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Randomized => "randomized",
            Self::Balanced => "balanced",
        }
    }
}

/// The first measured arm a declared plan realizes, computed from the plan
/// before any result exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealizedFirst {
    BaselineFirst,
    CandidateFirst,
}

impl RealizedFirst {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BaselineFirst => "baseline-first",
            Self::CandidateFirst => "candidate-first",
        }
    }
}

/// How relevant background load is handled. Uncontrolled execution load is
/// recorded and retained; it never authorizes a guessed utilization-factor
/// time correction, and unresolved differences follow the declared
/// uncertainty and stopping rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoadRule {
    /// Load evidence is recorded per arm where the controller observes it;
    /// unobserved shared load stays an explicit limit, and no utilization
    /// factor is invented.
    Recorded,
}

impl LoadRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
        }
    }
}

/// How observed faults are classified before results. Only a verified
/// transport-only idle wait may qualify for the existing blocking
/// adjustment; ordinary inference/tool execution and unattributed request
/// durations never do, and a fault that changed response, retries, context or
/// subsequent work cannot be repaired by subtracting its wall duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FaultRule {
    /// Classify every fault by its observed effect; preserve the original
    /// attempt, errors and usage.
    ObservedEffect,
}

impl FaultRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ObservedEffect => "observed-effect",
        }
    }
}

/// What governs retries. The frozen comparison policy's stopping rule is the
/// only retry budget: a failed measured attempt is never replayed
/// automatically, and a refusal before any model request follows the
/// configured dispatch policy, not a plan-authored favorable replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RetryRule {
    /// Retries follow the policy's own frozen stopping rule.
    PolicyStopping,
}

impl RetryRule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PolicyStopping => "policy-stopping",
        }
    }
}

/// The frozen nuisance-control plan of one paired comparison. It binds the
/// initial cache/warm-up state, the arm-order rule, the shared-state
/// disclosure, the load rule and the fault/retry rules before either arm
/// starts; preflight verifies the resulting actual owned state and the
/// controller records what it observed per arm.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NuisanceControlPlan {
    pub initial: InitialState,
    /// The prepared owned recipe token; required exactly for `owned-prepared`.
    #[serde(default)]
    pub recipe: Option<String>,
    pub shared: SharedState,
    pub order: OrderRule,
    /// The randomization seed; required exactly for `randomized`.
    #[serde(default)]
    pub seed: Option<u64>,
    /// The declared repetition count of a balanced schedule; required exactly
    /// for `balanced`.
    #[serde(default)]
    pub pairs: Option<u32>,
    pub load: LoadRule,
    pub faults: FaultRule,
    pub retries: RetryRule,
}

impl NuisanceControlPlan {
    /// The first measured arm this plan realizes, computed from its declared
    /// rule and seed. `None` when the plan is internally incomplete (a case
    /// [`Self::problem`] refuses).
    pub fn realized_first(&self) -> Option<RealizedFirst> {
        match self.order {
            OrderRule::Fixed | OrderRule::Balanced => Some(RealizedFirst::BaselineFirst),
            OrderRule::Randomized => self.seed.map(|seed| match seed % 2 {
                0 => RealizedFirst::BaselineFirst,
                _ => RealizedFirst::CandidateFirst,
            }),
        }
    }

    /// The bounded semantic problem, if any. Shared by the comparison policy
    /// and by any other owner that consumes a declared plan.
    pub fn problem(&self) -> Option<String> {
        match (self.initial, self.recipe.as_deref()) {
            (InitialState::OwnedPrepared, None) => Some(
                "an owned-prepared initial state must name the bounded recipe token both arms start from".to_owned(),
            ),
            (InitialState::OwnedPrepared, Some(recipe))
                if recipe.trim().is_empty()
                    || recipe.len() > MAX_NUISANCE_FIELD_BYTES
                    || recipe.contains(['\n', '\r', ';']) =>
            {
                Some(format!(
                    "the owned-prepared recipe must be one bounded line of at most {MAX_NUISANCE_FIELD_BYTES} bytes without ';'"
                ))
            }
            (InitialState::OwnedCold | InitialState::InheritDisclosed, Some(_)) => Some(
                "a recipe token is declared only with an owned-prepared initial state; cold or inherited state names no preparation recipe".to_owned(),
            ),
            _ => None,
        }
        .or_else(|| match (self.order, self.seed, self.pairs) {
            (OrderRule::Randomized, None, _) => Some(
                "a randomized arm order must declare its seed before results; an undeclared order cannot be fixed after the outcome"
                    .to_owned(),
            ),
            (OrderRule::Randomized, Some(_), Some(_)) => Some(
                "a randomized arm order declares a seed, not a balanced repetition count".to_owned(),
            ),
            (OrderRule::Balanced, _, None) => Some(
                "a balanced arm order must declare the repetition count it balances before results".to_owned(),
            ),
            (OrderRule::Balanced, Some(_), Some(_)) => Some(
                "a balanced arm order declares a repetition count, not a randomization seed".to_owned(),
            ),
            (OrderRule::Balanced, None, Some(pairs))
                if !(1..=MAX_NUISANCE_PAIRS).contains(&pairs) =>
            {
                Some(format!(
                "a balanced schedule declares 1..={MAX_NUISANCE_PAIRS} repetitions"
            ))
            }
            (OrderRule::Fixed, Some(_), _) | (OrderRule::Fixed, _, Some(_)) => Some(
                "a fixed arm order declares neither a seed nor a repetition count".to_owned(),
            ),
            _ => None,
        })
    }

    /// The recorded declaration shape of the plan, attached to every measured
    /// attempt before results.
    pub fn declaration(&self) -> Value {
        let mut value = json!({
            "initial": self.initial.as_str(),
            "shared": self.shared.as_str(),
            "order": self.order.as_str(),
            "load": self.load.as_str(),
            "faults": self.faults.as_str(),
            "retries": self.retries.as_str(),
            "realized_first": self.realized_first().map(RealizedFirst::as_str).unwrap_or("unresolved"),
            "exposure": "a single pair runs one order and retains its exposure to time/order drift; only owned state is reset",
        });
        if let Some(recipe) = &self.recipe {
            value["recipe"] = json!(recipe);
        }
        if let Some(seed) = self.seed {
            value["seed"] = json!(seed);
        }
        if let Some(pairs) = self.pairs {
            value["pairs"] = json!(pairs);
        }
        value
    }
}

/// The canonical nuisance-control clause. It follows the experiment-selection
/// clause and precedes the infrastructure binding.
pub fn nuisance_control_clause(plan: &NuisanceControlPlan) -> String {
    let mut clause = format!("{NUISANCE_CLAUSE}; initial={}; ", plan.initial.as_str());
    if let Some(recipe) = &plan.recipe {
        clause.push_str(&format!("recipe={recipe}; "));
    }
    clause.push_str(&format!(
        "shared={}; order={}; ",
        plan.shared.as_str(),
        plan.order.as_str()
    ));
    if let Some(seed) = plan.seed {
        clause.push_str(&format!("seed={seed}; "));
    }
    if let Some(pairs) = plan.pairs {
        clause.push_str(&format!("pairs={pairs}; "));
    }
    clause.push_str(&format!(
        "load={}; faults={}; retries={}",
        plan.load.as_str(),
        plan.faults.as_str(),
        plan.retries.as_str()
    ));
    clause
}

/// Parse the predeclared nuisance-control clause out of the uncertainty text.
/// `Ok(None)` when it is absent: an older default binds no plan. A present
/// but unusable clause is an error so it can never silently degrade into
/// different operating conditions.
pub fn parse_nuisance_control(uncertainty: &str) -> Result<Option<NuisanceControlPlan>, String> {
    let Some(start) = uncertainty.find(NUISANCE_CLAUSE) else {
        return Ok(None);
    };
    if let Some(infrastructure) = uncertainty.find(crate::infrastructure_accounting::RULE_VERSION)
        && infrastructure < start
    {
        return Err(
            "the nuisance-control clause must precede the infrastructure binding it qualifies"
                .to_owned(),
        );
    }
    if let Some(selection) = uncertainty.find(SELECTION_CLAUSE)
        && selection > start
    {
        return Err(
            "the nuisance-control clause must follow the experiment-selection clause so each owner parses its own fields"
                .to_owned(),
        );
    }
    let rest = &uncertainty[start + NUISANCE_CLAUSE.len()..];
    // The body ends at the next clause that belongs to another owner: the
    // baseline-reuse clause or the infrastructure binding.
    let end = [
        crate::infrastructure_accounting::RULE_VERSION,
        BASELINE_REUSE_CLAUSE,
    ]
    .into_iter()
    .filter_map(|marker| rest.find(marker))
    .min()
    .unwrap_or(rest.len());
    let body = &rest[..end];
    if body.len() > MAX_NUISANCE_CLAUSE_BYTES {
        return Err(
            "the nuisance-control clause exceeds its bounded size; keep every declared value within the clause bound"
                .to_owned(),
        );
    }
    let mut initial = None;
    let mut recipe: Option<String> = None;
    let mut shared = None;
    let mut order = None;
    let mut seed: Option<u64> = None;
    let mut pairs: Option<u32> = None;
    let mut load = None;
    let mut faults = None;
    let mut retries = None;
    for segment in body.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let Some((key, value)) = segment.split_once('=') else {
            return Err("nuisance-control fields must use key=value separated by ';'".to_owned());
        };
        let value = value.trim();
        match key.trim() {
            "initial" => {
                if initial.is_some() {
                    return Err("the nuisance-control initial state is declared twice".to_owned());
                }
                initial = Some(InitialState::parse(value).ok_or_else(|| {
                    "the declared initial state is not owned-cold, owned-prepared or inherit-disclosed"
                        .to_owned()
                })?);
            }
            "recipe" => {
                if recipe.is_some() {
                    return Err("the nuisance-control recipe is declared twice".to_owned());
                }
                recipe = Some(value.to_owned());
            }
            "shared" => {
                if shared.is_some() {
                    return Err("the nuisance-control shared state is declared twice".to_owned());
                }
                shared = Some(match value {
                    "unobserved" => SharedState::Unobserved,
                    "observed" | "reset" | "controlled" => {
                        return Err(
                            "OS, shared-compiler and inference caches are not observed by this controller, and a configured reset command alone never proves equivalent initial conditions; declare the shared state explicitly as unobserved"
                                .to_owned(),
                        );
                    }
                    _ => {
                        return Err(
                            "the declared shared state is not a recognized disclosure; shared cache state is only recognized as unobserved"
                                .to_owned(),
                        );
                    }
                });
            }
            "order" => {
                if order.is_some() {
                    return Err("the nuisance-control arm order is declared twice".to_owned());
                }
                order = Some(match value {
                    "fixed" => OrderRule::Fixed,
                    "randomized" => OrderRule::Randomized,
                    "balanced" => OrderRule::Balanced,
                    _ => {
                        return Err(
                            "the declared arm order is not fixed, randomized or balanced"
                                .to_owned(),
                        );
                    }
                });
            }
            "seed" => {
                if seed.is_some() {
                    return Err("the nuisance-control seed is declared twice".to_owned());
                }
                seed = Some(value.parse::<u64>().map_err(|_| {
                    "the nuisance-control seed must be an unsigned integer fixed before results"
                        .to_owned()
                })?);
            }
            "pairs" => {
                if pairs.is_some() {
                    return Err(
                        "the nuisance-control repetition count is declared twice".to_owned()
                    );
                }
                pairs = Some(value.parse::<u32>().map_err(|_| {
                    "the balanced repetition count must be an unsigned integer fixed before results"
                        .to_owned()
                })?);
            }
            "load" => {
                if load.is_some() {
                    return Err("the nuisance-control load rule is declared twice".to_owned());
                }
                load = Some(match value {
                    "recorded" => LoadRule::Recorded,
                    "utilization-adjusted" | "utilization-factor" | "cpu-scaled" => {
                        return Err(
                            "uncontrolled execution load never authorizes a guessed utilization-factor time correction; declare the recorded rule under which load evidence is retained and bounded"
                                .to_owned(),
                        );
                    }
                    _ => {
                        return Err(
                            "the declared load rule is not recognized; background load is only recognized as recorded, never as an invented utilization correction"
                                .to_owned(),
                        );
                    }
                });
            }
            "faults" => {
                if faults.is_some() {
                    return Err("the nuisance-control fault rule is declared twice".to_owned());
                }
                faults = Some(match value {
                    "observed-effect" => FaultRule::ObservedEffect,
                    _ => {
                        return Err(
                            "faults are classified by their observed effect: only a verified transport-only idle wait may qualify for the existing blocking adjustment, while ordinary inference/tool execution, unattributed latency and any fault that changed response, context or subsequent work are retained and never repaired by elapsed subtraction"
                                .to_owned(),
                        );
                    }
                });
            }
            "retries" => {
                if retries.is_some() {
                    return Err("the nuisance-control retry rule is declared twice".to_owned());
                }
                retries = Some(match value {
                    "policy-stopping" => RetryRule::PolicyStopping,
                    _ => {
                        return Err(
                            "retries follow the frozen policy stopping rule; a plan cannot declare a different retry budget or a favorable replay"
                                .to_owned(),
                        );
                    }
                });
            }
            _ => {
                return Err(
                    "the nuisance-control clause has an unknown field; undeclared operating conditions cannot be absorbed silently"
                        .to_owned(),
                );
            }
        }
    }
    let (Some(initial), Some(shared), Some(order), Some(load), Some(faults), Some(retries)) =
        (initial, shared, order, load, faults, retries)
    else {
        return Err(
            "the nuisance-control clause is incomplete; declare initial, shared, order, load, faults and retries before results"
                .to_owned(),
        );
    };
    let plan = NuisanceControlPlan {
        initial,
        recipe,
        shared,
        order,
        seed,
        pairs,
        load,
        faults,
        retries,
    };
    if let Some(problem) = plan.problem() {
        return Err(problem);
    }
    let rendered = nuisance_control_clause(&plan);
    if rendered.len() > MAX_NUISANCE_CLAUSE_BYTES {
        return Err(
            "the rendered nuisance-control clause exceeds its bounded size; shorten the declared fields"
                .to_owned(),
        );
    }
    Ok(Some(plan))
}

/// Marker of the predeclared baseline-reuse clause inside the policy's
/// uncertainty text. Clause order is: any statistical-analysis clause, then
/// the experiment-selection clause, then the nuisance-control clause, then
/// this baseline-reuse clause, then the infrastructure binding, so every
/// owner parses its own fields without consuming another's.
pub const BASELINE_REUSE_CLAUSE: &str = "baseline-reuse.v1";

/// The exact identity groups a retained baseline measurement must still match
/// before it can be reused. The set is fixed: a declared policy can withhold
/// reuse but can never waive input, runtime, acceptance, metric or relevant
/// condition comparability for it.
pub const REUSE_IDENTITIES: &str = "input+runtime+acceptance+metrics+conditions";

/// Bound on the rendered baseline-reuse clause.
const MAX_REUSE_CLAUSE_BYTES: usize = 320;

/// Upper bound on a declared retention age. A bounded window is required so
/// an unbounded reuse of old evidence is never assumed; evidence beyond the
/// declared window needs the smallest sufficient fresh control instead.
const MAX_REUSE_AGE_SECONDS: f64 = 30.0 * 24.0 * 60.0 * 60.0;

/// The frozen policy under which a retained baseline measurement may be
/// reused for a new candidate arm instead of executing the old variant again.
/// It declares before results that reuse is admissible at all and for how
/// long, and it states the fixed requirements a reuse must satisfy: the
/// retained input/runtime/acceptance/metric/condition identity, a retained
/// trace reference, and the selected runtime qualification and context
/// isolation for a model-dependent method. A method without model execution
/// keeps its model metrics inapplicable and requires no unrelated model
/// qualification.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaselineReusePlan {
    /// Retained evidence older than this is not reusable without a fresh
    /// control.
    pub max_age_seconds: f64,
}

impl BaselineReusePlan {
    /// The recorded declaration shape of the plan, attached to every measured
    /// attempt before results.
    pub fn declaration(&self) -> Value {
        json!({
            "identities": REUSE_IDENTITIES,
            "trace": "required",
            "qualification": "model-context",
            "max_age_seconds": self.max_age_seconds,
            "basis": "a retained baseline is reusable only while its input, runtime, acceptance, metric and relevant condition identity still matches the current comparison, its retained trace is present, and the declared age bound holds; model-dependent reuse additionally needs the selected runtime qualification and context isolation, while a method without model execution keeps its model metrics inapplicable",
        })
    }
}

/// The canonical baseline-reuse clause. It follows the nuisance-control
/// clause and precedes the infrastructure binding.
pub fn baseline_reuse_clause(plan: &BaselineReusePlan) -> String {
    format!(
        "{BASELINE_REUSE_CLAUSE}; identities={REUSE_IDENTITIES}; trace=required; qualification=model-context; max-age-seconds={}",
        plan.max_age_seconds
    )
}

/// Parse the predeclared baseline-reuse clause out of the uncertainty text.
/// `Ok(None)` when it is absent: the older default does not reuse retained
/// baseline evidence. A present but unusable clause is an error so it can
/// never silently degrade into different reuse conditions.
pub fn parse_baseline_reuse(uncertainty: &str) -> Result<Option<BaselineReusePlan>, String> {
    let Some(start) = uncertainty.find(BASELINE_REUSE_CLAUSE) else {
        return Ok(None);
    };
    if let Some(infrastructure) = uncertainty.find(crate::infrastructure_accounting::RULE_VERSION)
        && infrastructure < start
    {
        return Err(
            "the baseline-reuse clause must precede the infrastructure binding it qualifies"
                .to_owned(),
        );
    }
    for marker in [STATISTICAL_CLAUSE, SELECTION_CLAUSE, NUISANCE_CLAUSE] {
        if uncertainty[start + BASELINE_REUSE_CLAUSE.len()..].contains(marker) {
            return Err(
                "the baseline-reuse clause must follow the statistical-analysis, experiment-selection and nuisance-control clauses so each owner parses its own fields"
                    .to_owned(),
            );
        }
    }
    let rest = &uncertainty[start + BASELINE_REUSE_CLAUSE.len()..];
    let body = match rest.find(crate::infrastructure_accounting::RULE_VERSION) {
        Some(end) => &rest[..end],
        None => rest,
    };
    if body.len() > MAX_REUSE_CLAUSE_BYTES {
        return Err(
            "the baseline-reuse clause exceeds its bounded size; keep every declared value within the clause bound"
                .to_owned(),
        );
    }
    let mut identities = false;
    let mut trace = false;
    let mut qualification = false;
    let mut max_age: Option<f64> = None;
    for segment in body.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let Some((key, value)) = segment.split_once('=') else {
            return Err("baseline-reuse fields must use key=value separated by ';'".to_owned());
        };
        match (key.trim(), value.trim()) {
            ("identities", REUSE_IDENTITIES) => {
                if identities {
                    return Err("the baseline-reuse identity set is declared twice".to_owned());
                }
                identities = true;
            }
            ("identities", _) => {
                return Err(format!(
                    "baseline reuse must require the fixed identity set {REUSE_IDENTITIES}; a reuse cannot waive input, runtime, acceptance, metric or condition comparability"
                ));
            }
            ("trace", "required") => {
                if trace {
                    return Err("the baseline-reuse trace requirement is declared twice".to_owned());
                }
                trace = true;
            }
            ("trace", _) => {
                return Err(
                    "a retained trace reference is mandatory for reuse; a summary, identity hash or unavailable telemetry cannot stand in for it"
                        .to_owned(),
                );
            }
            ("qualification", "model-context") => {
                if qualification {
                    return Err("the baseline-reuse qualification is declared twice".to_owned());
                }
                qualification = true;
            }
            ("qualification", _) => {
                return Err(
                    "model-dependent reuse requires the selected runtime qualification and context isolation; methods without model execution keep their model metrics inapplicable and are not required to qualify"
                        .to_owned(),
                );
            }
            ("max-age-seconds", value) => {
                if max_age.is_some() {
                    return Err("the baseline-reuse age bound is declared twice".to_owned());
                }
                let parsed = value.parse::<f64>().map_err(|_| {
                    "the baseline-reuse age bound must be a finite positive number of seconds fixed before results"
                        .to_owned()
                })?;
                if !parsed.is_finite() || parsed <= 0.0 || parsed > MAX_REUSE_AGE_SECONDS {
                    return Err(
                        "the baseline-reuse age bound must be positive and within 30 days; retained evidence beyond a bounded window needs a fresh control rather than an unbounded reuse"
                            .to_owned(),
                    );
                }
                max_age = Some(parsed);
            }
            _ => {
                return Err(
                    "the baseline-reuse clause has an unknown field; undeclared reuse conditions cannot be absorbed silently"
                        .to_owned(),
                );
            }
        }
    }
    if !(identities && trace && qualification) {
        return Err(
            "the baseline-reuse clause is incomplete; declare identities, trace, qualification and max-age-seconds before results"
                .to_owned(),
        );
    }
    let Some(max_age_seconds) = max_age else {
        return Err(
            "the baseline-reuse clause is incomplete; declare identities, trace, qualification and max-age-seconds before results"
                .to_owned(),
        );
    };
    Ok(Some(BaselineReusePlan { max_age_seconds }))
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
        // The frozen nuisance-control plan is recorded with every measured
        // attempt before results: the initial cache/warm-up state, the arm
        // order rule, the shared-state disclosure, the load rule and the
        // fault/retry rules cannot be redefined after an outcome exists.
        if let Ok(Some(plan)) = parse_nuisance_control(&self.uncertainty) {
            value["nuisance_plan"] = plan.declaration();
        }
        // The declared baseline-reuse plan is recorded with every measured
        // attempt before results: whether a retained baseline may be reused at
        // all, and within which bounded age, cannot be declared after a
        // candidate outcome exists.
        if let Ok(Some(plan)) = parse_baseline_reuse(&self.uncertainty) {
            value["baseline_reuse"] = plan.declaration();
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
        if self.uncertainty.trim().is_empty() || self.uncertainty.len() > MAX_UNCERTAINTY_BYTES {
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
        // A present nuisance-control clause is parsed and validated by its own
        // owner before any result exists: an undeclared shared cache state,
        // an invented utilization correction, a loosened fault classification
        // or a plan-authored retry budget is refused at declaration.
        let _ = parse_nuisance_control(&self.uncertainty).map_err(invalid)?;
        // A present baseline-reuse clause is parsed and validated by its own
        // owner before any result exists: a waived identity, an optional
        // trace, an unbounded age or an unrelated model qualification is
        // refused at declaration.
        let _ = parse_baseline_reuse(&self.uncertainty).map_err(invalid)?;
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

/// One retained exclusion or unresolved classification of an adjusted metric,
/// copied into the durable evaluation with the arm and attempt it belongs to.
/// Every entry keeps its rule or reason and evidence reference, so no
/// deduction is recorded without its cause.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttributionExclusion {
    pub kind: String,
    pub metric: String,
    pub reason: String,
    #[serde(default)]
    pub rule: Option<String>,
    #[serde(default)]
    pub cause: Option<String>,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub attempt: String,
    #[serde(default)]
    pub arm: String,
    #[serde(default)]
    pub amount_seconds: Option<f64>,
    #[serde(default)]
    pub amount_tokens: Option<u64>,
    #[serde(default)]
    pub requests: Vec<String>,
}

/// The attribution identity and reconciled totals the adjusted decision was
/// bound to: the rule/version that reduced the retained native traces, the
/// declared metric view and treatment mechanism, whether the retained adjusted
/// views reproduce, and the raw/adjusted/excluded totals with every exclusion
/// reason. A changed rule or an unreconciled total cannot silently replace the
/// measured view or inherit an earlier adoption.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttributionEvidence {
    pub rule_version: Option<String>,
    pub lineage: Option<String>,
    pub view: String,
    pub mechanism: String,
    /// `reproduced`, `unavailable` or a mismatch named with its unit attempts.
    pub replay: String,
    /// Worst retained coverage across the included attempts.
    pub coverage: String,
    /// Worst reconciliation status across the included attempts.
    pub reconciliation: String,
    pub elapsed_excluded_seconds: Option<f64>,
    pub elapsed_unresolved_seconds: Option<f64>,
    pub tokens_raw_total: Option<u64>,
    pub tokens_excluded_total: Option<u64>,
    pub tokens_adjusted_total: Option<u64>,
    pub duplicates: Vec<String>,
    pub exclusions: Vec<AttributionExclusion>,
    pub exclusions_omitted: u64,
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
    /// The attribution identity and reconciled totals the adjusted decision
    /// was bound to. `None` when the report carries no attribution evidence.
    #[serde(default)]
    pub attribution: Option<AttributionEvidence>,
    /// The retained baselines this comparison consumed instead of executing
    /// the old variant again: the exact retained execution identity, its age,
    /// retained coverage and uncertainty, and its original cost. Empty when
    /// every arm was measured in this comparison.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reused_baselines: Vec<ReusedBaselineEvidence>,
}

/// One reused retained baseline as consumed by the decision: the retained
/// execution identity, when it was executed and selected, its age, the
/// retained coverage and uncertainty records, its original cost and the
/// model-metric applicability. The record preserves the original accounting;
/// it never presents the retained execution as a fresh measurement, and a
/// method without model execution keeps its model metrics inapplicable rather
/// than a measured zero or a model saving.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReusedBaselineEvidence {
    pub unit: String,
    /// The retained execution identity the reuse references.
    pub attempt: String,
    pub executed_at: f64,
    /// When the reuse selection and exclusions were fixed.
    pub selected_at: f64,
    pub age_seconds: f64,
    /// The retained execution's original cost; accounted once, never fabricated.
    pub original_seconds: Option<f64>,
    pub trace: String,
    /// The coverage recorded with the retained evidence.
    pub coverage: String,
    /// The unresolved uncertainty recorded with the retained evidence.
    pub uncertainty: String,
    /// `inapplicable` for a method without model execution; `measured` otherwise.
    pub model_metrics: String,
    /// `inapplicable` for a model-free method; `verified` for a model-dependent
    /// reuse whose runtime qualification and context isolation were checked.
    pub qualification: String,
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
    /// The unit's method executes no model call: its model metrics are
    /// recorded as inapplicable, never as a measured zero.
    model_free: bool,
    /// A model-free unit recorded the operation's own measured work
    /// (duration, exit and the declared inputs) on both arms.
    operation_work: bool,
    /// The declared model-free method name, when both arms agree on one.
    method: Option<String>,
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
    /// The unit's baseline result is retained evidence from an earlier
    /// execution reused for this comparison; `None` for a freshly measured
    /// baseline.
    reuse: Option<ReusedBaselineFacts>,
    limitations: Vec<String>,
}

/// The retained-baseline facts of one unit, taken from the evidence owner's
/// normalized reuse record before the frozen policy is applied.
struct ReusedBaselineFacts {
    attempt: String,
    executed_at: f64,
    selected_at: f64,
    age_seconds: f64,
    original_seconds: Option<f64>,
    trace: String,
    retained_coverage: String,
    retained_uncertainty: String,
    model_free: bool,
    model_metrics: String,
    qualification: String,
    /// The candidate arm's recorded start, used to locate the reuse selection
    /// before the candidate outcome existed.
    candidate_started_at: Option<f64>,
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

/// The declared model-free method name(s) a decision record can name, in a
/// stable order; `unspecified` when no method token was recorded. The method
/// distinction stays visible in the coverage and reason records instead of
/// being flattened into an anonymous exemption.
fn model_free_names(facts: &[UnitFacts]) -> String {
    let names: BTreeSet<&str> = facts
        .iter()
        .filter(|fact| fact.model_free)
        .filter_map(|fact| fact.method.as_deref())
        .collect();
    if names.is_empty() {
        "unspecified".to_owned()
    } else {
        names.into_iter().collect::<Vec<_>>().join("+")
    }
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

    // Every reuse refusal the evidence owner recorded is retained for the
    // decision: a reused baseline whose identity, conditions, trace,
    // qualification or selection could not be verified is not a comparable
    // baseline, whatever the measured difference looks like.
    let mut reuse_refused: BTreeSet<String> = BTreeSet::new();
    for row in attempts {
        for reason in row
            .get("excluded_reasons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(detail) = reason.strip_prefix("reuse-refused: ") {
                reuse_refused.insert(detail.to_owned());
            }
        }
    }
    for pair in report
        .get("comparisons")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for reason in pair
            .get("excluded_reasons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(detail) = reason.strip_prefix("reuse-refused: ") {
                reuse_refused.insert(detail.to_owned());
            }
        }
    }

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
        // A unit whose baseline result is retained evidence carries the
        // evidence owner's normalized reuse record: the retained execution
        // identity, its age, the retained coverage and uncertainty, its
        // original cost, and the model-metric applicability. The frozen
        // reuse policy is applied to exactly these facts.
        let reuse = unit
            .get("reuse")
            .and_then(Value::as_object)
            .and_then(|reuse| {
                let text = |key: &str| reuse.get(key).and_then(Value::as_str).map(str::to_owned);
                Some(ReusedBaselineFacts {
                    attempt: text("of")?,
                    executed_at: reuse.get("executed_at").and_then(number)?,
                    selected_at: reuse.get("selected_at").and_then(number)?,
                    age_seconds: reuse.get("age_seconds").and_then(number)?,
                    original_seconds: reuse.get("original_seconds").and_then(number),
                    trace: text("trace")?,
                    retained_coverage: text("coverage")?,
                    retained_uncertainty: text("uncertainty")?,
                    model_free: reuse.get("model_metrics").and_then(Value::as_str)
                        == Some(crate::outcome_report::MODEL_METRICS_INAPPLICABLE),
                    model_metrics: text("model_metrics")?,
                    qualification: text("qualification")?,
                    candidate_started_at: candidate_row.and_then(|row| number(&row["started_at"])),
                })
            });
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
            model_free: unit.get("model_metrics").and_then(Value::as_str)
                == Some(crate::outcome_report::MODEL_METRICS_INAPPLICABLE),
            operation_work: unit.get("operation_work") == Some(&Value::Bool(true)),
            method: unit
                .get("method")
                .and_then(Value::as_str)
                .map(str::to_owned),
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
            reuse,
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
    // The model-identity clause of the claim follows the declared method: a
    // method without model execution keeps its metrics inapplicable instead
    // of inheriting model-observation limits that do not apply to it.
    let model_identity = if !facts.is_empty() && facts.iter().all(|fact| fact.model_free) {
        "the declared method executes no model call, so its model metrics are inapplicable rather than a measured zero"
    } else {
        "model identity remains API-observed with unavailable weight hashes or hardware retained as limits"
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
            "{} complete paired unit(s); requests, rounds, tool operations and repeated readings within a task are dependent observations and are not replications; {}; {model_identity}; no statistical confidence level is assigned, and an undetected difference is not evidence of equivalence",
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
    let baseline_seconds = (facts.iter().all(|fact| fact.baseline_seconds.is_some())
        && !facts.is_empty())
    .then(|| {
        facts
            .iter()
            .filter_map(|fact| fact.baseline_seconds)
            .sum::<f64>()
    });
    let candidate_seconds = (facts.iter().all(|fact| fact.candidate_seconds.is_some())
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

    // Required measured coverage for the declared objective. A method without
    // model execution covers the non-primary dimensions with the operation's
    // own measured work (duration, exit and the declared inputs) instead of
    // model rounds and tool operations; its model metrics stay inapplicable,
    // never a measured zero.
    let model_free_dimension = |fact: &UnitFacts| {
        policy.objective == Objective::Time && fact.model_free && fact.operation_work
    };
    let model_free_work = !facts.is_empty() && facts.iter().all(model_free_dimension);
    let mut coverage_parts: Vec<String> = Vec::new();
    if !facts.is_empty() {
        if facts
            .iter()
            .all(|fact| fact.baseline_seconds.is_some() && fact.candidate_seconds.is_some())
        {
            coverage_parts.push("time".to_owned());
        }
        if facts.iter().all(|fact| fact.baseline_steps.is_some()) {
            coverage_parts.push("rounds+tool_ops".to_owned());
        } else if model_free_work {
            coverage_parts.push(format!(
                "method:{} model-metrics:{} operation-work:duration+exit+inputs",
                model_free_names(&facts),
                crate::outcome_report::MODEL_METRICS_INAPPLICABLE
            ));
        }
        if facts.iter().all(|fact| fact.usage_measured) {
            coverage_parts.push("usage".to_owned());
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
            Metric::Rounds | Metric::ToolOperations => facts.iter().all(|fact| {
                (fact.baseline_steps.is_some() && fact.candidate_steps.is_some())
                    || model_free_dimension(fact)
            }),
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
    let mut attribution: Option<AttributionEvidence> = None;
    // Set only when the recorded effect below the declared threshold decides
    // the rejection. A queue-only difference may explain that rejection, so
    // the infrastructure gate revisits it; acceptance, trade-off and
    // repayment rejections stay untouched.
    let mut effect_only_reject = false;
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
                    effect_only_reject = true;
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
        effect_only_reject,
        &mut decision,
        &mut reasons,
        &mut coverage,
        &mut attribution,
    );
    apply_nuisance_gate(
        policy,
        &included,
        &results,
        effect_only_reject,
        &mut decision,
        &mut reasons,
        &mut coverage,
    );
    let mut reused_baselines: Vec<ReusedBaselineEvidence> = Vec::new();
    apply_baseline_reuse_gate(
        policy,
        &facts,
        &reuse_refused.iter().cloned().collect::<Vec<_>>(),
        &mut decision,
        &mut reasons,
        &mut coverage,
        &mut reused_baselines,
    );

    // The decision record names the method distinction: a model-free adoption
    // rests on the operation's own measured work, and its model metrics stay
    // inapplicable rather than becoming a measured zero.
    if decision == PolicyDecision::Adopt && model_free_work {
        reasons.push(format!(
            "the declared model-free method(s) {} measured the operation's own work (duration, exit and declared inputs) on both arms; model metrics are inapplicable, never a measured zero, and no model round or tool-operation counter is required",
            model_free_names(&facts)
        ));
    }

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
        attribution,
        reused_baselines,
    })
}

/// Apply the frozen nuisance-control plan to the summarized evidence. The
/// gate only restricts a decision: it withholds an unsupported causal claim
/// and never softens an independent rejection into an adoption. Retained
/// attempts, errors and usage stay recorded; no elapsed subtraction is
/// applied to a fault that changed response, context or subsequent work.
fn apply_nuisance_gate(
    policy: &ComparisonPolicy,
    included: &[&Value],
    results: &BTreeMap<&str, &Value>,
    effect_only_reject: bool,
    decision: &mut PolicyDecision,
    reasons: &mut Vec<String>,
    coverage: &mut String,
) {
    let plan = match parse_nuisance_control(&policy.uncertainty) {
        Ok(None) => return,
        Ok(Some(plan)) => plan,
        Err(error) => {
            if *decision != PolicyDecision::Reject || effect_only_reject {
                *decision = PolicyDecision::Inconclusive;
            }
            reasons.push(format!(
                "the declared nuisance-control plan is not usable: {error}"
            ));
            coverage.push_str("; nuisance-plan");
            return;
        }
    };
    let realized = plan
        .realized_first()
        .map(RealizedFirst::as_str)
        .unwrap_or("unresolved");
    let mut missing = false;
    let mut violations: Vec<String> = Vec::new();
    let mut baseline_unrelated = false;
    let mut candidate_unrelated = false;
    let mut observed_load = false;
    for unit in included {
        let name = unit_name(unit);
        for (arm, key) in [
            ("baseline", "baseline_result"),
            ("candidate", "candidate_result"),
        ] {
            let Some(row) = arm_result(results, unit, key) else {
                missing = true;
                continue;
            };
            let Some(record) = row.get("nuisance") else {
                missing = true;
                continue;
            };
            if record.get("schema").and_then(Value::as_u64) != Some(1) {
                missing = true;
                continue;
            }
            let recorded_order = record.pointer("/order/realized").and_then(Value::as_str);
            if recorded_order != Some(realized) {
                violations.push(format!(
                    "unit {name} {arm} arm realized arm order {recorded_order:?} instead of the declared {realized:?}"
                ));
            }
            if record.pointer("/initial/violated").and_then(Value::as_bool) == Some(true) {
                let observed = record
                    .pointer("/initial/observed")
                    .and_then(Value::as_str)
                    .unwrap_or("unrecorded");
                violations.push(format!(
                    "unit {name} {arm} arm violated the declared {} initial state ({observed})",
                    record
                        .pointer("/initial/declared")
                        .and_then(Value::as_str)
                        .unwrap_or(plan.initial.as_str())
                ));
            }
            if let Some(faults) = record.get("faults").and_then(Value::as_array) {
                for fault in faults {
                    let class = fault.get("class").and_then(Value::as_str).unwrap_or("");
                    if class == "work-started-unverified" || class == "changed-trajectory" {
                        let detail = fault
                            .get("detail")
                            .and_then(Value::as_str)
                            .unwrap_or("no detail recorded");
                        violations.push(format!(
                            "unit {name} {arm} arm retained a fault that changed or unverified the response, context or work ({detail}); elapsed subtraction alone cannot make its trajectory comparable"
                        ));
                    }
                }
            }
            let unrelated = record
                .pointer("/load/observed/unrelated_wait")
                .and_then(Value::as_bool)
                == Some(true);
            if record
                .pointer("/load/observed/admission_evidence")
                .and_then(Value::as_str)
                == Some("recorded")
            {
                observed_load = true;
            }
            if arm == "baseline" {
                baseline_unrelated |= unrelated;
            } else {
                candidate_unrelated |= unrelated;
            }
        }
    }
    if missing {
        if *decision != PolicyDecision::Reject || effect_only_reject {
            *decision = PolicyDecision::Inconclusive;
        }
        reasons.push(
            "a measured attempt of the declared nuisance-control comparison does not record its plan, realized order, initial-state observation and fault classification; the comparison withholds a causal claim rather than assuming uncontrolled conditions"
                .to_owned(),
        );
        coverage.push_str("; nuisance-evidence-gap");
        return;
    }
    let mut material = !violations.is_empty();
    // Asymmetric recorded unrelated contention can explain an observed
    // difference by itself. The existing blocking adjustment bounds an
    // eligible external wait only under the declared work-efficiency binding;
    // without it the causal claim is withheld instead of inventing a
    // utilization-based time correction.
    if observed_load && baseline_unrelated != candidate_unrelated {
        let bounded = matches!(
            crate::infrastructure_accounting::parse_binding(&policy.uncertainty),
            Ok(Some(binding))
                if binding.view == crate::infrastructure_accounting::MetricView::WorkEfficiency
                    && !binding.mechanism.owns_queue()
                    && policy.objective == Objective::Time
        );
        if bounded {
            coverage.push_str("; nuisance-asymmetric-load-adjusted");
        } else {
            material = true;
            violations.push(
                "an unrelated external heavy command was measured over exactly one arm while no declared work-efficiency binding bounds that waiting; the raw difference is not corrected by a guessed utilization factor".to_owned(),
            );
        }
    } else if observed_load && baseline_unrelated && candidate_unrelated {
        coverage.push_str("; nuisance-comparable-recorded-load");
    } else if !observed_load {
        coverage.push_str("; nuisance-shared-and-load-state-unobserved");
    }
    coverage.push_str("; nuisance-order-exposure-retained");
    if plan.shared == SharedState::Unobserved {
        // The disclosure stays explicit in every plan-bound decision: unknown
        // shared/OS/inference-cache state is never presented as controlled,
        // and a configured reset is not treated as proof of equal conditions.
        coverage.push_str("; nuisance-shared-unobserved");
    }
    if material {
        if *decision != PolicyDecision::Reject || effect_only_reject {
            *decision = PolicyDecision::Inconclusive;
        }
        reasons.append(&mut violations);
        reasons.push(
            "the declared nuisance-control conditions were not met; retained attempts, errors and usage stay recorded, and further attempts follow the frozen stopping rule without selecting favorable failures or load"
                .to_owned(),
        );
    }
}

/// Apply the frozen baseline-reuse plan to the summarized evidence. A reused
/// baseline is admissible only while every declared requirement still holds:
/// the retained identity and relevant conditions matched the current
/// comparison, the trace reference is retained, the age is within the
/// declared bound, the reuse selection was fixed before the candidate
/// outcome, and a model-dependent reuse carries the verified runtime
/// qualification and context isolation. The gate only restricts a decision:
/// it withholds an unsupported claim, never softens a measured rejection, and
/// records the retained identity, age, coverage, uncertainty and original
/// cost with the decision instead of presenting the retained execution as a
/// fresh measurement.
fn apply_baseline_reuse_gate(
    policy: &ComparisonPolicy,
    facts: &[UnitFacts],
    refused: &[String],
    decision: &mut PolicyDecision,
    reasons: &mut Vec<String>,
    coverage: &mut String,
    reused: &mut Vec<ReusedBaselineEvidence>,
) {
    let hold = |decision: &mut PolicyDecision| {
        if *decision != PolicyDecision::Reject {
            *decision = PolicyDecision::Inconclusive;
        }
    };
    let plan = match parse_baseline_reuse(&policy.uncertainty) {
        Ok(None) => None,
        Ok(Some(plan)) => Some(plan),
        Err(error) => {
            hold(decision);
            reasons.push(format!(
                "the declared baseline-reuse policy is not usable: {error}"
            ));
            coverage.push_str("; baseline-reuse-plan");
            return;
        }
    };
    let mut violations: Vec<String> = Vec::new();
    for detail in refused.iter().take(4) {
        violations.push(format!("retained baseline evidence was refused: {detail}"));
    }
    if refused.len() > 4 {
        violations.push(format!(
            "{} further retained-baseline reuse refusal(s) are recorded in the report and not listed here",
            refused.len() - 4
        ));
    }
    let mut valid = 0usize;
    for fact in facts {
        let Some(reuse) = &fact.reuse else {
            continue;
        };
        let mut problems: Vec<String> = Vec::new();
        let Some(plan) = plan.as_ref() else {
            problems.push(format!(
                "unit {}: the comparison reuses retained baseline evidence without a predeclared baseline-reuse clause; retained evidence cannot be consumed under a policy fixed after it was selected",
                fact.name
            ));
            violations.append(&mut problems);
            continue;
        };
        if reuse.trace.trim().is_empty() {
            problems.push(format!(
                "unit {}: the retained baseline records no trace reference; reuse cannot verify the original execution",
                fact.name
            ));
        }
        if !(reuse.age_seconds.is_finite() && reuse.age_seconds >= 0.0) {
            problems.push(format!(
                "unit {}: the retained baseline records no usable age; retained age must be exposed rather than assumed",
                fact.name
            ));
        } else if reuse.age_seconds > plan.max_age_seconds {
            problems.push(format!(
                "unit {}: the retained baseline evidence is {:.1} s old, beyond the declared {:.1} s reuse bound; a fresh control is required instead of reusing drift-prone evidence",
                fact.name, reuse.age_seconds, plan.max_age_seconds
            ));
        }
        match reuse.candidate_started_at {
            Some(start) if reuse.selected_at <= start => {
                if reuse.executed_at > start {
                    problems.push(format!(
                        "unit {}: the recorded original execution at {:.1} does not predate the candidate attempt at {:.1}; a reused baseline must reference an earlier retained execution",
                        fact.name, reuse.executed_at, start
                    ));
                }
            }
            Some(start) => problems.push(format!(
                "unit {}: the retained baseline was selected at {:.1}, after the candidate arm started at {:.1}; post-result baseline selection cannot satisfy the frozen comparison policy",
                fact.name, reuse.selected_at, start
            )),
            None => problems.push(format!(
                "unit {}: the comparison records no candidate start time; the retained baseline selection cannot be located before the candidate outcome",
                fact.name
            )),
        }
        if reuse.model_free {
            if reuse.model_metrics != crate::outcome_report::MODEL_METRICS_INAPPLICABLE
                || reuse.qualification != crate::outcome_report::MODEL_METRICS_INAPPLICABLE
            {
                problems.push(format!(
                    "unit {}: a method without model execution keeps its model metrics inapplicable; the reused baseline records model-metrics {:?} and qualification {:?}, and no model metric may become a measured zero or a model saving",
                    fact.name, reuse.model_metrics, reuse.qualification
                ));
            }
        } else if reuse.qualification != "verified" {
            problems.push(format!(
                "unit {}: model-dependent reuse requires the retained runtime qualification and context isolation; the reused baseline records qualification {:?}",
                fact.name, reuse.qualification
            ));
        }
        if problems.is_empty() {
            valid += 1;
            reused.push(ReusedBaselineEvidence {
                unit: fact.name.clone(),
                attempt: reuse.attempt.clone(),
                executed_at: reuse.executed_at,
                selected_at: reuse.selected_at,
                age_seconds: reuse.age_seconds,
                original_seconds: reuse.original_seconds,
                trace: reuse.trace.clone(),
                coverage: reuse.retained_coverage.clone(),
                uncertainty: reuse.retained_uncertainty.clone(),
                model_metrics: reuse.model_metrics.clone(),
                qualification: reuse.qualification.clone(),
            });
        } else {
            violations.append(&mut problems);
        }
    }
    if !violations.is_empty() {
        hold(decision);
        for violation in violations.iter().take(4) {
            reasons.push(violation.clone());
        }
        reasons.push(
            "the declared baseline-reuse conditions were not met; the retained evidence is preserved unchanged, no fresh baseline is fabricated and no old variant is replayed, and the comparison needs the smallest sufficient fresh control or stays inconclusive"
                .to_owned(),
        );
        coverage.push_str("; baseline-reuse-violation");
        return;
    }
    if valid > 0 {
        coverage.push_str(&format!("; baseline-reuse:{valid} retained attempt(s)"));
        if let Some(plan) = plan.as_ref() {
            coverage.push_str(&format!(" within {}s", plan.max_age_seconds));
        }
        reasons.push(format!(
            "the comparison consumed {valid} retained baseline result(s) instead of executing the old variant again; the retained identity, age, coverage, uncertainty, model-metric applicability and original cost are recorded with the decision"
        ));
    }
}

fn apply_infrastructure_gate(
    policy: &ComparisonPolicy,
    report: &Value,
    effect_only_reject: bool,
    decision: &mut PolicyDecision,
    reasons: &mut Vec<String>,
    coverage: &mut String,
    attribution: &mut Option<AttributionEvidence>,
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
    let mut raw_wait_effect = false;
    let mut retained = AttributionCollector::new(binding);
    let mut enforcement_error: Option<String> = None;
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
        for (arm, arm_row) in [("baseline", baseline), ("candidate", candidate)] {
            if let Some(arm_row) = arm_row
                && let Some(attribution) = arm_row.get("attribution")
                && let Err(error) = retained.observe(
                    attribution,
                    arm_row
                        .get("attempt_id")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                    arm,
                )
            {
                enforcement_error.get_or_insert(error);
            }
        }
        if candidate.is_some_and(crate::infrastructure_accounting::failed_before_start) {
            failed_infra = true;
        }
        // A queue-only difference: at least one arm of this unit removed a
        // measured external wait while the unadjusted observed times cross a
        // decision boundary. The adjusted range below decides whether that
        // difference is a measured effect or only infrastructure exposure.
        let deducted = |row: Option<&Value>| {
            row.and_then(|row| row.get("infrastructure"))
                .and_then(|infrastructure| infrastructure.get("deductible_ns"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
                > 0
        };
        let raw_baseline = unit
            .get("effect")
            .and_then(|effect| number(&effect["baseline_seconds"]));
        let raw_candidate = unit
            .get("effect")
            .and_then(|effect| number(&effect["candidate_seconds"]));
        if (deducted(baseline) || deducted(candidate))
            && let (Some(raw_baseline), Some(raw_candidate)) = (raw_baseline, raw_candidate)
            && raw_baseline > 0.0
        {
            let reduction = (raw_baseline - raw_candidate) / raw_baseline * 100.0;
            let regression = (raw_candidate - raw_baseline) / raw_baseline * 100.0;
            if reduction >= threshold || regression > policy.tolerance_percent {
                raw_wait_effect = true;
            }
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
    *attribution = retained.finish();
    if let Some(error) = enforcement_error {
        // Contaminated or unreplayable adjusted evidence is not a decision
        // basis. An acceptance, trade-off or repayment rejection stands; an
        // effect-only rejection becomes inconclusive.
        if *decision != PolicyDecision::Reject || effect_only_reject {
            *decision = PolicyDecision::Inconclusive;
        }
        reasons.push(error);
        coverage.push_str("; infrastructure-evidence");
        return;
    }
    if !saw || missing || straddles {
        // A rejected acceptance, resource or trade-off gate stays rejected.
        // Uncertainty does not soften it, and it does not become adoption.
        if *decision != PolicyDecision::Reject || effect_only_reject {
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
        if *decision == PolicyDecision::Reject && effect_only_reject && raw_wait_effect {
            // The unadjusted difference crosses a decision boundary while the
            // whole adjusted range stays below the effect. That difference is
            // attributed to excluded external waiting, so it is neither a
            // measured gain nor a measured regression.
            *decision = PolicyDecision::Inconclusive;
            reasons.push(
                "the observed difference is attributed to excluded external waiting; the adjusted view supports no model or harness effect, so the verdict stays inconclusive rather than a queue-only gain or regression"
                    .to_owned(),
            );
            coverage.push_str("; raw-only-wait");
        }
    }
}

/// Accumulates the retained attribution evidence of the included unit attempts
/// into one durable record, enforcing the predeclared rule identity, replay
/// status and reconciliation before any adjusted figure may drive a decision.
struct AttributionCollector {
    view: &'static str,
    mechanism: &'static str,
    rule_version: Option<String>,
    lineage: Option<String>,
    replay: &'static str,
    coverage: &'static str,
    reconciliation: &'static str,
    elapsed_excluded: Option<f64>,
    elapsed_excluded_known: bool,
    elapsed_unresolved: Option<f64>,
    elapsed_unresolved_known: bool,
    tokens_raw: Option<u64>,
    tokens_raw_known: bool,
    tokens_excluded: Option<u64>,
    tokens_excluded_known: bool,
    tokens_adjusted: Option<u64>,
    tokens_adjusted_known: bool,
    duplicates: BTreeSet<String>,
    duplicates_omitted: u64,
    exclusions: Vec<AttributionExclusion>,
    exclusions_omitted: u64,
    seen: u64,
}

impl AttributionCollector {
    fn new(binding: crate::infrastructure_accounting::Binding) -> Self {
        use crate::infrastructure_accounting::{Mechanism, MetricView};
        Self {
            view: match binding.view {
                MetricView::WorkEfficiency => "work-efficiency",
                MetricView::Operational => "operational",
            },
            mechanism: match binding.mechanism {
                Mechanism::None => "none",
                Mechanism::Admission => "admission",
                Mechanism::Scheduling => "scheduling",
                Mechanism::Waiting => "waiting",
                Mechanism::Cache => "cache",
            },
            rule_version: None,
            lineage: None,
            replay: "reproduced",
            coverage: "measured",
            reconciliation: "consistent",
            elapsed_excluded: None,
            elapsed_excluded_known: true,
            elapsed_unresolved: None,
            elapsed_unresolved_known: true,
            tokens_raw: None,
            tokens_raw_known: true,
            tokens_excluded: None,
            tokens_excluded_known: true,
            tokens_adjusted: None,
            tokens_adjusted_known: true,
            duplicates: BTreeSet::new(),
            duplicates_omitted: 0,
            exclusions: Vec::new(),
            exclusions_omitted: 0,
            seen: 0,
        }
    }

    fn observe(&mut self, retained: &Value, attempt: &str, arm: &str) -> Result<(), String> {
        let rule = crate::infrastructure_accounting::RULE_VERSION;
        let Some(object) = retained.as_object() else {
            return Err(format!(
                "attempt {attempt} ({arm}) retains an attribution record that is not an object; the adjusted view cannot be bound to the predeclared rule"
            ));
        };
        self.seen += 1;
        let recorded_rule = object.get("rule_version").and_then(Value::as_str);
        if recorded_rule != Some(rule) {
            return Err(format!(
                "attempt {attempt} ({arm}) retains adjusted evidence under attribution rule {}, not the predeclared {rule}; a changed rule/version cannot inherit the comparison",
                recorded_rule.unwrap_or("<none>")
            ));
        }
        let recorded_lineage = object.get("lineage").and_then(Value::as_str);
        if recorded_lineage != Some(crate::infrastructure_accounting::MEASUREMENT_LINEAGE) {
            return Err(format!(
                "attempt {attempt} ({arm}) retains adjusted evidence outside the predeclared measurement lineage; a changed lineage cannot inherit the comparison"
            ));
        }
        self.rule_version = Some(rule.to_owned());
        self.lineage = Some(crate::infrastructure_accounting::MEASUREMENT_LINEAGE.to_owned());
        let replay = object
            .get("replay")
            .and_then(Value::as_str)
            .unwrap_or("unavailable");
        if replay == "mismatch" {
            return Err(format!(
                "attempt {attempt} ({arm}) retains an adjusted view that does not reproduce from its retained native trace; contaminated evidence is not a decision basis"
            ));
        }
        let reconciliation = object
            .get("reconciliation")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if reconciliation == "gap" {
            return Err(format!(
                "attempt {attempt} ({arm}) records raw/adjusted/excluded totals that do not reconcile; inconsistent or duplicated usage is an accounting gap, not a saving"
            ));
        }
        let coverage = object
            .get("coverage")
            .and_then(Value::as_str)
            .unwrap_or("unresolved");
        if coverage_severity(coverage) > coverage_severity(self.coverage) {
            self.coverage = match coverage {
                "measured" => "measured",
                "partial" => "partial",
                _ => "unresolved",
            };
        }
        if replay_severity(replay) > replay_severity(self.replay) {
            self.replay = match replay {
                "reproduced" => "reproduced",
                _ => "unavailable",
            };
        }
        if reconciliation_severity(reconciliation) > reconciliation_severity(self.reconciliation) {
            self.reconciliation = match reconciliation {
                "consistent" => "consistent",
                "degraded" => "degraded",
                "gap" => "gap",
                _ => "unknown",
            };
        }
        if let Some(duplicates) = object.get("duplicates").and_then(Value::as_array) {
            for duplicate in duplicates.iter().filter_map(Value::as_str) {
                if self.duplicates.len() >= 64 {
                    self.duplicates_omitted += 1;
                } else {
                    self.duplicates.insert(duplicate.to_owned());
                }
            }
        }
        let elapsed = object
            .get("reconciliation")
            .and_then(|value| value.get("elapsed"));
        let add_seconds =
            |value: Option<f64>, total: &mut Option<f64>, known: &mut bool| match value {
                Some(value) if *known => {
                    *total = Some(match *total {
                        Some(sum) => sum + value,
                        None => value,
                    });
                }
                Some(_) => {}
                None => {
                    *known = false;
                    *total = None;
                }
            };
        add_seconds(
            elapsed
                .and_then(|elapsed| elapsed.get("excluded_seconds"))
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite()),
            &mut self.elapsed_excluded,
            &mut self.elapsed_excluded_known,
        );
        add_seconds(
            elapsed
                .and_then(|elapsed| elapsed.get("unresolved_seconds"))
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite()),
            &mut self.elapsed_unresolved,
            &mut self.elapsed_unresolved_known,
        );
        let tokens = object
            .get("reconciliation")
            .and_then(|value| value.get("tokens"));
        let token_value = |key: &str| {
            tokens
                .and_then(|tokens| tokens.get(key))
                .and_then(Value::as_u64)
        };
        let add_tokens = |value: Option<u64>, total: &mut Option<u64>, known: &mut bool| match value
        {
            Some(value) if *known => {
                *total = match *total {
                    Some(sum) => sum.checked_add(value),
                    None => Some(value),
                };
            }
            Some(_) => {}
            None => {
                *known = false;
                *total = None;
            }
        };
        add_tokens(
            token_value("raw_total_tokens"),
            &mut self.tokens_raw,
            &mut self.tokens_raw_known,
        );
        add_tokens(
            token_value("excluded_total_tokens"),
            &mut self.tokens_excluded,
            &mut self.tokens_excluded_known,
        );
        add_tokens(
            token_value("adjusted_total_tokens"),
            &mut self.tokens_adjusted,
            &mut self.tokens_adjusted_known,
        );
        if let Some(entries) = object.get("exclusions").and_then(Value::as_array) {
            for entry in entries {
                if self.exclusions.len() >= 64 {
                    self.exclusions_omitted += 1;
                    continue;
                }
                self.exclusions
                    .push(attribution_exclusion(entry, attempt, arm));
            }
        }
        Ok(())
    }

    fn finish(self) -> Option<AttributionEvidence> {
        if self.seen == 0 {
            return None;
        }
        let mut duplicates: Vec<String> = self.duplicates.into_iter().collect();
        if self.duplicates_omitted > 0 {
            duplicates.push(format!(
                "{} further duplicate request identities",
                self.duplicates_omitted
            ));
        }
        Some(AttributionEvidence {
            rule_version: self.rule_version,
            lineage: self.lineage,
            view: self.view.to_owned(),
            mechanism: self.mechanism.to_owned(),
            replay: self.replay.to_owned(),
            coverage: self.coverage.to_owned(),
            reconciliation: self.reconciliation.to_owned(),
            elapsed_excluded_seconds: self
                .elapsed_excluded_known
                .then_some(self.elapsed_excluded)
                .flatten(),
            elapsed_unresolved_seconds: self
                .elapsed_unresolved_known
                .then_some(self.elapsed_unresolved)
                .flatten(),
            tokens_raw_total: self.tokens_raw_known.then_some(self.tokens_raw).flatten(),
            tokens_excluded_total: self
                .tokens_excluded_known
                .then_some(self.tokens_excluded)
                .flatten(),
            tokens_adjusted_total: self
                .tokens_adjusted_known
                .then_some(self.tokens_adjusted)
                .flatten(),
            duplicates,
            exclusions: self.exclusions,
            exclusions_omitted: self.exclusions_omitted,
            basis: "the adjusted decision is bound to the predeclared attribution rule, view and mechanism; the replay status and reconciled raw/adjusted/excluded totals cover every included unit attempt, and every exclusion keeps its rule or reason and evidence reference".to_owned(),
        })
    }
}

fn attribution_exclusion(entry: &Value, attempt: &str, arm: &str) -> AttributionExclusion {
    let text = |key: &str| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default()
    };
    AttributionExclusion {
        kind: text("kind"),
        metric: text("metric"),
        reason: text("reason"),
        rule: entry.get("rule").and_then(Value::as_str).map(str::to_owned),
        cause: entry
            .get("cause")
            .and_then(Value::as_str)
            .map(str::to_owned),
        evidence: text("evidence"),
        attempt: attempt.to_owned(),
        arm: arm.to_owned(),
        amount_seconds: entry.get("amount_seconds").and_then(Value::as_f64),
        amount_tokens: entry.get("amount_tokens").and_then(Value::as_u64),
        requests: entry
            .get("requests")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn coverage_severity(value: &str) -> u8 {
    match value {
        "measured" => 0,
        "partial" => 1,
        _ => 2,
    }
}

fn replay_severity(value: &str) -> u8 {
    match value {
        "reproduced" => 0,
        _ => 1,
    }
}

fn reconciliation_severity(value: &str) -> u8 {
    match value {
        "consistent" => 0,
        "unknown" => 1,
        "degraded" => 2,
        _ => 3,
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
