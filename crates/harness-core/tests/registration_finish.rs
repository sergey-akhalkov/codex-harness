#![cfg(windows)]

use harness_core::{
    config_create::ConfigCreation,
    config_file::ConfigSnapshot,
    inventory::{Connection, Link},
    registration::{COMMIT, COMPLETION, LinkChange, Registration},
};
use std::{
    fs,
    os::windows::fs::{FileTypeExt, symlink_dir, symlink_file},
    path::{Path, PathBuf},
};

fn fixture() -> PathBuf {
    let root = tempfile::Builder::new()
        .prefix("harness-registration-finish-")
        .tempdir()
        .unwrap()
        .keep();
    println!("finish evidence: {}", root.display());
    fs::write(root.join("old-file"), b"old source").unwrap();
    fs::create_dir(root.join("old-dir")).unwrap();
    fs::write(root.join("old-dir/keep"), b"old directory data").unwrap();
    fs::create_dir(root.join("new-dir")).unwrap();
    fs::write(root.join("new-dir/keep"), b"new directory data").unwrap();
    fs::write(root.join("existing.toml"), b"original").unwrap();
    fs::write(root.join("unrelated"), b"keep").unwrap();
    symlink_file(root.join("old-file"), root.join("replace")).unwrap();
    symlink_dir(root.join("old-dir"), root.join("remove")).unwrap();
    root
}

fn apply(root: &Path) -> Registration {
    let reg = Registration::open(&root.join("state")).unwrap();
    let fresh = Link {
        kind: "instructions".into(),
        name: "fresh".into(),
        source: root.join("old-file"),
        destination: root.join("fresh"),
        connection: Connection::Missing,
    };
    reg.apply_with_changes(
        &[fresh],
        &[ConfigSnapshot::read(&root.join("existing.toml"))
            .unwrap()
            .plan_replace(b"candidate")
            .unwrap()],
        &[
            ConfigCreation::new(&root.join("metadata.json"), b"PRIVATE_FINISH_METADATA").unwrap(),
            ConfigCreation::new(&root.join("created.toml"), b"created").unwrap(),
        ],
        &[
            LinkChange::replace(
                &root.join("replace"),
                &root.join("old-file"),
                &root.join("new-dir"),
            )
            .unwrap(),
            LinkChange::remove(&root.join("remove"), &root.join("old-dir")).unwrap(),
        ],
    )
    .unwrap();
    reg
}

fn candidates(root: &Path) {
    assert_eq!(fs::read(root.join("existing.toml")).unwrap(), b"candidate");
    assert_eq!(fs::read(root.join("created.toml")).unwrap(), b"created");
    assert_eq!(
        fs::read(root.join("metadata.json")).unwrap(),
        b"PRIVATE_FINISH_METADATA"
    );
    assert_eq!(
        fs::read_link(root.join("fresh")).unwrap(),
        root.join("old-file")
    );
    assert_eq!(
        fs::read_link(root.join("replace")).unwrap(),
        root.join("new-dir")
    );
    assert!(fs::symlink_metadata(root.join("remove")).is_err());
    assert_eq!(fs::read(root.join("old-file")).unwrap(), b"old source");
    assert_eq!(
        fs::read(root.join("old-dir/keep")).unwrap(),
        b"old directory data"
    );
    assert_eq!(
        fs::read(root.join("new-dir/keep")).unwrap(),
        b"new directory data"
    );
    assert_eq!(fs::read(root.join("unrelated")).unwrap(), b"keep");
}

fn backups(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".codex-harness-old-")
        })
        .collect()
}

#[test]
fn mixed_finish_repeat_and_next_operation_retain_only_committed_candidates() {
    for metadata in ["metadata.json", "existing.toml"] {
        let root = fixture();
        let reg = apply(&root);
        assert_eq!(backups(&root).len(), 2);
        assert!(reg.recover().is_err()); // accepted completed/uncommitted distinction
        let report = reg.finish(&root.join(metadata)).unwrap();
        assert!(report.committed);
        assert!(report.removed.is_empty() && report.restored.is_empty());
        candidates(&root);
        assert!(backups(&root).is_empty());
        for path in [
            reg.journal_path(),
            reg.state().join(COMMIT),
            reg.state().join(COMPLETION),
        ] {
            assert!(!path.exists());
        }
        assert!(!reg.finish(&root.join(metadata)).unwrap().committed); // no pending work
        assert!(!reg.recover().unwrap().committed);
        assert!(!reg.disconnect().unwrap().committed);
        candidates(&root);
        let next = ConfigSnapshot::read(&root.join("existing.toml"))
            .unwrap()
            .plan_replace(b"next")
            .unwrap();
        reg.apply_with_configs(&[], &[next]).unwrap();
        let rollback = reg.disconnect().unwrap();
        assert!(!rollback.committed);
        candidates(&root); // next operation restores the committed baseline
    }
}

