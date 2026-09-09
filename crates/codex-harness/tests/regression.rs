//! Exercise harness-observe against compiled owned fixtures.
//! No models, live services or global mutations.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn observe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-observe"))
}

fn evidence(name: &str) -> PathBuf {
    let root = tempfile::Builder::new()
        .prefix(&format!("harness-rust-regression-{name}-"))
        .tempdir()
        .unwrap()
        .keep();
    eprintln!("regression evidence: {}", root.display());
    root
}

fn fixture_args(role: &str) -> Vec<String> {
    vec![
        observe().to_string_lossy().into_owned(),
        "--fixture".into(),
        role.into(),
    ]
}

fn run(root: &Path, extra: &[&str], role: &str) -> (i32, Value) {
    let observe = observe();
    let cwd = root.join("cwd");
    fs::create_dir_all(&cwd).unwrap();
    let mut command = Command::new(&observe);
    command
        .arg("--cwd")
        .arg(&cwd)
        .args(extra)
        .arg("--")
        .args(fixture_args(role))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command.output().unwrap();
    fs::write(
        root.join(format!("{role}-cli.txt")),
        [output.stdout.as_slice(), output.stderr.as_slice()].concat(),
    )
    .unwrap();
    let code = output.status.code().unwrap_or(-1);
    assert!(
        code == 0 || code == 1 || code == 2,
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if code == 2 {
        return (
            code,
            serde_json::json!({
                "cli_error": String::from_utf8_lossy(&output.stderr),
            }),
        );
    }
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    fs::write(
        root.join(format!("{role}-result.json")),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    (code, result)
}

fn assert_report(result: &Value) {
    let case = PathBuf::from(result["root"].as_str().unwrap());
    assert!(case.join("report.json").is_file());
    assert!(case.join("observed.json").is_file());
    assert!(case.join("request.json").is_file());
    assert_eq!(
        result["streams"]["stdout"]["path"],
        case.join("stdout.txt").to_string_lossy().as_ref()
    );
}

#[test]
fn rejects_relative_and_unbounded_requests() {
    let observe = observe();
    for args in [
        vec!["python"],
        vec!["--timeout", "true", "--", observe.to_str().unwrap()],
        vec!["--timeout", "601", "--", observe.to_str().unwrap()],
        vec![
            "--timeout",
            "10",
            "--ready-timeout",
            "11",
            "--",
            observe.to_str().unwrap(),
        ],
        vec!["--output-limit", "0", "--", observe.to_str().unwrap()],
    ] {
        let output = Command::new(&observe).args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("absolute")
                || stderr.contains("timeout")
                || stderr.contains("output_limit")
                || stderr.contains("integer"),
            "{stderr}"
        );
    }
}

#[test]
fn success_preserves_args_cwd_env_and_stdin() {
    let root = evidence("success");
    let stdin = root.join("stdin.txt");
    fs::write(&stdin, "from stdin\n").unwrap();
    let observe = observe();
    let cwd = root.join("owned cwd");
    fs::create_dir_all(&cwd).unwrap();
    let output = Command::new(&observe)
        .args([
            "--cwd",
            cwd.to_str().unwrap(),
            "--timeout",
            "10",
            "--ready-timeout",
            "3",
            "--stdin",
            stdin.to_str().unwrap(),
            "--",
            observe.to_str().unwrap(),
            "--fixture",
            "ok",
            "space here",
            "проверка",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    fs::write(
        root.join("ok-result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    assert_eq!(result["status"], "exited");
    assert_eq!(result["ready"], true);
    assert_eq!(result["native"]["ExitCode"], 0);
    assert_eq!(result["native"]["AssignedBeforeResume"], true);
    assert_eq!(result["native"]["Status"], "exited");
    assert_report(&result);
    let case = PathBuf::from(result["root"].as_str().unwrap());
    let stdout = fs::read_to_string(case.join("stdout.txt")).unwrap();
    assert!(stdout.contains("from stdin"));
    assert!(stdout.contains("fixture stdout"));
    assert_eq!(
        fs::read_to_string(case.join("stderr.txt")).unwrap().trim(),
        "fixture stderr"
    );
    let request: Value =
        serde_json::from_slice(&fs::read(case.join("request.json")).unwrap()).unwrap();
    assert_eq!(request["arguments"][2], "space here");
    assert_eq!(request["arguments"][3], "проверка");
    assert_eq!(request["environment"]["PROCESS_CASE_ROOT"], result["root"]);
    let env = Command::new(&observe)
        .args([
            "--cwd",
            cwd.to_str().unwrap(),
            "--timeout",
            "10",
            "--",
            observe.to_str().unwrap(),
            "--fixture",
            "env",
        ])
        .output()
        .unwrap();
    let env_result: Value = serde_json::from_slice(&env.stdout).unwrap();
    let env_root = PathBuf::from(env_result["root"].as_str().unwrap());
    assert_eq!(
        fs::read_to_string(env_root.join("stdout.txt"))
            .unwrap()
            .trim(),
        env_result["root"].as_str().unwrap()
    );
    let cwd_out = Command::new(&observe)
        .args([
            "--cwd",
            cwd.to_str().unwrap(),
            "--timeout",
            "10",
            "--",
            observe.to_str().unwrap(),
            "--fixture",
            "cwd",
        ])
        .output()
        .unwrap();
    let cwd_result: Value = serde_json::from_slice(&cwd_out.stdout).unwrap();
    let cwd_root = PathBuf::from(cwd_result["root"].as_str().unwrap());
    let reported = fs::read_to_string(cwd_root.join("stdout.txt")).unwrap();
    assert!(
        fs::canonicalize(&cwd)
            .unwrap()
            .to_string_lossy()
            .eq_ignore_ascii_case(reported.trim()),
        "{reported}"
    );
}

#[test]
fn natural_nonzero_is_distinct_from_readiness_and_timeout() {
    let root = evidence("statuses");
    let (code, fail) = run(&root, &["--timeout", "10"], "fail");
    assert_eq!(code, 1);
    assert_eq!(fail["status"], "exited");
    assert_eq!(fail["native"]["ExitCode"], 7);
    assert_eq!(
        fs::read_to_string(PathBuf::from(fail["root"].as_str().unwrap()).join("stderr.txt"))
            .unwrap()
            .trim(),
        "original failure"
    );

    let (code, missing) = run(
        &root,
        &["--timeout", "8", "--ready-timeout", "2"],
        "no-ready",
    );
    assert_eq!(code, 1);
    assert_eq!(missing["status"], "readiness-failure");
    assert_eq!(missing["native"]["ExitCode"], 0);
    assert_eq!(missing["ready"], false);

    let start = Instant::now();
    let (code, timeout) = run(&root, &["--timeout", "8", "--ready-timeout", "2"], "hold");
    assert_eq!(code, 1);
    assert_eq!(timeout["status"], "readiness-timeout", "{timeout}");
    assert!(start.elapsed() < Duration::from_secs(15));
    assert_eq!(timeout["native"]["ExitCode"], 130);
}

#[test]
fn output_limit_and_execution_timeout_reap_owned_descendants() {
    let root = evidence("limits");
    let (code, flood) = run(
        &root,
        &["--timeout", "10", "--output-limit", "1024"],
        "flood",
    );
    assert_eq!(code, 1);
    assert_eq!(flood["status"], "output-limit");
    assert_eq!(flood["output_limit_reached"], true);

    let (code, timeout) = run(&root, &["--timeout", "1"], "tree-hold");
    assert_eq!(code, 1);
    assert_eq!(timeout["status"], "timeout", "{timeout}");
    assert_eq!(timeout["native"]["ExitCode"], 124);
    let case = PathBuf::from(timeout["root"].as_str().unwrap());
    std::thread::sleep(Duration::from_secs(6));
    assert!(
        !case.join("descendant-after-timeout.txt").exists(),
        "Descendant survived Job close"
    );
}

#[test]
fn concurrent_streams_and_file_cancellation_are_observed() {
    let root = evidence("streams");
    let (code, streams) = run(
        &root,
        &["--timeout", "15", "--ready-timeout", "5"],
        "streams",
    );
    assert_eq!(code, 0);
    assert_eq!(streams["status"], "exited");
    assert_eq!(streams["ready"], true);
    assert_eq!(streams["streams"]["stdout"]["bytes"], 2 * 1024 * 1024);
    assert_eq!(streams["streams"]["stderr"]["bytes"], 2 * 1024 * 1024);

    let observe = observe();
    let cwd = root.join("cancel-cwd");
    fs::create_dir_all(&cwd).unwrap();
    let case = root.join("cancel-root");
    let child = Command::new(&observe)
        .args([
            "--cwd",
            cwd.to_str().unwrap(),
            "--timeout",
            "20",
            "--root",
            case.to_str().unwrap(),
            "--",
            observe.to_str().unwrap(),
            "--fixture",
            "cancel-hold",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let wait = Instant::now() + Duration::from_secs(8);
    while !case.join("wait-cancel.txt").is_file() {
        assert!(Instant::now() < wait, "fixture never reached cancel wait");
        std::thread::sleep(Duration::from_millis(20));
    }
    fs::write(case.join("cancel.txt"), "CANCEL").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    fs::write(
        root.join("cancel-result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    assert_eq!(result["status"], "cancelled", "{result}");
    assert_eq!(result["native"]["ExitCode"], 130);
}

#[test]
fn missing_executable_is_infrastructure_failure() {
    let root = evidence("infra");
    let observe = observe();
    let missing = root.join("missing.exe");
    let output = Command::new(&observe)
        .args([
            "--cwd",
            root.to_str().unwrap(),
            "--timeout",
            "5",
            "--",
            missing.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "infrastructure-failure");
    assert!(result["native"].is_null());
    assert!(!result["error"].as_str().unwrap().is_empty());
    assert_report(&result);
}

#[test]
fn preexisting_root_preserves_private_files() {
    let root = evidence("preexisting-root");
    let observe = observe();
    let existing = root.join("already");
    fs::create_dir_all(&existing).unwrap();
    let keep = existing.join("private.txt");
    fs::write(&keep, "keep").unwrap();
    let output = Command::new(&observe)
        .args([
            "--cwd",
            root.to_str().unwrap(),
            "--timeout",
            "5",
            "--root",
            existing.to_str().unwrap(),
            "--",
            observe.to_str().unwrap(),
            "--fixture",
            "ok",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read_to_string(&keep).unwrap(), "keep");
    assert!(!existing.join("request.json").exists());
    assert!(!existing.join("report.json").exists());
}

#[test]
fn report_symlink_preserves_foreign_target() {
    let root = evidence("report-link");
    let observe = observe();
    let parent = root.join("parent");
    fs::create_dir_all(&parent).unwrap();
    let case = parent.join("new-root");
    let foreign = root.join("foreign.txt");
    fs::write(&foreign, "foreign").unwrap();
    let output = Command::new(&observe)
        .args([
            "--cwd",
            root.to_str().unwrap(),
            "--timeout",
            "10",
            "--root",
            case.to_str().unwrap(),
            "--",
            observe.to_str().unwrap(),
            "--fixture",
            "link-report",
            foreign.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&foreign).unwrap(), "foreign");
    if case.exists() {
        assert!(
            !case.join("report.json").is_file()
                || fs::symlink_metadata(case.join("report.json"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
        );
    }
}

#[test]
fn oversized_ready_marker_is_not_readiness() {
    let root = evidence("oversize-ready");
    let observe = observe();
    let case = root.join("new-root");
    let output = Command::new(&observe)
        .args([
            "--cwd",
            root.to_str().unwrap(),
            "--timeout",
            "8",
            "--ready-timeout",
            "2",
            "--root",
            case.to_str().unwrap(),
            "--",
            observe.to_str().unwrap(),
            "--fixture",
            "oversize-ready",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "readiness-timeout", "{result}");
    assert_eq!(result["ready"], false);
}
