//! Deterministic foreign actors at the selection publication boundary.
use super::*;
use crate::registration_native::{FileGuard, LinkIdentity};

fn owned_state() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    fs::create_dir_all(state.join("builds")).unwrap();
    fs::write(state.join("owner"), b"codex-harness-native-state-v1\n").unwrap();
    (root, state)
}

fn object(path: &Path) -> LinkIdentity {
    FileGuard::read_regular(path)
        .unwrap()
        .0
        .object_identity()
        .unwrap()
}

fn foreign_bytes(name: &str, after: Option<&[u8]>) {
    let (_root, state) = owned_state();
    let _lock = native_build::lock_owned_state(&state).unwrap();
    let path = state.join(ACTIVE);
    fs::write(&path, b"before").unwrap();
    let original = object(&path);
    let mut mutation = None;
    let result = replace_expected_at_publication(&path, Some(b"before"), after, || {
        mutation = Some(fs::write(&path, b"foreign"));
    });
    let mutation = mutation.expect("the actor must reach the publication boundary");
    eprintln!("{name}: foreign_write={mutation:?}, selection={result:?}");
    if mutation.is_ok() {
        assert!(result.is_err(), "{name} accepted stale before bytes");
        assert_eq!(fs::read(&path).unwrap(), b"foreign");
        assert_eq!(object(&path), original);
    } else {
        result.unwrap();
        assert_eq!(read_optional(&path).unwrap().as_deref(), after);
    }
}

#[test]
fn publication_boundary_preserves_foreign_bytes_on_replacement() {
    foreign_bytes("replace", Some(b"after"));
}

#[test]
fn publication_boundary_preserves_foreign_bytes_on_deletion() {
    foreign_bytes("delete", None);
}

fn foreign_objects(name: &str, after: Option<&[u8]>) {
    let (_root, state) = owned_state();
    let _lock = native_build::lock_owned_state(&state).unwrap();
    let path = state.join(ACTIVE);
    let displaced = state.join("displaced");
    fs::write(&path, b"before").unwrap();
    let original = object(&path);
    let mut mutation = None;
    let mut foreign = None;
    let result = replace_expected_at_publication(&path, Some(b"before"), after, || {
        let renamed = fs::rename(&path, &displaced);
        if renamed.is_ok() {
            fs::write(&path, b"before").unwrap();
            foreign = Some(object(&path));
        }
        mutation = Some(renamed);
    });
    let mutation = mutation.expect("the actor must reach the publication boundary");
    eprintln!("{name}: foreign_swap={mutation:?}, selection={result:?}");
    if mutation.is_ok() {
        assert!(
            result.is_err(),
            "{name} accepted a foreign object with matching bytes"
        );
        assert_eq!(fs::read(&path).unwrap(), b"before");
        assert_eq!(Some(object(&path)), foreign);
        assert_ne!(foreign.unwrap(), original);
        assert_eq!(object(&displaced), original);
    } else {
        result.unwrap();
        assert_eq!(read_optional(&path).unwrap().as_deref(), after);
        assert!(!displaced.exists());
    }
}

#[test]
fn publication_boundary_preserves_foreign_objects_on_replacement() {
    foreign_objects("replace", Some(b"after"));
}

#[test]
fn publication_boundary_preserves_foreign_objects_on_deletion() {
    foreign_objects("delete", None);
}

#[test]
fn creation_reserves_absence_until_commit() {
    let (_root, state) = owned_state();
    let _lock = native_build::lock_owned_state(&state).unwrap();
    let path = state.join(ACTIVE);
    let mut reached = false;
    replace_expected_at_publication(&path, None, Some(b"after"), || {
        reached = true;
        assert!(fs::write(&path, b"foreign").is_err());
        assert!(fs::create_dir(&path).is_err());
        assert!(fs::rename(&state, state.with_file_name("displaced")).is_err());
    })
    .unwrap();
    assert!(reached);
    assert_eq!(fs::read(&path).unwrap(), b"after");
}

