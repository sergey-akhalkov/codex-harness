//! Pacing decisions from scoped account observations.
//!
//! Pacing shapes *new* work only: assignment admission, supported concurrency,
//! reasoning effort and feedback cadence. A healthy executor already running
//! keeps its slot, its model and its instruction. Every deviation from the
//! configured limits carries a visible reason and the observation it was
//! derived from, expires with that observation, and can be withdrawn
//! explicitly, so pacing is inspectable and reversible. There is no background
//! scheduler: the lead records a native `pacing-decision v1` or
//! `pacing-revoke v1` board comment at a decision boundary (see
//! `.agents/skills/board-workflow`), and this module reports the decisions
//! that still apply.

const DECISION_PREFIX: &str = "pacing-decision v1";
const REVOKE_PREFIX: &str = "pacing-revoke v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacingKnob {
    NewAssignments,
    Concurrency,
    Effort,
    FeedbackCadence,
}

impl PacingKnob {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NewAssignments => "new-assignments",
            Self::Concurrency => "concurrency",
            Self::Effort => "effort",
            Self::FeedbackCadence => "feedback-cadence",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "new-assignments" => Some(Self::NewAssignments),
            "concurrency" => Some(Self::Concurrency),
            "effort" => Some(Self::Effort),
            "feedback-cadence" => Some(Self::FeedbackCadence),
            _ => None,
        }
    }
}

/// One inspectable pacing change with the observation it came from and the
/// moment it stops applying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacingDecision {
    pub scope: String,
    pub knob: PacingKnob,
    pub from: String,
    pub to: String,
    pub reason: String,
    pub basis: String,
    pub expires_at: Option<u64>,
}

impl PacingDecision {
    pub fn id(&self) -> String {
        format!("{}:{}", self.scope, self.knob.as_str())
    }

    pub fn to_comment(&self) -> String {
        format!(
            "{DECISION_PREFIX} id={} scope={} knob={} from={} to={} expires_at={} reason={} basis={}",
            self.id(),
            self.scope,
            self.knob.as_str(),
            self.from,
            self.to,
            self.expires_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_owned()),
            self.reason,
            self.basis
        )
    }

    fn parse(comment: &str) -> Option<Self> {
        let rest = comment.strip_prefix(DECISION_PREFIX)?.trim();
        let head = rest.split(" reason=").next()?;
        let tail = rest.split(" reason=").nth(1)?;
        let (reason, basis) = tail.split_once(" basis=").unwrap_or((tail, ""));
        let mut scope = None;
        let mut knob = None;
        let mut from = None;
        let mut to = None;
        let mut expires_at = None;
        for part in head.split_whitespace() {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            match key {
                "scope" => scope = Some(value.to_owned()),
                "knob" => knob = PacingKnob::parse(value),
                "from" => from = Some(value.to_owned()),
                "to" => to = Some(value.to_owned()),
                "expires_at" => {
                    expires_at = match value {
                        "none" => None,
                        _ => Some(value.parse().ok()?),
                    }
                }
                _ => {}
            }
        }
        Some(Self {
            scope: scope?,
            knob: knob?,
            from: from?,
            to: to?,
            reason: reason.to_owned(),
            basis: basis.to_owned(),
            expires_at,
        })
    }
}

/// Recorded decisions and withdrawals read back from a board item.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PacingRecords {
    pub decisions: Vec<PacingDecision>,
    pub revoked: Vec<String>,
}

pub fn parse_pacing_comments(comments: &[String]) -> PacingRecords {
    let mut records = PacingRecords::default();
    for comment in comments {
        if let Some(decision) = PacingDecision::parse(comment) {
            records.decisions.push(decision);
        } else if let Some(rest) = comment.strip_prefix(REVOKE_PREFIX)
            && let Some(id) = rest
                .split_whitespace()
                .find_map(|part| part.strip_prefix("id="))
        {
            records.revoked.push(id.to_owned());
        }
    }
    records
}

