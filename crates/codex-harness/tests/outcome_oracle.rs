//! Actual native preparer/executor/oracle consumers, with an explicit model-free
//! upstream double. The independent product/process targets are real executables.
#![cfg(windows)]
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn output(command: &mut Command, code: i32) -> Value {
    let out = command.output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(code),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
struct Case {
    _host: tempfile::TempDir,
    root: PathBuf,
    request: PathBuf,
    native_root: PathBuf,
}
impl Case {
    fn new(case: &str, arm: &str) -> Self {
        let host = tempfile::tempdir().unwrap();
        let mut prepare = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        prepare
            .args(["outcome-prepare", "--case", case])
            .current_dir(host.path());
        if case == "process" {
            prepare.args(["--observer", env!("CARGO_BIN_EXE_harness-observe")]);
        }
        let prepared = output(&mut prepare, 0);
        let root = PathBuf::from(prepared["case_root"].as_str().unwrap());
        let home = host.path().join("home");
        fs::create_dir(&home).unwrap();
        let native_request = host.path().join("native-request.json");
        write(
            &native_request,
            &json!({"case_root":root,"codex_home":home,
            "launcher":env!("CARGO_BIN_EXE_harness-launch-fixture"),"prompt":"Explicit owned native double; no model.","timeout":10}),
        );
        let mut launch = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        launch
            .args(["outcome-run", "--request"])
            .arg(&native_request)
            .arg("--run-model-probes")
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "outcome")
            .env("HARNESS_OUTCOME_FIXTURE", "oracle")
            .env_remove("HARNESS_OUTCOME_ORACLE_SKILL")
            .current_dir(host.path());
        if arm == "candidate" && case != "negative" {
            launch.env(
                "HARNESS_OUTCOME_ORACLE_SKILL",
                if case == "process" {
                    "reproduce-regression"
                } else {
                    "project-verification"
                },
            );
        }
        let native = output(&mut launch, 0);
        let native_root = PathBuf::from(native["evidence_root"].as_str().unwrap());
        let request = host.path().join("oracle-request.json");
        write(
            &request,
            &json!({"case_root":root,"setup":prepared["setup"],"execution":native_root.join("native.json"),"arm":arm}),
        );
        Self {
            _host: host,
            root,
            request,
            native_root,
        }
    }
    fn report(&self, blocked: bool) {
        write(
            &self.root.join("outcome.json"),
            &json!({"status":if blocked {"blocked"} else {"passed"},
            "command":if blocked {"unavailable-checker.exe --verify"} else {"owned verification"},
            "scope":"Only requested controlled behavior", "result":"see owned evidence"}),
        );
    }
    fn product(&self, mode: &str) {
        let out = Command::new(self.root.join("case.exe"))
            .args(["--outcome-case", mode])
            .current_dir(self._host.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        command
            .args(["outcome-oracle", "--request"])
            .arg(&self.request)
            .current_dir(self._host.path());
        command
    }
    fn check(&self, passed: bool) -> Value {
        let record = output(&mut self.command(), if passed { 0 } else { 1 });
        assert_eq!(record["passed"], passed);
        assert_eq!(record["executed"], true);
        assert_eq!(record["model_calls"], 0);
        let evidence = Path::new(record["evidence"].as_str().unwrap());
        assert_eq!(record, read(evidence));
        assert!(!evidence.starts_with(&self.root));
        record
    }
    fn append_event(&self, item: Value) {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(self.native_root.join("events.jsonl"))
            .unwrap();
        writeln!(file, "{}", json!({"type":"item.completed","item":item})).unwrap();
    }
}
fn check(record: &Value, name: &str) -> bool {
    record["details"]["checks"][name].as_bool().unwrap()
}
fn corrected_guide(case: &Case) {
    fs::write(
        case.root.join("README.md"),
        "# Guide\n\nRun verification; see [details](guide.md).\n",
    )
    .unwrap();
}

#[test]
fn real_entrypoint_freshness_missing_and_narrow_document_cases_pass_with_scoped_evidence() {
    for (id, arm) in [
        ("entrypoint", "baseline"),
        ("freshness", "candidate"),
        ("missing", "candidate"),
        ("negative", "candidate"),
    ] {
        let case = Case::new(id, arm);
        case.report(id == "missing");
        if ["entrypoint", "freshness"].contains(&id) {
            case.product("build");
            case.product("cli");
        }
        if id == "freshness" {
            use std::io::Write;
            writeln!(fs::OpenOptions::new().append(true).open(case.root.join("docs/validation.md")).unwrap(),"Current source version 2: rebuilt and ran the product CLI, stdout 2, exit 0. Earlier result is historical only.").unwrap();
        }
        if id == "negative" {
            corrected_guide(&case);
        }
        let result = case.check(true);
        assert!(check(&result, "immutable_after_check"));
        assert!(check(&result, "complete_evidence"));
        assert!(
            result["details"]["skill_signal_scope"]
                .as_str()
                .unwrap()
                .contains("not proof")
        );
    }
}

#[test]
fn independent_cli_probe_cannot_supply_missing_candidate_execution_even_when_repeated() {
    let case = Case::new("entrypoint", "candidate");
    case.report(false);
    case.product("build");
    for _ in 0..2 {
        let result = case.check(false);
        assert!(check(&result, "build_executed"));
        assert!(!check(&result, "cli_executed"));
        assert!(check(&result, "actual_entrypoint_v2"));
    }
    let rows: Vec<Value> = fs::read_to_string(case.root.join("execution-audit.jsonl"))
        .unwrap()
        .lines()
        .map(|row| serde_json::from_str(row).unwrap())
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1]["origin"], "independent-oracle");
    assert_eq!(rows[2]["origin"], "independent-oracle");
}

