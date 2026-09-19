//! One-change comparisons against a fixed surrounding library.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    AddAbsence,
    UpdateOld,
    MergeOldSet,
    RetireRetention,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub kind: Kind,
    pub intended: String,
    pub negative: String,
    pub boundary: String,
    pub held_out: String,
    pub model: String,
    pub effort: String,
}

impl Plan {
    pub fn cases(&self) -> [&str; 4] {
        [
            &self.intended,
            &self.negative,
            &self.boundary,
            &self.held_out,
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseResult {
    pub case_id: String,
    pub arm: String,
    pub skill_used: bool,
    pub absence_expected: bool,
    pub order: u32,
}

pub fn bypass(result: &CaseResult) -> bool {
    !result.absence_expected && !result.skill_used
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;

    #[test]
    fn shortening_plan_includes_held_out_distinct_from_refinement() {
        let shortening = Plan {
            kind: Kind::UpdateOld,
            intended: plan::INTENDED_CASE.into(),
            negative: plan::NEGATIVE_CASE.into(),
            boundary: plan::BOUNDARY_CASE.into(),
            held_out: plan::HELD_OUT_CASE.into(),
            model: plan::RUNNER_MODEL.into(),
            effort: plan::RUNNER_EFFORT.into(),
        };
        assert_eq!(shortening.cases().len(), 4);
        assert_ne!(shortening.intended, shortening.held_out);
    }

    #[test]
    fn negative_absence_is_not_bypass() {
        let negative = CaseResult {
            case_id: "negative".into(),
            arm: "candidate".into(),
            skill_used: false,
            absence_expected: true,
            order: 2,
        };
        assert!(!bypass(&negative));
        let intended = CaseResult {
            case_id: "entrypoint".into(),
            arm: "candidate".into(),
            skill_used: false,
            absence_expected: false,
            order: 1,
        };
        assert!(bypass(&intended));
    }
}
