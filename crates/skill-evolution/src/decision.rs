//! Fixed accept / reject / inconclusive rules. Not a proof that a skill is useful.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Accept,
    Reject,
    Inconclusive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Claim {
    Capability,
    NetSavings,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonEvidence {
    pub integrity_ok: bool,
    pub evidence_complete: bool,
    pub provider_matched: bool,
    pub must_pass: bool,
    pub selection_demonstrated: bool,
    pub protected_regression: bool,
    pub benefit_established: bool,
    pub within_budgets: bool,
    pub claim: Claim,
    pub skipped_required_check: bool,
    pub single_lucky_run: bool,
    pub meaningful_difference: bool,
}

pub fn decide(evidence: &ComparisonEvidence) -> Verdict {
    if !evidence.integrity_ok || evidence.protected_regression || evidence.skipped_required_check {
        return Verdict::Reject;
    }
    if !evidence.evidence_complete || !evidence.provider_matched {
        return Verdict::Inconclusive;
    }
    if !evidence.must_pass || !evidence.selection_demonstrated || !evidence.within_budgets {
        return Verdict::Inconclusive;
    }
    if evidence.single_lucky_run || !evidence.meaningful_difference {
        return Verdict::Inconclusive;
    }
    if evidence.benefit_established {
        Verdict::Accept
    } else {
        Verdict::Inconclusive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ComparisonEvidence {
        ComparisonEvidence {
            integrity_ok: true,
            evidence_complete: true,
            provider_matched: true,
            must_pass: true,
            selection_demonstrated: true,
            protected_regression: false,
            benefit_established: true,
            within_budgets: true,
            claim: Claim::Capability,
            skipped_required_check: false,
            single_lucky_run: false,
            meaningful_difference: true,
        }
    }

    #[test]
    fn three_decisions_are_unambiguous_on_example_data() {
        assert_eq!(decide(&base()), Verdict::Accept);
        let mut reject = base();
        reject.integrity_ok = false;
        assert_eq!(decide(&reject), Verdict::Reject);
        let mut incomplete = base();
        incomplete.evidence_complete = false;
        incomplete.benefit_established = false;
        assert_eq!(decide(&incomplete), Verdict::Inconclusive);
    }

    #[test]
    fn cost_and_capability_claims_are_distinct_and_quota_failure_is_inconclusive() {
        let capability = base();
        assert_eq!(capability.claim, Claim::Capability);
        let mut savings = base();
        savings.claim = Claim::NetSavings;
        savings.benefit_established = false;
        assert_eq!(decide(&savings), Verdict::Inconclusive);
        assert_ne!(savings.claim, capability.claim);
        let mut quota = base();
        quota.provider_matched = false;
        quota.evidence_complete = false;
        quota.benefit_established = false;
        assert_eq!(decide(&quota), Verdict::Inconclusive);
    }

    #[test]
    fn a_shortcut_without_must_pass_cannot_accept() {
        let mut skipped = base();
        skipped.skipped_required_check = true;
        assert_eq!(decide(&skipped), Verdict::Reject);
        let mut lucky = base();
        lucky.single_lucky_run = true;
        assert_eq!(decide(&lucky), Verdict::Inconclusive);
    }
}
