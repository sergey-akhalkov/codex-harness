//! Benefit gate for promoted orchestration improvements.
//!
//! A promoted improvement becomes a default only after a matched comparison
//! shows unchanged-or-better quality and no delivery-time regression beyond
//! the declared tolerance. Delivery time is accounted per arm as the measured
//! check time plus coordination plus rework, so the comparison cannot win by
//! pushing work outside the measurement. The lead records the comparison as a
//! native `benefit-gate v1` board comment (see `.agents/skills/board-workflow`);
//! this module reads those recorded comparisons back, and
//! [`publish_decision`] writes the evidence-bound `benefit-gate v2` record for
//! self-improvement hypothesis decisions.
//!
//! The reader separates the recorded decision from the recorded comparison
//! evidence. Every attributable record is retained - including incomplete,
//! contradictory or malformed ones - so a newer record is never silently
//! skipped in favor of an older adoption. [`assess`] classifies the newest
//! attributable record; [`default_allowed`] is true only for an adoption whose
//! recorded comparison is complete and arithmetically consistent. An efficiency
//! adoption additionally requires a positive effect: an improved quality outcome
//! or a reduced candidate time. A record that stays within tolerance only because
//! quality is unchanged and the candidate is not faster - still slower, or
//! exactly at baseline - is a consistent record, not a demonstrated improvement.
//! An explicit `engineering-judgment` record opts into a different, predeclared
//! adoption basis: it keeps the measured time/token cost as actual cost (equal or
//! slower included), requires its bounded basis reference and still refuses
//! quality regression, missing bindings or decisive uncertainty.
//!
//! A `benefit-gate v2` decision binds the verdict to the experiment identity
//! and its exact revisions, the acceptance evidence, measured metric coverage,
//! the decision scope, the reason and - for an opt-in engineering judgment - the
//! predeclared basis reference; a v2 record missing or contradicting any of
//! those bindings is retained and reported but can never authorize an adoption.
//! These checks validate what the record says; they are not independent
//! execution and not proof of the underlying experiment.

use crate::board_cli::json_ok_actor;
use crate::board_feedback;
use crate::board_hypothesis::{self, require_line, require_token};
use std::{io, path::Path};

pub const GATE_PREFIX: &str = "benefit-gate v1";
/// The evidence-bound decision record written by [`publish_decision`].
pub const GATE_PREFIX_V2: &str = "benefit-gate v2";
/// The actor recorded on native board comments written by this module.
const ACTOR: &str = "hypothesis-loop";
const MAX_DETAIL: usize = 512;
/// The predeclared basis reference is a single `key=value` token, so it is
/// bounded like the other evidence references rather than carrying prose.
pub(crate) const MAX_BASIS_REFERENCE: usize = 160;

/// Quality measured by the recorded comparison, independent of the timing
/// verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityOutcome {
    Unchanged,
    Improved,
    Regressed,
    Unmeasurable,
}

impl QualityOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Improved => "improved",
            Self::Regressed => "regressed",
            Self::Unmeasurable => "unmeasurable",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "unchanged" => Some(Self::Unchanged),
            "improved" => Some(Self::Improved),
            "regressed" => Some(Self::Regressed),
            "unmeasurable" => Some(Self::Unmeasurable),
            _ => None,
        }
    }
}

/// The recorded decision outcome of a benefit-gate record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionOutcome {
    Adopt,
    Reject,
    Inconclusive,
}

impl DecisionOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Adopt => "adopt",
            Self::Reject => "reject",
            Self::Inconclusive => "inconclusive",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "adopt" => Some(Self::Adopt),
            "reject" => Some(Self::Reject),
            "inconclusive" => Some(Self::Inconclusive),
            _ => None,
        }
    }
}

/// The explicit adoption basis a decision draft records. An absent `basis`
/// field on a v2 record keeps the historical efficiency reading. Only the
/// engineering-judgment basis permits an adoption without a numeric savings
/// effect; it retains the measured cost and requires the predeclared
/// lasting-benefit reference. Legacy maintenance decisions keep the historical
/// unmarked record shape and their strict numeric semantics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DecisionBasis {
    #[default]
    Efficiency,
    Maintenance {
        basis: String,
    },
    EngineeringJudgment {
        basis: String,
    },
}

impl DecisionBasis {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Efficiency => "efficiency",
            Self::Maintenance { .. } => "maintenance",
            Self::EngineeringJudgment { .. } => "engineering-judgment",
        }
    }

    /// The predeclared reference recorded for a non-efficiency basis.
    pub fn reference(&self) -> Option<&str> {
        match self {
            Self::Efficiency => None,
            Self::Maintenance { basis } | Self::EngineeringJudgment { basis } => Some(basis),
        }
    }
}

/// One parsed `benefit-gate v1` board comment with every documented
/// comparison field retained as recorded. A comment that names no `item`
/// cannot be attributed to an item and is skipped; everything else is
/// retained, including a record that carries no readable decision or
/// malformed values, so a newer record always supersedes an older adoption
/// instead of being silently ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRecord {
    /// 1 for `benefit-gate v1`, 2 for the evidence-bound v2 record.
    pub version: u8,
    pub item: String,
    /// `outcome` exactly as recorded; `None` when the field is absent.
    pub outcome: Option<String>,
    /// `quality` exactly as recorded; `None` when the field is absent.
    pub quality: Option<String>,
    pub matched: Option<String>,
    pub tolerance_percent: Option<String>,
    pub baseline_seconds: Option<String>,
    pub candidate_seconds: Option<String>,
    pub regression_percent: Option<String>,
    pub baseline: Option<String>,
    pub candidate: Option<String>,
    pub accounting: Option<String>,
    /// v2 binding: the experiment identity this decision belongs to.
    pub experiment: Option<String>,
    /// v2 binding: `<baseline>..<candidate>` exact evaluated revisions.
    pub revisions: Option<String>,
    /// v2 binding: the independently accepted evidence reference.
    pub acceptance: Option<String>,
    /// v2 binding: the measured metric coverage.
    pub coverage: Option<String>,
    /// v2 binding: the decision scope.
    pub scope: Option<String>,
    /// v2 binding: the recorded decision reason.
    pub reason: Option<String>,
    /// v2 binding: the explicit adoption basis. `None` keeps the historical
    /// efficiency reading for records written before this field existed.
    pub basis: Option<String>,
    /// v2 binding: the predeclared reference for a maintenance or
    /// engineering-judgment basis.
    pub basis_ref: Option<String>,
    /// v2 engineering-judgment cost: complete baseline token cost for the
    /// compared experiment, including the accepted-result path.
    pub baseline_tokens: Option<String>,
    /// v2 engineering-judgment cost: complete candidate token cost for the
    /// compared experiment, including the accepted-result path.
    pub candidate_tokens: Option<String>,
    /// v2 engineering-judgment cost: retained-baseline token cost accumulated
    /// across adopted changes.
    pub cumulative_baseline_tokens: Option<String>,
    /// v2 engineering-judgment cost: candidate token cost accumulated across
    /// the same adopted changes.
    pub cumulative_candidate_tokens: Option<String>,
    /// v2 engineering-judgment cost: the retained baseline identity the
    /// cumulative totals are bound to.
    pub cumulative_baseline: Option<String>,
    /// v2 engineering-judgment payback: the predeclared mechanism.
    pub payback_mechanism: Option<String>,
    /// v2 engineering-judgment payback: `+`-separated observable measures.
    pub payback_measures: Option<String>,
    /// v2 engineering-judgment payback: the positive relevant-use horizon.
    pub payback_horizon: Option<String>,
    /// v2 engineering-judgment payback: the evidence the observed return is
    /// bound to (the recorded acceptance reference).
    pub payback_evidence: Option<String>,
}

impl GateRecord {
    /// The recorded quality when it is one of the four documented outcomes.
    pub fn quality_outcome(&self) -> Option<QualityOutcome> {
        QualityOutcome::parse(self.quality.as_deref()?)
    }

    /// The recorded adoption basis. A record without the field keeps the
    /// historical efficiency meaning; a non-efficiency kind without its
    /// predeclared reference is incomplete and cannot read as a decision.
    fn recorded_basis(&self) -> Result<RecordedBasis<'_>, Limitation> {
        // The explicit basis field is a v2 binding. A v1 record keeps its
        // historical efficiency reading even if a writer left a basis-looking
        // field in it: an older record cannot become an engineering-judgment
        // adoption by relabelling.
        if self.version < 2 {
            return Ok(RecordedBasis::Efficiency);
        }
        match self.basis.as_deref() {
            None | Some("efficiency") => Ok(RecordedBasis::Efficiency),
            Some("maintenance") => self
                .basis_ref
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(RecordedBasis::Maintenance)
                .ok_or(Limitation::BasisReference),
            Some("engineering-judgment") => self
                .basis_ref
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(RecordedBasis::EngineeringJudgment)
                .ok_or(Limitation::BasisReference),
            Some(_) => Err(Limitation::BasisUnrecognized),
        }
    }
}

/// The recognized basis of one retained record. Efficiency is the default so
/// every record written before the explicit field existed keeps its strict
/// numeric meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordedBasis<'a> {
    Efficiency,
    Maintenance(&'a str),
    EngineeringJudgment(&'a str),
}

/// How the newest attributable record for an item reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The newest record states no readable decision.
    Unreadable,
    /// The newest record is a recorded non-adoption (`reject`/`inconclusive`).
    NonAdoption,
    /// The newest record adopts, and its recorded comparison supports it.
    Consistent,
    /// The newest record adopts, but its recorded comparison cannot support
    /// the adoption.
    Unsupported,
}

/// One reason the newest adoption is not supported by its recorded
/// comparison. Each phrase is stable so the ledger and its callers can report
/// the limitation without inventing values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limitation {
    OutcomeAbsent,
    OutcomeUnrecognized,
    Experiment,
    Revisions,
    RevisionsNotDistinct,
    Acceptance,
    Coverage,
    Scope,
    Reason,
    QualityAbsent,
    QualityUnrecognized,
    QualityRegressed,
    QualityUnmeasurable,
    MatchedCount,
    Tolerance,
    BaselineSeconds,
    CandidateSeconds,
    RegressionPercent,
    RegressionArithmetic,
    RegressionBeyondTolerance,
    BaselineArm,
    CandidateArm,
    ArmsNotDistinct,
    Accounting,
    BasisUnrecognized,
    BasisReference,
    BasisNotAdoptable,
    BasisReinterpreted,
    ToleratedRegressionNotBenefit,
    NoPositiveEffect,
    TokenBaseline,
    TokenCandidate,
    CumulativeTokenCost,
    CumulativeBaseline,
    TokenCostInconsistent,
    PaybackRequired,
    PaybackMechanism,
    PaybackMeasures,
    PaybackUnobserved,
    PaybackHorizon,
    PaybackEvidence,
    CumulativeBaselineChanged,
    CumulativeCostReset,
    CumulativeHistory,
    TokenCostUnexpected,
}

