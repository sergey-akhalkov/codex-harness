#![cfg(windows)]
use std::{fs, path::Path, process::Command};

fn invoke(source: &Path, home: &Path, command: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args([command, "--source"])
        .arg(source)
        .arg("--codex-home")
        .arg(home)
        .output()
        .unwrap()
}

#[test]
fn legacy_migration_preserves_local_conflicts_unknown_fields_and_recovery() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let home = root.path().join("home");
    fs::create_dir_all(source.join("global")).unwrap();
    fs::create_dir_all(&home).unwrap();
    let shared = source.join("global/harness.config.toml");
    let shared_text = "model = 'shared-fixture'\napproval_policy = 'never'\nsandbox_mode = 'danger-full-access'\n[projects.'C:/synthetic-consumer']\ntrust_level = 'trusted'\n[tui]\nstatus_line = ['model']\n";
    fs::write(&shared, shared_text).unwrap();
    let before = "# user note\nmodel = 'local-fixture'\n[unrelated]\nkeep = true\n";
    fs::write(home.join("config.toml"), before).unwrap();
    fs::write(home.join("auth.json"), "synthetic-auth-sentinel").unwrap();
    std::os::windows::fs::symlink_file(&shared, home.join("harness.config.toml")).unwrap();
    let migrated = invoke(&source, &home, "config-localize");
    assert!(
        migrated.status.success(),
        "{}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    assert!(String::from_utf8_lossy(&migrated.stderr).contains("1 configuration conflicts"));
    assert!(!String::from_utf8_lossy(&migrated.stderr).contains("local-fixture"));
    let bytes = fs::read_to_string(home.join("config.toml")).unwrap();
    let local: toml::Table = bytes.parse().unwrap();
    assert_eq!(local["model"].as_str(), Some("local-fixture"));
    assert_eq!(local["unrelated"]["keep"].as_bool(), Some(true));
    assert_eq!(
        local["projects"]["C:/synthetic-consumer"]["trust_level"].as_str(),
        Some("trusted")
    );
    assert!(!local.contains_key("approval_policy"));
    assert_eq!(fs::read_to_string(&shared).unwrap(), shared_text);
    assert_eq!(
        fs::read_to_string(home.join("auth.json")).unwrap(),
        "synthetic-auth-sentinel"
    );
    let backups: Vec<_> = fs::read_dir(home.join("harness/private-profile-migration"))
        .unwrap()
        .map(|entry| fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect();
    assert!(backups.iter().any(|text| text == shared_text));
    assert!(backups.iter().any(|text| text == before));
    assert!(invoke(&source, &home, "config-localize").status.success());
    assert_eq!(fs::read_to_string(home.join("config.toml")).unwrap(), bytes);
    // The standard install journal owns retiring this link after migration.
    fs::remove_file(home.join("harness.config.toml")).unwrap();
    assert!(invoke(&source, &home, "config-localize").status.success());
}

#[test]
fn malformed_or_foreign_local_state_is_preserved_without_source_disclosure() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let home = root.path().join("home");
    fs::create_dir_all(source.join("global")).unwrap();
    fs::create_dir_all(&home).unwrap();
    let shared = source.join("global/harness.config.toml");
    fs::write(&shared, "model = 'synthetic-private-model'\n").unwrap();
    std::os::windows::fs::symlink_file(&shared, home.join("harness.config.toml")).unwrap();
    fs::write(
        home.join("config.toml"),
        "invalid [ synthetic-private-value",
    )
    .unwrap();
    let output = invoke(&source, &home, "config-localize");
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-private"));
    assert_eq!(
        fs::read_to_string(home.join("config.toml")).unwrap(),
        "invalid [ synthetic-private-value"
    );
    assert_eq!(
        home.join("harness.config.toml").canonicalize().unwrap(),
        shared.canonicalize().unwrap()
    );
    assert!(!home.join("harness/private-profile-migration").exists());
}
