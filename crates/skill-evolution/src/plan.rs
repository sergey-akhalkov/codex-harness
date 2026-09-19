//! Declared comparison plan for one library change. Numbers are recorded, not invented.

use serde::{Deserialize, Serialize};

pub const RUNNER_MODEL: &str = "gpt-6-astra";
pub const RUNNER_EFFORT: &str = "xhigh";
pub const OWNED_SKILL: &str = "project-verification";
pub const FIRST_CASE: &str = "entrypoint";
pub const INTENDED_CASE: &str = "entrypoint";
pub const NEGATIVE_CASE: &str = "negative";
pub const BOUNDARY_CASE: &str = "missing";
pub const HELD_OUT_CASE: &str = "freshness";
pub const ACCEPT_SKILL: &str = "harness-product-cli";
pub const ACCEPT_INTENDED: &str = "cli-now";
pub const ACCEPT_NEGATIVE: &str = "typo-fix";
pub const ACCEPT_HELD_OUT: &str = "cli-prior";
pub const ACCEPT_MODEL: &str = "grok-4.6";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Shortening,
    Retirement,
    Addition,
    Update,
    Merge,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeBudget {
    pub max_paired_runs: u32,
    pub timeout_seconds: u64,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonPlan {
    pub skill_name: String,
    pub operation: Operation,
    pub case_id: String,
    pub model: String,
    pub effort: String,
    pub surrounding_library: String,
    pub budget: EpisodeBudget,
    pub measured_units: Vec<String>,
    pub unsupported_measurements: Vec<String>,
}

pub fn pilot() -> ComparisonPlan {
    ComparisonPlan {
        skill_name: OWNED_SKILL.into(),
        operation: Operation::Shortening,
        case_id: FIRST_CASE.into(),
        model: RUNNER_MODEL.into(),
        effort: RUNNER_EFFORT.into(),
        surrounding_library: "isolated home with only the compared skill".into(),
        budget: EpisodeBudget {
            max_paired_runs: 1,
            timeout_seconds: 600,
            notes: "Reuse codex-harness outcome-prepare/outcome-run/outcome-oracle. Both arms use the runner model and effort. No extra model call for planning.".into(),
        },
        measured_units: vec![
            "paired_runs".into(),
            "elapsed_seconds".into(),
            "native_oracle_passed".into(),
            "reported_token_totals_if_present".into(),
            "explicit_or_implicit_skill_use".into(),
        ],
        unsupported_measurements: vec![
            "subscription_quota_attribution".into(),
            "unified_token_price".into(),
            "account_allowance_percent".into(),
        ],
    }
}

pub fn shortening_batch() -> crate::comparison::Plan {
    crate::comparison::Plan {
        kind: crate::comparison::Kind::UpdateOld,
        intended: INTENDED_CASE.into(),
        negative: NEGATIVE_CASE.into(),
        boundary: BOUNDARY_CASE.into(),
        held_out: HELD_OUT_CASE.into(),
        model: RUNNER_MODEL.into(),
        effort: RUNNER_EFFORT.into(),
    }
}

pub fn independent_acceptance_batch() -> crate::comparison::Plan {
    crate::comparison::Plan {
        kind: crate::comparison::Kind::AddAbsence,
        intended: ACCEPT_INTENDED.into(),
        negative: ACCEPT_NEGATIVE.into(),
        boundary: ACCEPT_NEGATIVE.into(),
        held_out: ACCEPT_HELD_OUT.into(),
        model: ACCEPT_MODEL.into(),
        effort: RUNNER_EFFORT.into(),
    }
}

pub fn provider_limited_batch_evidence() -> crate::decision::ComparisonEvidence {
    crate::decision::ComparisonEvidence {
        integrity_ok: true,
        evidence_complete: false,
        provider_matched: false,
        must_pass: false,
        selection_demonstrated: false,
        protected_regression: false,
        benefit_established: false,
        within_budgets: true,
        claim: crate::decision::Claim::Capability,
        skipped_required_check: false,
        single_lucky_run: false,
        meaningful_difference: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pilot_keeps_model_effort_and_marks_quota_unknown() {
        let plan = pilot();
        assert_eq!(plan.skill_name, "project-verification");
        assert_eq!(plan.operation, Operation::Shortening);
        assert_eq!(plan.model, RUNNER_MODEL);
        assert_eq!(plan.effort, RUNNER_EFFORT);
        assert!(
            plan.unsupported_measurements
                .iter()
                .any(|item| item == "subscription_quota_attribution")
        );
        assert!(
            !plan
                .measured_units
                .iter()
                .any(|item| item.contains("percent"))
        );
    }

    #[test]
    fn shortening_batch_keeps_held_out_out_of_refinement_and_provider_limit_is_inconclusive() {
        let batch = shortening_batch();
        assert_eq!(batch.intended, INTENDED_CASE);
        assert_eq!(batch.negative, NEGATIVE_CASE);
        assert_eq!(batch.boundary, BOUNDARY_CASE);
        assert_eq!(batch.held_out, HELD_OUT_CASE);
        assert_ne!(batch.intended, batch.held_out);
        assert_eq!(batch.model, RUNNER_MODEL);
        assert_eq!(batch.effort, RUNNER_EFFORT);
        let evidence = provider_limited_batch_evidence();
        assert_eq!(
            crate::decision::decide(&evidence),
            crate::decision::Verdict::Inconclusive
        );
        assert!(!evidence.single_lucky_run);
        assert!(!evidence.benefit_established);
        assert_eq!(evidence.claim, crate::decision::Claim::Capability);
    }

    #[test]
    fn independent_acceptance_batch_is_add_absence_on_unused_cases() {
        let batch = independent_acceptance_batch();
        assert_eq!(batch.kind, crate::comparison::Kind::AddAbsence);
        assert_eq!(batch.intended, ACCEPT_INTENDED);
        assert_eq!(batch.negative, ACCEPT_NEGATIVE);
        assert_eq!(batch.held_out, ACCEPT_HELD_OUT);
        assert_ne!(batch.intended, batch.held_out);
        assert_ne!(batch.intended, INTENDED_CASE);
        assert_ne!(batch.held_out, HELD_OUT_CASE);
        assert_ne!(batch.negative, NEGATIVE_CASE);
        assert_eq!(batch.model, ACCEPT_MODEL);
        assert_eq!(batch.effort, RUNNER_EFFORT);
    }
}
