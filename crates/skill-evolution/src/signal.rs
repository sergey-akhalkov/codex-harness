//! Signal-driven review. No model run without a new signal.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Failure,
    Misuse,
    Cost,
    Overlap,
    ModelChange,
    CataloguePressure,
    UserAnalysis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    Add,
    Shorten,
    Merge,
    Retire,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    pub kind: Kind,
    pub skill: String,
    pub revision: String,
    pub fingerprint: String,
}

pub fn dedupe(previous: &[Signal], incoming: &Signal) -> bool {
    previous.iter().any(|seen| {
        seen.kind == incoming.kind
            && seen.revision == incoming.revision
            && seen.fingerprint == incoming.fingerprint
    })
}

pub fn review(signal: Option<&Signal>, seen: &[Signal]) -> Review {
    let Some(signal) = signal else {
        return Review::None;
    };
    if dedupe(seen, signal) {
        return Review::None;
    }
    match signal.kind {
        Kind::Failure | Kind::Misuse => Review::Shorten,
        Kind::Overlap => Review::Merge,
        Kind::Cost | Kind::CataloguePressure => Review::Retire,
        Kind::ModelChange => Review::Shorten,
        Kind::UserAnalysis => Review::Retire,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_signal_and_duplicate_failure_do_not_start_a_cycle() {
        assert_eq!(review(None, &[]), Review::None);
        let signal = Signal {
            kind: Kind::Failure,
            skill: "demo".into(),
            revision: "abc".into(),
            fingerprint: "same".into(),
        };
        assert_eq!(review(Some(&signal), &[]), Review::Shorten);
        let seen = signal.clone();
        assert_eq!(
            review(Some(&signal), std::slice::from_ref(&seen)),
            Review::None
        );
        let rename = Signal {
            skill: "demo-renamed".into(),
            ..signal
        };
        assert_eq!(review(Some(&rename), &[seen]), Review::None);
    }
}
