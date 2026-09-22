//! Verification records over compiled owned fixtures: selected scope,
//! declared inputs, executable and Git identity across success, input
//! mutation, refusal, failure, timeout and a non-Git working directory.
//! No models, live services or global mutations.
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn observe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-observe"))
}

fn evidence(name: &str) -> PathBuf {
    let root = tempfile::Builder::new()
        .prefix(&format!("harness-verification-{name}-"))
        .tempdir()
        .unwrap()
        .keep();
    eprintln!("verification evidence: {}", root.display());
    root
}

/// A non-Git working directory with one declared input file.
fn declared_input(root: &Path) -> (PathBuf, PathBuf) {
    let cwd = root.join("cwd");
    fs::create_dir_all(&cwd).unwrap();
    let input = root.join("input.txt");
    fs::write(&input, "declared input\n").unwrap();
    (cwd, input)
}

/// Invokes `harness-observe` with the given flags and fixture role.
fn invoke(cwd: &Path, extra: &[&str], role: &str, role_args: &[&str]) -> (i32, Value) {
    let observe = observe();
    let output = Command::new(&observe)
        .args(["--cwd", cwd.to_str().unwrap()])
        .args(extra)
        .arg("--")
        .arg(&observe)
        .args(["--fixture", role])
        .args(role_args)
        .output()
        .unwrap();
    let code = output.status.code().unwrap_or(-1);
    let result = if output.stdout.is_empty() {
        serde_json::json!({"cli_error": String::from_utf8_lossy(&output.stderr)})
    } else {
        serde_json::from_slice(&output.stdout).unwrap()
    };
    (code, result)
}