impl Limitation {
    /// The stable phrase the ledger prints for this limitation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OutcomeAbsent => "no outcome recorded",
            Self::OutcomeUnrecognized => "outcome is not adopt, reject or inconclusive",
            Self::Experiment => "experiment reference missing",
            Self::Revisions => "revisions missing or not <baseline>..<candidate>",
            Self::RevisionsNotDistinct => "baseline and candidate revisions are identical",
            Self::Acceptance => "acceptance evidence reference missing",
            Self::Coverage => "metric coverage missing",
            Self::Scope => "decision scope missing",
            Self::Reason => "decision reason missing",
            Self::QualityAbsent => "no quality recorded",
            Self::QualityUnrecognized => {
                "quality is not unchanged, improved, regressed or unmeasurable"
            }
            Self::QualityRegressed => "quality regressed",
            Self::QualityUnmeasurable => "quality unmeasurable",
            Self::MatchedCount => "matched count missing or not a positive integer",
            Self::Tolerance => "tolerance_percent missing or not a finite non-negative number",
            Self::BaselineSeconds => "baseline_seconds missing or not a finite positive number",
            Self::CandidateSeconds => {
                "candidate_seconds missing or not a finite non-negative number"
            }
            Self::RegressionPercent => "regression_percent missing or not a finite number",
            Self::RegressionArithmetic => "regression_percent contradicts the recorded arm seconds",
            Self::RegressionBeyondTolerance => "regression beyond the declared tolerance",
            Self::BaselineArm => "baseline arm missing",
            Self::CandidateArm => "candidate arm missing",
            Self::ArmsNotDistinct => "baseline and candidate arms are not distinct",
            Self::Accounting => "accounting basis missing",
            Self::BasisUnrecognized => {
                "adoption basis is not efficiency, maintenance or engineering-judgment"
            }
            Self::BasisReference => "recorded basis reference missing",
            Self::BasisNotAdoptable => "maintenance basis cannot authorize an adoption",
            Self::BasisReinterpreted => {
                "an engineering-judgment adoption cannot reinterpret an earlier record for the same experiment"
            }
            Self::ToleratedRegressionNotBenefit => {
                "candidate slower within tolerance: nonregression is not a positive effect"
            }
            Self::NoPositiveEffect => "no positive quality or time effect recorded",
            Self::TokenBaseline => "baseline token cost missing or not a positive integer",
            Self::TokenCandidate => "candidate token cost missing or not a non-negative integer",
            Self::CumulativeTokenCost => {
                "cumulative token cost missing or not a non-negative integer pair"
            }
            Self::CumulativeBaseline => {
                "cumulative token cost is not bound to the retained baseline"
            }
            Self::TokenCostInconsistent => {
                "complete token cost is inconsistent with the local comparison or the prior adopted total"
            }
            Self::PaybackRequired => {
                "increased full token cost requires a predeclared payback mechanism, observable measures, a bounded horizon and evidence"
            }
            Self::PaybackMechanism => "payback mechanism missing or not a single bounded token",
            Self::PaybackMeasures => {
                "payback measures missing, unsupported, or not single bounded tokens"
            }
            Self::PaybackUnobserved => {
                "a declared payback measure is not part of the recorded metric coverage"
            }
            Self::PaybackHorizon => "payback horizon missing or not a positive integer",
            Self::PaybackEvidence => {
                "payback evidence is missing or not bound to the recorded acceptance evidence"
            }
            Self::CumulativeBaselineChanged => {
                "cumulative token cost is bound to a different retained baseline than the earlier adopted change"
            }
            Self::CumulativeCostReset => {
                "cumulative token cost moved backwards for the retained baseline"
            }
            Self::CumulativeHistory => {
                "an earlier adopted engineering-judgment change has no usable cumulative token cost; it cannot be treated as zero"
            }
            Self::TokenCostUnexpected => {
                "token cost or payback fields are recorded for a decision basis that does not use them"
            }
        }
    }
}

/// The reader's assessment of the newest record attributable to an item.
#[derive(Debug, Clone, PartialEq)]
pub struct Assessment<'a> {
    /// The newest retained record naming the item.
    pub latest: &'a GateRecord,
    /// How many retained records name the item.
    pub recorded: usize,
    /// How the newest record reads.
    pub verdict: Verdict,
    /// Why the newest record cannot support an adoption; empty for
    /// [`Verdict::Consistent`] and [`Verdict::NonAdoption`].
    pub limitations: Vec<Limitation>,
}

/// Parses `benefit-gate v1` and `benefit-gate v2` board comments. Only the
/// comment prefix and a non-empty `item=` value decide attribution: a comment naming an item is
/// retained even when its decision or comparison fields are missing or
/// malformed, so a newer record supersedes an older adoption rather than
/// disappearing. The documented `detail=` tail stays on the board comment and
/// is not read back here.
pub fn parse_gate_comments(comments: &[String]) -> Vec<GateRecord> {
    comments
        .iter()
        .filter_map(|comment| {
            let (version, rest) = match comment.strip_prefix(GATE_PREFIX_V2) {
                Some(rest) => (2u8, rest.trim()),
                None => (1u8, comment.strip_prefix(GATE_PREFIX)?.trim()),
            };
            let head = rest.split_once(" detail=").map_or(rest, |(head, _)| head);
            let mut item = None;
            let mut outcome = None;
            let mut quality = None;
            let mut matched = None;
            let mut tolerance_percent = None;
            let mut baseline_seconds = None;
            let mut candidate_seconds = None;
            let mut regression_percent = None;
            let mut baseline = None;
            let mut candidate = None;
            let mut accounting = None;
            let mut experiment = None;
            let mut revisions = None;
            let mut acceptance = None;
            let mut coverage = None;
            let mut scope = None;
            let mut reason = None;
            let mut basis = None;
            let mut basis_ref = None;
            let mut baseline_tokens = None;
            let mut candidate_tokens = None;
            let mut cumulative_baseline_tokens = None;
            let mut cumulative_candidate_tokens = None;
            let mut cumulative_baseline = None;
            let mut payback_mechanism = None;
            let mut payback_measures = None;
            let mut payback_horizon = None;
            let mut payback_evidence = None;
            for part in head.split_whitespace() {
                let Some((key, value)) = part.split_once('=') else {
                    continue;
                };
                match key {
                    "item" => item = Some(value.to_owned()),
                    "outcome" => outcome = Some(value.to_owned()),
                    "quality" => quality = Some(value.to_owned()),
                    "matched" => matched = Some(value.to_owned()),
                    "tolerance_percent" => tolerance_percent = Some(value.to_owned()),
                    "baseline_seconds" => baseline_seconds = Some(value.to_owned()),
                    "candidate_seconds" => candidate_seconds = Some(value.to_owned()),
                    "regression_percent" => regression_percent = Some(value.to_owned()),
                    "baseline" => baseline = Some(value.to_owned()),
                    "candidate" => candidate = Some(value.to_owned()),
                    "accounting" => accounting = Some(value.to_owned()),
                    "experiment" => experiment = Some(value.to_owned()),
                    "revisions" => revisions = Some(value.to_owned()),
                    "acceptance" => acceptance = Some(value.to_owned()),
                    "coverage" => coverage = Some(value.to_owned()),
                    "scope" => scope = Some(value.to_owned()),
                    "reason" => reason = Some(value.to_owned()),
                    "basis" => basis = Some(value.to_owned()),
                    "basis_ref" => basis_ref = Some(value.to_owned()),
                    "baseline_tokens" => baseline_tokens = Some(value.to_owned()),
                    "candidate_tokens" => candidate_tokens = Some(value.to_owned()),
                    "cumulative_baseline_tokens" => {
                        cumulative_baseline_tokens = Some(value.to_owned())
                    }
                    "cumulative_candidate_tokens" => {
                        cumulative_candidate_tokens = Some(value.to_owned())
                    }
                    "cumulative_baseline" => cumulative_baseline = Some(value.to_owned()),
                    "payback_mechanism" => payback_mechanism = Some(value.to_owned()),
                    "payback_measures" => payback_measures = Some(value.to_owned()),
                    "payback_horizon" => payback_horizon = Some(value.to_owned()),
                    "payback_evidence" => payback_evidence = Some(value.to_owned()),
                    _ => {}
                }
            }
            let item = item.filter(|value| !value.is_empty())?;
            Some(GateRecord {
                version,
                item,
                outcome,
                quality,
                matched,
                tolerance_percent,
                baseline_seconds,
                candidate_seconds,
                regression_percent,
                baseline,
                candidate,
                accounting,
                experiment,
                revisions,
                acceptance,
                coverage,
                scope,
                reason,
                basis,
                basis_ref,
                baseline_tokens,
                candidate_tokens,
                cumulative_baseline_tokens,
                cumulative_candidate_tokens,
                cumulative_baseline,
                payback_mechanism,
                payback_measures,
                payback_horizon,
                payback_evidence,
            })
        })
        .collect()
}

/// The assessment of the newest record attributable to the item, or `None`
/// when no retained record names it.
pub fn assess<'a>(records: &'a [GateRecord], item: &str) -> Option<Assessment<'a>> {
    let latest_index = records.iter().rposition(|record| record.item == item)?;
    let latest = &records[latest_index];
    let recorded = records.iter().filter(|record| record.item == item).count();
    let (mut verdict, mut limitations) = classify(latest);
    // An explicit engineering-judgment adoption is a new decision basis, not a
    // reinterpretation of an already evaluated experiment. Any earlier record
    // for the same experiment under a different or absent basis keeps its
    // strict meaning, so the judgment cannot rewrite it. A repeated identical
    // judgment record remains idempotent because its basis compares equal.
    let latest_basis = latest.recorded_basis();
    if verdict == Verdict::Consistent
        && matches!(latest_basis, Ok(RecordedBasis::EngineeringJudgment(_)))
    {
        let reinterpreted = records.iter().enumerate().any(|(index, record)| {
            index != latest_index
                && record.item == item
                && record.experiment == latest.experiment
                && record.recorded_basis() != latest_basis
        });
        if reinterpreted {
            verdict = Verdict::Unsupported;
            limitations.push(Limitation::BasisReinterpreted);
        }
    }
    // Cumulative adopted changes stay bound to the original retained
    // baseline: a missing history, changed identity or backward total is
    // unknown or drift, never a fresh zero-cost start.
    if matches!(latest_basis, Ok(RecordedBasis::EngineeringJudgment(_))) {
        let cumulative = cumulative_limitations(records, latest_index);
        if !cumulative.is_empty() {
            verdict = Verdict::Unsupported;
            for limitation in cumulative {
                push_unique(&mut limitations, limitation);
            }
        }
    }
    Some(Assessment {
        latest,
        recorded,
        verdict,
        limitations,
    })
}

/// True only when the newest attributable record is an adoption the recorded
/// comparison supports and records a positive effect for quality or time. A
/// missing, incomplete, contradictory, malformed, non-adoption or
/// consistent-without-benefit record leaves the improvement unadopted.
pub fn default_allowed(records: &[GateRecord], item: &str) -> bool {
    assess(records, item).is_some_and(|assessment| assessment.verdict == Verdict::Consistent)
}

/// Whole-percent rounding allowance when the recorded regression is compared
/// with the value computed from the recorded arm seconds.
const ARITHMETIC_ALLOWANCE: f64 = 0.5;

/// A recorded number that must be finite to be usable in the comparison.
fn finite(value: Option<&str>) -> Option<f64> {
    let parsed: f64 = value?.parse().ok()?;
    parsed.is_finite().then_some(parsed)
}

fn nonnegative_tokens(value: Option<&str>) -> Option<u64> {
    value?.parse::<u64>().ok()
}

fn positive_tokens(value: Option<&str>) -> Option<u64> {
    nonnegative_tokens(value).filter(|tokens| *tokens > 0)
}

fn has_token_or_payback(record: &GateRecord) -> bool {
    [
        record.baseline_tokens.is_some(),
        record.candidate_tokens.is_some(),
        record.cumulative_baseline_tokens.is_some(),
        record.cumulative_candidate_tokens.is_some(),
        record.cumulative_baseline.is_some(),
        record.payback_mechanism.is_some(),
        record.payback_measures.is_some(),
        record.payback_horizon.is_some(),
        record.payback_evidence.is_some(),
    ]
    .into_iter()
    .any(|present| present)
}

fn has_payback(record: &GateRecord) -> bool {
    [
        record.payback_mechanism.is_some(),
        record.payback_measures.is_some(),
        record.payback_horizon.is_some(),
        record.payback_evidence.is_some(),
    ]
    .into_iter()
    .any(|present| present)
}

/// The measured dimensions named by a coverage field, before its trailing
/// descriptive clauses. A payback measure is observable only when it is part
/// of this recorded set.
fn coverage_dimensions(coverage: &str) -> Vec<&str> {
    coverage
        .split(';')
        .next()
        .unwrap_or("")
        .split('+')
        .filter(|dimension| !dimension.is_empty())
        .collect()
}

fn push_unique(limitations: &mut Vec<Limitation>, limitation: Limitation) {
    if !limitations.contains(&limitation) {
        limitations.push(limitation);
    }
}

