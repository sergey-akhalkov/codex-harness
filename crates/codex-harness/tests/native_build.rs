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

fn manager_source(program: &str) -> String {
    let dispatch = r#"
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "finalize-build-v1") {
        if let Err(error) = harness_core::native_build::finalize(std::path::Path::new(&args[1])) {
            eprintln!("{error}"); std::process::exit(2);
        }
        return;
    }
    if args.first().is_some_and(|a| a == "check") {
        let source = args.windows(2).find(|w| w[0] == "--source").map(|w| std::path::Path::new(&w[1]));
        let build = args.windows(2).find(|w| w[0] == "--build").unwrap();
        let report = harness_core::build_identity::check(std::path::Path::new(&build[1]), source);
        println!("{}", serde_json::to_string(&report).unwrap());
        std::process::exit(if report.runtime_allowed { 0 } else { 1 });
    }
    if args.first().is_some_and(|a| a == "activate-build") {
        let state = args.windows(2).find(|w| w[0] == "--state").unwrap();
        let build = args.windows(2).find(|w| w[0] == "--build").unwrap();
        match harness_core::build_selection::activate(std::path::Path::new(&state[1]), std::path::Path::new(&build[1])) {
            Ok(result) => println!("{}", serde_json::to_string(&result).unwrap()),
            Err(error) => { eprintln!("{error}"); std::process::exit(2); }
        }
        return;
    }
"#;
    program.replacen("fn main() {", &format!("fn main() {{{dispatch}"), 1)
}

fn copy_source_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        assert!(!entry.file_type().unwrap().is_symlink());
        if path.is_dir() {
            copy_source_tree(&path, &destination.join(entry.file_name()));
        } else {
            fs::copy(&path, destination.join(entry.file_name())).unwrap();
        }
    }
}

