//! Inert external-language analysis samples stay data, not harness helpers.
use harness_core::analysis_samples;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn checkout() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn observe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-observe"))
}

fn inspect() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-inspect"))
}

#[test]
fn declared_analysis_samples_exist_as_test_data() {
    let root = checkout();
    for path in analysis_samples::declared_paths(&root) {
        assert!(path.is_file(), "{}", path.display());
        assert!(analysis_samples::is_declared_sample(&root, &path));
        assert!(analysis_samples::is_sample_program(&path));
        assert!(analysis_samples::looks_like_python_sample(&path));
        let error = analysis_samples::refuse_sample_program(&path).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("inert analysis sample cannot execute as a harness helper")
        );
        let error = analysis_samples::refuse_helper_launch(&root, &path).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("inert analysis sample cannot execute as a harness helper")
        );
    }
}

#[test]
fn observe_and_inspect_refuse_declared_samples_as_helpers() {
    let root = checkout();
    let sample = analysis_samples::declared_paths(&root)
        .into_iter()
        .next()
        .unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let observed = Command::new(observe())
        .args([
            "--cwd",
            cwd.path().to_str().unwrap(),
            "--timeout",
            "10",
            "--",
            sample.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(observed.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&observed.stderr);
    assert!(
        stderr.contains("inert analysis sample cannot execute as a harness helper"),
        "{stderr}"
    );

    let prompt = cwd.path().join("prompt.txt");
    fs::write(&prompt, "inspect this fixture").unwrap();
    let inspected = Command::new(inspect())
        .args([
            "--cwd",
            cwd.path().to_str().unwrap(),
            "--prompt-file",
            prompt.to_str().unwrap(),
            "--command-json",
            &serde_json::to_string(&[sample.to_str().unwrap()]).unwrap(),
            "--oracle-json",
            &serde_json::to_string(&[observe().to_str().unwrap()]).unwrap(),
            "--model",
            "fixture-model",
            "--provider",
            "fixture-provider",
            "--subscription",
            "fixture-subscription",
            "--input",
            "prompt.txt",
            "--schema",
            checkout()
                .join(".agents/skills/structured-codex-run/assets/inspection.schema.json")
                .to_str()
                .unwrap(),
        ])
        .output()
        .unwrap();
    assert_ne!(inspected.status.code(), Some(0));
    let inspect_err = format!(
        "{}{}",
        String::from_utf8_lossy(&inspected.stdout),
        String::from_utf8_lossy(&inspected.stderr)
    );
    assert!(
        inspect_err.contains("inert analysis sample cannot execute as a harness helper"),
        "{inspect_err}"
    );
}

#[test]
fn native_helper_executables_remain_distinct_from_samples() {
    analysis_samples::require_native_helper(&observe()).unwrap();
    analysis_samples::refuse_sample_program(&observe()).unwrap();
    let sample = analysis_samples::declared_paths(&checkout())
        .into_iter()
        .next()
        .unwrap();
    let error = analysis_samples::require_native_helper(&sample).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Native helper must be an absolute EXE")
    );
}