/// The native observable return dimensions a declared payback measure may
/// name. `usage` is the token spending being repaid, not a return, and no
/// other recorded dimension has a comparable before/after total.
fn observable_payback_measure(measure: &str) -> bool {
    matches!(measure, "time" | "rounds" | "tool_ops")
}

fn payback_limitations(record: &GateRecord) -> Vec<Limitation> {
    let mut limitations = Vec::new();
    let mechanism = record.payback_mechanism.as_deref().filter(|value| {
        crate::board_hypothesis::require_token("payback mechanism", value, 64).is_ok()
    });
    if mechanism.is_none() {
        push_unique(&mut limitations, Limitation::PaybackMechanism);
    }
    let measures = record.payback_measures.as_deref().and_then(|value| {
        let measures: Vec<&str> = value.split('+').collect();
        (!measures.is_empty()
            && measures.iter().all(|measure| {
                !measure.is_empty()
                    && observable_payback_measure(measure)
                    && crate::board_hypothesis::require_token("payback measure", measure, 64)
                        .is_ok()
            }))
        .then_some(measures)
    });
    if measures.is_none() {
        push_unique(&mut limitations, Limitation::PaybackMeasures);
    }
    if positive_tokens(record.payback_horizon.as_deref()).is_none() {
        push_unique(&mut limitations, Limitation::PaybackHorizon);
    }
    let evidence_bound = match (
        record.payback_evidence.as_deref(),
        record.acceptance.as_deref(),
    ) {
        (Some(evidence), Some(acceptance)) => !evidence.is_empty() && evidence == acceptance,
        _ => false,
    };
    if !evidence_bound {
        push_unique(&mut limitations, Limitation::PaybackEvidence);
    }
    if let Some(measures) = measures {
        let dimensions = coverage_dimensions(record.coverage.as_deref().unwrap_or(""));
        if measures.iter().any(|measure| !dimensions.contains(measure)) {
            push_unique(&mut limitations, Limitation::PaybackUnobserved);
        }
    }
    limitations
}

/// Complete token-cost evidence for one explicit engineering-judgment
/// adoption. Missing or malformed values stay unknown and never read as zero
/// or as a nonincrease. Equal or lower cost needs no payback; any increase,
/// including a cumulative increase across adopted changes, needs the complete
/// predeclared mechanism, observable measures, positive horizon and acceptance
/// binding.
fn token_cost_limitations(record: &GateRecord) -> Vec<Limitation> {
    let mut limitations = Vec::new();
    let baseline = positive_tokens(record.baseline_tokens.as_deref());
    if baseline.is_none() {
        push_unique(&mut limitations, Limitation::TokenBaseline);
    }
    let candidate = nonnegative_tokens(record.candidate_tokens.as_deref());
    if candidate.is_none() {
        push_unique(&mut limitations, Limitation::TokenCandidate);
    }
    let cumulative_baseline = nonnegative_tokens(record.cumulative_baseline_tokens.as_deref());
    let cumulative_candidate = nonnegative_tokens(record.cumulative_candidate_tokens.as_deref());
    if cumulative_baseline.is_none() || cumulative_candidate.is_none() {
        push_unique(&mut limitations, Limitation::CumulativeTokenCost);
    }
    if record
        .cumulative_baseline
        .as_deref()
        .is_none_or(str::is_empty)
    {
        push_unique(&mut limitations, Limitation::CumulativeBaseline);
    }
    if let (
        Some(baseline),
        Some(candidate),
        Some(cumulative_baseline),
        Some(cumulative_candidate),
    ) = (
        baseline,
        candidate,
        cumulative_baseline,
        cumulative_candidate,
    ) {
        if cumulative_baseline < baseline || cumulative_candidate < candidate {
            push_unique(&mut limitations, Limitation::TokenCostInconsistent);
        }
        let increased = candidate > baseline || cumulative_candidate > cumulative_baseline;
        if increased || has_payback(record) {
            for limitation in payback_limitations(record) {
                push_unique(&mut limitations, limitation);
            }
        }
    }
    limitations
}

/// The retained-baseline identity and additive totals of the latest explicit
/// engineering-judgment adoption. A first record starts at its local totals;
/// a later record extends the most recent adopted judgment record for the
/// same item. An earlier adopted record without usable cumulative evidence is
/// unknown, never zero; a changed identity or a backward total is drift.
fn cumulative_limitations(records: &[GateRecord], latest_index: usize) -> Vec<Limitation> {
    let Some(latest) = records.get(latest_index) else {
        return Vec::new();
    };
    let (Some(baseline), Some(candidate)) = (
        positive_tokens(latest.baseline_tokens.as_deref()),
        nonnegative_tokens(latest.candidate_tokens.as_deref()),
    ) else {
        return Vec::new();
    };
    let (Some(cumulative_baseline), Some(cumulative_candidate)) = (
        nonnegative_tokens(latest.cumulative_baseline_tokens.as_deref()),
        nonnegative_tokens(latest.cumulative_candidate_tokens.as_deref()),
    ) else {
        return Vec::new();
    };
    let Some(identity) = latest
        .cumulative_baseline
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    // Only a different adopted experiment extends the cumulative total: a
    // repeated or conflicting record for the same experiment is the same
    // experiment, not another change to add again.
    let prior: Vec<&GateRecord> = records[..latest_index]
        .iter()
        .filter(|record| {
            record != &latest
                && record.item == latest.item
                && record.outcome.as_deref() == Some("adopt")
                && record.experiment != latest.experiment
                && matches!(
                    record.recorded_basis(),
                    Ok(RecordedBasis::EngineeringJudgment(_))
                )
        })
        .collect();
    let Some(previous) = prior.last().copied() else {
        let mut limitations = Vec::new();
        let revisions_match = latest.revisions.as_deref().and_then(|revisions| {
            let (baseline_revision, _) = revisions.split_once("..")?;
            Some(baseline_revision == identity)
        });
        if revisions_match == Some(false) {
            push_unique(&mut limitations, Limitation::CumulativeBaseline);
        }
        if cumulative_baseline != baseline || cumulative_candidate != candidate {
            push_unique(&mut limitations, Limitation::TokenCostInconsistent);
        }
        return limitations;
    };
    if prior
        .first()
        .is_some_and(|first| first.cumulative_baseline.as_deref() != Some(identity))
        || previous.cumulative_baseline.as_deref() != Some(identity)
    {
        return vec![Limitation::CumulativeBaselineChanged];
    }
    let (Some(previous_baseline), Some(previous_candidate)) = (
        nonnegative_tokens(previous.cumulative_baseline_tokens.as_deref()),
        nonnegative_tokens(previous.cumulative_candidate_tokens.as_deref()),
    ) else {
        return vec![Limitation::CumulativeHistory];
    };
    if cumulative_baseline < previous_baseline || cumulative_candidate < previous_candidate {
        return vec![Limitation::CumulativeCostReset];
    }
    let expected_baseline = previous_baseline.checked_add(baseline);
    let expected_candidate = previous_candidate.checked_add(candidate);
    if expected_baseline != Some(cumulative_baseline)
        || expected_candidate != Some(cumulative_candidate)
    {
        return vec![Limitation::TokenCostInconsistent];
    }
    Vec::new()
}

fn classify(record: &GateRecord) -> (Verdict, Vec<Limitation>) {
    let Some(outcome) = record.outcome.as_deref() else {
        return (Verdict::Unreadable, vec![Limitation::OutcomeAbsent]);
    };
    if !matches!(outcome, "adopt" | "reject" | "inconclusive") {
        return (Verdict::Unreadable, vec![Limitation::OutcomeUnrecognized]);
    }

    // A v2 decision must bind the verdict to its experiment, exact evaluated
    // revisions, acceptance evidence, metric coverage, scope and reason. A
    // record missing or contradicting any binding - even a non-adoption - is
    // retained but cannot read as a complete decision.
    let mut limitations = if record.version >= 2 {
        binding_limitations(record)
    } else {
        Vec::new()
    };
    let basis = match record.recorded_basis() {
        Ok(basis) => Some(basis),
        Err(limitation) => {
            limitations.push(limitation);
            None
        }
    };
    if !matches!(basis, Some(RecordedBasis::EngineeringJudgment(_))) && has_token_or_payback(record)
    {
        limitations.push(Limitation::TokenCostUnexpected);
    }
    if outcome != "adopt" {
        return if limitations.is_empty() {
            (Verdict::NonAdoption, limitations)
        } else {
            (Verdict::Unsupported, limitations)
        };
    }

    match record.quality_outcome() {
        Some(QualityOutcome::Unchanged | QualityOutcome::Improved) => {}
        Some(QualityOutcome::Regressed) => limitations.push(Limitation::QualityRegressed),
        Some(QualityOutcome::Unmeasurable) => limitations.push(Limitation::QualityUnmeasurable),
        None => limitations.push(if record.quality.is_some() {
            Limitation::QualityUnrecognized
        } else {
            Limitation::QualityAbsent
        }),
    }

    // Only the explicit engineering-judgment basis may adopt equal or slower
    // independently accepted evidence. A legacy maintained record that names
    // the maintenance basis is retained but never reads as an adoption; the
    // historical unmarked maintenance decision keeps its strict numeric shape.
    if matches!(basis, Some(RecordedBasis::Maintenance(_))) {
        limitations.push(Limitation::BasisNotAdoptable);
    }

    if record
        .matched
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
        .is_none_or(|count| count == 0)
    {
        limitations.push(Limitation::MatchedCount);
    }
    let tolerance = finite(record.tolerance_percent.as_deref()).filter(|value| *value >= 0.0);
    if tolerance.is_none() {
        limitations.push(Limitation::Tolerance);
    }
    let baseline_seconds = finite(record.baseline_seconds.as_deref()).filter(|value| *value > 0.0);
    if baseline_seconds.is_none() {
        limitations.push(Limitation::BaselineSeconds);
    }
    let candidate_seconds =
        finite(record.candidate_seconds.as_deref()).filter(|value| *value >= 0.0);
    if candidate_seconds.is_none() {
        limitations.push(Limitation::CandidateSeconds);
    }
    let regression = finite(record.regression_percent.as_deref());
    if regression.is_none() {
        limitations.push(Limitation::RegressionPercent);
    }
    if let (Some(baseline_seconds), Some(candidate_seconds), Some(regression)) =
        (baseline_seconds, candidate_seconds, regression)
    {
        let computed = (candidate_seconds - baseline_seconds) / baseline_seconds * 100.0;
        if (computed - regression).abs() > ARITHMETIC_ALLOWANCE {
            limitations.push(Limitation::RegressionArithmetic);
        } else if tolerance.is_some_and(|tolerance| computed > tolerance)
            && !matches!(basis, Some(RecordedBasis::EngineeringJudgment(_)))
        {
            limitations.push(Limitation::RegressionBeyondTolerance);
        }
    }

    for (arm, missing) in [
        (&record.baseline, Limitation::BaselineArm),
        (&record.candidate, Limitation::CandidateArm),
    ] {
        if arm.as_deref().is_none_or(str::is_empty) {
            limitations.push(missing);
        }
    }
    if let (Some(baseline), Some(candidate)) =
        (record.baseline.as_deref(), record.candidate.as_deref())
        && !baseline.is_empty()
        && !candidate.is_empty()
        && baseline == candidate
    {
        limitations.push(Limitation::ArmsNotDistinct);
    }
    if record.accounting.as_deref().is_none_or(str::is_empty) {
        limitations.push(Limitation::Accounting);
    }

    // An otherwise consistent record is not a demonstrated improvement by
    // itself: the adoption must record a positive effect. Unchanged quality
    // with the candidate not faster is a consistent-but-inert comparison, and
    // a regression that stays inside the declared tolerance is tolerated
    // variation, not a benefit. This last check runs only when no earlier
    // limitation already denies the record, so an unsupported record keeps its
    // primary reasons instead of collecting a redundant trailing phrase.
    if limitations.is_empty()
        && !matches!(basis, Some(RecordedBasis::EngineeringJudgment(_)))
        && let (Some(baseline_seconds), Some(candidate_seconds), Some(_)) =
            (baseline_seconds, candidate_seconds, regression)
    {
        let quality = record.quality_outcome();
        let positive =
            quality == Some(QualityOutcome::Improved) || candidate_seconds < baseline_seconds;
        if !positive {
            let computed = (candidate_seconds - baseline_seconds) / baseline_seconds * 100.0;
            limitations.push(if computed > 0.0 {
                Limitation::ToleratedRegressionNotBenefit
            } else {
                Limitation::NoPositiveEffect
            });
        }
    }

    if matches!(basis, Some(RecordedBasis::EngineeringJudgment(_))) {
        for limitation in token_cost_limitations(record) {
            push_unique(&mut limitations, limitation);
        }
    }

    if limitations.is_empty() {
        (Verdict::Consistent, limitations)
    } else {
        (Verdict::Unsupported, limitations)
    }
}

