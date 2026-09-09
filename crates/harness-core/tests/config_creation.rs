#![cfg(windows)]

use harness_core::{
    config_create::ConfigCreation, config_file::ConfigSnapshot, registration::Registration,
};
use std::{fs, os::windows::fs::symlink_file, path::PathBuf};

fn fixture() -> PathBuf {
    let root = tempfile::Builder::new()
        .prefix("harness-config-creation-")
        .tempdir()
        .unwrap()
        .keep();
    println!("configuration creation evidence: {}", root.display());
    root
}

#[test]
fn absent_configuration_create_repeat_disconnect_preserves_unrelated_data() {
    for bytes in [
        vec![],
        b"# PRIVATE_CONFIG_SENTINEL\r\n[features]\r\nhooks = false\r\n".to_vec(),
        vec![b'x'; 128 * 1024],
    ] {
        let root = fixture();
        let path = root.join("новый дом/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Exercise NTFS name tunneling from a recently removed destination.
        fs::write(&path, b"previous object").unwrap();
        fs::remove_file(&path).unwrap();
        let existing = root.join("existing.toml");
        fs::write(&existing, b"before").unwrap();
        fs::write(root.join("unrelated"), b"keep").unwrap();
        let change = ConfigSnapshot::read(&existing)
            .unwrap()
            .plan_replace(b"after")
            .unwrap();
        let creation = ConfigCreation::new(&path, &bytes).unwrap();
        assert!(!format!("{creation:?}").contains("PRIVATE_CONFIG_SENTINEL"));
        let reg = Registration::open(&root.join("state")).unwrap();
        let report = reg.apply_with_files(&[], &[change], &[creation]).unwrap();
        assert_eq!(report.configurations, vec![existing.clone(), path.clone()]);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let repeat = ConfigSnapshot::read(&existing)
            .unwrap()
            .plan_replace(b"after")
            .unwrap();
        reg.apply_with_files(
            &[],
            &[repeat],
            &[ConfigCreation::new(&path, &bytes).unwrap()],
        )
        .unwrap();
        let undo = reg.disconnect().unwrap();
        assert_eq!(undo.removed, vec![path.clone()]);
        assert_eq!(undo.restored, vec![existing.clone()]);
        assert!(!path.exists());
        assert_eq!(fs::read(&existing).unwrap(), b"before");
        assert_eq!(fs::read(root.join("unrelated")).unwrap(), b"keep");
        assert!(!reg.journal_path().exists());
    }
}

#[test]
fn existing_regular_directory_and_dangling_symlink_are_never_adopted() {
    for kind in ["regular", "directory", "dangling"] {
        let root = fixture();
        let path = root.join("config.toml");
        match kind {
            "regular" => fs::write(&path, b"same bytes").unwrap(),
            "directory" => fs::create_dir(&path).unwrap(),
            _ => symlink_file(root.join("missing-foreign"), &path).unwrap(),
        }
        let reg = Registration::open(&root.join("state")).unwrap();
        assert!(
            reg.apply_with_files(
                &[],
                &[],
                &[ConfigCreation::new(&path, b"same bytes").unwrap()]
            )
            .is_err()
        );
        assert!(!reg.journal_path().exists());
        assert!(fs::symlink_metadata(&path).is_ok());
        if kind == "regular" {
            assert_eq!(fs::read(&path).unwrap(), b"same bytes");
        }
        if kind == "dangling" {
            assert_eq!(fs::read_link(&path).unwrap(), root.join("missing-foreign"));
        }
    }
}

#[test]
fn foreign_edit_or_same_content_replacement_stops_all_undo() {
    for replace in [false, true] {
        let root = fixture();
        let path = root.join("new.toml");
        let old = root.join("old.toml");
        fs::write(&old, b"original").unwrap();
        let reg = Registration::open(&root.join("state")).unwrap();
        reg.apply_with_files(
            &[],
            &[ConfigSnapshot::read(&old)
                .unwrap()
                .plan_replace(b"candidate")
                .unwrap()],
            &[ConfigCreation::new(&path, b"new bytes").unwrap()],
        )
        .unwrap();
        if replace {
            fs::rename(&path, root.join("retained-owned")).unwrap();
            fs::write(&path, b"new bytes").unwrap();
        } else {
            fs::write(&path, b"foreign").unwrap();
        }
        let journal = fs::read(reg.journal_path()).unwrap();
        assert!(reg.disconnect().is_err());
        assert_eq!(fs::read(&old).unwrap(), b"candidate");
        assert_eq!(fs::read(reg.journal_path()).unwrap(), journal);
        if replace {
            assert_eq!(fs::read(&path).unwrap(), b"new bytes");
            fs::remove_file(&path).unwrap(); // owned test actor resolves its own conflict
            fs::rename(root.join("retained-owned"), &path).unwrap();
        } else {
            assert_eq!(fs::read(&path).unwrap(), b"foreign");
            fs::write(&path, b"new bytes").unwrap();
        }
        reg.disconnect().unwrap();
        assert_eq!(fs::read(&old).unwrap(), b"original");
        assert!(!path.exists());
    }
}

#[test]
fn overlap_size_bounds_and_tampered_creation_intent_fail_closed() {
    let root = fixture();
    let state = root.join("state");
    let reg = Registration::open(&state).unwrap();
    let path = root.join("new.toml");
    for other in [path.clone(), path.join("child"), state.join("journal.json")] {
        assert!(
            reg.apply_with_files(
                &[],
                &[],
                &[
                    ConfigCreation::new(&path, b"a").unwrap(),
                    ConfigCreation::new(&other, b"b").unwrap(),
                ]
            )
            .is_err()
        );
        assert!(!path.exists());
        assert!(!reg.journal_path().exists());
    }
    assert!(ConfigCreation::new(&path, &vec![0; 16 * 1024 * 1024 + 1]).is_err());
    reg.apply_with_files(&[], &[], &[ConfigCreation::new(&path, b"data").unwrap()])
        .unwrap();
    let original = fs::read(reg.journal_path()).unwrap();
    let mut journal: serde_json::Value = serde_json::from_slice(&original).unwrap();
    journal["creations"][0]["bytes"] = serde_json::json!([0]);
    fs::write(reg.journal_path(), serde_json::to_vec(&journal).unwrap()).unwrap();
    assert!(reg.disconnect().is_err());
    assert_eq!(fs::read(&path).unwrap(), b"data");
    fs::write(reg.journal_path(), original).unwrap();
    reg.disconnect().unwrap();
}
