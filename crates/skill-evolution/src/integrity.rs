//! Deterministic integrity for skill comparisons. Not skill-text usefulness.

use crate::decision::{Claim, ComparisonEvidence, Verdict, decide};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Integrity {
    pub required_skill_use: bool,
    pub absence_expected: bool,
    pub oracle_mutated: bool,
    pub self_report_only: bool,
    pub revision_matches: bool,
}

pub fn selection_ok(integrity: &Integrity) -> bool {
    if integrity.absence_expected {
        !integrity.required_skill_use
    } else {
        integrity.required_skill_use && !integrity.self_report_only
    }
}

pub fn evidence(integrity: &Integrity, within_budgets: bool, claim: Claim) -> ComparisonEvidence {
    let ok = selection_ok(integrity)
        && !integrity.oracle_mutated
        && integrity.revision_matches
        && !integrity.self_report_only;
    ComparisonEvidence {
        integrity_ok: !integrity.oracle_mutated && integrity.revision_matches,
        evidence_complete: ok,
        provider_matched: true,
        must_pass: ok,
        selection_demonstrated: selection_ok(integrity),
        protected_regression: false,
        benefit_established: ok && !integrity.absence_expected,
        within_budgets,
        claim,
        skipped_required_check: false,
        single_lucky_run: false,
        meaningful_difference: ok,
    }
}

pub fn verdict(integrity: &Integrity) -> Verdict {
    decide(&evidence(integrity, true, Claim::Capability))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean() -> Integrity {
        Integrity {
            required_skill_use: true,
            absence_expected: false,
            oracle_mutated: false,
            self_report_only: false,
            revision_matches: true,
        }
    }

    #[test]
    fn bypass_oracle_write_self_report_and_drift_cannot_pass() {
        assert_eq!(verdict(&clean()), Verdict::Accept);
        let mut bypass = clean();
        bypass.required_skill_use = false;
        assert_ne!(verdict(&bypass), Verdict::Accept);
        let mut negative = clean();
        negative.required_skill_use = false;
        negative.absence_expected = true;
        assert!(selection_ok(&negative));
        let mut oracle = clean();
        oracle.oracle_mutated = true;
        assert_eq!(verdict(&oracle), Verdict::Reject);
        let mut report = clean();
        report.self_report_only = true;
        assert_ne!(verdict(&report), Verdict::Accept);
        let mut drift = clean();
        drift.revision_matches = false;
        assert_eq!(verdict(&drift), Verdict::Reject);
    }
}