/// The v2 binding fields a decision must identify: experiment, exact
/// revisions, acceptance evidence, metric coverage, scope and reason.
fn binding_limitations(record: &GateRecord) -> Vec<Limitation> {
    let mut limitations = Vec::new();
    if record.experiment.as_deref().is_none_or(str::is_empty) {
        limitations.push(Limitation::Experiment);
    }
    match record
        .revisions
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        None => limitations.push(Limitation::Revisions),
        Some(revisions) => match revisions.split_once("..") {
            Some((baseline, candidate)) if !baseline.is_empty() && !candidate.is_empty() => {
                if baseline == candidate {
                    limitations.push(Limitation::RevisionsNotDistinct);
                }
            }
            _ => limitations.push(Limitation::Revisions),
        },
    }
    for (value, missing) in [
        (&record.acceptance, Limitation::Acceptance),
        (&record.coverage, Limitation::Coverage),
        (&record.scope, Limitation::Scope),
        (&record.reason, Limitation::Reason),
    ] {
        if value.as_deref().is_none_or(str::is_empty) {
            limitations.push(missing);
        }
    }
    limitations
}

/// One complete hypothesis decision ready for board publication. Every field
/// is required except the bounded prose `detail`: the decision must identify
/// the hypothesis, its experiment, the exact evaluated revisions, the
/// independently accepted evidence, the measured metric coverage, its scope
/// and the reason.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionDraft {
    /// The hypothesis card this decision belongs to.
    pub item: String,
    /// The experiment identity the decision binds to.
    pub experiment: String,
    pub outcome: DecisionOutcome,
    pub quality: QualityOutcome,
    /// The explicit adoption basis. `Efficiency` and the legacy maintenance
    /// view keep the historical unmarked record shape; only an engineering
    /// judgment adds `basis`/`basis_ref` to the record.
    pub basis: DecisionBasis,
    pub matched: u64,
    pub tolerance_percent: f64,
    pub baseline_seconds: f64,
    pub candidate_seconds: f64,
    pub baseline_arm: String,
    pub candidate_arm: String,
    pub accounting: String,
    /// The exact baseline runtime revision evaluated.
    pub baseline_revision: String,
    /// The exact candidate runtime revision evaluated.
    pub candidate_revision: String,
    /// The independently accepted evidence reference.
    pub acceptance: String,
    /// The measured metric coverage (for example `time+rounds+tool_ops`).
    pub coverage: String,
    /// The decision scope (for example the task/model scope).
    pub scope: String,
    /// The recorded decision reason.
    pub reason: String,
    /// Bounded prose detail; the board record stays a reference.
    pub detail: Option<String>,
    /// Complete local baseline token cost for the compared experiment.
    pub baseline_tokens: Option<u64>,
    /// Complete local candidate token cost for the compared experiment.
    pub candidate_tokens: Option<u64>,
    /// Token cost of the retained baseline accumulated across adopted changes.
    pub cumulative_baseline_tokens: Option<u64>,
    /// Candidate token cost accumulated across the same adopted changes.
    pub cumulative_candidate_tokens: Option<u64>,
    /// The retained baseline identity the cumulative totals are bound to.
    pub cumulative_baseline: Option<String>,
    /// The predeclared payback mechanism for a measured increase.
    pub payback_mechanism: Option<String>,
    /// `+`-separated observable measures already in the recorded coverage.
    pub payback_measures: Option<String>,
    /// The positive relevant-use horizon in whole tasks.
    pub payback_horizon: Option<u64>,
    /// The acceptance evidence the observed return is bound to.
    pub payback_evidence: Option<String>,
}

impl DecisionDraft {
    /// Builds the `benefit-gate v2` record text, refusing missing or
    /// self-contradicting fields before anything reaches the board.
    pub fn record(&self) -> io::Result<String> {
        let item = require_token("item", &self.item, 64)?;
        let experiment = require_token("experiment", &self.experiment, 128)?;
        let baseline_revision = require_token("baseline revision", &self.baseline_revision, 128)?;
        let candidate_revision =
            require_token("candidate revision", &self.candidate_revision, 128)?;
        if baseline_revision == candidate_revision {
            return Err(board_hypothesis::invalid(
                "baseline and candidate revisions are identical: there is no treatment to compare",
            ));
        }
        let acceptance = require_token("acceptance", &self.acceptance, 160)?;
        let coverage = require_token("coverage", &self.coverage, 160)?;
        let scope = require_token("scope", &self.scope, 160)?;
        let reason = require_token("reason", &self.reason, 160)?;
        let baseline_arm = require_token("baseline arm", &self.baseline_arm, 64)?;
        let candidate_arm = require_token("candidate arm", &self.candidate_arm, 64)?;
        if baseline_arm == candidate_arm {
            return Err(board_hypothesis::invalid(
                "baseline and candidate arms are not distinct",
            ));
        }
        let token_fields = [
            self.baseline_tokens.is_some(),
            self.candidate_tokens.is_some(),
            self.cumulative_baseline_tokens.is_some(),
            self.cumulative_candidate_tokens.is_some(),
            self.cumulative_baseline.is_some(),
        ];
        let any_tokens = token_fields.iter().any(|present| *present);
        let complete_tokens = token_fields.iter().all(|present| *present);
        let payback_fields = [
            self.payback_mechanism.is_some(),
            self.payback_measures.is_some(),
            self.payback_horizon.is_some(),
            self.payback_evidence.is_some(),
        ];
        let any_payback = payback_fields.iter().any(|present| *present);
        let complete_payback = payback_fields.iter().all(|present| *present);
        if !matches!(self.basis, DecisionBasis::EngineeringJudgment { .. }) {
            if any_tokens || any_payback {
                return Err(board_hypothesis::invalid(
                    "token cost and payback fields belong to the engineering-judgment basis",
                ));
            }
        } else if (self.outcome == DecisionOutcome::Adopt || any_tokens || any_payback)
            && !complete_tokens
        {
            return Err(board_hypothesis::invalid(
                "an engineering-judgment decision needs complete local and cumulative token cost",
            ));
        }
        if any_payback && !complete_payback {
            return Err(board_hypothesis::invalid(
                "a payback record needs a mechanism, observable measures, a positive horizon and acceptance evidence",
            ));
        }
        if let (
            Some(baseline),
            Some(candidate),
            Some(cumulative_baseline),
            Some(cumulative_candidate),
            Some(identity),
        ) = (
            self.baseline_tokens,
            self.candidate_tokens,
            self.cumulative_baseline_tokens,
            self.cumulative_candidate_tokens,
            self.cumulative_baseline.as_deref(),
        ) {
            if baseline == 0 {
                return Err(board_hypothesis::invalid(
                    "baseline token cost must be a positive integer",
                ));
            }
            require_token("cumulative baseline", identity, 128)?;
            if cumulative_baseline < baseline || cumulative_candidate < candidate {
                return Err(board_hypothesis::invalid(
                    "cumulative token cost cannot be lower than the local comparison it extends",
                ));
            }
            let increased = candidate > baseline || cumulative_candidate > cumulative_baseline;
            if self.outcome == DecisionOutcome::Adopt && increased && !complete_payback {
                return Err(board_hypothesis::invalid(
                    "increased complete token cost requires a predeclared payback mechanism, observable measures, a positive horizon and acceptance evidence",
                ));
            }
        }
        if complete_payback {
            require_token(
                "payback mechanism",
                self.payback_mechanism.as_deref().unwrap_or(""),
                64,
            )?;
            let measures: Vec<&str> = self
                .payback_measures
                .as_deref()
                .unwrap_or("")
                .split('+')
                .collect();
            if measures.iter().any(|measure| measure.is_empty()) {
                return Err(board_hypothesis::invalid(
                    "every payback measure must be a single non-empty token",
                ));
            }
            let dimensions = coverage_dimensions(&coverage);
            let mut seen = std::collections::BTreeSet::new();
            for measure in &measures {
                require_token("payback measure", measure, 64)?;
                if !observable_payback_measure(measure) {
                    return Err(board_hypothesis::invalid(
                        "a payback measure must be an observable return dimension: time, rounds or tool_ops",
                    ));
                }
                if !seen.insert(*measure) {
                    return Err(board_hypothesis::invalid("a payback measure is repeated"));
                }
                if !dimensions.contains(measure) {
                    return Err(board_hypothesis::invalid(
                        "every payback measure must already be part of the recorded metric coverage",
                    ));
                }
            }
            if self.payback_horizon == Some(0) {
                return Err(board_hypothesis::invalid(
                    "the payback horizon must be a positive whole number of tasks",
                ));
            }
            let evidence = require_token(
                "payback evidence",
                self.payback_evidence.as_deref().unwrap_or(""),
                160,
            )?;
            if evidence != acceptance {
                return Err(board_hypothesis::invalid(
                    "payback evidence must be the recorded acceptance evidence",
                ));
            }
        }
        let accounting = require_token("accounting", &self.accounting, 160)?;
        if self.matched == 0 {
            return Err(board_hypothesis::invalid(
                "matched count must be a positive integer",
            ));
        }
        if !self.tolerance_percent.is_finite() || self.tolerance_percent < 0.0 {
            return Err(board_hypothesis::invalid(
                "tolerance_percent must be a finite non-negative number",
            ));
        }
        if !self.baseline_seconds.is_finite() || self.baseline_seconds <= 0.0 {
            return Err(board_hypothesis::invalid(
                "baseline_seconds must be a finite positive number",
            ));
        }
        if !self.candidate_seconds.is_finite() || self.candidate_seconds < 0.0 {
            return Err(board_hypothesis::invalid(
                "candidate_seconds must be a finite non-negative number",
            ));
        }
        let regression =
            (self.candidate_seconds - self.baseline_seconds) / self.baseline_seconds * 100.0;
        let mut text = format!(
            "{GATE_PREFIX_V2} item={item} experiment={experiment} revisions={baseline_revision}..{candidate_revision} acceptance={acceptance} coverage={coverage} scope={scope} reason={reason} outcome={} quality={} matched={} tolerance_percent={:.3} baseline_seconds={:.3} candidate_seconds={:.3} regression_percent={regression:.3} baseline={baseline_arm} candidate={candidate_arm} accounting={accounting}",
            self.outcome.as_str(),
            self.quality.as_str(),
            self.matched,
            self.tolerance_percent,
            self.baseline_seconds,
            self.candidate_seconds,
        );
        if let DecisionBasis::EngineeringJudgment { basis } = &self.basis {
            let basis_ref = require_token(
                "engineering-judgment basis reference",
                basis,
                MAX_BASIS_REFERENCE,
            )?;
            text.push_str(&format!(
                " basis=engineering-judgment basis_ref={basis_ref}"
            ));
        }
        if let (
            Some(baseline_tokens),
            Some(candidate_tokens),
            Some(cumulative_baseline_tokens),
            Some(cumulative_candidate_tokens),
            Some(cumulative_baseline),
        ) = (
            self.baseline_tokens,
            self.candidate_tokens,
            self.cumulative_baseline_tokens,
            self.cumulative_candidate_tokens,
            self.cumulative_baseline.as_deref(),
        ) {
            let cumulative_baseline =
                require_token("cumulative baseline", cumulative_baseline, 128)?;
            text.push_str(&format!(
                " baseline_tokens={baseline_tokens} candidate_tokens={candidate_tokens} cumulative_baseline_tokens={cumulative_baseline_tokens} cumulative_candidate_tokens={cumulative_candidate_tokens} cumulative_baseline={cumulative_baseline}"
            ));
        }
        if let (Some(mechanism), Some(measures), Some(horizon), Some(evidence)) = (
            self.payback_mechanism.as_deref(),
            self.payback_measures.as_deref(),
            self.payback_horizon,
            self.payback_evidence.as_deref(),
        ) {
            let mechanism = require_token("payback mechanism", mechanism, 64)?;
            let measures: Vec<&str> = measures.split('+').collect();
            let evidence = require_token("payback evidence", evidence, 160)?;
            text.push_str(&format!(
                " payback_mechanism={mechanism} payback_measures={} payback_horizon={horizon} payback_evidence={evidence}",
                measures.join("+")
            ));
        }
        if let Some(detail) = &self.detail {
            let detail = require_line("detail", detail, MAX_DETAIL)?;
            text.push_str(&format!(" detail={detail}"));
        }
        Ok(text)
    }
}

