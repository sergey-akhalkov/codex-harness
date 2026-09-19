//! Knowledge routing: procedures vs facts, hypotheses, tools and no-ops.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Knowledge {
    Procedure,
    Fact,
    Hypothesis,
    ExistingTool,
    NoOp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub reusable_procedure: bool,
    pub checkable_result: bool,
    pub project_fact_only: bool,
    pub tentative: bool,
    pub existing_tool: bool,
    pub read_only: bool,
    pub routine_success: bool,
}

pub fn classify(observation: &Observation) -> Knowledge {
    if observation.read_only || observation.routine_success {
        return Knowledge::NoOp;
    }
    if observation.existing_tool {
        return Knowledge::ExistingTool;
    }
    if observation.tentative {
        return Knowledge::Hypothesis;
    }
    if observation.project_fact_only {
        return Knowledge::Fact;
    }
    if observation.reusable_procedure && observation.checkable_result {
        return Knowledge::Procedure;
    }
    Knowledge::NoOp
}

pub fn may_stage(kind: Knowledge) -> bool {
    matches!(kind, Knowledge::Procedure)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> Observation {
        Observation {
            reusable_procedure: false,
            checkable_result: false,
            project_fact_only: false,
            tentative: false,
            existing_tool: false,
            read_only: false,
            routine_success: false,
        }
    }

    #[test]
    fn examples_distinguish_procedure_fact_hypothesis_tool_and_noop() {
        let mut procedure = blank();
        procedure.reusable_procedure = true;
        procedure.checkable_result = true;
        assert_eq!(classify(&procedure), Knowledge::Procedure);
        assert!(may_stage(Knowledge::Procedure));

        let mut fact = blank();
        fact.project_fact_only = true;
        assert_eq!(classify(&fact), Knowledge::Fact);
        assert!(!may_stage(Knowledge::Fact));

        let mut hypothesis = blank();
        hypothesis.tentative = true;
        assert_eq!(classify(&hypothesis), Knowledge::Hypothesis);

        let mut tool = blank();
        tool.existing_tool = true;
        tool.reusable_procedure = true;
        tool.checkable_result = true;
        assert_eq!(classify(&tool), Knowledge::ExistingTool);

        let mut read_only = blank();
        read_only.read_only = true;
        read_only.reusable_procedure = true;
        assert_eq!(classify(&read_only), Knowledge::NoOp);

        let mut routine = blank();
        routine.routine_success = true;
        assert_eq!(classify(&routine), Knowledge::NoOp);
    }
}
