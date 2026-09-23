#![cfg(windows)]
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn command(state: &Path, version: &str) -> Command {
    package_command(state, "@nuphus/nuphus-mcp", version)
}

fn package_command(state: &Path, package: &str, version: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    command
        .args([
            "dependencies",
            "stage",
            "--package",
            package,
            "--version",
            version,
            "--state",
        ])
        .arg(state);
    command
}

#[test]
fn invalid_version_and_foreign_state_are_preserved_without_acquisition() {
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("absent");
    let output = command(&absent, "PRIVATE-TOKEN/latest")
        .current_dir(root.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-TOKEN"));
    assert!(!absent.exists());
    fs::create_dir(&absent).unwrap();
    fs::write(absent.join("foreign"), b"retain all bytes").unwrap();
    let output = command(&absent, "0.2.2")
        .current_dir(root.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        fs::read(absent.join("foreign")).unwrap(),
        b"retain all bytes"
    );
    assert_eq!(fs::read_dir(&absent).unwrap().count(), 1);

    let unsupported = root.path().join("unsupported");
    let output = command(&unsupported, "0.2.2")
        .arg("--preview")
        .current_dir(root.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!unsupported.exists());
}

#[test]
fn codegraph_checksum_mismatch_is_rejected_without_creating_owned_state() {
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("absent-codegraph-state");
    let output = package_command(&absent, "@colbymchenry/codegraph", "1.6.1")
        .current_dir(root.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!absent.exists());
}

#[test]
#[ignore = "explicit public npm archive preparation and 404; only owned staging is written"]
fn actual_official_candidate_and_failed_download_preserve_prior_staging() {
    let root = tempfile::Builder::new()
        .prefix("native staging проверка-")
        .tempdir()
        .unwrap();
    let state = root.path().join("state");
    let temporary = root.path().join("temporary");
    fs::create_dir(&temporary).unwrap();
    let output = command(&state, "0.2.2")
        .current_dir(root.path())
        .env("PATH", root.path())
        .env("TEMP", &temporary)
        .env("TMP", &temporary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "staged-unverified");
    assert_eq!(report["activation_allowed"], false);
    assert_eq!(report["file_count"], 4);
    assert_eq!(report["model_calls"], 0);
    assert_eq!(report["package_code_executed"], false);
    let stage = Path::new(report["stage"].as_str().unwrap());
    assert_eq!(stage.parent().unwrap(), state.join("dependency-staging"));
    let before = fs::read(stage.join("manifest.json")).unwrap();
    let manifest: Value = serde_json::from_slice(&before).unwrap();
    for file in manifest["contents"]["files"].as_array().unwrap() {
        let path = stage.join("package").join(file["path"].as_str().unwrap());
        assert_eq!(
            fs::metadata(&path).unwrap().len(),
            file["size"].as_u64().unwrap()
        );
        use sha2::{Digest, Sha256};
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(path).unwrap())),
            file["sha256"].as_str().unwrap()
        );
    }
    assert_eq!(fs::read_dir(&temporary).unwrap().count(), 0);
    let failed = command(&state, "0.0.0")
        .current_dir(root.path())
        .env("TEMP", &temporary)
        .env("TMP", &temporary)
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(2));
    assert!(failed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("official-source-http-error"));
    assert_eq!(fs::read(stage.join("manifest.json")).unwrap(), before);
    assert_eq!(
        fs::read_dir(state.join("dependency-staging"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(fs::read_dir(&temporary).unwrap().count(), 0);
}
