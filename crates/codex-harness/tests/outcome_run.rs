//! Actual CLI and Rust process doubles. No live model, OAuth or global writes.
#![cfg(windows)]
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const PROMPT: &str =
    "Literal $() `quotes` 'single' \"double\" ; & проверка\nSecond line unchanged.";
struct Fixture {
    root: tempfile::TempDir,
    case: PathBuf,
    home: PathBuf,
    request: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("outcome-run проверка-")
            .tempdir()
            .unwrap();
        let case = root.path().join("case");
        let home = root.path().join("home");
        fs::create_dir(&case).unwrap();
        fs::create_dir(&home).unwrap();
        let request = root.path().join("request.json");
        let value = json!({"case_root":case,"codex_home":home,"launcher":env!("CARGO_BIN_EXE_harness-launch-fixture"),
            "prompt":PROMPT,"timeout":5,"useful_command_pattern":"^native verify$","extra_config":{"skills.config":[]}});
        fs::write(&request, serde_json::to_vec(&value).unwrap()).unwrap();
        Self {
            root,
            case,
            home,
            request,
        }
    }
    fn edit(&self, key: &str, value: Value) {
        let mut request: Value = read(&self.request);
        request[key] = value;
        fs::write(&self.request, serde_json::to_vec(&request).unwrap()).unwrap();
    }
    fn command(&self, mode: &str) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        cmd.args(["outcome-run", "--request"])
            .arg(&self.request)
            .arg("--run-model-probes")
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "outcome")
            .env("HARNESS_OUTCOME_FIXTURE", mode)
            .current_dir(self.root.path());
        cmd
    }
    fn run(&self, mode: &str, code: i32) -> Value {
        let out = self.command(mode).output().unwrap();
        self.check(out, code)
    }
    fn check(&self, out: Output, code: i32) -> Value {
        assert_eq!(
            out.status.code(),
            Some(code),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        let evidence = Path::new(value["evidence_root"].as_str().unwrap());
        assert!(!evidence.starts_with(self.root.path()));
        assert!(!evidence.starts_with(Path::new(env!("CARGO_MANIFEST_DIR"))));
        assert_eq!(read(&evidence.join("native.json")), value);
        assert!(!String::from_utf8_lossy(&out.stdout).contains("private fixture credential"));
        assert!(!String::from_utf8_lossy(&out.stderr).contains(PROMPT));
        value
    }
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn evidence(row: &Value) -> PathBuf {
    PathBuf::from(row["evidence_root"].as_str().unwrap())
}
fn has_error(row: &Value, error: &str) -> bool {
    row["evidence_errors"]
        .as_array()
        .is_some_and(|a| a.iter().any(|v| v == error))
}

#[test]
fn default_does_not_read_or_execute_and_selection_is_explicit() {
    let fixture = Fixture::new();
    let out = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["outcome-run", "--request", "missing-private-request.json"])
        .current_dir(fixture.root.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["status"], "skipped");
    assert_eq!(result["model_calls"], 0);
    assert!(!fixture.case.join("fixture-call.json").exists());
    for args in [
        vec!["--run-model-probes"],
        vec!["--run-model-probes", "--all"],
        vec!["--run-model-probes", "--run-model-probes"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .arg("outcome-run")
            .args(args)
            .current_dir(fixture.root.path())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
    }
}

#[test]
fn actual_prompt_argv_home_evidence_and_usage_are_preserved() {
    let f = Fixture::new();
    let before = fs::read(&f.request).unwrap();
    let row = f.run("success", 0);
    let call = read(&f.case.join("fixture-call.json"));
    assert_eq!(row["status"], "completed");
    assert_eq!(row["model"], "gpt-6-astra");
    assert_eq!(row["effort"], "xhigh");
    assert_eq!(call["prompt"], PROMPT);
    assert_eq!(
        Path::new(call["cwd"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        f.case.canonicalize().unwrap()
    );
    assert_eq!(
        Path::new(call["home"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        f.home.canonicalize().unwrap()
    );
    let args = call["argv"].as_array().unwrap();
    assert_eq!(args[0], "exec");
    assert_eq!(args.last().unwrap(), "-");
    assert!(args.contains(&json!("--strict-config")));
    assert!(args.contains(&json!("--skip-git-repo-check")));
    assert!(args.contains(&json!("model_reasoning_effort=\"xhigh\"")));
    assert!(args.contains(&json!("skills.config=[]")));
    assert_eq!(row["first_command_result"]["item_id"], "read");
    assert_eq!(row["first_useful_signal"]["item_id"], "check");
    assert_eq!(row["usage"]["totals"]["total_tokens"], 27);
    assert_eq!(row["observed_model_metadata_verified"], true);
    assert_eq!(row["process"]["AssignedBeforeResume"], true);
    assert_eq!(
        row["process"]["MemoryLimitBytes"],
        2_u64 * 1024 * 1024 * 1024
    );
    assert_eq!(row["process"]["job"]["active_processes"], 0);
    assert!(evidence(&row).join("observed.jsonl").is_file());
    assert!(evidence(&row).join("result.json").is_file());
    assert_eq!(row["process"], read(&evidence(&row).join("result.json")));
    assert_eq!(fs::read(&f.request).unwrap(), before);
    assert!(row.get("passed").is_none()); // Process completion is not correctness.
}

#[test]
fn no_policy_does_not_invent_usefulness_and_no_rollout_is_unknown() {
    let f = Fixture::new();
    f.edit("useful_command_pattern", Value::Null);
    let row = f.run("no-rollout", 0);
    assert!(row["first_useful_signal"].is_null());
    assert!(!row["first_command_result"].is_null());
    assert_eq!(row["usage"]["status"], "unknown");
    assert!(row["usage"]["totals"]["total_tokens"].is_null());
    assert_eq!(row["observed_model_metadata_verified"], false);
    assert!(has_error(&row, "observed_model_policy_unverified"));
}

#[test]
fn completed_process_requires_terminal_event_and_rejects_error_events() {
    for (mode, status, code) in [
        ("incomplete", "incomplete", 1),
        ("nonzero", "failed", 1),
        ("error", "failed", 1),
        ("failed-turn", "failed", 1),
    ] {
        let f = Fixture::new();
        let row = f.run(mode, code);
        assert_eq!(row["status"], status);
        assert_eq!(
            row["process"]["ExitCode"],
            if mode == "nonzero" { 19 } else { 0 }
        );
        assert!(evidence(&row).join("events.jsonl").is_file());
    }
}

#[test]
fn malformed_partial_conflicting_and_child_evidence_is_retained() {
    for (mode, error) in [
        ("corrupt", "malformed_native_event"),
        ("truncated", "truncated_native_event"),
        ("conflicting-thread", "conflicting_thread_identity"),
        ("missing-thread", "missing_thread"),
        ("children", "unexpected_delegation"),
    ] {
        let f = Fixture::new();
        let row = f.run(mode, 0);
        assert!(has_error(&row, error), "{row}");
        if mode == "children" {
            assert_eq!(row["children"].as_array().unwrap().len(), 1);
        }
    }
    let f = Fixture::new();
    let row = f.run("duplicate-rollout", 0);
    assert!(row["rollout_paths"].as_array().unwrap().is_empty());
    assert_eq!(row["usage"]["status"], "unknown");
    let f = Fixture::new();
    let row = f.run("wrong-model", 0);
    assert!(has_error(&row, "observed_model_policy_unverified"));
    assert_eq!(row["observed_model_metadata_verified"], false);
}

#[test]
fn invalid_home_launcher_policy_and_override_fail_without_process() {
    let f = Fixture::new();
    for (key, value) in [
        ("codex_home", json!(env!("CARGO_MANIFEST_DIR"))),
        ("codex_home", json!(f.case)),
        ("launcher", json!(f.root.path().join("missing.exe"))),
        ("timeout", json!(0)),
        ("output_limit", json!(1)),
        ("useful_command_pattern", json!("(?=unsupported)")),
    ] {
        let saved = fs::read(&f.request).unwrap();
        f.edit(key, value);
        let row = f.run("success", 1);
        assert_eq!(row["status"], "failed");
        assert!(!evidence(&row).join("started.json").exists());
        assert!(!f.case.join("fixture-call.json").exists());
        fs::write(&f.request, saved).unwrap();
    }
    for key in [
        "model",
        "model_provider",
        "model_providers.openai.base_url",
        "profile",
        "service_tier",
        "auth",
        "\"model\"",
        " model ",
    ] {
        f.edit("extra_config", json!({key:"PRIVATE_OVERRIDE"}));
        let row = f.run("success", 1);
        assert_eq!(row["status"], "failed");
        assert!(!row.to_string().contains("PRIVATE_OVERRIDE"));
        assert!(!f.case.join("fixture-call.json").exists());
    }
}

#[test]
fn launch_failure_preserves_unique_private_attempts() {
    let f = Fixture::new();
    let broken = f.root.path().join("broken.exe");
    fs::write(&broken, "not a PE executable").unwrap();
    f.edit("launcher", json!(broken));
    let a = f.run("success", 1);
    let b = f.run("success", 1);
    assert_ne!(a["evidence_root"], b["evidence_root"]);
    for row in [a, b] {
        assert_eq!(row["status"], "failed");
        assert!(row["usage"]["total_tokens"].is_null());
        assert!(evidence(&row).join("native-started.json").is_file());
    }
}

#[test]
fn provider_endpoint_is_not_an_outcome_treatment() {
    let f = Fixture::new();
    for key in ["openai_base_url", "chatgpt_base_url"] {
        f.edit("extra_config", json!({key:"http://127.0.0.1:9/v1"}));
        let row = f.run("success", 1);
        assert!(!f.case.join("fixture-call.json").exists());
        assert_eq!(row["status"], "failed");
    }
}

#[test]
fn private_failure_retains_the_launch_phase_and_original_windows_error() {
    let f = Fixture::new();
    let broken = f.root.path().join("broken.exe");
    fs::write(&broken, "not a PE executable").unwrap();
    let native_error = Command::new(&broken)
        .current_dir(&f.case)
        .output()
        .unwrap_err();
    let original_code = native_error
        .raw_os_error()
        .expect("native Windows launch error");
    f.edit("launcher", json!(broken));
    let row = f.run("success", 1);
    let failure = read(&evidence(&row).join("failure.json"));
    assert_eq!(failure["phase"], "launch");
    assert_eq!(failure["raw_os_error"], original_code);
    assert!(!failure["kind"].as_str().unwrap().is_empty());
    assert!(!failure["message"].as_str().unwrap().is_empty());
    assert!(row.get("raw_os_error").is_none());
    assert!(row.get("message").is_none());
}

#[test]
fn a_matching_rollout_filename_is_not_proof_of_its_thread_identity() {
    let f = Fixture::new();
    let row = f.run("misnamed-rollout", 0);
    assert!(has_error(&row, "rollout_identity_mismatch"));
    assert_eq!(row["observed_model_metadata_verified"], false);
    assert_eq!(row["usage"]["status"], "unknown");
    assert!(row["usage"]["totals"]["total_tokens"].is_null());
    assert_eq!(
        read(&evidence(&row).join("rejected-usage.json"))["totals"]["total_tokens"],
        27
    );
}

#[test]
fn timeout_cancellation_and_root_exit_clean_owned_descendants() {
    let mut cases = Vec::new();
    for mode in ["timeout", "root-exit-child", "cancel"] {
        let f = Fixture::new();
        f.edit("timeout", json!(1));
        let row = if mode == "cancel" {
            let marker = f.root.path().join("cancel");
            f.edit("cancel_file", json!(marker));
            f.edit("timeout", json!(10));
            let child = f
                .command(mode)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let until = Instant::now() + Duration::from_secs(5);
            while !f.case.join("descendant-pid.txt").exists() && Instant::now() < until {
                thread::sleep(Duration::from_millis(10));
            }
            assert!(f.case.join("descendant-pid.txt").exists());
            fs::write(marker, "").unwrap();
            f.check(child.wait_with_output().unwrap(), 1)
        } else {
            f.run(mode, if mode == "timeout" { 1 } else { 0 })
        };
        assert_eq!(
            row["process"]["Status"],
            match mode {
                "timeout" => "timeout",
                "cancel" => "cancelled",
                _ => "exited",
            }
        );
        assert_eq!(row["process"]["job"]["active_processes"], 0);
        cases.push(f);
    }
    thread::sleep(Duration::from_secs(3));
    for f in cases {
        assert!(!f.case.join("descendant-survived.txt").exists());
    }
}

#[test]
fn output_and_final_file_limits_stop_the_actual_process() {
    for mode in ["output-limit", "final-limit"] {
        let f = Fixture::new();
        f.edit("output_limit", json!(64 * 1024));
        let row = f.run(mode, 1);
        assert_eq!(row["status"], "failed");
        assert_eq!(row["process"]["Status"], "output-limit");
        assert_eq!(row["process"]["job"]["active_processes"], 0);
    }
}
