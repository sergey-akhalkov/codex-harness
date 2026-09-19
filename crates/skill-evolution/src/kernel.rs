//! Small state machine for reserve / decide / admit. Not a proof of skill value.

use crate::{
    budget::Ledger,
    catalogue::Admission,
    decision::{self, ComparisonEvidence, Verdict},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Idle,
    Reserved,
    WaitingBudget,
    Rejected,
    Inconclusive,
    Accepted,
    BlockedAdmission,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Controller {
    pub state: State,
    pub budget: Ledger,
    pub admission: Admission,
    pub library_revision: String,
}

impl Controller {
    pub fn start(owner: &str, period: &str, library_revision: impl Into<String>) -> Self {
        Self {
            state: State::Idle,
            budget: Ledger::new(owner, period),
            admission: Admission {
                owned_count: 0,
                limit: 32,
            },
            library_revision: library_revision.into(),
        }
    }

    pub fn on_signal(&mut self) -> State {
        match self.budget.reserve(1) {
            Ok(()) => {
                self.state = State::Reserved;
            }
            Err(_) => {
                self.state = State::WaitingBudget;
            }
        }
        self.state
    }

    pub fn on_eval(&mut self, evidence: &ComparisonEvidence, adding: bool, name: &str) -> Verdict {
        let _ = self.budget.complete(1);
        let verdict = decision::decide(evidence);
        self.state = match verdict {
            Verdict::Reject => State::Rejected,
            Verdict::Inconclusive => State::Inconclusive,
            Verdict::Accept if self.admission.allow_growth(name, adding) => State::Accepted,
            Verdict::Accept => State::BlockedAdmission,
        };
        if !matches!(self.state, State::Accepted) {
            // Reject/inconclusive/blocked admission leave the library revision.
        }
        verdict
    }

    pub fn on_restart(&mut self) {
        self.budget.restart();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::Claim;

    fn evidence(ok: bool) -> ComparisonEvidence {
        ComparisonEvidence {
            integrity_ok: ok,
            evidence_complete: ok,
            provider_matched: ok,
            must_pass: ok,
            selection_demonstrated: ok,
            protected_regression: !ok,
            benefit_established: ok,
            within_budgets: ok,
            claim: Claim::Capability,
            skipped_required_check: false,
            single_lucky_run: false,
            meaningful_difference: ok,
        }
    }

    #[test]
    fn traces_cover_success_failure_unknown_restart_exhaustion_and_overflow() {
        let mut ctl = Controller::start("owner", "day", "lib-1");
        ctl.budget.period_limit = Some(1);
        assert_eq!(ctl.on_signal(), State::Reserved);
        assert_eq!(ctl.on_eval(&evidence(true), true, "demo"), Verdict::Accept);
        assert_eq!(ctl.state, State::Accepted);
        ctl.on_restart();
        assert_eq!(ctl.on_signal(), State::WaitingBudget);

        let mut fail = Controller::start("owner", "day", "lib-1");
        fail.on_signal();
        assert_eq!(
            fail.on_eval(&evidence(false), true, "demo"),
            Verdict::Reject
        );
        assert_eq!(fail.library_revision, "lib-1");

        let mut unknown = Controller::start("owner", "day", "lib-1");
        unknown.on_signal();
        let mut incomplete = evidence(true);
        incomplete.evidence_complete = false;
        incomplete.benefit_established = false;
        incomplete.protected_regression = false;
        assert_eq!(
            unknown.on_eval(&incomplete, true, "demo"),
            Verdict::Inconclusive
        );

        let mut overflow = Controller::start("owner", "day", "lib-1");
        overflow.admission = Admission {
            owned_count: 2,
            limit: 1,
        };
        overflow.on_signal();
        overflow.on_eval(&evidence(true), true, "demo");
        assert_eq!(overflow.state, State::BlockedAdmission);
        assert_eq!(overflow.library_revision, "lib-1");
    }
}
