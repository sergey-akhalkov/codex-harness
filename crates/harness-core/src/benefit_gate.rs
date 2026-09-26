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
//! An inconclusive or rejected result never marks the improvement as a proven
//! default, and an item with no comparison recorded stays unadopted:
//! [`default_allowed`] accepts only the latest adopted record for the item.

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

/// The parsed board record of a gate decision: the item it gates, the
/// recorded outcome and the measured quality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateRecord {
    pub item: String,
    pub outcome: String,
    pub quality: QualityOutcome,
}

/// Parses `benefit-gate v1` board comments. The documented record also carries
/// the matched tasks, the declared tolerance and the accounting basis before
/// its `detail=` field; only item, outcome and quality are read back here, and
/// a record missing any of them is skipped rather than guessed.
pub fn parse_gate_comments(comments: &[String]) -> Vec<GateRecord> {
    comments
        .iter()
        .filter_map(|comment| {
            let rest = comment.strip_prefix(GATE_PREFIX)?.trim();
            let head = rest.split_once(" detail=").map_or(rest, |(head, _)| head);
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
        .rfind(|record| record.item == item)
        .is_some_and(|record| record.outcome == "adopt")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One documented `benefit-gate v1` record as the board-workflow skill
    /// defines it, including the fields the ledger does not read back.
    fn comment(item: &str, outcome: &str, quality: &str) -> String {
        format!(
            "benefit-gate v1 item={item} improvement=lane-reuse outcome={outcome} quality={quality} matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=99.0 regression_percent=-1.0 baseline=direct candidate=lane accounting=check+coordination+rework detail=measured over two matched tasks"
        )
    }

    #[test]
    fn documented_records_parse_and_incomplete_or_unknown_ones_are_skipped() {
        let comments = vec![
            comment("codex-harness-pvr.5", "adopt", "unchanged"),
            "benefit-gate v1 item=codex-harness-pvr.5 outcome=adopt".to_owned(),
            "benefit-gate v1 item=codex-harness-pvr.5 outcome=adopt quality=recovered".to_owned(),
            "pacing-decision v1 id=gpt:concurrency scope=gpt knob=concurrency from=4 to=1 expires_at=none reason=pressure basis=dashboard".to_owned(),
        ];
        let parsed = parse_gate_comments(&comments);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].item, "codex-harness-pvr.5");
        assert_eq!(parsed[0].outcome, "adopt");
        assert_eq!(parsed[0].quality, QualityOutcome::Unchanged);
        assert_eq!(parsed[0].quality.as_str(), "unchanged");
    }

    #[test]
    fn only_the_latest_record_for_an_item_is_a_proven_default() {
        let adopted = comment("item-a", "adopt", "improved");
        let rejected = comment("item-a", "reject", "regressed");
        let inconclusive = comment("item-a", "inconclusive", "unmeasurable");
        let other = comment("item-b", "adopt", "unchanged");

        assert!(
            !default_allowed(&[], "item-a"),
            "missing evidence stays unadopted"
        );
        let other_only = parse_gate_comments(&[other]);
        assert!(default_allowed(&other_only, "item-b"));
        assert!(!default_allowed(&other_only, "item-a"));

        let withdrawn = parse_gate_comments(&[adopted.clone(), rejected]);
        assert!(
            !default_allowed(&withdrawn, "item-a"),
            "a later rejection withdraws the default"
        );

        let readopted = parse_gate_comments(&[inconclusive, adopted]);
        assert!(
            default_allowed(&readopted, "item-a"),
            "a later adoption restores the default"
        );
    }
}
