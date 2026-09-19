//! Compact accepted-revision identity for other consumers. Not in-session delivery.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    pub name: String,
    pub path: String,
    pub revision: String,
    pub operation: String,
}

pub fn from_package(root: &Path, operation: &str) -> std::io::Result<Revision> {
    let package = crate::package::load(root)?;
    Ok(Revision {
        name: package.name,
        path: package.root.to_string_lossy().into_owned(),
        revision: package.revision,
        operation: operation.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_compact_and_has_no_body() {
        let id = Revision {
            name: "demo".into(),
            path: "skills/demo".into(),
            revision: "abc".into(),
            operation: "update".into(),
        };
        let text = serde_json::to_string(&id).unwrap();
        assert!(text.contains("demo"));
        assert!(!text.contains("SKILL.md body"));
    }

    #[test]
    fn current_descriptor_revision_is_read_from_the_live_path() {
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join("demo");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\ndescription: Identity fixture.\n---\nv1\n",
        )
        .unwrap();
        let first = from_package(&skill, "create").unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\ndescription: Identity fixture.\n---\nv2\n",
        )
        .unwrap();
        let second = from_package(&skill, "update").unwrap();
        assert_ne!(first.revision, second.revision);
        assert_eq!(second.operation, "update");
    }
}