fn fixture(source: &Path) {
    let schema = source.join(harness_core::build_identity::INSPECTION_SCHEMA);
    fs::create_dir_all(schema.parent().unwrap()).unwrap();
    fs::write(schema, "{}").unwrap();
    fs::create_dir_all(source.join("crates/manager/src")).unwrap();
    fs::create_dir_all(source.join("tools/rtk-adapter/src")).unwrap();
    fs::write(
        source.join("Cargo.toml"),
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"))
            .unwrap()
            .replace("crates/codex-harness", "crates/manager")
            .split("[profile.release]")
            .next()
            .unwrap()
            .to_owned()
            + "\n[profile.release]\nopt-level=0\n",
    )
    .unwrap();
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../harness-core");
    copy_source_tree(&core.join("src"), &source.join("crates/harness-core/src"));
    fs::copy(
        core.join("Cargo.toml"),
        source.join("crates/harness-core/Cargo.toml"),
    )
    .unwrap();
    for (directory, name) in [
        ("crates/manager", "codex-harness"),
        ("tools/rtk-adapter", "harness-rtk"),
    ] {
        fs::write(
            source.join(directory).join("Cargo.toml"),
            format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\n{}", if name == "codex-harness" { "[dependencies]\nharness-core={path='../harness-core'}\nserde_json.workspace=true\n" } else { "" }),
        )
        .unwrap();
        fs::write(
            source.join(directory).join("src/main.rs"),
            if name == "codex-harness" {
                manager_source("fn main() { println!(\"owned native fixture\"); }\n")
            } else {
                "fn main() {}\n".into()
            },
        )
        .unwrap();
    }
    fs::create_dir_all(source.join("crates/manager/src/bin")).unwrap();
    for name in harness_core::build_identity::BINARIES {
        if !["codex-harness.exe", "harness-rtk.exe"].contains(name) {
            fs::write(
                source
                    .join("crates/manager/src/bin")
                    .join(name.replace(".exe", ".rs")),
                "fn main() {}\n",
            )
            .unwrap();
        }
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
        manager_source("fn main() { println!(\"repaired fixture\"); }\n"),
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
    fs::write(
        &code,
        manager_source("fn main() { println!(\"candidate-a\"); }\n"),
    )
    .unwrap();
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
    fs::write(
        &code,
        manager_source("fn main() { println!(\"candidate-b\"); }\n"),
    )
    .unwrap();
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
        manager_source("fn main() { println!(\"{}\", include_str!(\"banner.md\")); }\n"),
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
        manager_source("fn main() { println!(\"{}\", include_str!(\"../README.md\")); }\n"),
    )
    .unwrap();
    let count = fs::read_dir(state.join("builds")).unwrap().count();
    let uncovered = cli(&args);
    assert!(!uncovered.status.success());
    assert!(
        String::from_utf8_lossy(&uncovered.stderr).contains("Fresh manager finalization failed")
    );
    assert!(fs::read_dir(state.join("staging")).unwrap().any(|entry| {
        fs::read_to_string(entry.unwrap().path().join("finalize.log"))
            .is_ok_and(|log| log.contains("outside the native input inventory"))
    }));
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

#[test]
fn real_four_binary_producer_finalizes_five_binary_consumer_with_new_input_rules() {
    use harness_core::process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason};
    use std::time::Duration;
    let temp = tempfile::Builder::new()
        .prefix("harness-native-transition-")
        .tempdir()
        .unwrap();
    // Retain all artifacts on failures as well as success for version inspection.
    let root = temp.keep();
    let source = root.join("source");
    let state = root.join("state");
    fixture(&source);
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"),
        source.join("crates/manager/src/main.rs"),
    )
    .unwrap();
    let identity_path = source.join("crates/harness-core/src/build_identity.rs");
    let current_identity = fs::read_to_string(&identity_path).unwrap();
    assert!(current_identity.contains("    \"harness-observe.exe\",\n"));
    let old_identity = current_identity.replacen("    \"harness-observe.exe\",\n", "", 1)
        .replacen("    let sha256 = hash_bytes(&serde_json::to_vec(&files)?);",
            "    collect(&root, &root.join(INSPECTION_SCHEMA), &mut files)?;\n    let sha256 = hash_bytes(&serde_json::to_vec(&files)?);", 1);
    assert_ne!(old_identity, current_identity);
    fs::write(&identity_path, old_identity).unwrap();
    let target = tempfile::Builder::new().prefix("hct-").tempdir().unwrap();
    let log = fs::File::create(root.join("bridge-bootstrap.log")).unwrap();
    let cargo = Command::new("where.exe").arg("cargo.exe").output().unwrap();
    assert!(cargo.status.success());
    let cargo = String::from_utf8(cargo.stdout).unwrap();
    let mut command = CommandSpec::new(std::path::PathBuf::from(cargo.lines().next().unwrap()));
    command.args = vec![
        "build".into(),
        "--release".into(),
        "--offline".into(),
        "--locked".into(),
        "--jobs".into(),
        "1".into(),
        "-p".into(),
        "codex-harness".into(),
        "--bin".into(),
        "codex-harness".into(),
        "--target-dir".into(),
        target.path().as_os_str().to_owned(),
    ];
    command.current_dir = Some(source.clone());
    command.stdout = Some(log.try_clone().unwrap());
    command.stderr = Some(log);
    let job = Job::new(Limits {
        memory_bytes: Some(2048 * 1024 * 1024),
        cpu_percent: Some(50.0),
    })
    .unwrap();
    let child = job.spawn(&command).unwrap();
    let outcome = job
        .wait(
            &child,
            Deadline::after(Duration::from_secs(300)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(5),
        )
        .unwrap();
    assert_eq!(
        (outcome.reason, outcome.exit_code),
        (StopReason::Exited, 0),
        "{}",
        root.display()
    );
    let bare_manager = target.path().join("release/codex-harness.exe");
    let build_args = [
        "build",
        "--source",
        source.to_str().unwrap(),
        "--state",
        state.to_str().unwrap(),
    ];
    let first = Command::new(&bare_manager)
        .args(build_args)
        .output()
        .unwrap();
    fs::write(root.join("old-prepare.stderr"), &first.stderr).unwrap();
    fs::write(root.join("old-prepare.json"), &first.stdout).unwrap();
    assert!(
        first.status.success(),
        "{}; {}",
        String::from_utf8_lossy(&first.stderr),
        root.display()
    );
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    let old_build = Path::new(first["build"].as_str().unwrap());
    let old_manager = old_build.join("codex-harness.exe");
    let old_record = harness_core::build_identity::read_record(old_build).unwrap();
    assert_eq!(old_record.binaries.len(), 4);
    assert!(
        old_record
            .source
            .files
            .contains_key(harness_core::build_identity::INSPECTION_SCHEMA)
    );
    assert!(
        Command::new(&old_manager)
            .args([
                "activate-build",
                "--state",
                state.to_str().unwrap(),
                "--build",
                old_build.to_str().unwrap()
            ])
            .status()
            .unwrap()
            .success()
    );
    let old_pointer = fs::read(state.join("active-build.json")).unwrap();
    fs::write(&identity_path, &current_identity).unwrap();
    let stale = Command::new(&old_manager)
        .args(["check", "--build", old_build.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(stale.status.code(), Some(1));
    assert_eq!(
        serde_json::from_slice::<Value>(&stale.stdout).unwrap()["management_allowed"],
        true
    );
    let updated = Command::new(&old_manager)
        .args(build_args)
        .output()
        .unwrap();
    fs::write(root.join("transition.stderr"), &updated.stderr).unwrap();
    fs::write(root.join("transition.json"), &updated.stdout).unwrap();
    assert!(
        updated.status.success(),
        "{}; {}",
        String::from_utf8_lossy(&updated.stderr),
        root.display()
    );
    assert_eq!(
        fs::read(state.join("active-build.json")).unwrap(),
        old_pointer
    );
    let updated: Value = serde_json::from_slice(&updated.stdout).unwrap();
    let new_build = Path::new(updated["build"].as_str().unwrap());
    let new_record = harness_core::build_identity::read_record(new_build).unwrap();
    assert_eq!(new_record.binaries.len(), 5);
    assert!(new_build.join("harness-observe.exe").is_file());
    assert!(
        !new_record
            .source
            .files
            .contains_key(harness_core::build_identity::INSPECTION_SCHEMA)
    );
    let request: Value =
        serde_json::from_slice(&fs::read(new_build.join("finalize-request.json")).unwrap())
            .unwrap();
    assert!(
        request["before"]["files"]
            .get(harness_core::build_identity::INSPECTION_SCHEMA)
            .is_some()
    );
    let cargo_log = fs::read_to_string(new_build.join("cargo.log")).unwrap();
    assert_eq!(cargo_log.matches("Finished `release`").count(), 1);
    let healthy = Command::new(new_build.join("codex-harness.exe"))
        .args(["check", "--build", new_build.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(healthy.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&healthy.stdout).unwrap()["status"],
        "healthy"
    );
    let activated = Command::new(&old_manager)
        .args([
            "activate-build",
            "--state",
            state.to_str().unwrap(),
            "--build",
            new_build.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    fs::write(root.join("transition-activation.stderr"), &activated.stderr).unwrap();
    assert!(
        activated.status.success(),
        "{}; {}",
        String::from_utf8_lossy(&activated.stderr),
        root.display()
    );
    let history: Vec<Value> = fs::read_dir(state.join("build-selection-history"))
        .unwrap()
        .map(|entry| serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap())
        .collect();
    assert!(
        history
            .iter()
            .any(|entry| entry["before"].is_array() && entry["rollback_usable"] == true)
    );
    let journal = history
        .iter()
        .find(|entry| entry["before"].is_array() && entry["rollback_usable"] == true)
        .unwrap();
    let journal_bytes = serde_json::to_vec_pretty(journal).unwrap();
    let after: Vec<u8> = serde_json::from_value(journal["after"].clone()).unwrap();
    let new_manager = new_build.join("codex-harness.exe");
    let recovery_args = ["recover-build", "--state", state.to_str().unwrap()];
    let new_activation_args = [
        "activate-build",
        "--state",
        state.to_str().unwrap(),
        "--build",
        new_build.to_str().unwrap(),
    ];
    for replaced in [false, true] {
        fs::write(
            state.join("active-build.json"),
            if replaced { &after } else { &old_pointer },
        )
        .unwrap();
        fs::write(state.join("build-selection-journal.json"), &journal_bytes).unwrap();
        let recovered = Command::new(&new_manager)
            .args(recovery_args)
            .output()
            .unwrap();
        assert!(
            recovered.status.success(),
            "{}; {}",
            String::from_utf8_lossy(&recovered.stderr),
            root.display()
        );
        assert_eq!(
            fs::read(state.join("active-build.json")).unwrap(),
            old_pointer
        );
        assert!(!state.join("build-selection-journal.json").exists());
        assert!(
            Command::new(&new_manager)
                .args(new_activation_args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let original_manager = fs::read(&old_manager).unwrap();
    fs::write(
        &old_manager,
        [original_manager.as_slice(), b"changed after interruption"].concat(),
    )
    .unwrap();
    fs::write(state.join("build-selection-journal.json"), &journal_bytes).unwrap();
    let refused = Command::new(&new_manager)
        .args(recovery_args)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert_eq!(fs::read(state.join("active-build.json")).unwrap(), after);
    assert_eq!(
        fs::read(state.join("build-selection-journal.json")).unwrap(),
        journal_bytes
    );
    fs::write(&old_manager, &original_manager).unwrap();
    fs::write(state.join("active-build.json"), b"foreign edit").unwrap();
    let conflicted = Command::new(&new_manager)
        .args(recovery_args)
        .output()
        .unwrap();
    assert!(!conflicted.status.success());
    assert_eq!(
        fs::read(state.join("active-build.json")).unwrap(),
        b"foreign edit"
    );
    assert_eq!(
        fs::read(state.join("build-selection-journal.json")).unwrap(),
        journal_bytes
    );
    fs::write(state.join("active-build.json"), &after).unwrap();
    assert!(
        Command::new(&new_manager)
            .args(recovery_args)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        fs::read(state.join("active-build.json")).unwrap(),
        old_pointer
    );
    assert!(
        Command::new(&new_manager)
            .args(new_activation_args)
            .output()
            .unwrap()
            .status
            .success()
    );
    fs::write(root.join("cross-version-recovery.json"), b"{\"before_swap\":true,\"after_swap\":true,\"altered_previous_preserved\":true,\"foreign_pointer_preserved\":true,\"retry_after_restoration\":true}").unwrap();
    let reused = Command::new(&old_manager)
        .args(build_args)
        .output()
        .unwrap();
    assert!(
        reused.status.success(),
        "{}",
        String::from_utf8_lossy(&reused.stderr)
    );
    let reused: Value = serde_json::from_slice(&reused.stdout).unwrap();
    assert_eq!(reused["reused"], true);
    assert_eq!(reused["build"], updated["build"]);
    fs::write(
        source.join(harness_core::build_identity::INSPECTION_SCHEMA),
        "{\"changed\":true}",
    )
    .unwrap();
    let live_data = Command::new(&old_manager)
        .args(build_args)
        .output()
        .unwrap();
    assert!(live_data.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&live_data.stdout).unwrap()["reused"],
        true
    );
    println!("transition evidence {}", root.display());
}
