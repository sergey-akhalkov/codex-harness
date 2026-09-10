//! Actual manager audit entrypoint; package content is inert and test-owned.
#![cfg(windows)]
use serde_json::{Value, json};
use std::{fs, process::Command};

#[test]
fn invalid_identity_and_options_fail_privately_before_acquisition() {
    let root = tempfile::Builder::new()
        .prefix("audit проверка-")
        .tempdir()
        .unwrap();
    let manifest = root.path().join("package.json");
    let body = br#"{"name":"PRIVATE-CREDENTIAL-UNSELECTED","version":"1.2.3"}"#;
    fs::write(&manifest, body).unwrap();
    for option in ["--package-root", "--preview"] {
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["dependencies", "audit", option])
            .arg(root.path())
            .env("PATH", root.path())
            .current_dir(root.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-CREDENTIAL"));
        assert_eq!(fs::read(&manifest).unwrap(), body);
    }
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["dependencies", "audit", "--help"])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
}

#[test]
#[ignore = "explicit public npm metadata/archive GETs; no package or model execution"]
fn actual_official_archive_is_audited_in_the_native_worker_without_installing() {
    let root = tempfile::Builder::new()
        .prefix("audit official проверка-")
        .tempdir()
        .unwrap();
    let package = root.path().join("package");
    fs::create_dir(&package).unwrap();
    let manifest = serde_json::to_vec(
        &json!({"name":"@nuphus/nuphus-mcp","version":"0.2.2","owned_fixture":true}),
    )
    .unwrap();
    fs::write(package.join("package.json"), &manifest).unwrap();
    fs::write(package.join("extra-owned-file"), b"retain this file").unwrap();
    let temporary = root.path().join("private-temporary");
    fs::create_dir(&temporary).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["dependencies", "audit", "--package-root"])
        .arg(&package)
        .env("PATH", root.path())
        .env("CURL_CA_BUNDLE", "PRIVATE-TLS-OVERRIDE")
        .env("TEMP", &temporary)
        .env("TMP", &temporary)
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["package"], "@nuphus/nuphus-mcp");
    assert_eq!(report["version"], "0.2.2");
    assert_eq!(report["status"], "differences-observed");
    assert_eq!(report["files"], 4);
    assert_eq!(report["missing"], 3);
    assert_eq!(report["modified"], 1);
    assert_eq!(report["unavailable"], 0);
    assert_eq!(report["activation_allowed"], false);
    assert_eq!(report["limits"]["job_memory_bytes"], 512 * 1024 * 1024u64);
    assert_eq!(report["model_calls"], 0);
    assert_eq!(report["package_code_executed"], false);
    assert!(!report.to_string().contains("PRIVATE-TLS-OVERRIDE"));
    assert_eq!(fs::read(package.join("package.json")).unwrap(), manifest);
    assert_eq!(
        fs::read(package.join("extra-owned-file")).unwrap(),
        b"retain this file"
    );
    assert_eq!(fs::read_dir(&package).unwrap().count(), 2);
    assert_eq!(fs::read_dir(&temporary).unwrap().count(), 0);

    // The same real entrypoint must clean nested downloads on a source failure.
    let missing = br#"{"name":"@nuphus/nuphus-mcp","version":"0.0.0"}"#;
    fs::write(package.join("package.json"), missing).unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["dependencies", "audit", "--package-root"])
        .arg(&package)
        .env("TEMP", &temporary)
        .env("TMP", &temporary)
        .current_dir(root.path())
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(2));
    assert!(failed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("official-source-http-error"));
    assert_eq!(fs::read(package.join("package.json")).unwrap(), missing);
    assert_eq!(fs::read_dir(&temporary).unwrap().count(), 0);
}
