//! Benefit gate for promoted orchestration improvements.
//!
//! A promoted improvement becomes a default only after a matched comparison
//! shows unchanged-or-better quality and no delivery-time regression beyond
//! the declared tolerance. Delivery time is accounted per arm as the measured
//! check time plus coordination plus rework, so the comparison cannot win by
//! pushing work outside the measurement. The gate evaluates orchestration
//! defaults; it is not the skill-evaluation contract for library mutations.
//!
//! An inconclusive or rejected result never marks the improvement as a proven
//! default: [`default_allowed`] accepts only the latest adopted record for the
//! item.
use std::io;

pub const GATE_PREFIX: &str = "benefit-gate v1";
/// Declared tolerance when a comparison does not declare its own.
pub const DEFAULT_TOLERANCE_PERCENT: f64 = 10.0;
pub const MAX_TOLERANCE_PERCENT: f64 = 100.0;

/// One arm's measured delivery cost, in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmTiming {
    /// The owned check itself: build, test, lint or run.
    pub check_seconds: f64,
    /// Setup and handover the strategy adds: worktree creation, reset, merge.
    pub coordination_seconds: f64,
    /// Repeated work caused by an imperfect first delivery.
    pub rework_seconds: f64,
}

impl ArmTiming {
    pub fn new(
        check_seconds: f64,
        coordination_seconds: f64,
        rework_seconds: f64,
    ) -> io::Result<Self> {
        let timing = Self {
            check_seconds,
            coordination_seconds,
            rework_seconds,
        };
        if [check_seconds, coordination_seconds, rework_seconds]
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(invalid("arm timings must be finite and non-negative"));
        }
        Ok(timing)
    }

    pub fn total(&self) -> f64 {
        self.check_seconds + self.coordination_seconds + self.rework_seconds
    }
}

/// One matched task executed under both strategies.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchedTask {
    pub task: String,
    pub baseline_passed: bool,
    pub candidate_passed: bool,
    pub baseline: ArmTiming,
    pub candidate: ArmTiming,
}

/// The honest record of a matched comparison: what was compared, how, and the
/// numbers. It is written to the board so the outcome is inspectable.
#[derive(Debug, Clone, PartialEq)]
pub struct ComparisonRecord {
    /// Promoted improvement this comparison gates (board id).
    pub item: String,
    pub improvement: String,
    pub baseline_label: String,
    pub candidate_label: String,
    /// Declared before the run; a regression beyond it fails the gate.
    pub tolerance_percent: f64,
    pub matched: Vec<MatchedTask>,
}

impl ComparisonRecord {
    pub fn bounded(self) -> io::Result<Self> {
        require_token("item", &self.item, 64)?;
        require_token("improvement", &self.improvement, 64)?;
        require_token("baseline_label", &self.baseline_label, 40)?;
        require_token("candidate_label", &self.candidate_label, 40)?;
        if !self.tolerance_percent.is_finite()
            || self.tolerance_percent < 0.0
            || self.tolerance_percent > MAX_TOLERANCE_PERCENT
        {
            return Err(invalid(format!(
                "tolerance_percent must be between 0 and {MAX_TOLERANCE_PERCENT}"
            )));
        }
        let mut tasks = std::collections::BTreeSet::new();
        for matched in &self.matched {
            require_token("task", &matched.task, 64)?;
            if !tasks.insert(matched.task.clone()) {
                return Err(invalid(format!(
                    "matched task {} is duplicated",
                    matched.task
                )));
            }
        }
        Ok(self)
    }

    pub fn baseline_seconds(&self) -> f64 {
        self.matched.iter().map(|item| item.baseline.total()).sum()
    }