#[test]
fn successive_commits_reuse_the_decision_name_without_identity_drift() {
    let root = fixture();
    let reg = apply(&root);
    let metadata = root.join("metadata.json");
    assert!(reg.finish(&metadata).unwrap().committed);
    // commit.json is recreated immediately after deletion. This exercises NTFS
    // name tunneling at creation/publication, unlike a repeated no-op finish.
    for generation in 0..4 {
        let bytes = format!("metadata generation {generation}").into_bytes();
        let change = ConfigSnapshot::read(&metadata)
            .unwrap()
            .plan_replace(&bytes)
            .unwrap();
        reg.apply_with_configs(&[], &[change]).unwrap();
        assert!(reg.finish(&metadata).unwrap().committed);
        assert_eq!(fs::read(&metadata).unwrap(), bytes);
        assert!(!reg.state().join(COMMIT).exists());
        assert!(!reg.journal_path().exists());
        assert!(!reg.state().join(COMPLETION).exists());
    }
}

#[test]
fn wrong_witness_same_byte_replacement_and_missing_live_data_preserve_all_backups() {
    for conflict in ["wrong", "identity", "missing", "backup"] {
        let root = fixture();
        let reg = apply(&root);
        let metadata = root.join("metadata.json");
        let saved = fs::read(reg.journal_path()).unwrap();
        let retained = backups(&root);
        let nominated = if conflict == "wrong" {
            root.join("unrelated")
        } else {
            metadata.clone()
        };
        if conflict == "identity" {
            fs::rename(&metadata, root.join("retained-metadata")).unwrap();
            fs::write(&metadata, b"PRIVATE_FINISH_METADATA").unwrap();
        } else if conflict == "missing" {
            fs::rename(root.join("fresh"), root.join("retained-fresh")).unwrap();
        } else if conflict == "backup" {
            let path = &retained[0];
            let target = fs::read_link(path).unwrap();
            let dir = fs::symlink_metadata(path)
                .unwrap()
                .file_type()
                .is_symlink_dir();
            fs::rename(path, root.join("retained-backup")).unwrap();
            if dir {
                symlink_dir(target, path).unwrap();
            } else {
                symlink_file(target, path).unwrap();
            }
        }
        let error = reg.finish(&nominated).unwrap_err().to_string();
        assert!(!error.contains("PRIVATE_FINISH_METADATA"));
        assert_eq!(fs::read(reg.journal_path()).unwrap(), saved);
        assert!(!reg.state().join(COMMIT).exists());
        for path in retained {
            assert!(fs::symlink_metadata(path).is_ok());
        }
        assert_eq!(fs::read(root.join("existing.toml")).unwrap(), b"candidate");
        assert_eq!(fs::read(root.join("created.toml")).unwrap(), b"created");
        assert_eq!(
            fs::read_link(root.join("replace")).unwrap(),
            root.join("new-dir")
        );
    }
}

#[test]
fn incomplete_publication_and_foreign_commitment_never_authorize_finish_or_undo() {
    for conflict in ["incomplete", "truncated", "symlink", "hardlink"] {
        let root = fixture();
        let reg = apply(&root);
        let saved = fs::read(reg.journal_path()).unwrap();
        if conflict == "incomplete" {
            fs::remove_file(reg.state().join(COMPLETION)).unwrap();
        } else if conflict == "truncated" {
            fs::write(
                reg.state().join(COMMIT),
                b"{\"private\":\"PRIVATE_FINISH_METADATA",
            )
            .unwrap();
        } else {
            fs::write(root.join("foreign"), b"foreign data").unwrap();
            if conflict == "symlink" {
                symlink_file(root.join("foreign"), reg.state().join(COMMIT)).unwrap();
            } else {
                fs::hard_link(root.join("foreign"), reg.state().join(COMMIT)).unwrap();
            }
        }
        assert!(reg.finish(&root.join("metadata.json")).is_err());
        if conflict != "incomplete" {
            assert!(reg.recover().is_err());
            assert!(reg.disconnect().is_err());
            assert!(reg.apply(&[]).is_err());
        }
        candidates(&root);
        assert_eq!(fs::read(reg.journal_path()).unwrap(), saved);
        assert_eq!(backups(&root).len(), 2);
    }
}
