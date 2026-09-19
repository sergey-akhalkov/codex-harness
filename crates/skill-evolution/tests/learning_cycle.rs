//! Full-cycle accounting against the declared policy. Not a subscription saving.
use skill_evolution::{
    budget::Ledger,
    decision::{Claim, ComparisonEvidence, Verdict, decide},
    kernel::{Controller, State},
    signal::{self, Kind, Signal},
};

fn evidence(complete: bool, benefit: bool) -> ComparisonEvidence {
    ComparisonEvidence {
        integrity_ok: true,
        evidence_complete: complete,
        provider_matched: complete,
        must_pass: complete,
        selection_demonstrated: complete,
        protected_regression: false,
        benefit_established: benefit,
        within_budgets: true,
        claim: Claim::Capability,
        skipped_required_check: false,
        single_lucky_run: false,
        meaningful_difference: benefit,
    }
}

#[test]
fn learning_cycle_records_failures_and_keeps_inconclusive_benefit_open() {
    let mut ctl = Controller::start("owner", "day", "lib-1");
    ctl.budget.period_limit = Some(3);
    assert_eq!(signal::review(None, &[]), signal::Review::None);
    assert_eq!(ctl.on_signal(), State::Reserved);
    let failed = decide(&evidence(false, false));
    assert_eq!(failed, Verdict::Inconclusive);
    ctl.on_eval(&evidence(false, false), true, "demo");
    assert_eq!(ctl.state, State::Inconclusive);
    assert_eq!(ctl.library_revision, "lib-1");
    let signal = Signal {
        kind: Kind::Failure,
        skill: "demo".into(),
        revision: "lib-1".into(),
        fingerprint: "quota".into(),
    };
    assert_eq!(
        signal::review(Some(&signal), std::slice::from_ref(&signal)),
        signal::Review::None
    );
    ctl.on_restart();
    assert_eq!(ctl.budget.spent, 1);
    let mut lost = Ledger::new("owner", "day");
    lost.period_limit = None;
    assert!(lost.available().is_none());
    lost.reserve(1).unwrap();
    assert!(ctl.admission.allow_growth("demo", true));
}

#[test]
fn learning_cycle_policy_records_required_dimensions_without_a_benefit_claim() {
    let plan = skill_evolution::plan::pilot();
    assert!(
        plan.measured_units
            .iter()
            .any(|u| u == "explicit_or_implicit_skill_use")
    );
    assert!(plan.measured_units.iter().any(|u| u == "elapsed_seconds"));
    assert!(
        plan.unsupported_measurements
            .iter()
            .any(|u| u == "subscription_quota_attribution")
    );
    assert_eq!(signal::review(None, &[]), signal::Review::None);
    let mut ctl = Controller::start("owner", "day", "lib-1");
    ctl.budget.period_limit = Some(2);
    ctl.admission.limit = 1;
    ctl.admission.owned_count = 1;
    assert_eq!(ctl.on_signal(), State::Reserved);
    assert_eq!(
        ctl.on_eval(&evidence(false, false), true, "demo"),
        Verdict::Inconclusive
    );
    assert_eq!(ctl.library_revision, "lib-1");
    ctl.on_restart();
    assert_eq!(ctl.budget.spent, 1);
    assert_eq!(ctl.on_signal(), State::Reserved);
    assert_eq!(
        ctl.on_eval(&evidence(true, true), true, "extra"),
        Verdict::Accept
    );
    assert_eq!(ctl.state, State::BlockedAdmission);
    assert_eq!(ctl.library_revision, "lib-1");
    let mut lost = Ledger::new("owner", "day");
    lost.period_limit = None;
    assert!(lost.available().is_none());
    let batch = skill_evolution::plan::provider_limited_batch_evidence();
    assert_eq!(decide(&batch), Verdict::Inconclusive);
    assert!(!batch.benefit_established);
    assert_eq!(batch.claim, Claim::Capability);
}
