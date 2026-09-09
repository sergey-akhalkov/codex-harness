#![cfg(windows)]

use harness_core::feature_edit::discover_features;
use std::{fs, path::PathBuf, time::Duration};

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX; model-free native observation without config"]
fn native_discovery_does_not_create_an_absent_configuration() {
    let upstream = PathBuf::from(
        std::env::var_os("HARNESS_NATIVE_CODEX").expect("original native executable"),
    );
    let home = tempfile::Builder::new()
        .prefix("harness-feature-empty-home-")
        .tempdir()
        .unwrap()
        .keep();
    let observed = discover_features(&upstream, &home, Duration::from_secs(30)).unwrap();
    assert!(!home.join("config.toml").exists());
    println!(
        "empty home: {}; evidence: {}",
        home.display(),
        observed.evidence.display()
    );
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX; model-free owned native feature observation"]
fn native_discovery_selects_exact_home_and_preserves_configurations() {
    let upstream = PathBuf::from(
        std::env::var_os("HARNESS_NATIVE_CODEX").expect("original native executable"),
    );
    let root = tempfile::Builder::new()
        .prefix("harness-feature-discovery-")
        .tempdir()
        .unwrap()
        .keep();
    println!("native discovery fixture: {}", root.display());
    for (name, hooks, code_mode) in [("home-a", false, true), ("home-b", true, false)] {
        let home = root.join(name);
        fs::create_dir(&home).unwrap();
        let config = format!(
            "# PRIVATE_DISCOVERY_SENTINEL\nmodel = 'gpt-6-astra'\n[features]\nhooks = {hooks}\ncode_mode = {code_mode}\n"
        );
        fs::write(home.join("config.toml"), &config).unwrap();
        fs::write(home.join("keep"), b"unrelated").unwrap();
        let observed = discover_features(&upstream, &home, Duration::from_secs(30)).unwrap();
        assert_eq!((observed.hooks, observed.code_mode), (hooks, code_mode));
        assert_eq!(
            fs::read_to_string(home.join("config.toml")).unwrap(),
            config
        );
        assert_eq!(fs::read(home.join("keep")).unwrap(), b"unrelated");
        assert!(!format!("{observed:?}").contains("PRIVATE_DISCOVERY_SENTINEL"));
        let process: serde_json::Value =
            serde_json::from_slice(&fs::read(observed.evidence.join("process.json")).unwrap())
                .unwrap();
        assert_eq!(process["exit_code"], 0);
        println!(
            "feature discovery evidence: {}",
            observed.evidence.display()
        );
    }
    let malformed = root.join("malformed");
    fs::create_dir(&malformed).unwrap();
    let config = b"PRIVATE_DISCOVERY_SENTINEL = [";
    fs::write(malformed.join("config.toml"), config).unwrap();
    let error = discover_features(&upstream, &malformed, Duration::from_secs(30)).unwrap_err();
    assert!(!error.to_string().contains("PRIVATE_DISCOVERY_SENTINEL"));
    assert!(error.to_string().contains("command did not succeed"));
    assert_eq!(fs::read(malformed.join("config.toml")).unwrap(), config);
    println!("private failure: {error}");
    let absent = root.join("absent-home");
    assert!(discover_features(&upstream, &absent, Duration::from_secs(30)).is_err());
    assert!(!absent.exists());
}