    pub fn candidate_seconds(&self) -> f64 {
        self.matched.iter().map(|item| item.candidate.total()).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityOutcome {
    Unchanged,
    Improved,
    Regressed,
    /// At least one task failed under both strategies: nothing was measured.
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

#[derive(Debug, Clone, PartialEq)]
pub enum GateOutcome {
    Adopt {
        quality: QualityOutcome,
        baseline_seconds: f64,
        candidate_seconds: f64,
        regression_percent: f64,
    },
    Reject {
        quality: QualityOutcome,
        reason: String,
    },
    Inconclusive {
        quality: QualityOutcome,
        reason: String,
    },
}

impl GateOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Adopt { .. } => "adopt",
            Self::Reject { .. } => "reject",
            Self::Inconclusive { .. } => "inconclusive",
        }
    }

    pub fn quality(&self) -> QualityOutcome {
        match self {
            Self::Adopt { quality, .. } => *quality,
            Self::Reject { quality, .. } => *quality,
            Self::Inconclusive { quality, .. } => *quality,
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::Adopt { .. } => {
                "quality unchanged or better; no material delivery-time regression".to_owned()
            }
            Self::Reject { reason, .. } | Self::Inconclusive { reason, .. } => reason.clone(),
        }
    }
}

/// Quality measured by the comparison, independent of the timing verdict.
pub fn quality_outcome(record: &ComparisonRecord) -> QualityOutcome {
    let mut outcome = QualityOutcome::Unchanged;
    for matched in &record.matched {
        match (matched.baseline_passed, matched.candidate_passed) {
            (true, false) => return QualityOutcome::Regressed,
            (false, false) => outcome = QualityOutcome::Unmeasurable,
            (false, true) => {
                if outcome != QualityOutcome::Unmeasurable {
                    outcome = QualityOutcome::Improved;
                }
            }
            (true, true) => {}
        }
    }
    outcome
}

/// Evaluates one declared comparison. Deterministic; no model call and no
/// additional measurement happens here.
pub fn evaluate(record: &ComparisonRecord) -> GateOutcome {
    let quality = quality_outcome(record);
    if let Err(error) = record.clone().bounded() {
        return GateOutcome::Inconclusive {
            quality,
            reason: error.to_string(),
        };
    }
    if record.matched.is_empty() {
        return GateOutcome::Inconclusive {
            quality,
            reason: "no matched task was measured".to_owned(),
        };
    }
    for matched in &record.matched {
        match (matched.baseline_passed, matched.candidate_passed) {
            (true, false) => {
                return GateOutcome::Reject {
                    quality,
                    reason: format!(
                        "quality regressed on matched task {}: the baseline passed and the candidate failed",
                        matched.task
                    ),
                };
            }
            (false, false) => {
                return GateOutcome::Inconclusive {
                    quality,
                    reason: format!(
                        "quality was not measurable on matched task {}: both arms failed",
                        matched.task
                    ),
                };
            }
            (false, true) => {}
            (true, true) => {}
        }
        if matched.baseline.total() <= 0.0 {
            return GateOutcome::Inconclusive {
                quality,
                reason: format!(
                    "no baseline delivery time was measured on matched task {}",
                    matched.task
                ),
            };
        }
    }
    let baseline_seconds = record.baseline_seconds();
    let candidate_seconds = record.candidate_seconds();
    let regression_percent = (candidate_seconds - baseline_seconds) / baseline_seconds * 100.0;
    if regression_percent > record.tolerance_percent {
        return GateOutcome::Reject {
            quality,
            reason: format!(
                "delivery time regressed {:.1}% (baseline {:.1}s, candidate {:.1}s), beyond the declared tolerance of {:.1}%",
                regression_percent, baseline_seconds, candidate_seconds, record.tolerance_percent
            ),
        };
    }
    GateOutcome::Adopt {
        quality,
        baseline_seconds,
        candidate_seconds,
        regression_percent,
    }
}

