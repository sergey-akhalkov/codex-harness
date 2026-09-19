//! Configured executor dispatch. Live subscription work is opt-in.
#![cfg(windows)]

use serde_json::Value;
use std::{fs, path::PathBuf, process::Command, time::Duration};

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

#[test]
fn configured_executor_rejects_unlisted_profile() {
    let root = std::env::temp_dir().join(format!("executor-spawn-{}", std::process::id()));
    let source = root.join("source");
    let home = root.join("home");
    let workspace = root.join("workspace");
    fs::create_dir_all(source.join("global")).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    fs::write(
        source.join("global/orchestration.toml"),
        include_str!("../../../global/orchestration.toml"),
    )
    .unwrap();
    let out = Command::new(manager())
        .args([
            "executor",
            "spawn",
            "--source",
            source.to_str().unwrap(),
            "--codex-home",
            home.to_str().unwrap(),
            "--workspace",
            workspace.to_str().unwrap(),
            "--profile",
            "zai",
            "--exec",
            "unused",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("is not an executor"), "{err}");
    assert!(!err.to_lowercase().contains("substitut"), "{err}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn shared_checkout_without_worktrees_is_refused() {
    let root = std::env::temp_dir().join(format!("executor-wt-{}", std::process::id()));
    let source = root.join("source");
    let home = root.join("home");
    let workspace = root.join("workspace");
    fs::create_dir_all(source.join("global")).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    fs::write(
        source.join("global/orchestration.toml"),
        include_str!("../../../global/orchestration.toml"),
    )
    .unwrap();
    fs::write(home.join("xai.config.toml"), "model = 'grok-4.6'\n").unwrap();
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(&workspace)
        .status()
        .unwrap();
    fs::write(workspace.join(".git/keep"), b"").unwrap();
    // A real git dir is enough for shared-checkout detection.
    let out = Command::new(manager())
        .args([
            "executor",
            "spawn",
            "--source",
            source.to_str().unwrap(),
            "--codex-home",
            home.to_str().unwrap(),
            "--workspace",
            workspace.to_str().unwrap(),
            "--exec",
            "unused",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        err.contains("managed worktrees unavailable")
            || err.contains("refusing the shared checkout"),
        "{err}"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
#[ignore = "requires HARNESS_LIVE_CODEX_HOME with xai profile and subscription; owned temp workspace only"]
fn configured_xai_executor_serves_a_subscribed_tool_from_an_external_workspace() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let home = PathBuf::from(std::env::var_os("HARNESS_LIVE_CODEX_HOME").expect("live Codex home"));
    assert!(home.is_absolute());
    let workspace = std::env::temp_dir().join(format!(
        "executor-xai-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::create_dir_all(&workspace).unwrap();
    let mut child = Command::new(manager());
    child
        .args([
            "executor",
            "spawn",
            "--source",
            source.canonicalize().unwrap().to_str().unwrap(),
            "--codex-home",
            home.to_str().unwrap(),
            "--workspace",
            workspace.to_str().unwrap(),
            "--exec",
            "Create proof.txt containing exactly orch-xai. Use a local shell tool. Do not spawn agents.",
        ])
        .current_dir(&workspace);
    let out = child.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "executor spawn failed: {stderr}{stdout}"
    );
    let receipt: Value =
        serde_json::from_slice(&fs::read(workspace.join("executor-spawn.json")).unwrap()).unwrap();
    assert_eq!(receipt["profile"], "xai");
    assert_eq!(receipt["args"][0], "--profile");
    assert_eq!(receipt["args"][1], "xai");
    assert_eq!(receipt["model"], "grok-4.6");
    assert_eq!(receipt["modelProvider"], "xai");
    assert_eq!(receipt["reasoningEffort"], "xhigh");
    assert_eq!(receipt["visible"], true);
    assert!(
        !receipt["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == "exec" || arg == "--json"),
        "visible spawn must not be headless exec: {receipt}"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt"))
            .unwrap()
            .trim(),
        "orch-xai"
    );
    let _ = stdout;
    let _ = Duration::from_secs(1);
    let _ = fs::remove_dir_all(&workspace);
}
