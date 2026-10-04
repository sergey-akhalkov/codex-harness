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

/// A fast writer that exits zero after overflowing the declared output bound
/// cannot pass, and the retained stream obeys its reported hard bound while
/// the original outcome stays recorded beside the limit cause.
#[test]
fn fast_output_overrun_cannot_pass_and_retention_is_bounded() {
    const FLOOD: u64 = 16 * 1024 * 1024;
    const LIMIT: u64 = 8 * 1024 * 1024;
    let program = PathBuf::from(env!("CARGO_BIN_EXE_harness-process-fixture"));
    let artifacts = tempfile::tempdir().unwrap();
    let marker = artifacts.path().join("flood.json");
    let case = RequestCase::new(
        &program,
        vec![
            "flood".to_owned(),
            marker.to_string_lossy().into_owned(),
            FLOOD.to_string(),
        ],
        60,
    );
    let output = case.run(&[]);
    let record = record(&output);
    assert!(!output.status.success(), "{record}");
    assert_eq!(record["passed"], false);
    assert_eq!(record["checker_executed"], true);
    let run = &record["runs"][0];
    assert_eq!(run["status"], "output-limit", "{run}");
    assert_eq!(run["stopped_by"], "output-limit");
    assert_eq!(run["output_limit_reached"], true);
    // The original observation stays recorded separately from the limit cause:
    // either the payload's own zero exit, or the watchdog cancelling a tree
    // still writing when it observed the overrun.
    match run["native"]["Status"].as_str().unwrap() {
        "exited" => {
            assert_eq!(run["native"]["ExitCode"], 0, "{run}");
            assert_eq!(run["exit_code"], 0);
        }
        "cancelled" => {
            assert_eq!(run["native"]["ExitCode"], 130, "{run}");
            assert_eq!(run["exit_code"], 130);
        }
        other => panic!("unexpected original status {other}: {run}"),
    }
    let stdout = &run["streams"]["stdout"];
    assert_eq!(stdout["originalBytes"], json!(FLOOD));
    assert_eq!(stdout["truncated"], true);
    assert_eq!(stdout["finalized"], true);
    assert_eq!(stdout["bytes"], json!(LIMIT));
    let retained = fs::metadata(stdout["path"].as_str().unwrap())
        .unwrap()
        .len();
    assert_eq!(
        retained, LIMIT,
        "the retained stream exceeds its reported bound"
    );
    let marker: Value = serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    assert_eq!(marker["flooded_bytes"], json!(FLOOD));
}

/// One minimal, dependency-free stand-in for the harness checkout: a
/// `harness-core` lib suite and a `codex-harness` crate whose integration
/// targets come from its own manifest and `tests/` directory. The synthetic
/// tree keeps the checker's real dispatch, discovery and cargo runs without
/// compiling the full harness suite in a unit test.
struct OracleWorkspace {
    _root: tempfile::TempDir,
    root: PathBuf,
    checkout: PathBuf,
}

const PASSING_TARGET: &str = "#[test]\nfn works() {\n    assert_eq!(2 + 2, 4);\n}\n";
const FAILING_TARGET: &str =
    "#[test]\nfn breaks() {\n    assert!(false, \"deliberate target failure\");\n}\n";
const FAILING_LIB: &str = "pub fn smoke() -> u32 {\n    2\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn breaks() {\n        assert!(false, \"deliberate lib failure\");\n    }\n}\n";