/// The board record of one comparison. It states the matched tasks, the
/// declared tolerance and the accounting basis, so an adoption can be
/// reviewed later from the record alone.
pub fn format_gate_comment(record: &ComparisonRecord, outcome: &GateOutcome) -> String {
    let (baseline_seconds, candidate_seconds) = match outcome {
        GateOutcome::Adopt {
            baseline_seconds,
            candidate_seconds,
            ..
        } => (*baseline_seconds, *candidate_seconds),
        _ => (record.baseline_seconds(), record.candidate_seconds()),
    };
    format!(
        "{GATE_PREFIX} item={} improvement={} outcome={} quality={} matched={} tolerance_percent={:.1} baseline_seconds={:.1} candidate_seconds={:.1} regression_percent={:.1} baseline={} candidate={} accounting=check+coordination+rework detail={}",
        record.item,
        record.improvement,
        outcome.as_str(),
        outcome.quality().as_str(),
        record.matched.len(),
        record.tolerance_percent,
        baseline_seconds,
        candidate_seconds,
        regression_percent(outcome, baseline_seconds, candidate_seconds),
        record.baseline_label,
        record.candidate_label,
        outcome.detail().replace(' ', "_")
    )
}

fn regression_percent(outcome: &GateOutcome, baseline: f64, candidate: f64) -> f64 {
    match outcome {
        GateOutcome::Adopt {
            regression_percent, ..
        } => *regression_percent,
        _ if baseline > 0.0 => (candidate - baseline) / baseline * 100.0,
        _ => 0.0,
    }
}

/// The parsed board record of a gate decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRecord {
    pub item: String,
    pub outcome: String,
    pub quality: QualityOutcome,
}

pub fn parse_gate_comments(comments: &[String]) -> Vec<GateRecord> {
    comments
        .iter()
        .filter_map(|comment| {
            let rest = comment.strip_prefix(GATE_PREFIX)?.trim();
            let head = rest.split(" detail=").next()?;
            let mut item = None;
            let mut outcome = None;
            let mut quality = None;
            for part in head.split_whitespace() {
                let Some((key, value)) = part.split_once('=') else {
                    continue;
                };
                match key {
                    "item" => item = Some(value.to_owned()),
                    "outcome" => outcome = Some(value.to_owned()),
                    "quality" => quality = QualityOutcome::parse(value),
                    _ => {}
                }
            }
            Some(GateRecord {
                item: item?,
                outcome: outcome?,
                quality: quality?,
            })
        })
        .collect()
}

/// True only when the latest gate record for the item is an adoption. A
/// promoted improvement without a measured, adopted comparison stays
/// unadopted.
pub fn default_allowed(records: &[GateRecord], item: &str) -> bool {
    records
        .iter()
        .rev()
        .find(|record| record.item == item)
        .is_some_and(|record| record.outcome == "adopt")
}

/// Bounded intake for measured comparison evidence. The file is the lead's
/// private input: the pack holds the format, never the machine numbers.
///
/// ```text
/// item=codex-harness-pvr.5
/// improvement=lane-reuse-worktree
/// baseline=fresh-worktree
/// candidate=lane-reuse
/// tolerance_percent=10
/// matched=verify-crate-tests baseline=pass candidate=pass baseline_check=210.3 baseline_coordination=2.0 baseline_rework=0 candidate_check=0.5 candidate_coordination=3.0 candidate_rework=0
/// ```
pub fn parse_comparison_evidence(text: &str) -> io::Result<ComparisonRecord> {
    let mut item = None;
    let mut improvement = None;
    let mut baseline_label = None;
    let mut candidate_label = None;
    let mut tolerance_percent = None;
    let mut matched = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(invalid("comparison evidence line is missing '='"));
        };
        match key {
            "item" => item = Some(value.trim().to_owned()),
            "improvement" => improvement = Some(value.trim().to_owned()),
            "baseline" => baseline_label = Some(value.trim().to_owned()),
            "candidate" => candidate_label = Some(value.trim().to_owned()),
            "tolerance_percent" => {
                tolerance_percent = Some(
                    value
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| invalid("tolerance_percent is not a number"))?,
                )
            }
            "matched" => matched.push(parse_matched_task(value.trim())?),
            _ => return Err(invalid(format!("unknown comparison field {key}"))),
        }
    }
    ComparisonRecord {
        item: item.ok_or_else(|| invalid("item is required"))?,
        improvement: improvement.ok_or_else(|| invalid("improvement is required"))?,
        baseline_label: baseline_label.ok_or_else(|| invalid("baseline is required"))?,
        candidate_label: candidate_label.ok_or_else(|| invalid("candidate is required"))?,
        tolerance_percent: tolerance_percent
            .ok_or_else(|| invalid("tolerance_percent is required"))?,
        matched,
    }
    .bounded()
}

