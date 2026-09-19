//! Enforceable episode and period budgets in paired-run units.

use crate::invalid;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    pub owner: String,
    pub period_id: String,
    pub episode_limit: u32,
    pub period_limit: Option<u32>,
    pub spent: u32,
    pub reserved: u32,
}

impl Ledger {
    pub fn new(owner: impl Into<String>, period_id: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            period_id: period_id.into(),
            episode_limit: 1,
            period_limit: None,
            spent: 0,
            reserved: 0,
        }
    }

    pub fn available(&self) -> Option<u32> {
        let period = self.period_limit?;
        Some(period.saturating_sub(self.spent.saturating_add(self.reserved)))
    }

    pub fn reserve(&mut self, units: u32) -> io::Result<()> {
        if units == 0 || units > self.episode_limit {
            return Err(invalid("episode reservation exceeds the episode limit"));
        }
        if self.period_limit.is_some_and(|limit| {
            self.spent
                .saturating_add(self.reserved)
                .saturating_add(units)
                > limit
        }) {
            return Err(invalid("period budget exhausted"));
        }
        self.reserved = self.reserved.saturating_add(units);
        Ok(())
    }

    pub fn complete(&mut self, units: u32) -> io::Result<()> {
        if units > self.reserved {
            return Err(invalid("cannot complete more than reserved"));
        }
        self.reserved -= units;
        self.spent = self.spent.saturating_add(units);
        Ok(())
    }

    pub fn fail(&mut self, units: u32) -> io::Result<()> {
        self.complete(units)
    }

    pub fn restart(&mut self) {}

    pub fn invariant(&self) -> bool {
        match self.period_limit {
            Some(limit) => self.spent.saturating_add(self.reserved) <= limit,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_small_episodes_cannot_exceed_the_period_and_restart_keeps_spend() {
        let mut ledger = Ledger::new("owner", "day");
        ledger.period_limit = Some(2);
        ledger.reserve(1).unwrap();
        ledger.complete(1).unwrap();
        ledger.reserve(1).unwrap();
        ledger.fail(1).unwrap();
        ledger.restart();
        assert_eq!(ledger.spent, 2);
        assert!(ledger.reserve(1).is_err());
        assert!(ledger.invariant());
        let mut unknown = Ledger::new("owner", "day");
        unknown.period_limit = None;
        unknown.reserve(1).unwrap();
        assert!(unknown.available().is_none());
    }
}
