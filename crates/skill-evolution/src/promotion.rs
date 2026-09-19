//! Canonical promotion is a separate operation from local publication.

use crate::package;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Local,
    Pending,
    Canonical,
}

pub fn decide(
    origin_ok: bool,
    second_project_ok: bool,
    secrets_clean: bool,
    dependencies_ok: bool,
) -> Scope {
    if !origin_ok || !secrets_clean || !dependencies_ok {
        return Scope::Local;
    }
    if second_project_ok {
        Scope::Canonical
    } else {
        Scope::Pending
    }
}

pub fn secrets_clean(root: &Path) -> io::Result<bool> {
    let identity = package::load(root)?;
    for relative in identity.files.keys() {
        let bytes = fs::read(root.join(relative))?;
        let text = String::from_utf8_lossy(&bytes);
        if text.contains("sk-") || text.contains("BEGIN PRIVATE KEY") {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn promote(
    local: &Path,
    canonical: &Path,
    origin_ok: bool,
    second_project_ok: bool,
    dependencies_ok: bool,
) -> io::Result<Scope> {
    let clean = secrets_clean(local)?;
    let scope = decide(origin_ok, second_project_ok, clean, dependencies_ok);
    if scope == Scope::Canonical {
        if canonical.exists() {
            let local_id = package::load(local)?;
            let dest_id = package::load(canonical)?;
            if local_id.name == dest_id.name && local_id.revision != dest_id.revision {
                // Keep the canonical package; caller must reconcile uncommitted
                // local work without clobbering it.
                return Ok(Scope::Pending);
            }
        }
        if canonical.exists() {
            fs::remove_dir_all(canonical)?;
        }
        package::copy_into(local, canonical)?;
    }
    Ok(scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn non_portable_stays_local_or_pending() {
        assert_eq!(decide(true, true, true, true), Scope::Canonical);
        assert_eq!(decide(true, false, true, true), Scope::Pending);
        assert_eq!(decide(false, true, true, true), Scope::Local);
        assert_eq!(decide(true, true, false, true), Scope::Local);
    }

    #[test]
    fn secrets_and_overlapping_identities_stay_out_of_canonical() {
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("local");
        fs::create_dir_all(&local).unwrap();
        fs::write(
            local.join("SKILL.md"),
            "---\nname: demo\ndescription: Promotion fixture.\n---\nsk-secret\n",
        )
        .unwrap();
        assert!(!secrets_clean(&local).unwrap());
        fs::write(
            local.join("SKILL.md"),
            "---\nname: demo\ndescription: Promotion fixture.\n---\nclean\n",
        )
        .unwrap();
        let canonical = root.path().join("canonical");
        fs::create_dir_all(&canonical).unwrap();
        fs::write(
            canonical.join("SKILL.md"),
            "---\nname: demo\ndescription: Promotion fixture.\n---\ncanonical-uncommitted\n",
        )
        .unwrap();
        assert_eq!(
            promote(&local, &canonical, true, true, true).unwrap(),
            Scope::Pending
        );
        assert!(
            std::fs::read_to_string(canonical.join("SKILL.md"))
                .unwrap()
                .contains("canonical-uncommitted")
        );
    }
}
