#![cfg(windows)]
use std::{fs, process::Command};

#[test]
fn native_mcp_options_fail_before_opening_a_connection_or_creating_state() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["mcp"],
        vec!["mcp", "unknown"],
        vec!["mcp", "broker-retire"],
        vec!["mcp", "broker-retire", "--unknown", "x"],
        vec!["mcp", "codebase-memory", "--executable"],
        vec!["mcp", "codebase-memory", "--unknown", "x"],
        vec!["mcp", "codebase-memory", "--cache", "one", "--cache", "two"],
        vec!["mcp", "codebase-memory", "--connection-seconds", "0"],
        vec!["mcp", "codebase-memory", "--connection-seconds", "86401"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .current_dir(root.path())
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("invalid native MCP command options")
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
    let help = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .current_dir(root.path())
        .args(["mcp", "codebase-memory", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--catalogue-file"));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn prepare_and_retire_commands_use_an_explicit_owned_private_root() {
    use harness_core::broker_state::BrokerRoot;
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["mcp", "broker-prepare"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let path = std::path::PathBuf::from(result["broker_root"].as_str().unwrap());
    let root = BrokerRoot::open(&path).unwrap();
    let retired = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["mcp", "broker-retire", "--root"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        retired.status.success(),
        "{}",
        String::from_utf8_lossy(&retired.stderr)
    );
    let status: serde_json::Value = serde_json::from_slice(&retired.stdout).unwrap();
    assert_eq!(status["state"], "absent");
    assert!(path.join("broker-owner.json").is_file());
    assert!(!path.join("service.log").exists());
    drop(root);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn recorded_mcp_runtime_rejects_stale_source_while_management_remains_available() {
    use harness_core::build_identity::{self, BINARIES, BuildRecord, INSPECTION_SCHEMA, SCHEMA};
    use std::collections::BTreeMap;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let build = root.path().join("build");
    for directory in [
        source.join("crates/one/src"),
        source.join("tools/rtk-adapter/src"),
        source.join(INSPECTION_SCHEMA).parent().unwrap().into(),
        build.clone(),
    ] {
        fs::create_dir_all(directory).unwrap();
    }
    for file in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/one/src/lib.rs",
        INSPECTION_SCHEMA,
    ] {
        fs::write(source.join(file), "owned identity fixture").unwrap();
    }
    // Optional pinned baseline for the same actual-entrypoint regression.
    let selected = std::env::var_os("HARNESS_RUNTIME_GATE_MANAGER")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_codex-harness").into());
    let manager = build.join("codex-harness.exe");
    fs::copy(selected, &manager).unwrap();
    let mut binaries = BTreeMap::new();
    for name in BINARIES {
        let path = build.join(name);
        if *name != "codex-harness.exe" {
            fs::write(&path, name).unwrap();
        }
        binaries.insert(
            (*name).to_owned(),
            build_identity::hash_file(&path).unwrap(),
        );
    }
    // Fabricated owned identity metadata tests the gate; this is not release-build acceptance.
    let record = BuildRecord {
        schema: SCHEMA,
        source_root: source.clone(),
        source: build_identity::source_identity(&source).unwrap(),
        rustc: "fixture".into(),
        cargo: "fixture".into(),
        target: "x86_64-pc-windows-msvc".into(),
        profile: "release".into(),
        binaries,
    };
    fs::write(
        build.join("build.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let invoke = || {
        Command::new(&manager)
            .current_dir(root.path())
            .args(["mcp", "codebase-memory", "--help"])
            .output()
            .unwrap()
    };
    let healthy = invoke();
    assert!(
        healthy.status.success(),
        "{}",
        String::from_utf8_lossy(&healthy.stderr)
    );
    fs::write(source.join("crates/one/src/lib.rs"), "changed owned source").unwrap();
    let stale = invoke();
    assert_eq!(
        stale.status.code(),
        Some(2),
        "source-stale MCP runtime was admitted"
    );
    assert!(stale.stdout.is_empty());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("ordinary runtime is disabled"));
    let check = Command::new(&manager)
        .current_dir(root.path())
        .args(["check", "--build"])
        .arg(&build)
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1));
    let checked: serde_json::Value = serde_json::from_slice(&check.stdout).unwrap();
    assert_eq!(checked["status"], "source-stale");
    assert_eq!(checked["management_allowed"], true);
    assert_eq!(checked["runtime_allowed"], false);
    let prepared = harness_core::broker_state::BrokerRoot::prepare().unwrap();
    let retired = Command::new(&manager)
        .args(["mcp", "broker-retire", "--root"])
        .arg(prepared.root().path())
        .output()
        .unwrap();
    assert!(
        retired.status.success(),
        "{}",
        String::from_utf8_lossy(&retired.stderr)
    );
    let retired: serde_json::Value = serde_json::from_slice(&retired.stdout).unwrap();
    assert_eq!(retired["state"], "absent");
    // Source loss has the same runtime gate and cannot trigger acquisition.
    fs::rename(&source, root.path().join("moved-source")).unwrap();
    let missing = invoke();
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("ordinary runtime is disabled"));
}
