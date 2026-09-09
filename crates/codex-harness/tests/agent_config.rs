#![cfg(windows)]

use harness_core::{agent_config, inventory::Agent};
use std::{fs, os::windows::fs::symlink_file, path::PathBuf, process::Command};

#[test]
fn actual_inventory_checks_semantic_base_roles_without_executing_configuration() {
    let root = tempfile::Builder::new()
        .prefix("harness-agent-config-")
        .tempdir()
        .unwrap();
    let home = root.path().join("codex-home");
    let user = root.path().join("absent-user");
    fs::create_dir(&home).unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let config = home.join("config.toml");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .arg("inventory")
            .arg("--source")
            .arg(&source)
            .arg("--codex-home")
            .arg(&home)
            .arg("--user-home")
            .arg(&user)
            .current_dir(root.path())
            .output()
            .unwrap()
    };
    // Use TOML 1.1 multiline inline tables and escaped keys, both parsed by the
    // pinned upstream CLI's toml version. No referenced executable is invoked.
    let collision =
        b"agents = {\n \"princi\\u0070al\" = { config_file = 'private-sentinel', },\n}\n";
    fs::write(&config, collision).unwrap();
    let output = run();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("agent name collision in base configuration")
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("private-sentinel"));
    assert_eq!(fs::read(&config).unwrap(), collision);
    let unrelated = b"note = '''\n[agents.principal]\n'''\n[mcp_servers.inert]\ncommand = 'private-sentinel-never-run'\n";
    fs::write(&config, unrelated).unwrap();
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&config).unwrap(), unrelated);
    assert!(!user.exists());
    assert_eq!(fs::read_dir(&home).unwrap().count(), 1);
}

#[test]
fn reparse_and_malformed_base_configurations_are_preserved_private_failures() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    fs::create_dir(&home).unwrap();
    let config = home.join("config.toml");
    let foreign = root.path().join("foreign");
    let agents = vec![Agent {
        name: "principal".into(),
        source: "inert".into(),
    }];
    fs::write(&foreign, b"private-sentinel = {").unwrap();
    symlink_file(&foreign, &config).unwrap();
    assert!(agent_config::check(&home, &agents).is_err());
    assert_eq!(fs::read_link(&config).unwrap(), foreign);
    assert_eq!(fs::read(&foreign).unwrap(), b"private-sentinel = {");
    fs::remove_file(&config).unwrap();
    fs::write(&config, b"private-sentinel = {").unwrap();
    let error = agent_config::check(&home, &agents).unwrap_err();
    assert!(!error.to_string().contains("private-sentinel"));
    assert_eq!(fs::read(&config).unwrap(), b"private-sentinel = {");
}

#[test]
#[ignore = "requires explicit HARNESS_NATIVE_CODEX; actual model-free TOML 1.1 compatibility"]
fn actual_upstream_and_preflight_accept_toml_1_1_configuration() {
    let upstream = PathBuf::from(
        std::env::var_os("HARNESS_NATIVE_CODEX").expect("explicit native executable required"),
    );
    let root = tempfile::Builder::new()
        .prefix("harness-agent-toml11-")
        .tempdir()
        .unwrap()
        .keep();
    println!("TOML 1.1 evidence: {}", root.display());
    let home = root.join("codex-home");
    fs::create_dir(&home).unwrap();
    let config = home.join("config.toml");
    let bytes = b"features = {\n hooks = false,\n code_mode = true,\n}\n";
    fs::write(&config, bytes).unwrap();
    agent_config::check(&home, &[]).unwrap();
    let observation = harness_core::feature_edit::discover_features(
        &upstream,
        &home,
        std::time::Duration::from_secs(30),
    )
    .unwrap();
    assert!(!observation.hooks);
    assert!(observation.code_mode);
    assert_eq!(fs::read(&config).unwrap(), bytes);
    println!(
        "native process evidence: {}",
        observation.evidence.display()
    );
}