impl OracleWorkspace {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().to_path_buf();
        let checkout = path.join("checkout");
        write_text(
            &checkout.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/harness-core\", \"crates/codex-harness\"]\nresolver = \"2\"\n",
        );
        write_text(
            &checkout.join("crates/harness-core/Cargo.toml"),
            "[package]\nname = \"harness-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        write_text(
            &checkout.join("crates/harness-core/src/lib.rs"),
            "pub fn smoke() -> u32 {\n    2\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn keeps_working() {\n        assert_eq!(2 + 2, 4);\n    }\n}\n",
        );
        write_text(
            &checkout.join("crates/codex-harness/Cargo.toml"),
            "[package]\nname = \"codex-harness\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        write_text(
            &checkout.join("crates/codex-harness/src/lib.rs"),
            "pub fn smoke() -> u32 {\n    1\n}\n",
        );
        Self {
            _root: root,
            root: path,
            checkout,
        }
    }

    fn lib_source(&self, text: &str) {
        write_text(&self.checkout.join("crates/harness-core/src/lib.rs"), text);
    }

    fn manifest(&self, text: &str) {
        write_text(&self.checkout.join("crates/codex-harness/Cargo.toml"), text);
    }

    fn target(&self, name: &str, body: &str) {
        write_text(
            &self
                .checkout
                .join(format!("crates/codex-harness/tests/{name}.rs")),
            body,
        );
    }

    /// The frozen strict request, outside the checked tree like the real
    /// acceptance input.
    fn request(&self) -> (PathBuf, String) {
        let path = self.root.join("parallel-lib-request.json");
        write_text(
            &path,
            &serde_json::to_string(&json!({
                "schema": 1,
                "source_root": self.checkout,
                "lib_repetitions": 3,
                "regression_targets": "auto",
            }))
            .unwrap(),
        );
        (path.clone(), hash_file(&path).unwrap())
    }

    fn checker(&self, request: &Path, sha: &str, extra: &[&str]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_harness-executor-fixture"));
        command
            .args(["parallel-lib-oracle", "--request"])
            .arg(request)
            .args(["--request-sha256", sha, "--workspace"])
            .arg(&self.checkout)
            .args(extra);
        command.current_dir(&self.checkout);
        command.output().unwrap()
    }
}

fn write_text(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("not an array: {value}"))
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

