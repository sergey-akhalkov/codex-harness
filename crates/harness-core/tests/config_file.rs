#![cfg(windows)]

use harness_core::config_file::ConfigSnapshot;
use std::{
    fs,
    os::windows::fs::{MetadataExt, symlink_dir, symlink_file},
};

#[test]
fn existing_configuration_commits_whole_bytes_and_supports_guarded_rollback() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("конфигурация.toml");
    let before = b"# PRIVATE_CONFIG_SENTINEL\r\n[features]\r\nhooks = false\r\n";
    fs::write(&path, before).unwrap();
    let attributes = fs::metadata(&path).unwrap().file_attributes();
    let baseline = ConfigSnapshot::read(&path).unwrap();
    assert_eq!(baseline.contents(), before);
    baseline.verify_unchanged().unwrap();
    assert!(!format!("{baseline:?}").contains("PRIVATE_CONFIG_SENTINEL"));
    for after in [vec![], b"short".to_vec(), vec![b'x'; 128 * 1024]] {
        let published = baseline.replace(&after).unwrap();
        assert_eq!(fs::read(&path).unwrap(), after);
        assert_eq!(published.contents(), after);
        assert_eq!(fs::metadata(&path).unwrap().file_attributes(), attributes);
        published.replace(before).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

#[test]
fn changed_bytes_and_same_content_replacement_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    fs::write(&path, b"original").unwrap();
    let baseline = ConfigSnapshot::read(&path).unwrap();
    fs::write(&path, b"foreign!").unwrap();
    assert!(baseline.verify_unchanged().is_err());
    assert!(baseline.replace(b"candidate").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"foreign!");
    fs::rename(&path, root.path().join("retained-original")).unwrap();
    fs::write(&path, b"original").unwrap();
    assert!(baseline.replace(b"candidate").is_err());
    assert!(baseline.verify_unchanged().is_err());
    assert_eq!(fs::read(&path).unwrap(), b"original");
    assert_eq!(
        fs::read(root.path().join("retained-original")).unwrap(),
        b"foreign!"
    );
}

#[test]
fn publication_refuses_symlink_hardlink_and_redirected_parent() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("home");
    let foreign = root.path().join("foreign");
    fs::create_dir(&parent).unwrap();
    fs::create_dir(&foreign).unwrap();
    let path = parent.join("config.toml");
    let target = foreign.join("config.toml");
    fs::write(&path, b"original").unwrap();
    fs::write(&target, b"original").unwrap();
    let baseline = ConfigSnapshot::read(&path).unwrap();
    fs::rename(&path, parent.join("retained-original")).unwrap();
    symlink_file(&target, &path).unwrap();
    assert!(ConfigSnapshot::read(&path).is_err());
    assert!(baseline.replace(b"candidate").is_err());
    fs::remove_file(&path).unwrap();
    fs::hard_link(&target, &path).unwrap();
    assert!(ConfigSnapshot::read(&path).is_err());
    assert!(baseline.replace(b"candidate").is_err());
    fs::remove_file(&path).unwrap();
    fs::rename(parent.join("retained-original"), &path).unwrap();
    fs::rename(&parent, root.path().join("retained-home")).unwrap();
    symlink_dir(&foreign, &parent).unwrap();
    assert!(ConfigSnapshot::read(&path).is_err());
    assert!(baseline.replace(b"candidate").is_err());
    assert_eq!(fs::read(&target).unwrap(), b"original");
    assert_eq!(
        fs::read(root.path().join("retained-home/config.toml")).unwrap(),
        b"original"
    );
}

#[test]
fn read_only_and_oversized_publication_preserve_the_original() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    fs::write(&path, b"original").unwrap();
    let baseline = ConfigSnapshot::read(&path).unwrap();
    assert!(baseline.replace(&vec![b'x'; 16 * 1024 * 1024 + 1]).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"original");
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let mut read_only = original_permissions.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&path, read_only).unwrap();
    let outcome = baseline.replace(b"candidate");
    fs::set_permissions(&path, original_permissions).unwrap();
    assert!(outcome.is_err());
    assert_eq!(fs::read(&path).unwrap(), b"original");
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    assert!(ConfigSnapshot::read(&path).is_err());
}
