//! The native oracle runs an independent checker; a task's success file cannot
//! replace execution or authorize a changed acceptance input. The real-task
//! check runs through the shared heavy resource owner, so these tests also pin
//! that route: a declared deadline above the generic regression case's 600
//! seconds, bounded timeout cleanup of the owned tree, the pinned resource
//! owner's nested admission, and the program/input tamper refusals.
#![cfg(windows)]
use harness_core::build_identity::hash_file;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

struct Case {
    _root: tempfile::TempDir,
    workspace: PathBuf,
    request: PathBuf,
    checker: PathBuf,
    expected: PathBuf,
    request_sha256: String,
    account: PathBuf,
}

impl Case {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("candidate");
        fs::create_dir(&workspace).unwrap();
        let account = root.path().join("heavy-account");
        let checker = root.path().join("independent.exe");
        fs::copy(env!("CARGO_BIN_EXE_harness-improvement-fixture"), &checker).unwrap();
        let expected = root.path().join("expected.txt");
        fs::write(&expected, "correct\n").unwrap();
        let request = root.path().join("oracle-request.json");
        let value = json!({
            "schema":1,"kind":"real-task","case_root":workspace,
            "task_contract_sha256":"a".repeat(64),"timeout_seconds":10,
            "oracle":{"program":checker,"program_sha256":hash_file(&checker).unwrap(),
                "arguments":["check","{workspace}",expected],
                "inputs":{expected.to_str().unwrap():hash_file(&expected).unwrap()}}
        });
        fs::write(&request, serde_json::to_vec(&value).unwrap()).unwrap();
        let request_sha256 = hash_file(&request).unwrap();
        Self {
            _root: root,
            workspace,
            request,
            checker,
            expected,
            request_sha256,
            account,
        }
    }

    fn run(&self) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        command
            .args(["outcome-oracle", "--request"])
            .arg(&self.request)
            .args(["--request-sha256", &self.request_sha256]);
        with_isolated_account(&mut command, &self.account);
        command.output().unwrap()
    }
}

/// A test run of the oracle needs its own heavy-command account, but when the
/// suite itself already runs inside an admitted heavy tree the oracle must
/// inherit that admission instead of nesting a second account directory.
fn with_isolated_account(command: &mut Command, account: &Path) {
    if std::env::var_os(harness_core::heavy_command::LEASE_ENV).is_none() {
        command.env(harness_core::heavy_command::ACCOUNT_ENV, account);
    }
}

#[test]
fn actual_checker_accepts_correct_result_and_rejects_wrong_missing_or_forged_success() {
    let case = Case::new();
    for result in [None, Some("incorrect\n"), Some("correct\n")] {
        if let Some(result) = result {
            fs::write(case.workspace.join("answer.txt"), result).unwrap();
        }
        fs::write(
            case.workspace.join("outcome.json"),
            "{\"passed\":true,\"exit_code\":0}",
        )
        .unwrap();
        let output = case.run();
        let record: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)));
        let correct = result == Some("correct\n");
        assert_eq!(output.status.success(), correct, "{record}");
        assert_eq!(record["passed"], correct);
        assert_eq!(record["checker_executed"], true);
        assert_eq!(record["model_calls"], 0);
        assert_eq!(record["runs"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn changed_or_candidate_owned_oracle_cannot_report_success() {
    let case = Case::new();
    fs::write(case.workspace.join("answer.txt"), "correct\n").unwrap();
    fs::write(&case.expected, "changed\n").unwrap();
    let output = case.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("frozen oracle input changed"));
    fs::write(&case.expected, "correct\n").unwrap();
    fs::write(&case.checker, "replaced checker").unwrap();
    assert!(!case.run().status.success());

    let mut case = Case::new();
    let mutable = case.workspace.join("candidate-checker.exe");
    fs::copy(&case.checker, &mutable).unwrap();
    let mut request: Value = serde_json::from_slice(&fs::read(&case.request).unwrap()).unwrap();
    request["oracle"]["program"] = json!(mutable);
    fs::write(&case.request, serde_json::to_vec(&request).unwrap()).unwrap();
    let output = case.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("frozen oracle request changed"));
    // A newly frozen host request must still reject a candidate-owned checker.
    case.request_sha256 = hash_file(&case.request).unwrap();
    let output = case.run();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside the candidate checkout"));
}

/// A frozen real-task request whose program runs through the shared heavy
/// resource owner. Each case keeps its own account directory so the tests
/// never queue behind, or disturb, the session's shared heavy workload.
struct RequestCase {
    root: tempfile::TempDir,
    request: PathBuf,
    request_sha256: String,
    account: PathBuf,
}

impl RequestCase {
    fn new(program: &Path, arguments: Vec<String>, timeout: u64) -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("candidate");
        fs::create_dir(&workspace).unwrap();
        let account = root.path().join("heavy-account");
        let request = root.path().join("oracle-request.json");
        let value = json!({
            "schema":1,"kind":"real-task","case_root":workspace,
            "task_contract_sha256":"a".repeat(64),"timeout_seconds":timeout,
            "oracle":{"program":program,"program_sha256":hash_file(program).unwrap(),
                "arguments":arguments,"inputs":{}}
        });
        fs::write(&request, serde_json::to_vec(&value).unwrap()).unwrap();
        let request_sha256 = hash_file(&request).unwrap();
        Self {
            root,
            request,
            request_sha256,
            account,
        }
    }

    fn run(&self, env: &[(&str, &str)]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        command
            .args(["outcome-oracle", "--request"])
            .arg(&self.request)
            .args(["--request-sha256", &self.request_sha256]);
        with_isolated_account(&mut command, &self.account);
        for (name, value) in env {
            command.env(name, value);
        }
        command.output().unwrap()
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }
}