/// The current `codex-harness` integration targets, discovered independently
/// by Cargo itself, as the reference for the checker's own discovery.
fn cargo_metadata_test_targets(checkout: &Path) -> Vec<String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(checkout)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut names: Vec<String> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|package| package["name"] == "codex-harness")
        .flat_map(|package| package["targets"].as_array().unwrap().iter())
        .filter(|target| {
            target["kind"]
                .as_array()
                .unwrap()
                .iter()
                .any(|kind| kind == "test")
        })
        .map(|target| target["name"].as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The checker runs three consecutive full parallel harness-core lib suites
/// with no thread override, then one plain cargo run per discovered target,
/// and the unchanged frozen real-task oracle drives exactly that acceptance.
#[test]
fn parallel_lib_oracle_passes_three_parallel_lib_repetitions_and_every_target() {
    let case = OracleWorkspace::new();
    case.target("alpha", PASSING_TARGET);
    case.target("beta", PASSING_TARGET);
    let (request, sha) = case.request();

    let output = case.checker(&request, &sha, &[]);
    let verdict: Value = record(&output);
    assert!(output.status.success(), "{verdict}");
    assert_eq!(verdict["passed"], true, "{verdict}");
    assert_eq!(verdict["libRepetitions"], 3, "{verdict}");
    assert_eq!(verdict["regressionTargets"], "auto", "{verdict}");
    assert_eq!(verdict["modelCalls"], 0, "{verdict}");
    assert_eq!(verdict["targets"], json!(["alpha", "beta"]), "{verdict}");
    let cargo = verdict["cargo"].as_str().unwrap().to_owned();
    let steps = verdict["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 5, "{verdict}");
    for (index, step) in steps.iter().enumerate() {
        assert_eq!(step["status"], "exited", "{step}");
        assert_eq!(step["exitCode"], 0, "{step}");
        let expected = match index {
            0..=2 => json!([cargo, "test", "-p", "harness-core", "--lib"]),
            3 => json!([cargo, "test", "-p", "codex-harness", "--test", "alpha"]),
            _ => json!([cargo, "test", "-p", "codex-harness", "--test", "beta"]),
        };
        assert_eq!(step["command"], expected, "{step}");
    }

    // The frozen schema 1 real-task request the comparison supervisor runs:
    // the checker image, the pinned strict request and {workspace}.
    let program = PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
    let request_text = request.to_string_lossy().into_owned();
    let mut inputs = serde_json::Map::new();
    inputs.insert(request_text.clone(), json!(sha));
    let acceptance = case.root.join("acceptance-request.json");
    write_text(
        &acceptance,
        &serde_json::to_string(&json!({
            "schema": 1,
            "kind": "real-task",
            "case_root": case.checkout,
            "task_contract_sha256": "b".repeat(64),
            "timeout_seconds": 600,
            "oracle": {
                "program": program,
                "program_sha256": hash_file(&program).unwrap(),
                "arguments": [
                    "parallel-lib-oracle", "--request", request_text,
                    "--request-sha256", sha, "--workspace", "{workspace}"
                ],
                "inputs": Value::Object(inputs),
            },
        }))
        .unwrap(),
    );
    let acceptance_sha = hash_file(&acceptance).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    command
        .args(["outcome-oracle", "--request"])
        .arg(&acceptance)
        .args(["--request-sha256", &acceptance_sha]);
    with_isolated_account(&mut command, &case.root.join("heavy-account"));
    let output = command.output().unwrap();
    let oracle: Value = record(&output);
    assert!(output.status.success(), "{oracle}");
    assert_eq!(oracle["passed"], true, "{oracle}");
    assert_eq!(oracle["checker_executed"], true, "{oracle}");
    assert_eq!(oracle["model_calls"], 0, "{oracle}");
    assert_eq!(oracle["runs"][0]["native"]["ExitCode"], 0, "{oracle}");
    assert_eq!(
        oracle["runs"][0]["declared_deadline_seconds"], 600,
        "{oracle}"
    );
}

/// A failing lib repetition stops the checker at the first failure and fails
/// the oracle with a bounded verdict naming the failed command and exit code.
#[test]
fn parallel_lib_oracle_fails_bounded_on_a_failing_lib_repetition() {
    let case = OracleWorkspace::new();
    case.lib_source(FAILING_LIB);
    case.target("alpha", PASSING_TARGET);
    let (request, sha) = case.request();

    let output = case.checker(&request, &sha, &[]);
    let verdict: Value = record(&output);
    assert!(!output.status.success(), "{verdict}");
    assert_eq!(verdict["passed"], false, "{verdict}");
    let failure = &verdict["failure"];
    assert_eq!(failure["id"], "lib-run-1", "{verdict}");
    assert_eq!(failure["exitCode"], 101, "{verdict}");
    assert_eq!(
        failure["command"],
        json!([verdict["cargo"], "test", "-p", "harness-core", "--lib"]),
        "{verdict}"
    );
    let steps = verdict["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1, "{verdict}");
    assert!(
        steps[0]["output"]
            .as_str()
            .unwrap()
            .contains("deliberate lib failure"),
        "{verdict}"
    );
    assert!(
        serde_json::to_vec(&verdict).unwrap().len() < 64 * 1024,
        "the verdict is not bounded: {verdict}"
    );
}

/// A failing discovered target is named with its exit code after the three
/// serialized lib repetitions, and the remaining targets are not run.
#[test]
fn parallel_lib_oracle_fails_bounded_on_a_failing_discovered_target() {
    let case = OracleWorkspace::new();
    case.target("alpha", PASSING_TARGET);
    case.target("zulu", FAILING_TARGET);
    let (request, sha) = case.request();

    let output = case.checker(&request, &sha, &[]);
    let verdict: Value = record(&output);
    assert!(!output.status.success(), "{verdict}");
    let failure = &verdict["failure"];
    assert_eq!(failure["id"], "target:zulu", "{verdict}");
    assert_eq!(failure["exitCode"], 101, "{verdict}");
    assert_eq!(
        failure["command"],
        json!([
            verdict["cargo"],
            "test",
            "-p",
            "codex-harness",
            "--test",
            "zulu"
        ]),
        "{verdict}"
    );
    let steps = verdict["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 5, "{verdict}");
    assert_eq!(steps[3]["id"], "target:alpha", "{verdict}");
    assert_eq!(steps[3]["exitCode"], 0, "{verdict}");
    assert!(
        steps[4]["output"]
            .as_str()
            .unwrap()
            .contains("deliberate target failure"),
        "{verdict}"
    );
    assert!(
        serde_json::to_vec(&verdict).unwrap().len() < 64 * 1024,
        "the verdict is not bounded: {verdict}"
    );
}

/// Discovery reads the current unsplit target list exactly as Cargo resolves
/// it and reads a split `[[test]]` list through the same logic.
#[test]
fn parallel_lib_oracle_discovery_reflects_the_current_and_a_split_target_list() {
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let current = cargo_metadata_test_targets(&checkout);
    assert!(!current.is_empty(), "the current target list is empty");
    let root = tempfile::tempdir().unwrap();
    let request = root.path().join("parallel-lib-request.json");
    write_text(
        &request,
        &serde_json::to_string(&json!({
            "schema": 1,
            "source_root": checkout,
            "lib_repetitions": 3,
            "regression_targets": "auto",
        }))
        .unwrap(),
    );
    let sha = hash_file(&request).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_harness-executor-fixture"));
    command
        .args(["parallel-lib-oracle", "--discover", "--request"])
        .arg(&request)
        .args(["--request-sha256", &sha, "--workspace"])
        .arg(&checkout);
    let output = command.output().unwrap();
    let verdict: Value = record(&output);
    assert!(output.status.success(), "{verdict}");
    assert_eq!(verdict["discoverOnly"], true, "{verdict}");
    assert!(verdict["passed"].is_null(), "{verdict}");
    assert_eq!(verdict["steps"].as_array().unwrap().len(), 0, "{verdict}");
    let mut discovered = string_array(&verdict["targets"]);
    discovered.sort();
    assert_eq!(discovered, current, "{verdict}");

    let split = OracleWorkspace::new();
    split.manifest(
        "[package]\nname = \"codex-harness\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[[test]]\nname = \"matrix\"\npath = \"tests/matrix.rs\"\n\n[[test]]\nname = \"vector\"\npath = \"tests/vector.rs\"\n",
    );
    split.target("matrix", PASSING_TARGET);
    split.target("vector", PASSING_TARGET);
    split.target("audit", PASSING_TARGET);
    let (request, sha) = split.request();
    let output = split.checker(&request, &sha, &["--discover"]);
    let verdict: Value = record(&output);
    assert!(output.status.success(), "{verdict}");
    assert_eq!(
        verdict["targets"],
        json!(["matrix", "vector", "audit"]),
        "{verdict}"
    );

    let plain = OracleWorkspace::new();
    plain.target("matrix", PASSING_TARGET);
    plain.target("vector", PASSING_TARGET);
    plain.target("audit", PASSING_TARGET);
    let (request, sha) = plain.request();
    let output = plain.checker(&request, &sha, &["--discover"]);
    let verdict: Value = record(&output);
    assert!(output.status.success(), "{verdict}");
    assert_eq!(
        verdict["targets"],
        json!(["audit", "matrix", "vector"]),
        "{verdict}"
    );

    let mut split_names = vec!["matrix".to_owned(), "vector".to_owned(), "audit".to_owned()];
    let mut plain_names = vec!["audit".to_owned(), "matrix".to_owned(), "vector".to_owned()];
    split_names.sort();
    plain_names.sort();
    assert_eq!(split_names, plain_names);
}
