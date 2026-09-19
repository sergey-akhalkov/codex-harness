//! Protected targets cannot be evolved. Ordinary owned skills can.

use crate::{invalid, ordinary_metadata};
use std::{io, path::Path};

pub fn protected_name(name: &str) -> bool {
    name.starts_with("openspec-")
        || matches!(
            name,
            "skill-evolution"
                | "skills-usage-analysis"
                | "skill-creator"
                | "project-verification"
                | "project-memory"
        )
}

pub fn allow_target(name: &str, path: &Path, discovery_root: &Path) -> io::Result<()> {
    if protected_name(name) {
        return Err(invalid("protected skill cannot be evolved"));
    }
    let meta = ordinary_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(invalid("skill evolution refuses reparse points"));
    }
    let path = path.canonicalize()?;
    let discovery = discovery_root.canonicalize()?;
    if !path.starts_with(&discovery) {
        return Err(invalid("target is outside the owned discovery root"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn protected_and_outside_targets_are_refused_owned_skill_is_allowed() {
        let root = tempfile::tempdir().unwrap();
        let discovery = root.path().join("skills");
        let owned = discovery.join("demo");
        fs::create_dir_all(&owned).unwrap();
        fs::write(owned.join("SKILL.md"), "ok").unwrap();
        allow_target("demo", &owned, &discovery).unwrap();
        assert!(allow_target("skill-evolution", &owned, &discovery).is_err());
        assert!(allow_target("openspec-apply-change", &owned, &discovery).is_err());
        let outside = root.path().join("foreign");
        fs::create_dir(&outside).unwrap();
        assert!(allow_target("demo", &outside, &discovery).is_err());
    }
}