fn parse_matched_task(text: &str) -> io::Result<MatchedTask> {
    let mut parts = text.split_whitespace();
    let task = parts
        .next()
        .ok_or_else(|| invalid("matched task needs a name"))?
        .to_owned();
    let mut fields = std::collections::BTreeMap::new();
    for part in parts {
        let Some((key, value)) = part.split_once('=') else {
            return Err(invalid(format!("matched field {part} is missing '='")));
        };
        fields.insert(key.to_owned(), value.to_owned());
    }
    let passed = |key: &str| -> io::Result<bool> {
        match fields.get(key).map(String::as_str) {
            Some("pass") => Ok(true),
            Some("fail") => Ok(false),
            _ => Err(invalid(format!("matched field {key} must be pass or fail"))),
        }
    };
    let seconds = |key: &str| -> io::Result<f64> {
        fields
            .get(key)
            .ok_or_else(|| invalid(format!("matched field {key} is required")))?
            .parse::<f64>()
            .map_err(|_| invalid(format!("matched field {key} is not a number")))
    };
    Ok(MatchedTask {
        task,
        baseline_passed: passed("baseline")?,
        candidate_passed: passed("candidate")?,
        baseline: ArmTiming::new(
            seconds("baseline_check")?,
            seconds("baseline_coordination")?,
            seconds("baseline_rework")?,
        )?,
        candidate: ArmTiming::new(
            seconds("candidate_check")?,
            seconds("candidate_coordination")?,
            seconds("candidate_rework")?,
        )?,
    })
}

