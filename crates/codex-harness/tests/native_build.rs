//! Real CLI and Cargo cases in owned temporary roots, without models or globals.
#![cfg(windows)]
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(args)
        .output()
        .unwrap()
}

fn fixture(source: &Path) {
    fs::create_dir_all(source.join("crates/manager/src")).unwrap();
    fs::create_dir_all(source.join("tools/rtk-adapter/src")).unwrap();
    fs::write(
        source.join("Cargo.toml"),
        "[workspace]\nmembers=['crates/manager','tools/rtk-adapter']\nresolver='3'\n",
    )
    .unwrap();
    for (directory, name) in [
        ("crates/manager", "codex-harness"),
        ("tools/rtk-adapter", "harness-rtk"),
    ] {
        fs::write(
            source.join(directory).join("Cargo.toml"),
            format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\n"),
        )
        .unwrap();
        fs::write(
            source.join(directory).join("src/main.rs"),
            "fn main() { println!(\"owned native fixture\"); }\n",
        )
        .unwrap();
    }
    let lock = Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(source)
        .output()
        .unwrap();
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
}

#[test]
fn cli_build_reuse_source_staleness_integrity_and_failed_update() {
    let temp = tempfile::Builder::new()
        .prefix("harness-native-build-Ж ")
        .tempdir()
        .unwrap();
    let source = temp.path().join("исходники with spaces");
    let state = temp.path().join("native state");
    fixture(&source);
    let arguments = [
        "build",
        "--source",
        source.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
    ];
    let first = cli(&arguments);
    if !first.status.success() {
        let retained = temp.keep();
        panic!(
            "{}; retained {}",
            String::from_utf8_lossy(&first.stderr),
            retained.display()
        );
    }
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["reused"], false);
    let build = first["build"].as_str().unwrap();
    let healthy = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["check", "--build", build])
        .env("PATH", temp.path().join("no-tools"))
        .output()
        .unwrap();
    assert!(
        healthy.status.success(),
        "{}",
        String::from_utf8_lossy(&healthy.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&healthy.stdout).unwrap()["status"],
        "healthy"
    );
    fs::write(source.join("README.md"), "new source-linked documentation").unwrap();
    let reused: Value = serde_json::from_slice(&cli(&arguments).stdout).unwrap();
    assert_eq!(reused["reused"], true);
    assert_eq!(reused["build"], build);
    let activation_args = [
        "activate-build",
        "--state",
        state.to_str().unwrap(),
        "--build",
        build,
    ];
    let activation = cli(&activation_args);
    assert!(
        activation.status.success(),
        "{}",
        String::from_utf8_lossy(&activation.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&activation.stdout).unwrap()["changed"],
        true
    );
    let pointer = fs::read(state.join("active-build.json")).unwrap();
    let repeated = cli(&activation_args);
    assert!(repeated.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&repeated.stdout).unwrap()["changed"],
        false
    );
    fs::write(
        source.join("crates/manager/src/main.rs"),
        "fn main() { syntax error }\n",
    )
    .unwrap();
    let stale = cli(&["check", "--build", build]);
    assert_eq!(stale.status.code(), Some(1));
    let stale: Value = serde_json::from_slice(&stale.stdout).unwrap();
    assert_eq!(stale["status"], "source-stale");
    assert_eq!(stale["management_allowed"], true);
    assert_eq!(stale["runtime_allowed"], false);
    let failed = cli(&arguments);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Candidate build failed"));
    assert!(Path::new(build).join("build.json").exists());
    assert_eq!(fs::read_dir(state.join("builds")).unwrap().count(), 1);
    assert_eq!(fs::read(state.join("active-build.json")).unwrap(), pointer);
    let stale_activation = cli(&activation_args);
    assert!(!stale_activation.status.success());
    assert_eq!(fs::read(state.join("active-build.json")).unwrap(), pointer);
    let recovery = cli(&["recover-build", "--state", state.to_str().unwrap()]);
    assert!(recovery.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&recovery.stdout).unwrap()["changed"],
        false
    );
    fs::write(Path::new(build).join("harness-rtk.exe"), "altered").unwrap();
    let altered = cli(&["check", "--build", build]);
    let altered: Value = serde_json::from_slice(&altered.stdout).unwrap();
    assert_eq!(altered["status"], "altered");
    assert_eq!(altered["management_allowed"], true);
    fs::write(
        source.join("crates/manager/src/main.rs"),
        "fn main() { println!(\"repaired fixture\"); }\n",
    )
    .unwrap();
    let repair = cli(&arguments);
    assert!(
        repair.status.success(),
        "{}",
        String::from_utf8_lossy(&repair.stderr)
    );
    let repair: Value = serde_json::from_slice(&repair.stdout).unwrap();
    let new_build = repair["build"].as_str().unwrap();
    assert_ne!(new_build, build);
    let replaced = cli(&[
        "activate-build",
        "--state",
        state.to_str().unwrap(),
        "--build",
        new_build,
    ]);
    assert!(
        replaced.status.success(),
        "{}",
        String::from_utf8_lossy(&replaced.stderr)
    );
    assert!(cli(&["check", "--build", new_build]).status.success());
    assert_eq!(
        fs::read_to_string(Path::new(build).join("harness-rtk.exe")).unwrap(),
        "altered"
    );
}