#[test]
fn stale_bytes_and_foreign_types_are_preserved() {
    use std::os::windows::fs::symlink_file;
    for kind in ["bytes", "directory", "symlink", "dangling", "hardlink"] {
        for before in [None, Some(b"before".as_slice())] {
            for after in [None, Some(b"after".as_slice())] {
                let (_root, state) = owned_state();
                let _lock = native_build::lock_owned_state(&state).unwrap();
                let path = state.join(ACTIVE);
                let foreign = state.join("foreign");
                fs::write(&foreign, b"before").unwrap();
                match kind {
                    "bytes" => fs::write(&path, b"foreign").unwrap(),
                    "directory" => fs::create_dir(&path).unwrap(),
                    "symlink" => symlink_file(&foreign, &path).unwrap(),
                    "dangling" => symlink_file(state.join("absent"), &path).unwrap(),
                    "hardlink" => fs::hard_link(&foreign, &path).unwrap(),
                    _ => unreachable!(),
                }
                assert!(replace_expected(&path, before, after).is_err(), "{kind}");
                assert_eq!(fs::read(&foreign).unwrap(), b"before");
                match kind {
                    "bytes" => assert_eq!(fs::read(&path).unwrap(), b"foreign"),
                    "directory" => assert!(fs::read_dir(&path).unwrap().next().is_none()),
                    "symlink" => assert_eq!(fs::read_link(&path).unwrap(), foreign),
                    "dangling" => {
                        assert_eq!(fs::read_link(&path).unwrap(), state.join("absent"));
                        assert!(!state.join("absent").exists());
                    }
                    "hardlink" => assert_eq!(fs::read(&path).unwrap(), b"before"),
                    _ => unreachable!(),
                }
            }
        }
    }
}

#[test]
fn unavailable_guards_preserve_existing_writer_and_reparse_parent() {
    use std::os::windows::fs::symlink_dir;
    let (_root, state) = owned_state();
    let _lock = native_build::lock_owned_state(&state).unwrap();
    let path = state.join(ACTIVE);
    fs::write(&path, b"before").unwrap();
    let writer = fs::OpenOptions::new().write(true).open(&path).unwrap();
    for after in [None, Some(b"after".as_slice())] {
        assert!(replace_expected(&path, Some(b"before"), after).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"before");
    }
    drop(writer);
    let alias = state.join("alias");
    let destination = state.join("destination");
    fs::create_dir(&destination).unwrap();
    symlink_dir(&destination, &alias).unwrap();
    assert!(replace_expected(&alias.join(ACTIVE), None, Some(b"after")).is_err());
    assert!(fs::read_dir(&destination).unwrap().next().is_none());
    assert_eq!(fs::read_link(&alias).unwrap(), destination);
}

#[test]
fn interrupted_publication_rolls_back_without_sibling_debris() {
    for (before, after) in [
        (None, Some(b"after".as_slice())),
        (Some(b"before".as_slice()), Some(b"after".as_slice())),
        (Some(b"before".as_slice()), None),
    ] {
        let (_root, state) = owned_state();
        let _lock = native_build::lock_owned_state(&state).unwrap();
        let path = state.join(ACTIVE);
        if let Some(bytes) = before {
            fs::write(&path, bytes).unwrap();
        }
        let original = before.map(|_| object(&path));
        let count = fs::read_dir(&state).unwrap().count();
        let result = std::panic::catch_unwind(|| {
            replace_expected_at_publication(&path, before, after, || {
                panic!("interrupted at publication");
            })
        });
        assert!(result.is_err());
        assert_eq!(read_optional(&path).unwrap().as_deref(), before);
        assert_eq!(before.map(|_| object(&path)), original);
        assert_eq!(fs::read_dir(&state).unwrap().count(), count);
        replace_expected(&path, before, after).unwrap();
        assert_eq!(read_optional(&path).unwrap().as_deref(), after);
    }
}