fn require_token(name: &str, value: &str, max: usize) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid(format!("{name} is required")));
    }
    if trimmed.len() > max {
        return Err(invalid(format!("{name} exceeds {max} bytes")));
    }
    if trimmed.bytes().any(|byte| {
        !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/' | b'@'))
    }) {
        return Err(invalid(format!("{name} contains unsupported characters")));
    }
    Ok(trimmed.to_owned())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(name: &str, passed: bool, baseline: f64, candidate: f64) -> MatchedTask {
        MatchedTask {
            task: name.to_owned(),
            baseline_passed: passed,
            candidate_passed: passed,
            baseline: ArmTiming::new(baseline, 2.0, 0.0).unwrap(),
            candidate: ArmTiming::new(candidate, 1.0, 0.0).unwrap(),
        }
    }

    fn record(matched: Vec<MatchedTask>) -> ComparisonRecord {
        ComparisonRecord {
            item: "codex-harness-pvr.5".into(),
            improvement: "lane-reuse".into(),
            baseline_label: "fresh-worktree".into(),
            candidate_label: "lane-reuse".into(),
            tolerance_percent: DEFAULT_TOLERANCE_PERCENT,
            matched,
        }
    }

    #[test]
    fn a_measured_win_with_unchanged_quality_is_adopted() {
        let record = record(vec![
            task("verify-crate-tests", true, 900.0, 640.0),
            task("verify-lint", true, 300.0, 20.0),
        ]);
        let outcome = evaluate(&record);
        match outcome {
            GateOutcome::Adopt {
                quality,
                baseline_seconds,
                candidate_seconds,
                regression_percent,
            } => {
                assert_eq!(quality, QualityOutcome::Unchanged);
                assert_eq!(baseline_seconds, 1204.0);
                assert_eq!(candidate_seconds, 660.0 + 2.0);
                assert!(regression_percent < 0.0);
            }
            other => panic!("expected an adoption, got {other:?}"),
        }
    }

    #[test]
    fn quality_regression_rejects_regardless_of_speed() {
        let mut matched = task("verify-crate-tests", true, 900.0, 100.0);
        matched.candidate_passed = false;
        let outcome = evaluate(&record(vec![matched]));
        match outcome {
            GateOutcome::Reject {
                quality, reason, ..
            } => {
                assert_eq!(quality, QualityOutcome::Regressed);
                assert!(reason.contains("quality regressed"));
                assert!(reason.contains("verify-crate-tests"));
            }
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    #[test]
    fn regression_beyond_the_declared_tolerance_is_rejected() {
        let record = record(vec![task("verify-crate-tests", true, 1000.0, 1200.0)]);
        match evaluate(&record) {
            GateOutcome::Reject { reason, .. } => {
                assert!(reason.contains("beyond the declared tolerance"));
            }
            other => panic!("expected a rejection, got {other:?}"),
        }
        let within = record.clone();
        let within = ComparisonRecord {
            matched: vec![task("verify-crate-tests", true, 1000.0, 1080.0)],
            ..within
        };
        assert!(matches!(evaluate(&within), GateOutcome::Adopt { .. }));
    }

    #[test]
    fn an_unmeasured_or_invalid_comparison_is_inconclusive() {
        assert!(matches!(
            evaluate(&record(Vec::new())),
            GateOutcome::Inconclusive { .. }
        ));
        let mut both_failed = task("verify-crate-tests", true, 900.0, 90.0);
        both_failed.baseline_passed = false;
        both_failed.candidate_passed = false;
        match evaluate(&record(vec![both_failed])) {
            GateOutcome::Inconclusive { reason, .. } => {
                assert!(reason.contains("not measurable"));
            }
            other => panic!("expected an inconclusive result, got {other:?}"),
        }
        let no_baseline = MatchedTask {
            task: "verify-crate-tests".into(),
            baseline_passed: true,
            candidate_passed: true,
            baseline: ArmTiming::new(0.0, 0.0, 0.0).unwrap(),
            candidate: ArmTiming::new(10.0, 1.0, 0.0).unwrap(),
        };
        match evaluate(&record(vec![no_baseline])) {
            GateOutcome::Inconclusive { reason, .. } => {
                assert!(reason.contains("no baseline delivery time"));
            }
            other => panic!("expected an inconclusive result, got {other:?}"),
        }
    }

    #[test]
    fn gate_records_round_trip_and_only_the_latest_adoption_is_a_default() {
        let record = record(vec![task("verify-crate-tests", true, 900.0, 200.0)]);
        let outcome = evaluate(&record);
        let comment = format_gate_comment(&record, &outcome);
        assert!(comment.contains("benefit-gate v1"));
        assert!(comment.contains("item=codex-harness-pvr.5"));
        assert!(comment.contains("outcome=adopt"));
        assert!(comment.contains("quality=unchanged"));
        assert!(comment.contains("accounting=check+coordination+rework"));
        let parsed = parse_gate_comments(std::slice::from_ref(&comment));
        assert_eq!(parsed.len(), 1);
        assert!(default_allowed(&parsed, "codex-harness-pvr.5"));
        assert!(!default_allowed(&parsed, "codex-harness-other"));
        assert!(!default_allowed(&[], "codex-harness-pvr.5"));

        let rejected = format_gate_comment(
            &record,
            &GateOutcome::Reject {
                quality: QualityOutcome::Unchanged,
                reason: "quality regressed".into(),
            },
        );
        let both = parse_gate_comments(&[comment.clone(), rejected.clone()]);
        assert!(
            !default_allowed(&both, "codex-harness-pvr.5"),
            "a later rejection withdraws the default"
        );
        let reversed = parse_gate_comments(&[rejected, comment]);
        assert!(default_allowed(&reversed, "codex-harness-pvr.5"));
    }

    #[test]
    fn an_inconclusive_outcome_never_becomes_a_proven_default() {
        let record = record(Vec::new());
        let outcome = evaluate(&record);
        assert!(matches!(outcome, GateOutcome::Inconclusive { .. }));
        let comment = format_gate_comment(&record, &outcome);
        assert!(comment.contains("outcome=inconclusive"));
        let parsed = parse_gate_comments(&[comment]);
        assert!(!default_allowed(&parsed, "codex-harness-pvr.5"));
    }

    #[test]
    fn timings_and_records_reject_unusable_input() {
        assert!(ArmTiming::new(-1.0, 0.0, 0.0).is_err());
        assert!(ArmTiming::new(f64::NAN, 0.0, 0.0).is_err());
        let duplicated = record(vec![
            task("verify-crate-tests", true, 900.0, 100.0),
            task("verify-crate-tests", true, 900.0, 100.0),
        ]);
        let error = duplicated.bounded().unwrap_err();
        assert!(error.to_string().contains("duplicated"));
    }

    #[test]
    fn comparison_evidence_intake_feeds_the_gate_and_rejects_bad_input() {
        let text = "\
item=codex-harness-pvr.5
improvement=lane-reuse-worktree
baseline=fresh-worktree
candidate=lane-reuse
tolerance_percent=10
matched=verify-crate-tests baseline=pass candidate=pass baseline_check=210.3 baseline_coordination=2.0 baseline_rework=0 candidate_check=0.5 candidate_coordination=3.0 candidate_rework=0
";
        let record = parse_comparison_evidence(text).unwrap();
        assert_eq!(record.matched.len(), 1);
        assert_eq!(record.baseline_seconds(), 212.3);
        assert_eq!(record.candidate_seconds(), 3.5);
        let outcome = evaluate(&record);
        assert!(matches!(outcome, GateOutcome::Adopt { .. }));
        let comments = [format_gate_comment(&record, &outcome)];
        assert!(default_allowed(
            &parse_gate_comments(&comments),
            "codex-harness-pvr.5"
        ));

        assert!(parse_comparison_evidence("item=only-an-item\n").is_err());
        assert!(
            parse_comparison_evidence(
                "item=x\nimprovement=y\nbaseline=a\ncandidate=b\ntolerance_percent=ten\n"
            )
            .is_err()
        );
        assert!(parse_comparison_evidence("not a field line\n").is_err());
        assert!(
            parse_comparison_evidence(
                "item=x\nimprovement=y\nbaseline=a\ncandidate=b\ntolerance_percent=10\nmatched=t baseline=maybe candidate=pass baseline_check=1 baseline_coordination=0 baseline_rework=0 candidate_check=1 candidate_coordination=0 candidate_rework=0\n"
            )
            .is_err()
        );
    }

    #[test]
    #[ignore = "requires HARNESS_PACING_COMPARISON pointing at a private evidence file; prints the gate record"]
    fn live_comparison_gate_from_measured_evidence() {
        let path = std::path::PathBuf::from(
            std::env::var_os("HARNESS_PACING_COMPARISON").expect("HARNESS_PACING_COMPARISON"),
        );
        let text = std::fs::read_to_string(&path).unwrap();
        let record = parse_comparison_evidence(&text).unwrap();
        let outcome = evaluate(&record);
        let comment = format_gate_comment(&record, &outcome);
        println!("{comment}");
        println!(
            "proven default for {}: {}",
            record.item,
            default_allowed(&parse_gate_comments(&[comment]), &record.item)
        );
    }
}