fn record(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "the oracle printed no record ({error}): {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// A declared real-task deadline above the generic regression case's 600
/// seconds is accepted and the check actually runs to its own exit.
#[test]
fn declared_deadline_above_the_regression_case_bound_is_accepted() {
    let program = PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
    let case = RequestCase::new(&program, Vec::new(), 700);
    let started = Instant::now();
    let output = case.run(&[
        ("HARNESS_EXECUTOR_FIXTURE_MODE", "slow"),
        ("HARNESS_EXECUTOR_FIXTURE_DELAY_MS", "1500"),
    ]);
    let record = record(&output);
    assert!(output.status.success(), "{record}");
    assert_eq!(record["passed"], true);
    assert_eq!(record["checker_executed"], true);
    assert_eq!(record["checker_route"], "shared-heavy-command");
    let run = &record["runs"][0];
    assert_eq!(run["status"], "exited");
    assert_eq!(run["native"]["ExitCode"], 0);
    assert_eq!(run["declared_deadline_seconds"], 700);
    assert_eq!(run["admission"]["route"], "shared-heavy-command");
    assert!(
        run["elapsed_seconds"].as_f64().unwrap() >= 1.5,
        "the check did not run its measured program: {run}"
    );
    assert!(started.elapsed() < Duration::from_secs(120), "{record}");
}

/// The declared deadline bounds the whole admitted call, and its cleanup
/// leaves no checker-owned descendant: the fixture's recorded grandchild is
/// gone once the oracle returns.
#[test]
fn timed_out_admitted_check_cleans_its_descendants() {
    let program = PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
    let case = RequestCase::new(&program, Vec::new(), 5);
    let child_marker = case.path("child.json");
    let release = case.path("release");
    let started = Instant::now();
    let output = case.run(&[
        ("HARNESS_EXECUTOR_FIXTURE_MODE", "descendant"),
        (
            "HARNESS_EXECUTOR_FIXTURE_CHILD_MARKER",
            child_marker.to_str().unwrap(),
        ),
        (
            "HARNESS_EXECUTOR_FIXTURE_RELEASE",
            release.to_str().unwrap(),
        ),
    ]);
    let record = record(&output);
    assert!(!output.status.success(), "{record}");
    assert_eq!(record["passed"], false);
    assert_eq!(record["checker_executed"], true);
    let run = &record["runs"][0];
    assert_eq!(run["status"], "timeout");
    assert_eq!(run["stopped_by"], "declared-deadline");
    assert!(
        started.elapsed() >= Duration::from_secs(4),
        "the declared deadline was not reached: {run}"
    );
    assert!(started.elapsed() < Duration::from_secs(90), "{record}");
    let marker: Value = serde_json::from_slice(&fs::read(&child_marker).unwrap()).unwrap();
    let pid = marker["pid"].as_u64().unwrap() as u32;
    let created = marker["creation_time"].as_u64().unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let gone =
            match harness_core::process_service::ServiceProcess::observe(pid, &program, 0, &user) {
                Ok(observed) => observed.identity().creation_time != created,
                Err(_) => true,
            };
        if gone {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the fixture descendant survived the admitted check's timeout cleanup"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The pinned shared resource owner still works inside the oracle: its CLI
/// takes the nested (inherited) admission instead of failing or queuing for a
/// second slot, exactly as it must for the checker's own preparation steps.
#[test]
fn pinned_resource_owner_runs_nested_under_the_oracle() {
    let owner = PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"));
    let inner = PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
    let case = RequestCase::new(
        &owner,
        vec![
            "heavy".to_owned(),
            "--".to_owned(),
            inner.to_string_lossy().into_owned(),
        ],
        60,
    );
    let output = case.run(&[]);
    let record = record(&output);
    assert!(output.status.success(), "{record}");
    assert_eq!(record["passed"], true);
    let run = &record["runs"][0];
    assert_eq!(run["status"], "exited");
    assert_eq!(run["native"]["ExitCode"], 0);
    let stderr = fs::read_to_string(run["streams"]["stderr"]["path"].as_str().unwrap()).unwrap();
    assert!(
        stderr.contains("inheriting the aggregate heavy-command budget"),
        "the pinned owner did not take the nested admission: {stderr}"
    );
}

/// Deadlines outside the frozen request contract are refused before any
/// process is admitted.
#[test]
fn declared_deadlines_outside_the_contract_are_refused_before_dispatch() {
    let program = PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
    let case = RequestCase::new(&program, Vec::new(), 86_401);
    let output = case.run(&[]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid real-task acceptance request"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A program reachable only through a reparse point is refused: the pinned
/// oracle input must be an ordinary file with ordinary parents.
#[test]
fn reparse_program_parents_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    let program = real.join("program.exe");
    fs::copy(env!("CARGO_BIN_EXE_harness-executor-fixture"), &program).unwrap();
    let link = root.path().join("link");
    let created = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&real)
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "junction creation failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    let workspace = root.path().join("candidate");
    fs::create_dir(&workspace).unwrap();
    let request = root.path().join("oracle-request.json");
    let linked_program = link.join("program.exe");
    let value = json!({
        "schema":1,"kind":"real-task","case_root":workspace,
        "task_contract_sha256":"a".repeat(64),"timeout_seconds":30,
        "oracle":{"program":linked_program,"program_sha256":hash_file(&linked_program).unwrap(),
            "arguments":[],"inputs":{}}
    });
    fs::write(&request, serde_json::to_vec(&value).unwrap()).unwrap();
    let request_sha256 = hash_file(&request).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["outcome-oracle", "--request"])
        .arg(&request)
        .args(["--request-sha256", &request_sha256])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("reparse point"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