#[test]
fn missing_prerequisite_and_foreign_state_do_not_mutate_installation() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fixture(&source);
    for forbidden in [
        source.join("native-state"),
        source.clone(),
        temp.path().to_path_buf(),
    ] {
        let result = cli(&[
            "build",
            "--source",
            source.to_str().unwrap(),
            "--state",
            forbidden.to_str().unwrap(),
        ]);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("outside the source checkout"));
        assert!(!forbidden.join("owner").exists());
    }
    assert!(!source.join("native-state").exists());
    let state = temp.path().join("state");
    let result = cli(&[
        "build",
        "--source",
        source.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
        "--cargo",
        temp.path().join("missing-cargo.exe").to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(!state.exists());
    fs::create_dir(&state).unwrap();
    fs::write(state.join("foreign"), "keep").unwrap();
    let result = cli(&[
        "build",
        "--source",
        source.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert_eq!(fs::read_to_string(state.join("foreign")).unwrap(), "keep");
    assert!(!state.join("owner").exists());
}

#[test]
fn changed_bytes_with_restored_mtime_cannot_certify_a_cached_old_binary() {
    let temp = tempfile::Builder::new()
        .prefix("harness-native-mtime-")
        .tempdir()
        .unwrap();
    let source = temp.path().join("source");
    let state = temp.path().join("state");
    fixture(&source);
    let code = source.join("crates/manager/src/main.rs");
    fs::write(&code, "fn main() { println!(\"candidate-a\"); }\n").unwrap();
    let original_time = fs::metadata(&code).unwrap().modified().unwrap();
    let arguments = [
        "build",
        "--source",
        source.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
    ];
    let first = cli(&arguments);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    fs::write(&code, "fn main() { println!(\"candidate-b\"); }\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&code)
        .unwrap()
        .set_modified(original_time)
        .unwrap();
    let second = cli(&arguments);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let result: Value = serde_json::from_slice(&second.stdout).unwrap();
    let output =
        Command::new(Path::new(result["build"].as_str().unwrap()).join("codex-harness.exe"))
            .output()
            .unwrap();
    let retained = temp.keep();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "candidate-b",
        "evidence {}",
        retained.display()
    );
}

#[test]
fn preexisting_cache_reparse_point_cannot_redirect_compilation_writes() {
    let temp = tempfile::Builder::new()
        .prefix("harness-native-cache-link-")
        .tempdir()
        .unwrap();
    let source = temp.path().join("source");
    let state = temp.path().join("state");
    let foreign = temp.path().join("foreign");
    fixture(&source);
    fs::create_dir_all(state.join("cargo-target")).unwrap();
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("sentinel"), "preserve").unwrap();
    fs::write(state.join("owner"), "codex-harness-native-state-v1\n").unwrap();
    std::os::windows::fs::symlink_dir(&foreign, state.join("cargo-target/x86_64-pc-windows-msvc"))
        .unwrap();
    let result = cli(&[
        "build",
        "--source",
        source.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
    ]);
    fs::write(temp.path().join("result.stderr"), &result.stderr).unwrap();
    let retained = temp.keep();
    assert_eq!(
        fs::read_to_string(foreign.join("sentinel")).unwrap(),
        "preserve"
    );
    assert_eq!(
        fs::read_dir(&foreign).unwrap().count(),
        1,
        "compilation touched foreign target; evidence {}",
        retained.display()
    );
}

#[test]
fn compiler_resources_are_covered_or_refused_and_overrides_do_not_reuse_builds() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let state = temp.path().join("state");
    fixture(&source);
    let code = source.join("crates/manager/src/main.rs");
    let resource = source.join("crates/manager/src/banner.md");
    fs::write(&resource, "resource-a").unwrap();
    fs::write(
        &code,
        "fn main() { println!(\"{}\", include_str!(\"banner.md\")); }\n",
    )
    .unwrap();
    let args = [
        "build",
        "--source",
        source.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
    ];
    let first = cli(&args);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    fs::write(&resource, "resource-b").unwrap();
    let second = cli(&args);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let candidate: Value = serde_json::from_slice(&second.stdout).unwrap();
    let candidate = Path::new(candidate["build"].as_str().unwrap());
    let output = Command::new(candidate.join("codex-harness.exe"))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "resource-b");
    for key in [
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS",
        "CARGO_PROFILE_RELEASE_OPT_LEVEL",
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(args)
            .env(key, "private-test-value")
            .output()
            .unwrap();
        assert!(!result.status.success());
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains(key));
        assert!(!stderr.contains("private-test-value"));
    }
    fs::write(
        source.join("crates/manager/README.md"),
        "outside-source-doc",
    )
    .unwrap();
    fs::write(
        &code,
        "fn main() { println!(\"{}\", include_str!(\"../README.md\")); }\n",
    )
    .unwrap();
    let count = fs::read_dir(state.join("builds")).unwrap().count();
    let uncovered = cli(&args);
    assert!(!uncovered.status.success());
    assert!(
        String::from_utf8_lossy(&uncovered.stderr).contains("outside the native input inventory")
    );
    assert_eq!(fs::read_dir(state.join("builds")).unwrap().count(), count);
}

#[test]
fn ancestor_cargo_configuration_changes_invalidate_build_identity() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let state = temp.path().join("state");
    fixture(&source);
    let built = cli(&[
        "build",
        "--source",
        source.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
    ]);
    assert!(built.status.success());
    let candidate: Value = serde_json::from_slice(&built.stdout).unwrap();
    let build = candidate["build"].as_str().unwrap();
    fs::create_dir(temp.path().join(".cargo")).unwrap();
    fs::write(
        temp.path().join(".cargo/config.toml"),
        "[build]\nrustflags=['-C','opt-level=1']\n",
    )
    .unwrap();
    let check = cli(&["check", "--build", build]);
    assert_eq!(
        serde_json::from_slice::<Value>(&check.stdout).unwrap()["status"],
        "source-stale"
    );
}
