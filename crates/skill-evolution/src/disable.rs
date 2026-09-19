//! Reversible discovery removal. Physical delete is out of scope here.

use crate::{invalid, package};
use std::{fs, io, path::Path};

pub fn disable(source: &Path, recovery: &Path) -> io::Result<package::Identity> {
    let identity = package::load(source)?;
    if recovery.exists() {
        return Err(invalid("recovery package already exists"));
    }
    package::copy_into(source, recovery)?;
    fs::remove_dir_all(source)?;
    Ok(identity)
}

pub fn restore(recovery: &Path, source: &Path) -> io::Result<package::Identity> {
    if source.exists() {
        return Err(invalid("active package already exists"));
    }
    package::copy_into(recovery, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn disable_is_reversible_and_listing_does_not_publish() {
        let root = tempfile::tempdir().unwrap();
        let active = root.path().join("skills/demo");
        fs::create_dir_all(&active).unwrap();
        fs::write(
            active.join("SKILL.md"),
            "---\nname: demo\ndescription: Disable fixture.\n---\n",
        )
        .unwrap();
        let recovery = root.path().join("recovery/demo");
        let before = package::load(&active).unwrap().revision;
        disable(&active, &recovery).unwrap();
        assert!(!active.exists());
        restore(&recovery, &active).unwrap();
        assert_eq!(package::load(&active).unwrap().revision, before);
    }
}