/// How a decision publication resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Publication {
    /// A new decision comment was written.
    Recorded { text: String },
    /// The identical decision was already recorded; nothing was written.
    Confirmed { text: String },
}

/// Publishes one evidence-bound hypothesis decision as a `benefit-gate v2`
/// comment on the hypothesis card.
///
/// - The record must satisfy its own claimed outcome: an `adopt` whose
///   comparison is incomplete, contradictory or shows no positive effect is
///   refused, and so is a `reject`/`inconclusive` missing its bindings. An
///   explicit engineering-judgment adoption is exempt only from the positive
///   numeric effect: it still needs complete, consistent evidence and its
///   predeclared basis reference.
/// - A new engineering-judgment record cannot reinterpret an earlier record
///   for the same experiment that was decided on a different or absent basis.
/// - The item must be an existing hypothesis card; the decision is never
///   inferred from an evaluator verdict and never written to another tracker.
/// - Publication is idempotent: retrying the identical decision confirms the
///   recorded record without adding another comment or counting the
///   experiment twice. A *different* decision for the same experiment is a
///   deliberate new record; the newest record controls the item's state.
pub fn publish_decision(
    bd: &Path,
    project: &Path,
    draft: &DecisionDraft,
) -> io::Result<Publication> {
    let text = draft.record()?;
    let head = head_of(&text);
    let parsed = parse_gate_comments(std::slice::from_ref(&text));
    let (verdict, limitations) = parsed.first().map_or(
        (Verdict::Unreadable, vec![Limitation::OutcomeAbsent]),
        classify,
    );
    let expected = match draft.outcome {
        DecisionOutcome::Adopt => Verdict::Consistent,
        DecisionOutcome::Reject | DecisionOutcome::Inconclusive => Verdict::NonAdoption,
    };
    if verdict != expected {
        let reasons: Vec<&str> = limitations
            .iter()
            .map(|limitation| limitation.as_str())
            .collect();
        return Err(board_hypothesis::invalid(format!(
            "the recorded {} decision is not supported by its own comparison: {}",
            draft.outcome.as_str(),
            if reasons.is_empty() {
                "the record is unreadable".to_owned()
            } else {
                reasons.join("; ")
            }
        )));
    }

    board_hypothesis::require_hypothesis_card(bd, project, &draft.item)?;
    let comments = board_feedback::list_comments(bd, project, &draft.item)?;
    if comments.iter().any(|comment| head_of(comment) == head) {
        return Ok(Publication::Confirmed { text });
    }
    if draft.outcome == DecisionOutcome::Adopt
        && let DecisionBasis::EngineeringJudgment { basis } = &draft.basis
    {
        let existing = parse_gate_comments(&comments);
        let reinterpreted = existing.iter().any(|record| {
            record.item == draft.item
                && record.experiment.as_deref() == Some(draft.experiment.as_str())
                && record.recorded_basis().ok()
                    != Some(RecordedBasis::EngineeringJudgment(basis.as_str()))
        });
        if reinterpreted {
            return Err(board_hypothesis::invalid(
                Limitation::BasisReinterpreted.as_str(),
            ));
        }
    }
    if draft.outcome == DecisionOutcome::Adopt
        && matches!(draft.basis, DecisionBasis::EngineeringJudgment { .. })
        && let Some(candidate) = parsed.first()
    {
        let mut combined = parse_gate_comments(&comments);
        combined.push(candidate.clone());
        let latest_index = combined.len() - 1;
        let cumulative = cumulative_limitations(&combined, latest_index);
        if !cumulative.is_empty() {
            let reasons: Vec<&str> = cumulative.iter().map(|value| value.as_str()).collect();
            return Err(board_hypothesis::invalid(format!(
                "the engineering-judgment adoption is not bound to its retained cumulative cost: {}",
                reasons.join("; ")
            )));
        }
    }
    json_ok_actor(
        bd,
        project,
        ACTOR,
        &["comment", &draft.item, "--json", &text],
    )?;
    Ok(Publication::Recorded { text })
}

fn head_of(comment: &str) -> &str {
    comment
        .split_once(" detail=")
        .map_or(comment, |(head, _)| head)
}

/// One supported non-adoption whose matched measurements are absent or
/// incomplete. The record binds the verdict to the experiment, the exact
/// evaluated revisions, the acceptance evidence, the measured metric
/// coverage, the scope and the reason; it never fabricates a matched count,
/// arm seconds, tolerance or regression the accounting did not produce, and
/// it can never state an adoption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonAdoptionDraft {
    pub item: String,
    pub experiment: String,
    pub outcome: DecisionOutcome,
    pub quality: QualityOutcome,
    /// Recorded attempt and task counts, for example
    /// `attempts:2 tasks:1 accepted:0 per_success:undefined`.
    pub accounting: String,
    /// The exact baseline revision evaluated (the accepted Git base).
    pub baseline_revision: String,
    /// The exact candidate revision evaluated (the candidate checkout).
    pub candidate_revision: String,
    /// The independently accepted evidence reference.
    pub acceptance: String,
    /// The measured metric coverage actually recorded.
    pub coverage: String,
    /// The decision scope.
    pub scope: String,
    /// The recorded decision reason.
    pub reason: String,
    /// Bounded prose detail; the board record stays a reference.
    pub detail: Option<String>,
}

impl NonAdoptionDraft {
    /// Builds the `benefit-gate v2` non-adoption record text. Every binding
    /// field is required: an incomplete binding is refused instead of being
    /// written as a weaker decision.
    pub fn record(&self) -> io::Result<String> {
        match self.outcome {
            DecisionOutcome::Adopt => {
                return Err(board_hypothesis::invalid(
                    "a non-adoption record cannot state an adoption; an adoption needs matched, independently accepted arm evidence",
                ));
            }
            DecisionOutcome::Reject | DecisionOutcome::Inconclusive => {}
        }
        let item = require_token("item", &self.item, 64)?;
        let experiment = require_token("experiment", &self.experiment, 128)?;
        let baseline_revision = require_token("baseline revision", &self.baseline_revision, 128)?;
        let candidate_revision =
            require_token("candidate revision", &self.candidate_revision, 128)?;
        if baseline_revision == candidate_revision {
            return Err(board_hypothesis::invalid(
                "baseline and candidate revisions are identical: there is no treatment to compare",
            ));
        }
        let acceptance = require_token("acceptance", &self.acceptance, 160)?;
        let coverage = require_token("coverage", &self.coverage, 160)?;
        let scope = require_token("scope", &self.scope, 160)?;
        let reason = require_token("reason", &self.reason, 160)?;
        let accounting = require_token("accounting", &self.accounting, 160)?;
        let quality = require_token("quality", self.quality.as_str(), 32)?;
        let mut text = format!(
            "{GATE_PREFIX_V2} item={item} experiment={experiment} revisions={baseline_revision}..{candidate_revision} acceptance={acceptance} coverage={coverage} scope={scope} reason={reason} outcome={} quality={quality} accounting={accounting}",
            self.outcome.as_str(),
        );
        if let Some(detail) = &self.detail {
            let detail = require_line("detail", detail, MAX_DETAIL)?;
            text.push_str(&format!(" detail={detail}"));
        }
        Ok(text)
    }
}

