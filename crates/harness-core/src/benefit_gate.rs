//! Benefit gate for promoted orchestration improvements.
//!
//! A promoted improvement becomes a default only after a matched comparison
//! shows unchanged-or-better quality and no delivery-time regression beyond
//! the declared tolerance. Delivery time is accounted per arm as the measured
//! check time plus coordination plus rework, so the comparison cannot win by
//! pushing work outside the measurement. The lead records the comparison as a
//! native `benefit-gate v1` board comment (see `.agents/skills/board-workflow`);
//! this module reads those recorded comparisons back.
//!
//! The reader separates the recorded decision from the recorded comparison
//! evidence. Every attributable record is retained - including incomplete,
//! contradictory or malformed ones - so a newer record is never silently
//! skipped in favor of an older adoption. [`assess`] classifies the newest
//! attributable record; [`default_allowed`] is true only for an adoption whose
//! recorded comparison is complete, arithmetically consistent and within its
//! declared tolerance. These checks validate what the record says; they are
//! not independent execution and not proof of the underlying experiment.

pub const GATE_PREFIX: &str = "benefit-gate v1";

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

    fn parse(value: &str) -> Option<Self> {
        match value {
            "unchanged" => Some(Self::Unchanged),
            "improved" => Some(Self::Improved),
            "regressed" => Some(Self::Regressed),
            "unmeasurable" => Some(Self::Unmeasurable),
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
}

impl Limitation {
    /// The stable phrase the ledger prints for this limitation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OutcomeAbsent => "no outcome recorded",
            Self::OutcomeUnrecognized => "outcome is not adopt, reject or inconclusive",
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

/// Parses `benefit-gate v1` board comments. Only the comment prefix and a
/// non-empty `item=` value decide attribution: a comment naming an item is
/// retained even when its decision or comparison fields are missing or
/// malformed, so a newer record supersedes an older adoption rather than
/// disappearing. The documented `detail=` tail stays on the board comment and
/// is not read back here.
pub fn parse_gate_comments(comments: &[String]) -> Vec<GateRecord> {
    comments
        .iter()
        .filter_map(|comment| {
            let rest = comment.strip_prefix(GATE_PREFIX)?.trim();
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
                    _ => {}
                }
            }
            let item = item.filter(|value| !value.is_empty())?;
            Some(GateRecord {
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
/// comparison supports. A missing, incomplete, contradictory, malformed or
/// non-adoption record leaves the improvement unadopted.
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
    match outcome {
        "reject" | "inconclusive" => return (Verdict::NonAdoption, Vec::new()),
        "adopt" => {}
        _ => return (Verdict::Unreadable, vec![Limitation::OutcomeUnrecognized]),
    }

    let mut limitations = Vec::new();
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

    if limitations.is_empty() {
        (Verdict::Consistent, limitations)
    } else {
        (Verdict::Unsupported, limitations)
    }
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
        assert!(
            default_allowed(&parse_gate_comments(&[boundary]), "item-a"),
            "a zero-tolerance record with no regression is consistent"
        );
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
        assert!(
            default_allowed(&parse_gate_comments(&[within]), "item-a"),
            "a regression inside the declared tolerance remains consistent"
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
}
