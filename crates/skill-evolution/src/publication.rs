//! Hash compare-and-swap for a staged package. Mixed trees are refused.

use crate::{invalid, package};
use std::{
    fs::{self, OpenOptions},
    io,
    path::Path,
};

pub fn publish(staged: &Path, dest: &Path, expected_parent: &str) -> io::Result<package::Identity> {
    let lock_path = dest.with_extension("publish.lock");
    let lock = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path);
    let lock = match lock {
        Ok(file) => file,
        Err(_) => return Err(invalid("publication lock is busy")),
    };
    let result = publish_inner(staged, dest, expected_parent);
    drop(lock);
    let _ = fs::remove_file(&lock_path);
    result
}

fn publish_inner(
    staged: &Path,
    dest: &Path,
    expected_parent: &str,
) -> io::Result<package::Identity> {
    if dest.exists() {
        let current = package::load(dest)?;
        if current.revision != expected_parent {
            return Err(invalid("destination changed after evaluation"));
        }
    } else if !expected_parent.is_empty() {
        return Err(invalid("expected parent is missing at destination"));
    }
    let staged = package::load(staged)?;
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    package::copy_into(&staged.root, dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn skill(root: &Path, body: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            format!("---\nname: demo\ndescription: Publication fixture.\n---\n{body}\n"),
        )
        .unwrap();
    }

    #[test]
    fn concurrent_parent_change_cannot_publish_a_stale_candidate() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("dest");
        let staged = root.path().join("staged");
        skill(&dest, "v1");
        skill(&staged, "v2");
        let parent = package::load(&dest).unwrap().revision;
        fs::write(
            dest.join("SKILL.md"),
            "---\nname: demo\ndescription: Publication fixture.\n---\nchanged\n",
        )
        .unwrap();
        assert!(publish(&staged, &dest, &parent).is_err());
        assert_ne!(
            package::load(&dest).unwrap().revision,
            package::load(&staged).unwrap().revision
        );
        let dest2 = root.path().join("dest2");
        skill(&dest2, "v1");
        let parent = package::load(&dest2).unwrap().revision;
        publish(&staged, &dest2, &parent).unwrap();
        assert_eq!(
            package::load(&dest2).unwrap().revision,
            package::load(&staged).unwrap().revision
        );
        let dest3 = root.path().join("dest3");
        skill(&dest3, "v1");
        fs::write(dest3.with_extension("publish.lock"), b"held").unwrap();
        assert!(publish(&staged, &dest3, &package::load(&dest3).unwrap().revision).is_err());
    }
}