#[test]
fn claimed_success_stale_generated_data_and_removed_history_are_rejected() {
    let stale = Case::new("entrypoint", "baseline");
    stale.report(false);
    let result = stale.check(false);
    assert!(!check(&result, "actual_entrypoint_v2"));
    assert!(!check(&result, "build_executed"));
    assert!(!check(&result, "generated_matches_source"));
    let history = Case::new("freshness", "candidate");
    history.report(false);
    history.product("build");
    history.product("cli");
    fs::write(
        history.root.join("docs/validation.md"),
        "Current source version 2, stdout 2.\n",
    )
    .unwrap();
    assert!(!check(&history.check(false), "historical_record_retained"));
}

#[test]
fn altered_immutable_target_and_reparse_input_never_reach_independent_execution() {
    let altered = Case::new("entrypoint", "baseline");
    altered.report(false);
    fs::write(altered.root.join("case.exe"), b"altered target").unwrap();
    let result = altered.check(false);
    assert!(!check(&result, "immutable_inputs"));
    assert!(result["details"]["runs"].as_array().unwrap().is_empty());
    assert!(!altered.root.join("execution-audit.jsonl").exists());
    let redirected = Case::new("negative", "baseline");
    redirected.report(false);
    corrected_guide(&redirected);
    let foreign = redirected._host.path().join("foreign.md");
    fs::write(&foreign, b"preserve foreign").unwrap();
    fs::remove_file(redirected.root.join("guide.md")).unwrap();
    std::os::windows::fs::symlink_file(&foreign, redirected.root.join("guide.md")).unwrap();
    assert!(!check(&redirected.check(false), "oracle_completed"));
    assert_eq!(fs::read(&foreign).unwrap(), b"preserve foreign");
}

#[test]
fn malformed_or_absent_events_and_incomplete_native_result_never_count_as_success() {
    let case = Case::new("negative", "baseline");
    case.report(false);
    corrected_guide(&case);
    let mut native = read(&case.native_root.join("native.json"));
    native["turn_completed"] = json!(false);
    write(&case.native_root.join("native.json"), &native);
    assert!(!check(&case.check(false), "native_completed"));
    native["turn_completed"] = json!(true);
    write(&case.native_root.join("native.json"), &native);
    case.append_event(json!({"type":"command_execution","command":"private malformed command"}));
    let result = case.check(false);
    assert!(!check(&result, "complete_evidence"));
    assert!(!result.to_string().contains("private malformed command"));
    fs::remove_file(case.native_root.join("events.jsonl")).unwrap();
    assert!(!check(&case.check(false), "oracle_completed"));
}

