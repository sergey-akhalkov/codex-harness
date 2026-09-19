//! Owned catalogue admission. Overflow blocks growth, not required skills.

use crate::ownership;
use crate::package;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Admission {
    pub owned_count: u32,
    pub limit: u32,
}

impl Admission {
    pub fn overflow(&self) -> bool {
        self.owned_count > self.limit
    }

    pub fn allow_growth(&self, name: &str, adding: bool) -> bool {
        if ownership::protected_name(name) {
            return true;
        }
        if !adding {
            return true;
        }
        self.owned_count < self.limit
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub revision: String,
    pub applicability: String,
    pub enabled: bool,
}

pub fn build(
    roots: &[(PathBuf, bool)],
    retired: &[String],
    metadata_limit: usize,
) -> (Vec<Entry>, bool, usize) {
    let mut entries = Vec::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    for (root, project) in roots {
        let Ok(read) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path
                .components()
                .any(|c| c.as_os_str() == "skill-evolution" && path.join("evidence.json").exists())
            {
                continue;
            }
            let Ok(identity) = package::load(&path) else {
                continue;
            };
            if retired.iter().any(|name| name == &identity.name) {
                continue;
            }
            if !project
                && path
                    .components()
                    .any(|c| c.as_os_str() == "foreign-project")
            {
                continue;
            }
            let row = Entry {
                name: identity.name,
                applicability: identity.description,
                path: identity.root,
                revision: identity.revision,
                enabled: true,
            };
            let size = row.name.len()
                + row.applicability.len()
                + row.path.as_os_str().len()
                + row.revision.len();
            bytes = bytes.saturating_add(size);
            if bytes > metadata_limit {
                truncated = true;
                continue;
            }
            entries.push(row);
        }
    }
    (entries, truncated, bytes)
}

pub fn remainder_route() -> &'static str {
    "codex-harness skills usage"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_blocks_managed_growth_and_keeps_protected_skills() {
        let admission = Admission {
            owned_count: 3,
            limit: 2,
        };
        assert!(admission.overflow());
        assert!(!admission.allow_growth("demo", true));
        assert!(admission.allow_growth("demo", false));
        assert!(admission.allow_growth("project-verification", true));
    }

    #[test]
    fn truncated_catalogue_keeps_a_remainder_route() {
        assert_eq!(remainder_route(), "codex-harness skills usage");
        let (entries, truncated, _) = build(&[], &[], 1);
        assert!(entries.is_empty());
        assert!(!truncated);
    }
}