#[test]
fn success_records_scope_executable_inputs_and_explicit_non_git_identity() {
    let root = evidence("success");
    let (cwd, input) = declared_input(&root);
    let (code, result) = invoke(
        &cwd,
        &[
            "--timeout",
            "10",
            "--scope",
            "fixture success check",
            "--input",
            input.to_str().unwrap(),
        ],
        "ok",
        &[],
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["status"], "exited");

    let verification = &result["verification"];
    assert_eq!(verification["scope"], "fixture success check");
    assert_eq!(
        verification["cwd"],
        fs::canonicalize(&cwd).unwrap().to_string_lossy().as_ref()
    );
    assert_eq!(verification["argv0"], observe().to_string_lossy().as_ref());
    let executable = &verification["executable"];
    assert!(
        executable["resolved"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .ends_with("harness-observe.exe"),
        "{executable}"
    );
    assert_eq!(executable["sha256_before"].as_str().unwrap().len(), 64);
    assert_eq!(executable["sha256_after"], executable["sha256_before"]);
    assert_eq!(executable["state"], "unchanged");
    let recorded = &verification["inputs"][0];
    assert_eq!(recorded["path"], input.to_string_lossy().as_ref());
    assert_eq!(recorded["bytes_before"], "declared input\n".len());
    assert_eq!(recorded["state"], "unchanged");
    assert!(recorded["unavailable"].is_null());

    // A non-Git working directory states Git identity as explicitly
    // unavailable while the executable, inputs and process results stay recorded.
    let git = &verification["git"];
    assert_eq!(git["available"], false, "{git}");
    assert!(!git["reason"].as_str().unwrap().is_empty(), "{git}");
    assert_eq!(git["worktree_state"], "unavailable");
    assert!(git["head_before"].is_null());
    let coverage = verification["coverage"].as_str().unwrap();
    assert!(coverage.contains("not covered"), "{coverage}");
    assert!(coverage.contains("does not accept"), "{coverage}");

    // The record stays in the existing local evidence lifecycle.
    let case = PathBuf::from(result["root"].as_str().unwrap());
    let observed: Value =
        serde_json::from_slice(&fs::read(case.join("observed.json")).unwrap()).unwrap();
    assert_eq!(observed["verification"]["scope"], "fixture success check");
    let report: Value =
        serde_json::from_slice(&fs::read(case.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["verification"]["inputs"][0]["state"], "unchanged");
    let request: Value =
        serde_json::from_slice(&fs::read(case.join("request.json")).unwrap()).unwrap();
    assert_eq!(request["scope"], "fixture success check");
    assert_eq!(
        request["declaredInputs"][0],
        input.to_string_lossy().as_ref()
    );
}

#[test]
fn mutated_input_keeps_the_natural_exit_without_claiming_unchanged_verification() {
    let root = evidence("mutation");
    let (cwd, input) = declared_input(&root);
    let (code, result) = invoke(
        &cwd,
        &[
            "--timeout",
            "10",
            "--scope",
            "input mutation check",
            "--input",
            input.to_str().unwrap(),
        ],
        "mutate-input",
        &[input.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["status"], "exited");
    assert_eq!(result["native"]["ExitCode"], 0);
    let recorded = &result["verification"]["inputs"][0];
    assert_eq!(recorded["state"], "changed", "{recorded}");
    assert_ne!(recorded["sha256_before"], recorded["sha256_after"]);
    assert_eq!(
        recorded["bytes_after"],
        "mutated during execution\n".len(),
        "{recorded}"
    );
}

#[test]
fn missing_linked_or_empty_declarations_fail_before_any_case_state_exists() {
    let root = evidence("refusal");
    let (cwd, input) = declared_input(&root);

    let missing = root.join("absent.txt");
    let case = root.join("case-missing");
    let (code, result) = invoke(
        &cwd,
        &[
            "--timeout",
            "5",
            "--scope",
            "missing input",
            "--input",
            missing.to_str().unwrap(),
            "--root",
            case.to_str().unwrap(),
        ],
        "ok",
        &[],
    );
    assert_eq!(code, 2, "{result}");
    let error = result["cli_error"].as_str().unwrap();
    assert!(error.contains("declared input is missing"), "{error}");
    assert!(error.contains(missing.to_str().unwrap()), "{error}");
    assert!(
        !case.exists(),
        "a refused declaration creates no case state"
    );

    let foreign = root.join("foreign.txt");
    fs::write(&foreign, "foreign").unwrap();
    let link = root.join("linked.txt");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&foreign, &link).unwrap();
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&foreign, &link).unwrap();
    let case = root.join("case-linked");
    let (code, result) = invoke(
        &cwd,
        &[
            "--timeout",
            "5",
            "--scope",
            "escaped input",
            "--input",
            link.to_str().unwrap(),
            "--root",
            case.to_str().unwrap(),
        ],
        "ok",
        &[],
    );
    assert_eq!(code, 2, "{result}");
    let error = result["cli_error"].as_str().unwrap();
    assert!(error.contains("is a link"), "{error}");
    assert_eq!(fs::read_to_string(&foreign).unwrap(), "foreign");
    assert!(
        !case.exists(),
        "a refused declaration creates no case state"
    );

    let (code, result) = invoke(
        &cwd,
        &[
            "--timeout",
            "5",
            "--scope",
            "   ",
            "--input",
            input.to_str().unwrap(),
        ],
        "ok",
        &[],
    );
    assert_eq!(code, 2, "{result}");
    assert!(
        result["cli_error"].as_str().unwrap().contains("scope"),
        "{result}"
    );
}

#[test]
fn failure_and_timeout_records_preserve_the_natural_outcome() {
    let root = evidence("outcomes");
    let (cwd, input) = declared_input(&root);
    let flags = [
        "--timeout",
        "10",
        "--scope",
        "failure path",
        "--input",
        input.to_str().unwrap(),
    ];
    let (code, fail) = invoke(&cwd, &flags, "fail", &[]);
    assert_eq!(code, 1, "{fail}");
    assert_eq!(fail["status"], "exited");
    assert_eq!(fail["native"]["ExitCode"], 7);
    assert_eq!(fail["verification"]["inputs"][0]["state"], "unchanged");
    assert_eq!(fail["verification"]["executable"]["state"], "unchanged");

    let flags = [
        "--timeout",
        "1",
        "--scope",
        "timeout path",
        "--input",
        input.to_str().unwrap(),
    ];
    let (code, timeout) = invoke(&cwd, &flags, "hold", &[]);
    assert_eq!(code, 1, "{timeout}");
    assert_eq!(timeout["status"], "timeout");
    assert_eq!(timeout["native"]["ExitCode"], 124);
    let recorded = &timeout["verification"]["inputs"][0];
    assert_eq!(recorded["state"], "unchanged", "{recorded}");
    assert_eq!(recorded["sha256_after"].as_str().unwrap().len(), 64);
    assert_eq!(timeout["verification"]["executable"]["state"], "unchanged");
}

#[test]
fn git_identity_records_head_and_bounded_status_inside_a_repository() {
    let _root = evidence("git");
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap();
    let input = checkout
        .join("crates")
        .join("codex-harness")
        .join("Cargo.toml");
    let (code, result) = invoke(
        &checkout,
        &[
            "--timeout",
            "10",
            "--scope",
            "git identity check",
            "--input",
            input.to_str().unwrap(),
        ],
        "ok",
        &[],
    );
    assert_eq!(code, 0, "{result}");
    let git = &result["verification"]["git"];
    assert_eq!(git["available"], true, "{git}");
    let head = git["head_before"].as_str().unwrap();
    assert_eq!(head.len(), 40, "{head}");
    assert!(head.chars().all(|c| c.is_ascii_hexdigit()), "{head}");
    assert_eq!(git["head_after"], git["head_before"]);
    assert_eq!(git["worktree_state"], "unchanged");
    assert_eq!(
        git["status_sha256_before"].as_str().unwrap().len(),
        64,
        "{git}"
    );
    assert_eq!(git["status_truncated"], false);
}

#[test]
fn a_receipt_without_declaration_keeps_its_previous_shape() {
    let root = evidence("plain");
    let (cwd, _input) = declared_input(&root);
    let (code, result) = invoke(&cwd, &["--timeout", "10"], "ok", &[]);
    assert_eq!(code, 0, "{result}");
    assert!(result.get("verification").is_none(), "{result}");
    let case = PathBuf::from(result["root"].as_str().unwrap());
    let request: Value =
        serde_json::from_slice(&fs::read(case.join("request.json")).unwrap()).unwrap();
    assert!(request["scope"].is_null());
    assert_eq!(request["declaredInputs"].as_array().unwrap().len(), 0);
}
