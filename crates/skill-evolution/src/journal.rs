//! Accepted / published / delivered are distinct. Recovery checks current ownership.

use crate::{invalid, package};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Accepted,
    Published,
    Delivered,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub phase: Phase,
    pub name: String,
    pub dest: PathBuf,
    pub recovery: PathBuf,
    pub published_revision: String,
    pub recovery_revision: String,
}

pub fn write(path: &Path, record: &Record) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(record)?)
}

pub fn recover(record: &Record) -> io::Result<()> {
    if record.dest.is_symlink() {
        return Err(invalid("refusing to recursively delete a link target"));
    }
    if !record.recovery.is_dir() {
        return Err(invalid("recovery package is missing"));
    }
    let recovered = package::load(&record.recovery)?;
    if recovered.name != record.name {
        return Err(invalid("recovery ownership does not match the journal"));
    }
    if recovered.revision != record.recovery_revision {
        return Err(invalid("recovery package does not match the journal"));
    }
    if record.dest.exists() {
        let current = package::load(&record.dest)?;
        if current.revision != record.published_revision
            && current.revision != record.recovery_revision
        {
            return Err(invalid("foreign destination was not overwritten"));
        }
        if current.revision == record.recovery_revision {
            return Ok(());
        }
        fs::remove_dir_all(&record.dest)?;
    }
    package::copy_into(&record.recovery, &record.dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_restores_owned_package_and_refuses_a_symlink_target() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("dest");
        let recovery = root.path().join("recovery");
        fs::create_dir_all(&dest).unwrap();
        fs::write(
            dest.join("SKILL.md"),
            "---\nname: demo\ndescription: Journal fixture.\n---\nv1\n",
        )
        .unwrap();
        package::copy_into(&dest, &recovery).unwrap();
        fs::write(
            dest.join("SKILL.md"),
            "---\nname: demo\ndescription: Journal fixture.\n---\nv2\n",
        )
        .unwrap();
        let record = Record {
            phase: Phase::Published,
            name: "demo".into(),
            dest: dest.clone(),
            recovery: recovery.clone(),
            published_revision: package::load(&dest).unwrap().revision,
            recovery_revision: package::load(&recovery).unwrap().revision,
        };
        recover(&record).unwrap();
        assert_eq!(
            package::load(&dest).unwrap().revision,
            record.recovery_revision
        );
        let link = root.path().join("link");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&dest, &link).unwrap();
        #[cfg(not(windows))]
        std::os::unix::fs::symlink(&dest, &link).unwrap();
        let mut linked = record.clone();
        linked.dest = link;
        assert!(recover(&linked).is_err());
        assert!(dest.join("SKILL.md").exists());
    }
}
