#![cfg(windows)]
use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn invoke(args: &[&str], cwd: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap()
}

#[test]
fn apply_help_has_no_package_or_state_side_effects() {
    let root = tempfile::tempdir().unwrap();
    for operation in ["apply", "update"] {
        let output = invoke(&["dependencies", operation, "--help"], root.path());
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Check and preview"));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn apply_check_and_preview_are_silent_and_preserve_opencode_cache() {
    let root = tempfile::tempdir().unwrap();
    let user = root.path().join("user");
    let clangd = user.join(".cache/opencode/bin/clangd_20/bin");
    fs::create_dir_all(&clangd).unwrap();
    let cache = clangd.join("clangd.exe");
    fs::write(&cache, b"shared-opencode-cache").unwrap();
    let state = root.path().join("absent-state");
    let source = repo();
    for extra in ["--preview", "--check"] {
        let output = invoke(
            &[
                "dependencies",
                "apply",
                "--source",
                source.to_str().unwrap(),
                "--user-home",
                user.to_str().unwrap(),
                "--state",
                state.to_str().unwrap(),
                extra,
            ],
            root.path(),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["operation"], "apply");
        assert_eq!(report["read_only"], true);
        assert_eq!(report["mutated"], false);
        assert_eq!(report["packages_acquired"], false);
        assert_eq!(report["model_calls"], 0);
        assert!(!state.exists());
        assert_eq!(fs::read(&cache).unwrap(), b"shared-opencode-cache");
        for item in report["results"].as_array().unwrap() {
            assert_ne!(item["state"], "updated");
            assert_eq!(item["packages_acquired"], false);
        }
    }
}

#[test]
fn apply_rejects_ambiguous_options_before_mutation() {
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("absent");
    for options in [
        vec!["apply", "--preview", "--check"],
        vec![
            "apply",
            "--source",
            "PRIVATE-SOURCE",
            "--user-home",
            absent.to_str().unwrap(),
        ],
        vec!["update", "--state", absent.to_str().unwrap(), "--preview"],
    ] {
        let output = invoke(
            &std::iter::once("dependencies")
                .chain(options.iter().copied())
                .collect::<Vec<_>>(),
            root.path(),
        );
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-"));
        assert!(!absent.exists());
    }
}