/// Decisions that still apply: not withdrawn and not expired with their basis.
pub fn applicable(records: &PacingRecords, now: u64) -> Vec<&PacingDecision> {
    records
        .decisions
        .iter()
        .filter(|decision| !records.revoked.contains(&decision.id()))
        .filter(|decision| {
            decision
                .expires_at
                .is_none_or(|expires_at| expires_at > now)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_700_000_000;
    const DECISION: &str = "pacing-decision v1 id=gpt:concurrency scope=gpt knob=concurrency from=4 to=1 expires_at=none reason=pressure basis=dashboard";
    const EXPIRING: &str = "pacing-decision v1 id=gpt:effort scope=gpt knob=effort from=xhigh to=high expires_at=1700000000 reason=pressure basis=dashboard";

    #[test]
    fn documented_decisions_parse_with_their_fields_intact() {
        let records = parse_pacing_comments(&[DECISION.to_owned()]);
        assert_eq!(records.decisions.len(), 1);
        let decision = &records.decisions[0];
        assert_eq!(decision.id(), "gpt:concurrency");
        assert_eq!(decision.knob, PacingKnob::Concurrency);
        assert_eq!(decision.from, "4");
        assert_eq!(decision.to, "1");
        assert_eq!(decision.expires_at, None);
        assert_eq!(decision.reason, "pressure");
        assert_eq!(decision.basis, "dashboard");
        assert_eq!(decision.to_comment(), DECISION);
        assert_eq!(PacingKnob::FeedbackCadence.as_str(), "feedback-cadence");

        let expiring = parse_pacing_comments(&[EXPIRING.to_owned()]);
        assert_eq!(expiring.decisions[0].expires_at, Some(NOW));

        // The decision id is derived from scope and knob; a record without an
        // `id=` field still parses under its derived identity.
        let derived = parse_pacing_comments(&[
            "pacing-decision v1 scope=gpt knob=effort from=a to=b expires_at=none reason=p basis=d"
                .to_owned(),
        ]);
        assert_eq!(derived.decisions[0].id(), "gpt:effort");

        for skipped in [
            "pacing-decision v1 id=gpt:effort scope=gpt knob=provider-custom from=a to=b expires_at=none reason=p basis=d",
            "pacing-decision v1 id=gpt:effort scope=gpt knob=effort expires_at=none reason=p basis=d",
            "pacing-decision v1 id=gpt:effort scope=gpt knob=effort from=a expires_at=none reason=p basis=d",
            "unrelated comment",
        ] {
            assert!(
                parse_pacing_comments(&[skipped.to_owned()])
                    .decisions
                    .is_empty(),
                "{skipped}"
            );
        }
    }

    #[test]
    fn withdrawals_and_expiry_remove_a_decision_from_the_applicable_set() {
        let records = parse_pacing_comments(&[DECISION.to_owned(), EXPIRING.to_owned()]);
        assert_eq!(
            applicable(&records, NOW - 1).len(),
            2,
            "both decisions still apply before the expiry second"
        );
        let live = applicable(&records, NOW);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id(), "gpt:concurrency");

        let withdrawn = vec![
            DECISION.to_owned(),
            EXPIRING.to_owned(),
            "pacing-revoke v1 id=gpt:concurrency reason=lead restored the limit".to_owned(),
        ];
        let records = parse_pacing_comments(&withdrawn);
        assert_eq!(records.revoked, vec!["gpt:concurrency".to_owned()]);
        assert_eq!(applicable(&records, NOW - 1).len(), 1);
        assert!(applicable(&records, NOW).is_empty());
        assert!(applicable(&records, NOW - 1)[0].id() != "gpt:concurrency");
    }

    #[test]
    fn board_and_lead_skills_document_pacing_and_the_benefit_gate() {
        let board = include_str!("../../../.agents/skills/board-workflow/SKILL.md");
        let lead = include_str!("../../../.agents/skills/team-lead/SKILL.md");
        assert!(board.contains("pacing-observation v1"));
        assert!(board.contains("pacing-decision v1"));
        assert!(board.contains("pacing-revoke v1"));
        assert!(board.contains("benefit-gate v1"));
        assert!(board.contains("accounting=check+coordination+rework"));
        assert!(board.contains("applied to the next boundary") || board.contains("next boundary"));
        assert!(lead.contains("never preempted"));
        assert!(lead.contains("Unknown stays unknown"));
        assert!(lead.contains("synchronized burst"));
        assert!(lead.contains("board-workflow"));
        assert!(lead.contains("unadopted"));
    }
}