/// Publishes one supported non-adoption as a `benefit-gate v2` comment on the
/// hypothesis card under the same rules as [`publish_decision`]:
///
/// - the rendered record must classify as a non-adoption (a record missing or
///   contradicting its v2 bindings is refused);
/// - the item must be an existing hypothesis card;
/// - publication is idempotent and a different decision for the same
///   experiment is a deliberate new record controlled by the newest comment.
///
/// A non-adoption authorizes no integration or activation: the activation
/// owner requires the newest record to be a consistent adoption.
pub fn publish_non_adoption(
    bd: &Path,
    project: &Path,
    draft: &NonAdoptionDraft,
) -> io::Result<Publication> {
    let text = draft.record()?;
    let head = head_of(&text);
    let parsed = parse_gate_comments(std::slice::from_ref(&text));
    let (verdict, limitations) = parsed.first().map_or(
        (Verdict::Unreadable, vec![Limitation::OutcomeAbsent]),
        classify,
    );
    if verdict != Verdict::NonAdoption {
        let reasons: Vec<&str> = limitations
            .iter()
            .map(|limitation| limitation.as_str())
            .collect();
        return Err(board_hypothesis::invalid(format!(
            "the recorded {} non-adoption is not supported by its own bindings: {}",
            draft.outcome.as_str(),
            if reasons.is_empty() {
                "the record is unreadable".to_owned()
            } else {
                reasons.join("; ")
            }
        )));
    }
    board_hypothesis::require_hypothesis_card(bd, project, &draft.item)?;
    let comments = board_feedback::list_comments(bd, project, &draft.item)?;
    if comments.iter().any(|comment| head_of(comment) == head) {
        return Ok(Publication::Confirmed { text });
    }
    json_ok_actor(
        bd,
        project,
        ACTOR,
        &["comment", &draft.item, "--json", &text],
    )?;
    Ok(Publication::Recorded { text })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One documented `benefit-gate v1` record as the board-workflow skill
    /// defines it, including the fields the reader retains verbatim.
    fn comment(item: &str, outcome: &str, quality: &str) -> String {
        format!(
            "benefit-gate v1 item={item} improvement=lane-reuse outcome={outcome} quality={quality} matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=99.0 regression_percent=-1.0 baseline=direct candidate=lane accounting=check+coordination+rework detail=measured over two matched tasks"
        )
    }

    /// The documented record with individual fields replaced; an empty
    /// replacement removes the field.
    fn variant(item: &str, replacements: &[(&str, &str)]) -> String {
        let mut text = comment(item, "adopt", "unchanged");
        for (from, to) in replacements {
            assert!(text.contains(from), "fixture lacks {from}");
            text = text.replace(from, to);
        }
        text
    }

    fn limitations_of(records: &[GateRecord], item: &str) -> Vec<Limitation> {
        assess(records, item)
            .expect("attributable record")
            .limitations
    }

    #[test]
    fn documented_records_parse_and_unattributable_ones_are_skipped() {
        let comments = vec![
            comment("codex-harness-pvr.5", "adopt", "unchanged"),
            "benefit-gate v1 item=codex-harness-pvr.5 outcome=adopt".to_owned(),
            "benefit-gate v1 item=codex-harness-pvr.5 outcome=adopt quality=recovered".to_owned(),
            "benefit-gate v1 outcome=adopt quality=unchanged".to_owned(),
            "benefit-gate v1 item= outcome=adopt quality=unchanged".to_owned(),
            "prefix benefit-gate v1 item=codex-harness-pvr.5 outcome=adopt".to_owned(),
            "pacing-decision v1 id=gpt:concurrency scope=gpt knob=concurrency from=4 to=1 expires_at=none reason=pressure basis=dashboard".to_owned(),
        ];
        let parsed = parse_gate_comments(&comments);
        assert_eq!(
            parsed.len(),
            3,
            "attributable records stay, unattributable comments are skipped"
        );
        assert_eq!(parsed[0].item, "codex-harness-pvr.5");
        assert_eq!(parsed[0].outcome.as_deref(), Some("adopt"));
        assert_eq!(parsed[0].quality.as_deref(), Some("unchanged"));
        assert_eq!(parsed[0].quality_outcome(), Some(QualityOutcome::Unchanged));
        assert_eq!(
            parsed[0].quality_outcome().map(QualityOutcome::as_str),
            Some("unchanged")
        );
        assert_eq!(parsed[0].matched.as_deref(), Some("2"));
        assert_eq!(parsed[0].tolerance_percent.as_deref(), Some("10.0"));
        assert_eq!(parsed[0].baseline_seconds.as_deref(), Some("100.0"));
        assert_eq!(parsed[0].candidate_seconds.as_deref(), Some("99.0"));
        assert_eq!(parsed[0].regression_percent.as_deref(), Some("-1.0"));
        assert_eq!(parsed[0].baseline.as_deref(), Some("direct"));
        assert_eq!(parsed[0].candidate.as_deref(), Some("lane"));
        assert_eq!(
            parsed[0].accounting.as_deref(),
            Some("check+coordination+rework")
        );
        assert_eq!(parsed[1].outcome.as_deref(), Some("adopt"));
        assert_eq!(parsed[1].quality, None);
        assert_eq!(parsed[2].quality.as_deref(), Some("recovered"));
        assert_eq!(parsed[2].quality_outcome(), None);
    }

    #[test]
    fn consistent_adoptions_are_the_only_proven_default() {
        for quality in ["unchanged", "improved"] {
            let records = parse_gate_comments(&[comment("item-a", "adopt", quality)]);
            let assessment = assess(&records, "item-a").expect("attributable");
            assert_eq!(assessment.verdict, Verdict::Consistent, "{quality}");
            assert!(assessment.limitations.is_empty(), "{quality}");
            assert_eq!(assessment.recorded, 1, "{quality}");
            assert!(default_allowed(&records, "item-a"), "{quality}");
            assert!(!default_allowed(&records, "item-b"), "{quality}");
        }
        assert!(
            !default_allowed(&[], "item-a"),
            "missing evidence stays unadopted"
        );
        let boundary = variant(
            "item-a",
            &[
                ("tolerance_percent=10.0", "tolerance_percent=0.0"),
                ("candidate_seconds=99.0", "candidate_seconds=100.0"),
                ("regression_percent=-1.0", "regression_percent=0.0"),
            ],
        );
        let records = parse_gate_comments(&[boundary]);
        assert_eq!(
            limitations_of(&records, "item-a"),
            vec![Limitation::NoPositiveEffect],
            "an arithmetically consistent record with no effect is not a demonstrated improvement"
        );
        assert!(!default_allowed(&records, "item-a"));
    }

    #[test]
    fn regressed_or_unmeasurable_quality_never_revives_an_earlier_adoption() {
        let adopted = comment("item-a", "adopt", "unchanged");
        for (quality, limitation) in [
            ("regressed", Limitation::QualityRegressed),
            ("unmeasurable", Limitation::QualityUnmeasurable),
        ] {
            let records =
                parse_gate_comments(&[adopted.clone(), comment("item-a", "adopt", quality)]);
            let assessment = assess(&records, "item-a").expect("attributable");
            assert_eq!(assessment.verdict, Verdict::Unsupported, "{quality}");
            assert_eq!(assessment.latest.outcome.as_deref(), Some("adopt"));
            assert_eq!(assessment.latest.quality.as_deref(), Some(quality));
            assert_eq!(assessment.recorded, 2);
            assert_eq!(assessment.limitations, vec![limitation], "{quality}");
            assert!(
                !default_allowed(&records, "item-a"),
                "the newest record controls the status: {quality}"
            );
        }
    }

    #[test]
    fn a_record_that_only_claims_adoption_is_unsupported() {
        // The audit's counterexample: `outcome=adopt quality=regressed` with
        // no comparison data must not be presented as a proven default.
        let records = parse_gate_comments(&[
            "benefit-gate v1 item=demo outcome=adopt quality=regressed".to_owned(),
        ]);
        let assessment = assess(&records, "demo").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::Unsupported);
        for limitation in [
            Limitation::QualityRegressed,
            Limitation::MatchedCount,
            Limitation::Tolerance,
            Limitation::BaselineSeconds,
            Limitation::CandidateSeconds,
            Limitation::RegressionPercent,
            Limitation::BaselineArm,
            Limitation::CandidateArm,
            Limitation::Accounting,
        ] {
            assert!(
                assessment.limitations.contains(&limitation),
                "missing {limitation:?}"
            );
        }
        assert!(!default_allowed(&records, "demo"));
    }

    #[test]
    fn missing_or_invalid_comparison_fields_stay_unsupported() {
        let cases: &[(&str, &str, Limitation)] = &[
            (" matched=2", "", Limitation::MatchedCount),
            ("matched=2", "matched=0", Limitation::MatchedCount),
            ("matched=2", "matched=-1", Limitation::MatchedCount),
            ("matched=2", "matched=two", Limitation::MatchedCount),
            ("matched=2", "matched=2.5", Limitation::MatchedCount),
            (" tolerance_percent=10.0", "", Limitation::Tolerance),
            (
                "tolerance_percent=10.0",
                "tolerance_percent=NaN",
                Limitation::Tolerance,
            ),
            (
                "tolerance_percent=10.0",
                "tolerance_percent=inf",
                Limitation::Tolerance,
            ),
            (
                "tolerance_percent=10.0",
                "tolerance_percent=-1.0",
                Limitation::Tolerance,
            ),
            (" baseline_seconds=100.0", "", Limitation::BaselineSeconds),
            (
                "baseline_seconds=100.0",
                "baseline_seconds=0.0",
                Limitation::BaselineSeconds,
            ),
            (
                "baseline_seconds=100.0",
                "baseline_seconds=NaN",
                Limitation::BaselineSeconds,
            ),
            (" candidate_seconds=99.0", "", Limitation::CandidateSeconds),
            (
                "candidate_seconds=99.0",
                "candidate_seconds=-1.0",
                Limitation::CandidateSeconds,
            ),
            (
                "candidate_seconds=99.0",
                "candidate_seconds=1e999",
                Limitation::CandidateSeconds,
            ),
            (
                " regression_percent=-1.0",
                "",
                Limitation::RegressionPercent,
            ),
            (
                "regression_percent=-1.0",
                "regression_percent=NaN",
                Limitation::RegressionPercent,
            ),
            (" baseline=direct", "", Limitation::BaselineArm),
            (" candidate=lane", "", Limitation::CandidateArm),
            (
                "baseline=direct candidate=lane",
                "baseline=direct candidate=direct",
                Limitation::ArmsNotDistinct,
            ),
            (
                " accounting=check+coordination+rework",
                "",
                Limitation::Accounting,
            ),
        ];
        for (from, to, limitation) in cases {
            let records = parse_gate_comments(&[variant("item-a", &[(from, to)])]);
            let assessment = assess(&records, "item-a").expect("attributable");
            assert_eq!(assessment.verdict, Verdict::Unsupported, "{from} -> {to}");
            assert!(
                assessment.limitations.contains(limitation),
                "{from} -> {to}: {:?}",
                assessment.limitations
            );
            assert!(!default_allowed(&records, "item-a"), "{from} -> {to}");
        }
    }

    #[test]
    fn inconsistent_arithmetic_and_regression_beyond_tolerance_are_rejected() {
        let inconsistent = variant(
            "item-a",
            &[("regression_percent=-1.0", "regression_percent=5.0")],
        );
        let records = parse_gate_comments(&[inconsistent]);
        assert_eq!(
            limitations_of(&records, "item-a"),
            vec![Limitation::RegressionArithmetic]
        );
        assert!(!default_allowed(&records, "item-a"));

        let beyond = variant(
            "item-a",
            &[
                ("candidate_seconds=99.0", "candidate_seconds=111.0"),
                ("regression_percent=-1.0", "regression_percent=11.0"),
            ],
        );
        let records = parse_gate_comments(&[beyond]);
        assert_eq!(
            limitations_of(&records, "item-a"),
            vec![Limitation::RegressionBeyondTolerance]
        );
        assert!(!default_allowed(&records, "item-a"));

        let within = variant(
            "item-a",
            &[
                ("candidate_seconds=99.0", "candidate_seconds=105.0"),
                ("regression_percent=-1.0", "regression_percent=5.0"),
            ],
        );
        let records = parse_gate_comments(&[within]);
        assert_eq!(
            limitations_of(&records, "item-a"),
            vec![Limitation::ToleratedRegressionNotBenefit],
            "an unchanged-quality regression is tolerated, not a demonstrated improvement"
        );
        assert!(
            !default_allowed(&records, "item-a"),
            "staying inside tolerance does not turn a regression into a benefit"
        );
    }

    #[test]
    fn consistent_fields_without_a_positive_effect_are_not_a_demonstrated_improvement() {
        // The audit counterexample: every numeric field is internally
        // consistent, the arms are distinct and the accounting label is
        // present, yet the candidate is not faster and quality is unchanged.
        // Arithmetic and an accounting string alone must not label this a
        // demonstrated improvement.
        let no_change = variant(
            "item-a",
            &[
                ("candidate_seconds=99.0", "candidate_seconds=100.0"),
                ("regression_percent=-1.0", "regression_percent=0.0"),
            ],
        );
        let records = parse_gate_comments(&[no_change]);
        let assessment = assess(&records, "item-a").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::Unsupported);
        assert_eq!(assessment.limitations, vec![Limitation::NoPositiveEffect]);
        assert!(!default_allowed(&records, "item-a"));

        // An unchanged-quality adoption that is slower but stays inside the
        // declared tolerance is tolerated variation, not a benefit.
        let tolerated = variant(
            "item-a",
            &[
                ("candidate_seconds=99.0", "candidate_seconds=104.0"),
                ("regression_percent=-1.0", "regression_percent=4.0"),
            ],
        );
        let records = parse_gate_comments(&[tolerated]);
        assert_eq!(
            limitations_of(&records, "item-a"),
            vec![Limitation::ToleratedRegressionNotBenefit]
        );
        assert!(!default_allowed(&records, "item-a"));

        // A recorded quality improvement stays a positive effect even when the
        // candidate time regresses inside the declared tolerance: the time
        // change is permitted noncritical variation for the quality objective.
        let quality_first = variant(
            "item-a",
            &[
                ("quality=unchanged", "quality=improved"),
                ("candidate_seconds=99.0", "candidate_seconds=104.0"),
                ("regression_percent=-1.0", "regression_percent=4.0"),
            ],
        );
        assert!(
            default_allowed(&parse_gate_comments(&[quality_first]), "item-a"),
            "an improved quality outcome with an in-tolerance regression is supported"
        );

        // A record that is already unsupported for another reason keeps its
        // primary limitation instead of collecting a redundant trailing one.
        let missing_accounting = variant(
            "item-a",
            &[
                (" accounting=check+coordination+rework", ""),
                ("candidate_seconds=99.0", "candidate_seconds=100.0"),
                ("regression_percent=-1.0", "regression_percent=0.0"),
            ],
        );
        assert_eq!(
            limitations_of(&parse_gate_comments(&[missing_accounting]), "item-a"),
            vec![Limitation::Accounting]
        );
    }

    #[test]
    fn malformed_latest_records_do_not_revive_an_earlier_adoption() {
        let adopted = comment("item-a", "adopt", "unchanged");
        let cases = [
            (
                "benefit-gate v1 item=item-a",
                Verdict::Unreadable,
                Limitation::OutcomeAbsent,
            ),
            (
                "benefit-gate v1 item=item-a outcome=adopt",
                Verdict::Unsupported,
                Limitation::QualityAbsent,
            ),
            (
                "benefit-gate v1 item=item-a outcome=maybe quality=unchanged matched=2",
                Verdict::Unreadable,
                Limitation::OutcomeUnrecognized,
            ),
            (
                "benefit-gate v1 item=item-a outcome=adopt quality=recovered matched=2",
                Verdict::Unsupported,
                Limitation::QualityUnrecognized,
            ),
        ];
        for (newest, verdict, limitation) in cases {
            let records = parse_gate_comments(&[adopted.clone(), newest.to_owned()]);
            let assessment = assess(&records, "item-a").expect("attributable");
            assert_eq!(assessment.verdict, verdict, "{newest}");
            assert!(
                assessment.limitations.contains(&limitation),
                "{newest}: {:?}",
                assessment.limitations
            );
            assert_eq!(assessment.recorded, 2, "{newest}");
            assert!(!default_allowed(&records, "item-a"), "{newest}");
        }

        // A newer comment that names no item cannot shadow anyone.
        let outsider = "benefit-gate v1 outcome=reject quality=regressed".to_owned();
        let records = parse_gate_comments(&[adopted, outsider]);
        assert!(
            default_allowed(&records, "item-a"),
            "an unattributable record names no item"
        );
    }

    #[test]
    fn recorded_non_adoption_supersedes_and_readoption_restores() {
        let adopted = comment("item-a", "adopt", "unchanged");
        let rejected = comment("item-a", "reject", "regressed");
        let inconclusive = comment("item-a", "inconclusive", "unmeasurable");
        let other = parse_gate_comments(&[comment("item-b", "adopt", "unchanged")]);
        assert!(default_allowed(&other, "item-b"));
        assert!(!default_allowed(&other, "item-a"));

        for newest in [&rejected, &inconclusive] {
            let records = parse_gate_comments(&[adopted.clone(), newest.clone()]);
            let assessment = assess(&records, "item-a").expect("attributable");
            assert_eq!(assessment.verdict, Verdict::NonAdoption);
            assert!(assessment.limitations.is_empty());
            assert!(
                !default_allowed(&records, "item-a"),
                "a later rejection or inconclusive decision withdraws the default"
            );
        }

        let readopted = parse_gate_comments(&[rejected, adopted]);
        assert!(
            default_allowed(&readopted, "item-a"),
            "a later supported adoption restores the default"
        );
    }

    /// One complete evidence-bound `benefit-gate v2` adoption as
    /// [`DecisionDraft::record`] writes it.
    fn v2_comment(item: &str) -> String {
        format!(
            "benefit-gate v2 item={item} experiment=exp-1 revisions=base123..cand456 acceptance=evidence-9 coverage=time+rounds+tool_ops scope=task:synthetic.model:local reason=lane-contention outcome=adopt quality=unchanged matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=99.0 regression_percent=-1.0 baseline=direct candidate=lane accounting=check+coordination+rework detail=measured over two matched tasks"
        )
    }

    fn v2_variant(item: &str, replacements: &[(&str, &str)]) -> String {
        let mut text = v2_comment(item);
        for (from, to) in replacements {
            assert!(text.contains(from), "fixture lacks {from}");
            text = text.replace(from, to);
        }
        text
    }

    /// The same complete v2 record declared under the explicit engineering
    /// judgment basis, with an actually slower candidate whose measured cost
    /// is retained rather than treated as a savings claim.
    fn judgment_v2(item: &str) -> String {
        v2_variant(
            item,
            &[
                (
                    "outcome=adopt",
                    "basis=engineering-judgment basis_ref=reflection:lead-1 outcome=adopt",
                ),
                ("candidate_seconds=99.0", "candidate_seconds=115.0"),
                ("regression_percent=-1.0", "regression_percent=15.0"),
                (
                    "accounting=check+coordination+rework",
                    "accounting=check+coordination+rework baseline_tokens=1000 candidate_tokens=1000 cumulative_baseline_tokens=1000 cumulative_candidate_tokens=1000 cumulative_baseline=base123",
                ),
            ],
        )
    }

    #[test]
    fn engineering_judgment_retains_cost_and_repeated_records_are_idempotent() {
        let records = parse_gate_comments(&[judgment_v2("item-a")]);
        let assessment = assess(&records, "item-a").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::Consistent);
        assert!(
            assessment.limitations.is_empty(),
            "{:?}",
            assessment.limitations
        );
        assert_eq!(
            assessment.latest.basis.as_deref(),
            Some("engineering-judgment")
        );
        assert_eq!(
            assessment.latest.basis_ref.as_deref(),
            Some("reflection:lead-1")
        );
        assert_eq!(
            assessment.latest.candidate_seconds.as_deref(),
            Some("115.0"),
            "the measured added cost stays recorded"
        );
        assert!(default_allowed(&records, "item-a"));

        let repeated = parse_gate_comments(&[judgment_v2("item-a"), judgment_v2("item-a")]);
        let assessment = assess(&repeated, "item-a").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::Consistent);
        assert!(
            assessment.limitations.is_empty(),
            "{:?}",
            assessment.limitations
        );
        assert_eq!(assessment.recorded, 2);
    }

    #[test]
    fn engineering_judgment_requires_its_reference_and_complete_evidence() {
        let without_reference = v2_variant(
            "item-a",
            &[
                ("outcome=adopt", "basis=engineering-judgment outcome=adopt"),
                ("candidate_seconds=99.0", "candidate_seconds=115.0"),
                ("regression_percent=-1.0", "regression_percent=15.0"),
            ],
        );
        let records = parse_gate_comments(&[without_reference]);
        assert!(
            limitations_of(&records, "item-a").contains(&Limitation::BasisReference),
            "{:?}",
            limitations_of(&records, "item-a")
        );

        let unknown = v2_variant(
            "item-a",
            &[(
                "outcome=adopt",
                "basis=judgment basis_ref=reflection:lead-1 outcome=adopt",
            )],
        );
        let records = parse_gate_comments(&[unknown]);
        assert!(limitations_of(&records, "item-a").contains(&Limitation::BasisUnrecognized));

        let maintenance = v2_variant(
            "item-a",
            &[(
                "outcome=adopt",
                "basis=maintenance basis_ref=legacy-user-agreement outcome=adopt",
            )],
        );
        let records = parse_gate_comments(&[maintenance]);
        assert!(limitations_of(&records, "item-a").contains(&Limitation::BasisNotAdoptable));

        // The engineering-judgment basis waives only the positive numeric
        // effect; quality remains a hard gate even when the added time is
        // retained as judged actual cost.
        let regressed = v2_variant(
            &judgment_v2("item-a"),
            &[("quality=unchanged", "quality=regressed")],
        );
        let records = parse_gate_comments(&[regressed]);
        assert_eq!(
            limitations_of(&records, "item-a"),
            vec![Limitation::QualityRegressed],
            "a quality regression is refused, not waived by judgement"
        );

        let unmeasurable = v2_variant(
            &judgment_v2("item-a"),
            &[("quality=unchanged", "quality=unmeasurable")],
        );
        let records = parse_gate_comments(&[unmeasurable]);
        assert_eq!(
            limitations_of(&records, "item-a"),
            vec![Limitation::QualityUnmeasurable]
        );

        let unbound = v2_variant(&judgment_v2("item-a"), &[("acceptance=evidence-9 ", "")]);
        let records = parse_gate_comments(&[unbound]);
        assert!(limitations_of(&records, "item-a").contains(&Limitation::Acceptance));

        let same_revision = v2_variant(
            &judgment_v2("item-a"),
            &[("revisions=base123..cand456", "revisions=base123..base123")],
        );
        let records = parse_gate_comments(&[same_revision]);
        assert!(limitations_of(&records, "item-a").contains(&Limitation::RevisionsNotDistinct));
    }

    #[test]
    fn basis_less_equal_or_slower_records_keep_strict_semantics() {
        let equal = v2_variant(
            "item-a",
            &[
                ("candidate_seconds=99.0", "candidate_seconds=100.0"),
                ("regression_percent=-1.0", "regression_percent=0.0"),
            ],
        );
        let records = parse_gate_comments(&[equal]);
        assert_eq!(
            limitations_of(&records, "item-a"),
            vec![Limitation::NoPositiveEffect]
        );

        let slower = v2_variant(
            "item-a",
            &[
                ("candidate_seconds=99.0", "candidate_seconds=104.0"),
                ("regression_percent=-1.0", "regression_percent=4.0"),
            ],
        );
        let records = parse_gate_comments(&[slower]);
        assert_eq!(
            limitations_of(&records, "item-a"),
            vec![Limitation::ToleratedRegressionNotBenefit]
        );
    }

    #[test]
    fn engineering_judgment_cannot_reinterpret_an_earlier_strict_experiment() {
        let strict = v2_comment("item-a");
        let judgment = judgment_v2("item-a");
        let records = parse_gate_comments(&[strict, judgment.clone()]);
        let assessment = assess(&records, "item-a").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::Unsupported);
        assert_eq!(assessment.limitations, vec![Limitation::BasisReinterpreted]);
        assert!(!default_allowed(&records, "item-a"));

        // A judgment under a different predeclared reference is also a
        // different basis, not a continuation of the first judgment.
        let other_reference = v2_variant(
            &judgment,
            &[("basis_ref=reflection:lead-1", "basis_ref=reflection:lead-2")],
        );
        let records = parse_gate_comments(&[judgment, other_reference]);
        let assessment = assess(&records, "item-a").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::Unsupported);
        assert_eq!(assessment.limitations, vec![Limitation::BasisReinterpreted]);
    }

    #[test]
    fn judgment_decisions_round_trip_with_their_declared_basis() {
        let mut draft = publication();
        draft.basis = DecisionBasis::EngineeringJudgment {
            basis: "reflection:lead-1".to_owned(),
        };
        draft.candidate_seconds = 115.0;
        draft.baseline_tokens = Some(1000);
        draft.candidate_tokens = Some(1000);
        draft.cumulative_baseline_tokens = Some(1000);
        draft.cumulative_candidate_tokens = Some(1000);
        draft.cumulative_baseline = Some("base123".to_owned());
        let text = draft.record().expect("complete judgment decision");
        assert!(
            text.contains("basis=engineering-judgment basis_ref=reflection:lead-1"),
            "{text}"
        );
        let records = parse_gate_comments(std::slice::from_ref(&text));
        let assessment = assess(&records, "codex-harness-pvr.5").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::Consistent);
        assert!(
            assessment.limitations.is_empty(),
            "{:?}",
            assessment.limitations
        );
        assert_eq!(
            assessment.latest.basis.as_deref(),
            Some("engineering-judgment")
        );
        assert_eq!(
            assessment.latest.basis_ref.as_deref(),
            Some("reflection:lead-1")
        );

        let mut prose = draft.clone();
        prose.basis = DecisionBasis::EngineeringJudgment {
            basis: "lead reason with spaces".to_owned(),
        };
        assert!(
            prose
                .record()
                .unwrap_err()
                .to_string()
                .contains("unsupported characters"),
            "an unrecordable basis is refused instead of sanitized"
        );

        let mut empty = draft;
        empty.basis = DecisionBasis::EngineeringJudgment {
            basis: String::new(),
        };
        assert!(empty.record().unwrap_err().to_string().contains("required"));

        // The legacy maintenance view keeps the historical unmarked record
        // shape and its strict numeric reading.
        let mut maintenance = publication();
        maintenance.basis = DecisionBasis::Maintenance {
            basis: "legacy-user-agreement".to_owned(),
        };
        let text = maintenance.record().expect("legacy numeric decision");
        assert!(!text.contains("basis="), "{text}");
        let records = parse_gate_comments(std::slice::from_ref(&text));
        assert_eq!(
            assess(&records, "codex-harness-pvr.5")
                .expect("attributable")
                .verdict,
            Verdict::Consistent
        );
    }

    #[test]
    fn v2_bindings_are_required_before_any_decision_reads_as_complete() {
        let complete = parse_gate_comments(&[v2_comment("item-a")]);
        assert_eq!(complete[0].version, 2);
        assert_eq!(complete[0].experiment.as_deref(), Some("exp-1"));
        assert_eq!(complete[0].revisions.as_deref(), Some("base123..cand456"));
        assert_eq!(complete[0].acceptance.as_deref(), Some("evidence-9"));
        assert_eq!(
            complete[0].coverage.as_deref(),
            Some("time+rounds+tool_ops")
        );
        assert_eq!(
            complete[0].scope.as_deref(),
            Some("task:synthetic.model:local")
        );
        assert_eq!(complete[0].reason.as_deref(), Some("lane-contention"));
        assert!(default_allowed(&complete, "item-a"));

        let cases: &[(&str, &str, Limitation)] = &[
            ("experiment=exp-1 ", "", Limitation::Experiment),
            ("revisions=base123..cand456 ", "", Limitation::Revisions),
            (
                "revisions=base123..cand456",
                "revisions=base123",
                Limitation::Revisions,
            ),
            (
                "revisions=base123..cand456",
                "revisions=base123..base123",
                Limitation::RevisionsNotDistinct,
            ),
            ("acceptance=evidence-9 ", "", Limitation::Acceptance),
            ("coverage=time+rounds+tool_ops ", "", Limitation::Coverage),
            ("scope=task:synthetic.model:local ", "", Limitation::Scope),
            ("reason=lane-contention ", "", Limitation::Reason),
        ];
        for (from, to, limitation) in cases {
            let records = parse_gate_comments(&[v2_variant("item-a", &[(from, to)])]);
            let assessment = assess(&records, "item-a").expect("attributable");
            assert_eq!(assessment.verdict, Verdict::Unsupported, "{from} -> {to}");
            assert!(
                assessment.limitations.contains(limitation),
                "{from} -> {to}: {:?}",
                assessment.limitations
            );
            assert!(!default_allowed(&records, "item-a"), "{from} -> {to}");
        }
    }

    #[test]
    fn v2_non_adoptions_need_bindings_and_never_revive_an_older_adoption() {
        let rejected = v2_variant(
            "item-a",
            &[
                ("outcome=adopt", "outcome=reject"),
                ("quality=unchanged", "quality=regressed"),
                ("candidate_seconds=99.0", "candidate_seconds=140.0"),
                ("regression_percent=-1.0", "regression_percent=40.0"),
            ],
        );
        let records = parse_gate_comments(std::slice::from_ref(&rejected));
        let assessment = assess(&records, "item-a").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::NonAdoption);
        assert!(assessment.limitations.is_empty());
        assert!(!default_allowed(&records, "item-a"));

        let unbound = v2_variant(
            "item-a",
            &[
                ("experiment=exp-1 ", ""),
                ("outcome=adopt", "outcome=reject"),
            ],
        );
        let unbound = parse_gate_comments(&[unbound]);
        let assessment = assess(&unbound, "item-a").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::Unsupported);
        assert!(assessment.limitations.contains(&Limitation::Experiment));

        // A malformed newer v2 record does not fall back to the older v1
        // adoption: the newest attributable record controls.
        let broken = v2_variant("item-a", &[("experiment=exp-1 ", "")]);
        let records = parse_gate_comments(&[comment("item-a", "adopt", "unchanged"), broken]);
        assert!(!default_allowed(&records, "item-a"));
        assert_eq!(
            assess(&records, "item-a").expect("attributable").recorded,
            2
        );
    }

    fn publication() -> DecisionDraft {
        DecisionDraft {
            item: "codex-harness-pvr.5".to_owned(),
            experiment: "exp-1".to_owned(),
            outcome: DecisionOutcome::Adopt,
            quality: QualityOutcome::Unchanged,
            basis: DecisionBasis::Efficiency,
            matched: 2,
            tolerance_percent: 10.0,
            baseline_seconds: 100.0,
            candidate_seconds: 99.0,
            baseline_arm: "direct".to_owned(),
            candidate_arm: "lane".to_owned(),
            accounting: "check+coordination+rework".to_owned(),
            baseline_revision: "base123".to_owned(),
            candidate_revision: "cand456".to_owned(),
            acceptance: "evidence-9".to_owned(),
            coverage: "time+rounds+tool_ops".to_owned(),
            scope: "task:synthetic.model:local".to_owned(),
            reason: "lane-contention".to_owned(),
            detail: Some("measured over two matched tasks".to_owned()),
            baseline_tokens: None,
            candidate_tokens: None,
            cumulative_baseline_tokens: None,
            cumulative_candidate_tokens: None,
            cumulative_baseline: None,
            payback_mechanism: None,
            payback_measures: None,
            payback_horizon: None,
            payback_evidence: None,
        }
    }

    #[test]
    fn published_decisions_round_trip_through_the_reader() {
        let draft = publication();
        let text = draft.record().expect("complete decision");
        assert!(text.starts_with(GATE_PREFIX_V2));
        let records = parse_gate_comments(std::slice::from_ref(&text));
        let assessment = assess(&records, "codex-harness-pvr.5").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::Consistent);
        for field in [
            assessment.latest.experiment.as_deref(),
            assessment.latest.revisions.as_deref(),
            assessment.latest.acceptance.as_deref(),
            assessment.latest.coverage.as_deref(),
            assessment.latest.scope.as_deref(),
            assessment.latest.reason.as_deref(),
        ] {
            assert!(field.is_some(), "binding missing after round trip");
        }
        assert_eq!(
            head_of(&text),
            head_of(&draft.record().expect("same decision")),
            "a retry writes an identical head"
        );

        let mut incomplete = draft.clone();
        incomplete.acceptance = String::new();
        let error = incomplete.record().unwrap_err();
        assert!(error.to_string().contains("acceptance is required"));

        let mut identical = draft.clone();
        identical.candidate_revision = identical.baseline_revision.clone();
        let error = identical.record().unwrap_err();
        assert!(error.to_string().contains("identical"));

        let mut inert = draft;
        inert.candidate_seconds = 104.0;
        let text = inert.record().expect("numerically valid record");
        let records = parse_gate_comments(std::slice::from_ref(&text));
        let assessment = assess(&records, "codex-harness-pvr.5").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::Unsupported);
        assert_eq!(
            assessment.limitations,
            vec![Limitation::ToleratedRegressionNotBenefit],
            "an unchanged-quality adoption that is slower is not a positive effect"
        );
    }

    #[test]
    fn payback_measures_must_be_observable_returns() {
        let mut draft = publication();
        draft.basis = DecisionBasis::EngineeringJudgment {
            basis: "reflection:lead-1".to_owned(),
        };
        draft.baseline_tokens = Some(1000);
        draft.candidate_tokens = Some(1100);
        draft.cumulative_baseline_tokens = Some(1000);
        draft.cumulative_candidate_tokens = Some(1100);
        draft.cumulative_baseline = Some("base123".to_owned());
        draft.payback_mechanism = Some("fewer-corrections".to_owned());
        draft.payback_measures = Some("usage".to_owned());
        draft.payback_horizon = Some(3);
        draft.payback_evidence = Some("evidence-9".to_owned());
        draft.coverage = "time+rounds+tool_ops+usage".to_owned();
        let error = draft.record().unwrap_err();
        assert!(error.to_string().contains("observable return dimension"));

        draft.payback_measures = Some("elapsed".to_owned());
        let error = draft.record().unwrap_err();
        assert!(error.to_string().contains("observable return dimension"));

        let legacy = v2_variant(
            &judgment_v2("item-a"),
            &[
                (
                    "baseline_tokens=1000 candidate_tokens=1000",
                    "baseline_tokens=1000 candidate_tokens=1100",
                ),
                (
                    "cumulative_baseline_tokens=1000 cumulative_candidate_tokens=1000",
                    "cumulative_baseline_tokens=1000 cumulative_candidate_tokens=1100",
                ),
                (
                    "cumulative_baseline=base123",
                    "cumulative_baseline=base123 payback_mechanism=fewer-corrections payback_measures=usage payback_horizon=3 payback_evidence=evidence-9",
                ),
                (
                    "coverage=time+rounds+tool_ops",
                    "coverage=time+rounds+tool_ops+usage",
                ),
            ],
        );
        let records = parse_gate_comments(&[legacy]);
        assert!(
            limitations_of(&records, "item-a").contains(&Limitation::PaybackMeasures),
            "{:?}",
            limitations_of(&records, "item-a")
        );

        draft.outcome = DecisionOutcome::Reject;
        draft.payback_mechanism = None;
        draft.payback_measures = None;
        draft.payback_horizon = None;
        draft.payback_evidence = None;
        let text = draft
            .record()
            .expect("a rejection records increased cost without payback");
        let records = parse_gate_comments(std::slice::from_ref(&text));
        assert_eq!(
            assess(&records, "codex-harness-pvr.5")
                .expect("attributable")
                .verdict,
            Verdict::NonAdoption
        );
    }

    fn non_adoption() -> NonAdoptionDraft {
        NonAdoptionDraft {
            item: "codex-harness-pvr.5".to_owned(),
            experiment: "exp-1".to_owned(),
            outcome: DecisionOutcome::Reject,
            quality: QualityOutcome::Unmeasurable,
            accounting: "attempts:2,tasks:1,accepted:0,per_success:undefined".to_owned(),
            baseline_revision: "base123".to_owned(),
            candidate_revision: "cand456".to_owned(),
            acceptance: "outcome-oracle:private-request".to_owned(),
            coverage: "time".to_owned(),
            scope: "task:workload-b.model:local".to_owned(),
            reason: "independent_acceptance_failed:cand-1".to_owned(),
            detail: Some("the candidate solution failed the frozen checker".to_owned()),
        }
    }

    #[test]
    fn non_adoptions_bind_their_verdict_without_fabricated_measurements() {
        let draft = non_adoption();
        let text = draft.record().expect("a bounded non-adoption record");
        assert!(text.starts_with(GATE_PREFIX_V2));
        assert!(!text.contains("matched="), "{text}");
        assert!(!text.contains("baseline_seconds="), "{text}");
        assert!(!text.contains("regression_percent="), "{text}");
        let records = parse_gate_comments(std::slice::from_ref(&text));
        let assessment = assess(&records, "codex-harness-pvr.5").expect("attributable");
        assert_eq!(assessment.verdict, Verdict::NonAdoption);
        assert!(assessment.limitations.is_empty());
        assert!(!default_allowed(&records, "codex-harness-pvr.5"));

        let mut inconclusive = draft.clone();
        inconclusive.outcome = DecisionOutcome::Inconclusive;
        let records = parse_gate_comments(&[inconclusive.record().unwrap()]);
        assert_eq!(
            assess(&records, "codex-harness-pvr.5").unwrap().verdict,
            Verdict::NonAdoption
        );

        // A non-adoption can never be rendered as an adoption, and every
        // binding field stays required.
        let mut adopting = draft.clone();
        adopting.outcome = DecisionOutcome::Adopt;
        assert!(adopting.record().is_err());
        for mutate in [
            |draft: &mut NonAdoptionDraft| draft.experiment.clear(),
            |draft: &mut NonAdoptionDraft| draft.acceptance.clear(),
            |draft: &mut NonAdoptionDraft| draft.coverage.clear(),
            |draft: &mut NonAdoptionDraft| draft.scope.clear(),
            |draft: &mut NonAdoptionDraft| draft.reason.clear(),
            |draft: &mut NonAdoptionDraft| draft.accounting.clear(),
            |draft: &mut NonAdoptionDraft| {
                draft.candidate_revision = draft.baseline_revision.clone()
            },
        ] {
            let mut broken = draft.clone();
            mutate(&mut broken);
            assert!(
                broken.record().is_err(),
                "an incomplete non-adoption must be refused"
            );
        }

        // The newest non-adoption supersedes an older adoption: an item with a
        // rejected experiment is not adoptable.
        let records =
            parse_gate_comments(&[publication().record().unwrap(), draft.record().unwrap()]);
        assert!(!default_allowed(&records, "codex-harness-pvr.5"));
    }
}
