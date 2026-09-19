//! Evaluated merge keeps both suites. Retirement is reversible and not physical delete.

use crate::decision::Verdict;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Merge {
    pub replacement: String,
    pub suites: Vec<String>,
}

pub fn merge(replacement: impl Into<String>, left_suite: &str, right_suite: &str) -> Merge {
    Merge {
        replacement: replacement.into(),
        suites: vec![left_suite.into(), right_suite.into()],
    }
}

pub fn may_retire(
    required: bool,
    unknown_consumers: bool,
    low_frequency: bool,
    catalogue_pressure: bool,
    verdict: Verdict,
) -> bool {
    if required || unknown_consumers || low_frequency || catalogue_pressure {
        return false;
    }
    matches!(verdict, Verdict::Accept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_both_suites_and_required_or_unknown_consumers_block_retire() {
        let merged = merge("combined", "suite-a", "suite-b");
        assert_eq!(merged.suites, ["suite-a", "suite-b"]);
        assert!(!may_retire(true, false, false, false, Verdict::Accept));
        assert!(!may_retire(false, true, false, false, Verdict::Accept));
        assert!(!may_retire(false, false, true, false, Verdict::Accept));
        assert!(!may_retire(false, false, false, true, Verdict::Accept));
        assert!(!may_retire(
            false,
            false,
            false,
            false,
            Verdict::Inconclusive
        ));
        assert!(may_retire(false, false, false, false, Verdict::Accept));
    }
}