#[test]
fn negative_scope_rejects_extra_work_and_unknown_delegation_without_executing_it() {
    let case = Case::new("negative", "baseline");
    case.report(false);
    corrected_guide(&case);
    let mut native = read(&case.native_root.join("native.json"));
    native.as_object_mut().unwrap().remove("children");
    write(&case.native_root.join("native.json"), &native);
    assert!(!check(&case.check(false), "no_delegation"));
    native["children"] = json!([]);
    write(&case.native_root.join("native.json"), &native);
    case.append_event(json!({"type":"command_execution","id":"extra","status":"failed","command":"npm test","aggregated_output":"failed","exit_code":1}));
    assert!(!check(&case.check(false), "no_unrelated_execution"));
}

#[test]
fn frozen_host_request_external_cases_and_missing_tool_substitution_are_checked() {
    let case = Case::new("missing", "baseline");
    case.report(true);
    fs::write(
        case.root.join("unavailable-checker.exe"),
        b"unrequested substitute",
    )
    .unwrap();
    assert!(!check(&case.check(false), "prerequisite_still_absent"));
    let mut request = read(&case.request);
    request["setup"]["case_id"] = json!("focused");
    write(&case.request, &request);
    assert!(!check(&case.check(false), "oracle_completed"));
    let inner = case.root.join("candidate-request.json");
    fs::copy(&case.request, &inner).unwrap();
    let out: Output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["outcome-oracle", "--request"])
        .arg(inner)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn process_oracle_runs_the_candidate_checker_again_and_observes_real_targets() {
    let case = Case::new("process", "candidate");
    case.report(false);
    fs::copy(
        env!("CARGO_BIN_EXE_harness-launch-fixture"),
        case.root.join("check_process.exe"),
    )
    .unwrap();
    let previous = json!({"old":"must not count as a new result"});
    write(&case.root.join("process-results.json"), &previous);
    let result = output(
        case.command()
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "outcome-process-checker")
            .env_remove("HARNESS_OUTCOME_CHECKER_FAULT"),
        0,
    );
    assert_eq!(result["passed"], true);
    for name in [
        "executable_check",
        "all_real_targets_executed",
        "hang_did_not_exit_naturally",
        "flood_status",
        "fail_status",
        "no-ready_status",
        "hang_status",
        "complete_stdout",
        "complete_stderr",
        "descendant_cleaned",
        "unrelated_process_survives",
        "immutable_after_check",
    ] {
        assert!(check(&result, name), "{name}: {result}");
    }
    assert!(!case.root.join("descendant-survived.txt").exists());
    assert_ne!(read(&case.root.join("process-results.json")), previous);
    let evidence = Path::new(result["evidence"].as_str().unwrap())
        .parent()
        .unwrap();
    assert_eq!(
        read(&evidence.join("previous-process-results.json")),
        previous
    );
    assert_eq!(
        fs::read(case.root.join("unrelated.txt")).unwrap(),
        b"preserve\n"
    );
}

#[test]
fn process_oracle_rejects_stale_reports_forced_exit_codes_and_incomplete_streams() {
    for (fault, failed_check) in [
        ("stale", "oracle_completed"),
        ("wrong-forced-code", "hang_status"),
        ("truncated-stream", "complete_stdout"),
    ] {
        let case = Case::new("process", "candidate");
        case.report(false);
        fs::copy(
            env!("CARGO_BIN_EXE_harness-launch-fixture"),
            case.root.join("check_process.exe"),
        )
        .unwrap();
        let previous = json!({"flood":{"status":"exited","exit_code":0}});
        write(&case.root.join("process-results.json"), &previous);
        let result = output(
            case.command()
                .env("HARNESS_LAUNCH_FIXTURE_MODE", "outcome-process-checker")
                .env("HARNESS_OUTCOME_CHECKER_FAULT", fault),
            1,
        );
        assert_eq!(result["passed"], false);
        assert!(!check(&result, failed_check), "{fault}: {result}");
        assert!(check(&result, "unrelated_process_survives"));
        assert!(check(&result, "sentinel_cleaned"));
        let evidence = Path::new(result["evidence"].as_str().unwrap())
            .parent()
            .unwrap();
        assert_eq!(
            read(&evidence.join("previous-process-results.json")),
            previous
        );
    }
}
