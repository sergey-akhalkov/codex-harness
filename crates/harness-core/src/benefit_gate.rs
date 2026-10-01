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
//! recorded comparison is complete, arithmetically consistent, within its
//! declared tolerance and records a positive effect: an improved quality
//! outcome or a reduced candidate time. A record that stays within tolerance
//! only because quality is unchanged and the candidate is not faster - still
//! slower, or exactly at baseline - is a consistent record, not a demonstrated
//! improvement, so it cannot authorize a default.
//!
//! A `benefit-gate v2` decision binds the verdict to the experiment identity
//! and its exact revisions, the acceptance evidence, measured metric coverage,
//! the decision scope and the reason; a v2 record missing or contradicting any
//! of those bindings is retained and reported but can never authorize an
//! adoption. These checks validate what the record says; they are not
//! independent execution and not proof of the underlying experiment.

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
}

impl GateRecord {
    /// The recorded quality when it is one of the four documented outcomes.
    pub fn quality_outcome(&self) -> Option<QualityOutcome> {
        QualityOutcome::parse(self.quality.as_deref()?)
    }
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
    ToleratedRegressionNotBenefit,
    NoPositiveEffect,
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
            Self::ToleratedRegressionNotBenefit => {
                "candidate slower within tolerance: nonregression is not a positive effect"
            }
            Self::NoPositiveEffect => "no positive quality or time effect recorded",
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
            })
        })
        .collect()
}

/// The assessment of the newest record attributable to the item, or `None`
/// when no retained record names it.
pub fn assess<'a>(records: &'a [GateRecord], item: &str) -> Option<Assessment<'a>> {
    let latest = records.iter().rfind(|record| record.item == item)?;
    let recorded = records.iter().filter(|record| record.item == item).count();
    let (verdict, limitations) = classify(latest);
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
        } else if tolerance.is_some_and(|tolerance| computed > tolerance) {
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
///   refused, and so is a `reject`/`inconclusive` missing its bindings.
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
