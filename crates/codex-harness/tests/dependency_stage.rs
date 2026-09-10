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
#[ignore = "explicit public npm/GitHub Codebase Memory archive preparation; no package execution"]
fn actual_official_native_zip_streams_a_large_executable_into_owned_staging() {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let root = tempfile::tempdir().unwrap();
    let temporary = root.path().join("temporary");
    fs::create_dir(&temporary).unwrap();
    let state = root.path().join("state");
    let output = package_command(&state, "codebase-memory-mcp", "0.10.8")
        .current_dir(root.path())
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
    assert_eq!(report["activation_allowed"], false);
    assert_eq!(report["package_code_executed"], false);
    assert_eq!(report["runtime_compatibility"], "not-probed");
    assert_eq!(report["file_count"], 5);
    assert_eq!(report["native_asset"]["asset_id"], 520282424u64);
    assert_eq!(report["native_asset"]["archive_bytes"], 39172588u64);
    assert_eq!(
        report["native_asset"]["archive_sha256"],
        "b43ad982994c4d829670749e08d3b622a74bb20041fc0a7d02bef6113f81c34d"
    );
    let stage = Path::new(report["stage"].as_str().unwrap());
    assert_eq!(stage.parent().unwrap(), state.join("dependency-staging"));
    let manifest_bytes = fs::read(stage.join("manifest.json")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&manifest_bytes)),
        report["manifest_sha256"].as_str().unwrap()
    );
    let manifest: Value = serde_json::from_slice(&manifest_bytes).unwrap();
    let mut native_found = false;
    for file in manifest["contents"]["files"].as_array().unwrap() {
        let name = file["path"].as_str().unwrap();
        let path = stage.join("package").join(name);
        let mut input = fs::File::open(path).unwrap();
        assert_eq!(
            input.metadata().unwrap().len(),
            file["size"].as_u64().unwrap()
        );
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let size = input.read(&mut buffer).unwrap();
            if size == 0 {
                break;
            }
            hash.update(&buffer[..size]);
        }
        assert_eq!(
            format!("{:x}", hash.finalize()),
            file["sha256"].as_str().unwrap()
        );
        if name == "bin/codebase-memory-mcp.exe" {
            assert_eq!(file["size"], 296140288u64);
            native_found = true;
        }
    }
    assert!(native_found);
    assert_eq!(fs::read_dir(&temporary).unwrap().count(), 0);
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
