//! Isolated native subscription login CLI. Never targets the live global proxy.
#![cfg(windows)]

use std::{fs, process::Command};

#[test]
fn subscription_login_help_is_native() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["subscription-login", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("subscription-login xai|zai"), "{text}");
    assert!(!text.to_ascii_lowercase().contains("opencodex-login.ps1"));
    assert!(
        text.contains("Does not require an OpenCodex package") || text.contains("harness store"),
        "{text}"
    );
}

#[test]
fn subscription_login_zai_key_file_writes_store_and_preserves_profile() {
    let root = tempfile::tempdir().unwrap();
    let source = std::path::absolute(root.path().join("source")).unwrap();
    let home = std::path::absolute(root.path().join("codex")).unwrap();
    let user = std::path::absolute(root.path().join("user")).unwrap();
    fs::create_dir_all(source.join("global/opencodex/agents")).unwrap();
    // The retired OpenCodex kit sources are gone; the CLI no longer requires
    // them, so ordinary fixture files are enough for an isolated login.
    fs::write(
        source.join("global/opencodex/config.json"),
        b"{\"hostname\":\"127.0.0.1\",\"port\":10100}",
    )
    .unwrap();
    fs::write(source.join("global/opencodex/agents/middle.toml"), b"role").unwrap();
    fs::create_dir_all(user.join(".opencodex")).unwrap();
    std::os::windows::fs::symlink_file(
        source.join("global/opencodex/config.json"),
        user.join(".opencodex/config.json"),
    )
    .unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("zai.config.toml"), b"keep-profile").unwrap();
    let key = std::path::absolute(root.path().join("key.txt")).unwrap();
    fs::write(&key, b"cli-zai-secret\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args([
            "subscription-login",
            "zai",
            "--source",
            source.to_str().unwrap(),
            "--codex-home",
            home.to_str().unwrap(),
            "--user-home",
            user.to_str().unwrap(),
            "--key-file",
            key.to_str().unwrap(),
            "--no-open-browser",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(!stdout.contains("cli-zai-secret") && !stderr.contains("cli-zai-secret"));
    assert_eq!(
        fs::read(home.join("harness/subscriptions/zai-key.txt")).unwrap(),
        b"cli-zai-secret"
    );
    assert_eq!(
        fs::read(home.join("zai.config.toml")).unwrap(),
        b"keep-profile"
    );
}

#[test]
fn xai_responses_probe_help_is_native() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["xai-responses-probe", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("xai-responses-probe --user-home"), "{text}");
    assert!(!text.to_ascii_lowercase().contains("access_token"));
    assert!(!text.contains("opencodex-login.ps1"));
}

#[test]
fn xai_token_help_is_native() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["xai-token", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("xai-token --codex-home"), "{text}");
    assert!(!text.contains("access_token"));
}

#[test]
fn xai_token_missing_store_writes_nothing_to_stdout() {
    let root = tempfile::tempdir().unwrap();
    let home = std::path::absolute(root.path().join("codex")).unwrap();
    fs::create_dir_all(&home).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["xai-token", "--codex-home", home.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing") || stderr.contains("xAI OAuth store"),
        "{stderr}"
    );
    assert!(!stderr.contains("eyJ"));
}

#[test]
fn xai_token_valid_store_prints_only_access_token() {
    let root = tempfile::tempdir().unwrap();
    let home = std::path::absolute(root.path().join("codex")).unwrap();
    let store = home.join("harness/subscriptions/xai-oauth.json");
    fs::create_dir_all(store.parent().unwrap()).unwrap();
    fs::write(
        &store,
        br#"{
  "xai": {
    "activeAccountId": "fixture",
    "accounts": [{
      "id": "fixture",
      "credential": {
        "access": "access-token-value",
        "refresh": "refresh-token-value",
        "expires": 4102444800000,
        "source": "oauth"
      }
    }]
  }
}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["xai-token", "--codex-home", home.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert_eq!(stdout.trim(), "access-token-value");
    assert!(!stdout.contains("refresh-token-value"));
    assert!(!stderr.contains("access-token-value"));
    assert!(!stderr.contains("refresh-token-value"));
}
