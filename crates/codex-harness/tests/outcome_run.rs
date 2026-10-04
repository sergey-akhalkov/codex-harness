//! Actual CLI and Rust process doubles. No live model, OAuth or global writes.
#![cfg(windows)]
use harness_core::{
    build_identity::hash_file,
    outcome_qualification::{
        API_OBSERVED_LIMITS, ApiObservationPlan, ApiObservations, ApiObservedPolicy, ClientInput,
        ObservationFailureKind, QualificationAttempt, QualificationMode, QualificationStatus,
        RepeatabilityPolicy, RunnerRecord, collect_observations, drift, observed_drift,
        qualification_attempt, qualify, qualify_api_observed, recheck_observations, smoke,
    },
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    thread::JoinHandle,
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

/// Synthetic local runner values: never the supplied private model identity.
fn local_identity() -> Value {
    json!({
        "weights": "synth-gguf-sha256-0000",
        "quantization": "Q4_K_M",
        "tokenizer": "synth-tokenizer-v1",
        "template": "synth-template-v3",
        "backend": "synth-server-build-1",
        "sampling": "temperature=1.0;top_k=20;top_p=0.95",
        "seed": "server-default-random",
        "reasoning": "xhigh",
        "context": "262144",
        "cache": "single-slot",
        "environment": "owned fixture double; no inference"
    })
}

fn local_runner() -> Value {
    json!({
        "endpoint": "http://127.0.0.1:65500/v1",
        "model": "fixture-local-model",
        "identity": local_identity()
    })
}

fn policy() -> RepeatabilityPolicy {
    RepeatabilityPolicy {
        repeats: 2,
        required_outputs: vec!["solution.txt".into()],
        ignored_metadata: vec!["timing".into(), "thread_id".into()],
    }
}

fn observation(result: &Value, digest: &str) -> QualificationAttempt {
    qualification_attempt(
        result,
        BTreeMap::from([("solution.txt".to_owned(), digest.to_owned())]),
    )
    .unwrap()
}

fn recorded_runner(result: &Value) -> harness_core::outcome_qualification::LocalRunner {
    let record: RunnerRecord = serde_json::from_value(result["runner"].clone()).unwrap();
    record.runner()
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
fn optional_user_home_is_applied_and_kept_outside_the_case() {
    let f = Fixture::new();
    let user = f.root.path().join("user");
    fs::create_dir(&user).unwrap();
    f.edit("user_home", json!(user));
    let row = f.run("success", 0);
    let call = read(&f.case.join("fixture-call.json"));
    assert_eq!(row["status"], "completed");
    assert_eq!(
        Path::new(call["userprofile"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        user.canonicalize().unwrap()
    );
    assert_ne!(
        Path::new(call["userprofile"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        f.case.canonicalize().unwrap()
    );
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
    // The deliberate invalid-image attempt must produce an error code, not a
    // Windows loader dialog on the operator's desktop.
    harness_core::process::suppress_loader_dialogs();
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

#[test]
fn local_runner_configuration_is_explicit_recorded_and_shared() {
    let f = Fixture::new();
    f.edit("runner", local_runner());
    let row = f.run("success", 0);
    assert_eq!(row["status"], "completed");
    assert_eq!(row["model"], "fixture-local-model");
    assert_eq!(row["effort"], "xhigh");
    assert_eq!(row["runner"]["kind"], "local");
    assert_eq!(row["runner"]["wire_api"], "responses");
    assert_eq!(row["runner"]["endpoint"], "http://127.0.0.1:65500/v1");
    assert_eq!(row["runner"]["identity"]["quantization"], "Q4_K_M");
    assert!(
        row["runner"]["identity_missing"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // The actual launcher invocation consumed the explicit local route.
    let call = read(&f.case.join("fixture-call.json"));
    let args = call["argv"].as_array().unwrap();
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "-m" && pair[1] == "fixture-local-model")
    );
    assert!(args.contains(&json!("model_reasoning_effort=\"xhigh\"")));
    assert!(args.contains(&json!("model_provider=\"local\"")));
    assert!(args.contains(&json!("--disable")) && args.contains(&json!("hooks")));
    // The effective provider configuration in the isolated home is the
    // Responses-API local provider with no credentials.
    let text = fs::read_to_string(f.home.join("config.toml")).unwrap();
    let config: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(config["model"].as_str(), Some("fixture-local-model"));
    assert_eq!(config["model_provider"].as_str(), Some("local"));
    assert_eq!(
        config["model_providers"]["local"]["base_url"].as_str(),
        Some("http://127.0.0.1:65500/v1")
    );
    assert_eq!(
        config["model_providers"]["local"]["wire_api"].as_str(),
        Some("responses")
    );
    assert!(config["model_providers"]["local"].get("env_key").is_none());
    assert_eq!(config["approval_policy"].as_str(), Some("never"));
    // The observed rollout echoed the requested identity through real plumbing.
    assert_eq!(row["observed_model_metadata_verified"], true);
    assert_eq!(row["observed_threads"][0]["model"], "fixture-local-model");
    assert_eq!(row["observed_threads"][0]["reasoning"], "xhigh");
    assert!(row["observed_threads"][0]["provider"].is_null());
    // The verified transport limitation is evidence, not a silent assumption:
    // per-request seed/cache_prompt/decoding controls are not expressible.
    assert_eq!(row["determinism"]["client_overrides"], json!([]));
    assert_eq!(row["determinism"]["effect"], "server-side defaults apply");
    assert_eq!(
        row["determinism"]["unsupported_request_controls"],
        json!(["seed", "temperature", "top_p", "cache_prompt"])
    );
    // Local usage stays unattributed instead of being booked to a cloud route.
    assert_eq!(row["usage"]["unattributed"]["total_tokens"], 27);
    assert_eq!(row["usage"]["unattributed"]["thread_count"], 1);
    assert_eq!(row["usage"]["status"], "partial");
    assert!(
        row["usage"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "unsupported_or_missing_model")
    );
}

#[test]
fn local_runner_rejects_missing_invalid_or_conflicting_selection() {
    let f = Fixture::new();
    for (key, value) in [
        ("endpoint", json!("")),
        ("endpoint", json!("not-a-url")),
        ("endpoint", json!("ftp://127.0.0.1:65500/v1")),
        (
            "endpoint",
            json!("http://user:private-secret@127.0.0.1:65500/v1"),
        ),
        (
            "endpoint",
            json!("http://127.0.0.1:65500/v1?token=PRIVATE_QUERY"),
        ),
        ("endpoint", json!("http://127.0.0.1:65500/v1#fragment")),
        ("model", json!("")),
        ("model", json!("x".repeat(300))),
        ("identity.weights", json!("w".repeat(600))),
        ("identity.reasoning", json!("brisk")),
    ] {
        let mut runner = local_runner();
        if let Some(identity_key) = key.strip_prefix("identity.") {
            runner["identity"][identity_key] = value;
        } else {
            runner[key] = value;
        }
        f.edit("runner", runner);
        let row = f.run("success", 1);
        assert_eq!(row["status"], "failed", "{key}");
        assert!(!evidence(&row).join("started.json").exists());
        assert!(!f.case.join("fixture-call.json").exists());
        let serialized = row.to_string();
        for secret in ["private-secret", "PRIVATE_QUERY"] {
            assert!(!serialized.contains(secret), "{key}");
        }
    }
    // An explicit profile route and an explicit local runner never combine.
    f.edit("runner", local_runner());
    f.edit("profile", json!("xai"));
    let row = f.run("success", 1);
    assert_eq!(row["status"], "failed");
    assert!(!f.case.join("fixture-call.json").exists());
    // Structurally missing or unknown runner fields are rejected before any
    // evidence directory exists.
    for runner in [
        json!({"model": "fixture-local-model", "identity": local_identity()}),
        json!({"endpoint": "http://127.0.0.1:65500/v1", "identity": local_identity()}),
        json!({"endpoint": "http://127.0.0.1:65500/v1", "model": "fixture-local-model",
               "wire_api": "responses", "identity": local_identity()}),
        json!({"endpoint": "http://127.0.0.1:65500/v1", "model": "fixture-local-model",
               "identity": local_identity(), "extra": true}),
    ] {
        f.edit("runner", runner);
        let out = f.command("success").output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(!f.case.join("fixture-call.json").exists());
    }
}

#[test]
fn missing_identity_is_recorded_and_blocks_qualification() {
    let f = Fixture::new();
    let mut runner = local_runner();
    runner["identity"]["quantization"] = Value::Null;
    runner["identity"]["cache"] = Value::Null;
    f.edit("runner", runner);
    let row = f.run("success", 0);
    assert_eq!(row["observed_model_metadata_verified"], true);
    assert_eq!(
        row["runner"]["identity_missing"],
        json!(["quantization", "cache"])
    );
    let mut first = observation(&row, "digest-a");
    let mut second = first.clone();
    second.attempt_id.push_str("#2");
    first.attempt_id.push_str("#1");
    let result = qualify(&recorded_runner(&row), &policy(), &[first, second]).unwrap();
    assert!(result.blocks_comparisons());
    assert_eq!(result.missing_identity, vec!["quantization", "cache"]);
    assert_eq!(result.divergent_outputs.len(), 0);
}

#[test]
fn controlled_repeats_qualify_and_divergent_outputs_suspend_comparisons() {
    let f = Fixture::new();
    f.edit("runner", local_runner());
    let run = |content: &str| -> (Value, String) {
        let out = f
            .command("success")
            .env("HARNESS_OUTCOME_SOLUTION", content)
            .output()
            .unwrap();
        let row = f.check(out, 0);
        let digest = hash_file(&f.case.join("solution.txt")).unwrap();
        (row, digest)
    };
    let (first, first_digest) = run("repeatable controlled solution\n");
    assert_eq!(first["status"], "completed");
    assert_eq!(first["tool_operations"], 3);
    assert_eq!(first["rounds"], 1);
    let (second, second_digest) = run("repeatable controlled solution\n");
    assert_eq!(first_digest, second_digest);
    let runner = recorded_runner(&first);
    let mut a1 = observation(&first, &first_digest);
    let mut a2 = observation(&second, &second_digest);
    a1.attempt_id.push_str("#1");
    a2.attempt_id.push_str("#2");
    assert!(a1.model_metadata_verified && a2.model_metadata_verified);
    assert!(a1.tool_operations > 0 && a2.tool_operations > 0);
    let result = qualify(&runner, &policy(), &[a1.clone(), a2]).unwrap();
    assert_eq!(result.status, QualificationStatus::Qualified, "{result:?}");
    assert!(result.missing_identity.is_empty());

    // A repeat that produced different controlled output must suspend strict
    // dependent comparisons and name the divergent output.
    let (third, third_digest) = run("divergent controlled solution\n");
    assert_ne!(first_digest, third_digest);
    let mut a3 = observation(&third, &third_digest);
    a3.attempt_id.push_str("#3");
    let result = qualify(&runner, &policy(), &[a1.clone(), a3]).unwrap();
    assert!(result.blocks_comparisons());
    assert_eq!(result.divergent_outputs, vec!["solution.txt"]);
    assert!(result.missing_outputs.is_empty());
}

#[test]
fn text_only_attempt_reports_unsupported_tool_exchange() {
    let f = Fixture::new();
    f.edit("runner", local_runner());
    let row = f.run("text-only", 0);
    assert_eq!(row["status"], "completed");
    assert_eq!(row["tool_operations"], 0);
    assert_eq!(row["rounds"], 1);
    assert!(row["first_command_result"].is_null());
    assert_eq!(row["observed_model_metadata_verified"], true);
    let mut a1 = observation(&row, "digest-a");
    let mut a2 = a1.clone();
    a1.attempt_id.push_str("#1");
    a2.attempt_id.push_str("#2");
    let result = qualify(&recorded_runner(&row), &policy(), &[a1, a2]).unwrap();
    assert!(result.blocks_comparisons());
    assert_eq!(result.tool_exchange_missing.len(), 2);
}

#[test]
fn runner_drift_is_detected_from_recorded_attempt_evidence() {
    let f = Fixture::new();
    f.edit("runner", local_runner());
    let first = f.run("success", 0);
    let mut changed = local_runner();
    changed["identity"]["quantization"] = json!("Q5_K_M");
    f.edit("runner", changed);
    let second = f.run("success", 0);
    let result = drift(&recorded_runner(&first), &recorded_runner(&second));
    assert!(result.drifted);
    assert_eq!(result.changed, vec!["identity.quantization"]);
    let unchanged = drift(&recorded_runner(&first), &recorded_runner(&first));
    assert!(!unchanged.drifted && unchanged.changed.is_empty());
}

#[test]
fn local_route_refuses_provider_treatment_and_never_falls_back() {
    // A failed local attempt keeps its explicit route identity and does not
    // retry through another provider, model or endpoint.
    let f = Fixture::new();
    f.edit("runner", local_runner());
    let row = f.run("nonzero", 1);
    assert_eq!(row["status"], "failed");
    assert_eq!(row["model"], "fixture-local-model");
    assert_eq!(row["runner"]["kind"], "local");
    assert_eq!(row["tool_operations"], 3);
    assert_eq!(row["rounds"], 1);
    let request = read(&evidence(&row).join("request.json"));
    let args = request["arguments"].as_array().unwrap();
    assert_eq!(args.iter().filter(|arg| *arg == "-m").count(), 1);
    assert!(args.contains(&json!("fixture-local-model")));
    assert!(args.contains(&json!("model_provider=\"local\"")));
    assert_eq!(
        request["environment"]["removedProviderCredentials"],
        json!(["OPENAI_API_KEY", "OPENAI_BASE_URL"])
    );

    // Ambient provider credentials cannot reach an explicit local attempt.
    let f = Fixture::new();
    f.edit("runner", local_runner());
    let out = f
        .command("success")
        .env("OPENAI_API_KEY", "PRIVATE_AMBIENT_KEY")
        .output()
        .unwrap();
    let row = f.check(out, 0);
    assert!(!row.to_string().contains("PRIVATE_AMBIENT_KEY"));
    let call = read(&f.case.join("fixture-call.json"));
    assert_eq!(call["openai_api_key_present"], false);

    // Provider, model and runner settings stay invalid treatment options.
    let f = Fixture::new();
    f.edit("runner", local_runner());
    for key in [
        "model",
        "model_provider",
        "model_providers.local.base_url",
        "model_providers.local.env_key",
        "openai_base_url",
        "runner",
        "seed",
        "temperature",
        "top_p",
        "top_k",
        "cache_prompt",
    ] {
        f.edit("extra_config", json!({key: "PRIVATE_OVERRIDE"}));
        let row = f.run("success", 1);
        assert_eq!(row["status"], "failed", "{key}");
        assert!(!f.case.join("fixture-call.json").exists());
        assert!(!row.to_string().contains("PRIVATE_OVERRIDE"));
    }
}

#[test]
fn local_attempts_retain_counters_and_keep_accounting_scoped() {
    // Unknown usage stays unknown while observed counters are retained.
    let f = Fixture::new();
    f.edit("runner", local_runner());
    f.edit("useful_command_pattern", Value::Null);
    let row = f.run("no-rollout", 0);
    assert_eq!(row["status"], "completed");
    assert_eq!(row["tool_operations"], 3);
    assert_eq!(row["rounds"], 1);
    assert_eq!(row["usage"]["status"], "unknown");
    assert!(row["usage"]["totals"]["total_tokens"].is_null());

    // Overlapping token subsets are detected and stay recorded categories:
    // one model request carried several tool operations, and cached input or
    // reasoning tokens are not added into their supersets.
    let f = Fixture::new();
    f.edit("runner", local_runner());
    let out = f
        .command("success")
        .env("HARNESS_OUTCOME_TOKEN_OVERLAP", "1")
        .output()
        .unwrap();
    let row = f.check(out, 0);
    let totals = &row["usage"]["totals"];
    assert_eq!(totals["input_tokens"], 20);
    assert_eq!(totals["cached_input_tokens"], 25);
    assert_eq!(totals["output_tokens"], 7);
    assert_eq!(totals["reasoning_output_tokens"], 9);
    assert_eq!(totals["total_tokens"], 27);
    assert_eq!(row["usage"]["responses"]["response_count"], 1);
    assert_eq!(row["tool_operations"], 3);
    assert_eq!(row["rounds"], 1);
    assert_eq!(row["usage"]["status"], "partial");
    let warnings = row["usage"]["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w["code"] == "cached_input_exceeds_input")
    );
    assert!(
        warnings
            .iter()
            .any(|w| w["code"] == "reasoning_not_included_in_output")
    );

    // Timed-out work keeps its observed counters and process outcome.
    let f = Fixture::new();
    f.edit("runner", local_runner());
    f.edit("timeout", json!(1));
    let row = f.run("timeout", 1);
    assert_eq!(row["status"], "timeout");
    assert_eq!(row["process"]["Status"], "timeout");
    assert_eq!(row["tool_operations"], 3);
    assert_eq!(row["rounds"], 1);
    assert_eq!(row["usage"]["totals"]["total_tokens"], 27);
}

#[test]
fn smoke_observes_real_tool_round_trip_while_strict_qualification_stays_open() {
    let f = Fixture::new();
    // The supplied endpoint's weights and serving proof are not available on
    // this host; those material facts stay unknown instead of being invented.
    let mut runner = local_runner();
    for field in [
        "weights",
        "quantization",
        "tokenizer",
        "template",
        "backend",
        "sampling",
        "seed",
        "context",
        "cache",
    ] {
        runner["identity"][field] = Value::Null;
    }
    f.edit("runner", runner);
    let out = f
        .command("success")
        .env("HARNESS_OUTCOME_SOLUTION", "smoke solution\n")
        .output()
        .unwrap();
    let row = f.check(out, 0);
    assert_eq!(row["status"], "completed");
    assert_eq!(row["observed_model_metadata_verified"], true);
    let digest = hash_file(&f.case.join("solution.txt")).unwrap();
    let recorded = recorded_runner(&row);
    let attempt = observation(&row, &digest);
    let basic = smoke(&recorded, std::slice::from_ref(&attempt));
    assert!(basic.basic_execution_observed());
    assert_eq!(basic.verified_attempts, 1);
    assert_eq!(basic.tool_attempts, 1);
    assert_eq!(basic.tool_operations, 3);
    assert_eq!(basic.missing_identity.len(), 9);
    // Strict repeatability qualification stays open: the material identity is
    // incomplete, so dependent comparisons remain suspended.
    let mut second = attempt.clone();
    second.attempt_id.push_str("#2");
    let strict = qualify(&recorded, &policy(), &[attempt, second]).unwrap();
    assert!(strict.blocks_comparisons());
    assert_eq!(strict.missing_identity.len(), 9);
    assert!(strict.tool_exchange_missing.is_empty());
    assert!(strict.divergent_outputs.is_empty());
}

#[test]
fn failed_and_timed_out_attempts_cannot_qualify_even_with_matching_outputs() {
    let f = Fixture::new();
    f.edit("runner", local_runner());
    let run = |mode: &str, code: i32, content: &str| -> (Value, String) {
        let out = f
            .command(mode)
            .env("HARNESS_OUTCOME_SOLUTION", content)
            .output()
            .unwrap();
        let row = f.check(out, code);
        let digest = hash_file(&f.case.join("solution.txt")).unwrap();
        (row, digest)
    };
    let (success, success_digest) = run("success", 0, "same controlled output\n");
    let (failed, failed_digest) = run("nonzero", 1, "same controlled output\n");
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed_digest, success_digest);
    assert!(failed["tool_operations"].as_u64().unwrap() > 0);
    let mut a = observation(&success, &success_digest);
    let mut b = observation(&failed, &failed_digest);
    a.attempt_id.push_str("#1");
    b.attempt_id.push_str("#2");
    assert!(a.completed);
    assert!(!b.completed);
    let result = qualify(&recorded_runner(&success), &policy(), &[a, b]).unwrap();
    assert!(result.blocks_comparisons());
    assert_eq!(result.unfinished_attempts.len(), 1);
    assert!(result.unfinished_attempts[0].ends_with("#2"));
    assert!(result.divergent_outputs.is_empty());
    assert!(result.runner_mismatch.is_empty());

    // Timed-out work retains its artifacts and counters but is still
    // unfinished and cannot qualify.
    f.edit("timeout", json!(1));
    let (timeout, timeout_digest) = run("timeout", 1, "same controlled output\n");
    assert_eq!(timeout["status"], "timeout");
    assert_eq!(timeout_digest, success_digest);
    let mut c = observation(&timeout, &timeout_digest);
    c.attempt_id.push_str("#3");
    let mut a2 = observation(&success, &success_digest);
    a2.attempt_id.push_str("#4");
    let result = qualify(&recorded_runner(&success), &policy(), &[a2, c]).unwrap();
    assert!(result.blocks_comparisons());
    assert_eq!(result.unfinished_attempts.len(), 1);
}

#[test]
fn smoke_requires_one_completed_verified_tool_attempt() {
    let f = Fixture::new();
    f.edit("runner", local_runner());
    // Verified metadata without tools plus an unverified attempt with tools
    // never proves basic execution: no single attempt did both.
    let text_only = f.run("text-only", 0);
    let wrong_model = f.run("wrong-model", 0);
    assert_eq!(text_only["observed_model_metadata_verified"], true);
    assert_eq!(text_only["tool_operations"], 0);
    assert_eq!(wrong_model["observed_model_metadata_verified"], false);
    assert_eq!(wrong_model["tool_operations"], 3);
    let mut a = observation(&text_only, "digest-a");
    let mut b = observation(&wrong_model, "digest-a");
    a.attempt_id.push_str("#1");
    b.attempt_id.push_str("#2");
    let result = smoke(&recorded_runner(&text_only), &[a.clone(), b.clone()]);
    assert_eq!(result.verified_attempts, 1);
    assert_eq!(result.tool_attempts, 1);
    assert_eq!(result.executing_verified_attempts, 0);
    assert!(!result.basic_execution_observed());
    // One completed attempt with its own verified tool execution does.
    let success = f.run("success", 0);
    let mut c = observation(&success, "digest-a");
    c.attempt_id.push_str("#3");
    let result = smoke(&recorded_runner(&success), &[a, b, c]);
    assert_eq!(result.executing_verified_attempts, 1);
    assert_eq!(result.executing_attempt_ids.len(), 1);
    assert!(result.executing_attempt_ids[0].ends_with("#3"));
    assert!(result.basic_execution_observed());
}

#[test]
fn attempt_runner_identity_drift_blocks_qualification() {
    let f = Fixture::new();
    f.edit("runner", local_runner());
    let first = f.run("success", 0);
    let mut changed = local_runner();
    changed["identity"]["quantization"] = json!("Q5_K_M");
    f.edit("runner", changed);
    let second = f.run("success", 0);
    let mut a = observation(&first, "digest-a");
    let mut b = observation(&second, "digest-a");
    a.attempt_id.push_str("#1");
    b.attempt_id.push_str("#2");
    let result = qualify(&recorded_runner(&first), &policy(), &[a.clone(), b.clone()]).unwrap();
    assert!(result.blocks_comparisons());
    assert_eq!(result.runner_mismatch.len(), 1);
    assert_eq!(result.runner_mismatch[0].attempt_id, b.attempt_id);
    assert_eq!(
        result.runner_mismatch[0].changed,
        vec!["identity.quantization"]
    );
    assert!(result.divergent_outputs.is_empty());
    // An attempt that recorded no runner identity cannot prove the expected
    // configuration either.
    let mut unrecorded = a.clone();
    unrecorded.runner = None;
    unrecorded.attempt_id.push_str("-bare");
    let mut a2 = a;
    a2.attempt_id = "attempt-a2".into();
    let result = qualify(&recorded_runner(&first), &policy(), &[a2, unrecorded]).unwrap();
    assert!(result.blocks_comparisons());
    assert_eq!(result.runner_mismatch.len(), 1);
    assert!(result.runner_mismatch[0].changed.is_empty());
}

/// One owned loopback JSON fixture for declared local API observations. It
/// answers every request from its current state (with optional per-request
/// overrides) and records the request targets; nothing leaves loopback, no
/// model is involved and no private body enters a public error.
struct ObservationServer {
    address: SocketAddr,
    paths: Arc<Mutex<BTreeMap<String, ObservedResponse>>>,
    overrides: Arc<Mutex<BTreeMap<usize, ObservedResponse>>>,
    targets: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct ObservedResponse {
    status: u16,
    body: Vec<u8>,
    chunked: bool,
    raw: bool,
}

impl ObservedResponse {
    fn json(value: &Value) -> Self {
        Self {
            status: 200,
            body: serde_json::to_vec(value).unwrap(),
            chunked: false,
            raw: false,
        }
    }
    fn status(code: u16) -> Self {
        Self {
            status: code,
            body: b"{\"message\":\"private fixture body\"}".to_vec(),
            chunked: false,
            raw: false,
        }
    }
    fn raw(body: &str) -> Self {
        Self {
            status: 200,
            body: body.as_bytes().to_vec(),
            chunked: false,
            raw: true,
        }
    }
    fn chunked(body: &str) -> Self {
        Self {
            status: 200,
            body: body.as_bytes().to_vec(),
            chunked: true,
            raw: false,
        }
    }
    fn render(&self) -> Vec<u8> {
        if self.raw {
            return self.body.clone();
        }
        let mut response = format!("HTTP/1.1 {} Fixture\r\n", self.status);
        if self.chunked {
            response.push_str("Transfer-Encoding: chunked\r\n");
        } else {
            response.push_str(&format!("Content-Length: {}\r\n", self.body.len()));
        }
        response.push_str("Content-Type: application/json\r\nConnection: close\r\n\r\n");
        if self.chunked {
            response.push_str(&format!(
                "{:x}\r\n{}\r\n0\r\n\r\n",
                self.body.len(),
                String::from_utf8_lossy(&self.body)
            ));
        } else {
            response.push_str(&String::from_utf8_lossy(&self.body));
        }
        response.into_bytes()
    }
}

impl ObservationServer {
    fn start(initial: ObservedResponse) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let paths = Arc::new(Mutex::new(BTreeMap::new()));
        let overrides = Arc::new(Mutex::new(BTreeMap::new()));
        let default = Arc::new(Mutex::new(initial));
        let targets = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let paths = paths.clone();
            let overrides = overrides.clone();
            let default = default.clone();
            let targets = targets.clone();
            let stop = stop.clone();
            thread::spawn(move || {
                let mut served = 0_usize;
                while !stop.load(Ordering::Relaxed) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        return;
                    };
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut head = Vec::new();
                    let mut buffer = [0_u8; 1024];
                    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
                        match stream.read(&mut buffer) {
                            Ok(0) => break,
                            Ok(count) => head.extend_from_slice(&buffer[..count]),
                            Err(_) => break,
                        }
                    }
                    let request = String::from_utf8_lossy(&head);
                    let path = request
                        .lines()
                        .next()
                        .unwrap_or("")
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .to_owned();
                    targets.lock().unwrap().push(format!("GET {path}"));
                    let response = overrides
                        .lock()
                        .unwrap()
                        .get(&served)
                        .cloned()
                        .or_else(|| paths.lock().unwrap().get(&path).cloned())
                        .unwrap_or_else(|| default.lock().unwrap().clone());
                    served += 1;
                    let _ = stream.write_all(&response.render());
                    let _ = stream.flush();
                }
            })
        };
        Self {
            address,
            paths,
            overrides,
            targets,
            stop,
            thread: Some(thread),
        }
    }
    fn url(&self) -> String {
        format!("http://{}", self.address)
    }
    fn set(&self, path: &str, response: ObservedResponse) {
        self.paths.lock().unwrap().insert(path.to_owned(), response);
    }
    fn set_request(&self, index: usize, response: ObservedResponse) {
        self.overrides.lock().unwrap().insert(index, response);
    }
    fn targets(&self) -> Vec<String> {
        self.targets.lock().unwrap().clone()
    }
}

impl Drop for ObservationServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One declared API-observed policy for the synthetic local fixture; never a
/// private endpoint, model or client input.
fn api_plan() -> ApiObservationPlan {
    serde_json::from_value(json!({
        "requests": [
            {
                "path": "/props",
                "fields": [
                    {"name": "server.build", "pointer": "/build_info", "required": true},
                    {"name": "server.model", "pointer": "/model_alias", "required": true},
                    {"name": "server.context", "pointer": "/default_generation_settings/n_ctx",
                     "required": true},
                    {"name": "server.seed",
                     "pointer": "/default_generation_settings/params/seed", "required": true},
                    {"name": "server.temperature",
                     "pointer": "/default_generation_settings/params/temperature",
                     "required": true},
                    {"name": "server.template", "pointer": "/chat_template", "required": true,
                     "binding": "digest"},
                    {"name": "server.template_caps", "pointer": "/chat_template_caps",
                     "required": true, "binding": "digest"},
                    {"name": "limits.weights", "pointer": "/weights_sha256", "required": false}
                ]
            },
            {
                "path": "/v1/models",
                "fields": [
                    {"name": "server.model_id", "pointer": "/data/0/id", "required": true},
                    {"name": "server.model_meta", "pointer": "/data/0/meta", "required": true,
                     "binding": "digest"}
                ]
            }
        ],
        "required_client_inputs": ["profile", "catalogue"]
    }))
    .unwrap()
}

fn api_policy() -> ApiObservedPolicy {
    ApiObservedPolicy {
        output: policy(),
        plan: api_plan(),
    }
}

/// The synthetic observation document an owned local API would serve.
fn props(build: &str) -> Value {
    json!({
        "build_info": build,
        "model_alias": "synth-alias",
        "model_ftype": "Q4_K_M",
        "default_generation_settings": {
            "n_ctx": 262144,
            "params": {"seed": 42, "temperature": 1.0}
        },
        "total_slots": 1,
        "chat_template": "synthetic chat template ".repeat(60),
        "chat_template_caps": {"supports_tools": true}
    })
}

fn models() -> Value {
    json!({"data": [{"id": "synth-alias", "meta": {"n_ctx_train": 262144}}]})
}

/// Serves the declared sources of one unchanged synthetic local API.
fn serve_sources(server: &ObservationServer, build: &str) {
    server.set("/props", ObservedResponse::json(&props(build)));
    server.set("/v1/models", ObservedResponse::json(&models()));
}

/// Real temporary client files, never the supplied private client inputs.
struct ApiClientFiles {
    root: tempfile::TempDir,
    profile: PathBuf,
    catalogue: PathBuf,
}

impl ApiClientFiles {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("api-client-")
            .tempdir()
            .unwrap();
        let profile = root.path().join("profile.toml");
        let catalogue = root.path().join("code-tools.json");
        fs::write(&profile, "model = \"fixture-local-model\"\n").unwrap();
        fs::write(&catalogue, "{\"servers\":[]}\n").unwrap();
        Self {
            root,
            profile,
            catalogue,
        }
    }
    fn inputs(&self) -> Vec<ClientInput> {
        vec![
            ClientInput {
                name: "profile".to_owned(),
                path: self.profile.clone(),
            },
            ClientInput {
                name: "catalogue".to_owned(),
                path: self.catalogue.clone(),
            },
        ]
    }
}

fn api_runner(origin: &str) -> Value {
    let mut runner = local_runner();
    runner["endpoint"] = json!(format!("{origin}/v1"));
    runner
}

fn client_inputs_value(files: &ApiClientFiles) -> Value {
    json!([
        {"name": "profile", "path": files.profile},
        {"name": "catalogue", "path": files.catalogue}
    ])
}

fn prepare_api_observation(f: &Fixture, origin: &str, files: &ApiClientFiles) {
    f.edit("runner", api_runner(origin));
    f.edit(
        "api_observations",
        serde_json::to_value(api_plan()).unwrap(),
    );
    f.edit("client_inputs", client_inputs_value(files));
}

fn mode_command(mode: &str, request: &Path, cwd: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    command
        .args(["outcome-run", mode])
        .arg(request)
        .current_dir(cwd);
    command
}

#[test]
fn api_observed_identity_collects_qualifies_and_rechecks_before_arms() {
    let server = ObservationServer::start(ObservedResponse::status(404));
    serve_sources(&server, "b-synth-1");
    let f = Fixture::new();
    let files = ApiClientFiles::new();
    prepare_api_observation(&f, &server.url(), &files);
    let run = |content: &str| -> (Value, String) {
        let out = f
            .command("success")
            .env("HARNESS_OUTCOME_SOLUTION", content)
            .output()
            .unwrap();
        let row = f.check(out, 0);
        let digest = hash_file(&f.case.join("solution.txt")).unwrap();
        (row, digest)
    };
    let (first, first_digest) = run("repeatable api-observed solution\n");
    assert_eq!(first["status"], "completed");
    // The attempt collected the declared observations through the real entry
    // point, before and after the controlled attempt.
    assert_eq!(first["observed_api_verified"], true);
    assert!(first["observation_drift"].is_null());
    assert_eq!(
        first["observed_api"]["endpoint"],
        api_runner(&server.url())["endpoint"]
    );
    assert_eq!(
        first["observed_api"]["fields"]["server.build"]["value"],
        "b-synth-1"
    );
    assert_eq!(
        first["observed_api"]["fields"]["server.model"]["value"],
        "synth-alias"
    );
    assert_eq!(
        first["observed_api"]["fields"]["server.context"]["value"],
        "262144"
    );
    assert_eq!(
        first["observed_api"]["fields"]["server.seed"]["value"],
        "42"
    );
    assert_eq!(
        first["observed_api"]["fields"]["server.temperature"]["value"],
        "1.0"
    );
    assert_eq!(
        first["observed_api"]["fields"]["server.model_id"]["value"],
        "synth-alias"
    );
    // Large and structured facts are bound by digest with provenance instead
    // of retaining their full bodies.
    let template = first["observed_api"]["fields"]["server.template"]["value"]
        .as_str()
        .unwrap();
    assert_eq!(template.len(), 64);
    assert!(template.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(
        first["observed_api"]["fields"]["server.template"]["provenance"],
        "sha256 GET /props /chat_template"
    );
    let caps = first["observed_api"]["fields"]["server.template_caps"]["value"]
        .as_str()
        .unwrap();
    assert_eq!(caps.len(), 64);
    assert_eq!(
        first["observed_api"]["fields"]["server.build"]["provenance"],
        "GET /props"
    );
    assert_eq!(
        first["observed_api"]["unknown_optional"]["limits.weights"],
        "not reported"
    );
    assert_eq!(
        server.targets(),
        vec![
            "GET /props",
            "GET /v1/models",
            "GET /props",
            "GET /v1/models"
        ]
    );

    // Collection through the API directly is the same retained identity; the
    // effective client inputs are observed by digest, not by body.
    let runner = recorded_runner(&first);
    let retained: ApiObservations = serde_json::from_value(first["observed_api"].clone()).unwrap();
    let inputs = files.inputs();
    let collected = collect_observations(&runner, &api_plan(), &inputs).unwrap();
    assert_eq!(collected, retained);
    assert_eq!(
        collected.fields["profile"].value,
        hash_file(&files.profile).unwrap()
    );
    assert_eq!(
        collected.fields["catalogue"].value,
        hash_file(&files.catalogue).unwrap()
    );
    assert!(
        collected.fields["profile"]
            .provenance
            .starts_with("sha256 ")
    );

    let (second, second_digest) = run("repeatable api-observed solution\n");
    assert_eq!(first_digest, second_digest);
    let mut a1 = observation(&first, &first_digest);
    a1.attempt_id.push_str("#1");
    let mut a2 = observation(&second, &second_digest);
    a2.attempt_id.push_str("#2");
    let qualification =
        qualify_api_observed(&runner, &api_policy(), &collected, &[a1, a2]).unwrap();
    assert!(qualification.qualified(), "{qualification:?}");
    assert_eq!(qualification.mode, QualificationMode::ApiObserved);
    assert_eq!(qualification.observed_repeats, 2);
    assert_eq!(qualification.required_repeats, 2);
    assert!(qualification.missing_identity.is_empty());
    assert_eq!(qualification.limits, API_OBSERVED_LIMITS.to_vec());
    assert_eq!(
        qualification.observations.unknown_optional["limits.weights"],
        "not reported"
    );
    assert_eq!(qualification.policy_digest, api_policy().digest().unwrap());
    assert_eq!(
        qualification.observation_digest,
        collected.digest().unwrap()
    );

    // The pre-arm recheck passes only while the observed identity is unchanged.
    let recheck = recheck_observations(&qualification, &runner, &api_policy(), &inputs);
    assert!(!recheck.drifted, "{recheck:?}");
    assert!(recheck.failure.is_none());
}

#[test]
fn api_observed_recheck_refuses_server_client_and_required_field_drift() {
    let server = ObservationServer::start(ObservedResponse::status(404));
    serve_sources(&server, "b-synth-1");
    let f = Fixture::new();
    let files = ApiClientFiles::new();
    prepare_api_observation(&f, &server.url(), &files);
    let run = |content: &str| -> (Value, String) {
        let out = f
            .command("success")
            .env("HARNESS_OUTCOME_SOLUTION", content)
            .output()
            .unwrap();
        let row = f.check(out, 0);
        let digest = hash_file(&f.case.join("solution.txt")).unwrap();
        (row, digest)
    };
    let (first, first_digest) = run("stable solution\n");
    let (second, second_digest) = run("stable solution\n");
    let runner = recorded_runner(&first);
    let observations: ApiObservations =
        serde_json::from_value(first["observed_api"].clone()).unwrap();
    let inputs = files.inputs();
    let mut a1 = observation(&first, &first_digest);
    a1.attempt_id.push_str("#1");
    let mut a2 = observation(&second, &second_digest);
    a2.attempt_id.push_str("#2");
    let qualification =
        qualify_api_observed(&runner, &api_policy(), &observations, &[a1, a2]).unwrap();
    assert!(qualification.qualified());
    assert!(!recheck_observations(&qualification, &runner, &api_policy(), &inputs).drifted);

    // Observed server drift between qualification and an arm suspends it.
    serve_sources(&server, "b-synth-2");
    let refused = recheck_observations(&qualification, &runner, &api_policy(), &inputs);
    assert!(refused.drifted);
    assert!(
        refused
            .changed
            .contains(&"observed.server.build".to_owned()),
        "{refused:?}"
    );

    // Regression: a stored observation edited to match the changed server
    // while the old digest is retained must refuse, so matching fresh server
    // fields cannot be laundered into a pass.
    let mut tampered = qualification.clone();
    tampered
        .observations
        .fields
        .get_mut("server.build")
        .unwrap()
        .value = "b-synth-2".to_owned();
    assert_ne!(
        tampered.observations.digest().unwrap(),
        tampered.observation_digest
    );
    let refused = recheck_observations(&tampered, &runner, &api_policy(), &inputs);
    assert!(refused.drifted);
    assert!(
        refused
            .changed
            .contains(&"retained.observation-digest".to_owned()),
        "{refused:?}"
    );

    // An inconsistent retained mode is refused before any field comparison.
    let mut wrong_mode = qualification.clone();
    wrong_mode.mode = QualificationMode::FullMaterial;
    let refused = recheck_observations(&wrong_mode, &runner, &api_policy(), &inputs);
    assert!(refused.drifted);
    assert!(
        refused.changed.contains(&"retained.mode".to_owned()),
        "{refused:?}"
    );

    // A changed large template is visible through its retained digest without
    // any template body in the record.
    serve_sources(&server, "b-synth-1");
    let mut templated = props("b-synth-1");
    templated["chat_template"] = json!("different synthetic chat template ".repeat(60));
    server.set("/props", ObservedResponse::json(&templated));
    let refused = recheck_observations(&qualification, &runner, &api_policy(), &inputs);
    assert!(
        refused
            .changed
            .contains(&"observed.server.template".to_owned()),
        "{refused:?}"
    );

    // Effective client configuration drift suspends it too.
    serve_sources(&server, "b-synth-1");
    fs::write(
        &files.profile,
        "model = \"fixture-local-model\"\napproval_policy = \"never\"\n",
    )
    .unwrap();
    let refused = recheck_observations(&qualification, &runner, &api_policy(), &inputs);
    assert!(refused.changed.contains(&"observed.profile".to_owned()));
    fs::write(&files.profile, "model = \"fixture-local-model\"\n").unwrap();

    // A disappeared catalogue is a distinguishishable collection refusal, not
    // a pass with a silently dropped input.
    let moved = files.root.path().join("catalogue.moved");
    fs::rename(&files.catalogue, &moved).unwrap();
    let refused = recheck_observations(&qualification, &runner, &api_policy(), &inputs);
    assert!(refused.drifted);
    assert_eq!(
        refused.failure.as_ref().unwrap().kind,
        ObservationFailureKind::ClientInput
    );
    fs::rename(&moved, &files.catalogue).unwrap();

    // A required server field that disappears is a missing observation, not a
    // dropped one.
    server.set(
        "/props",
        ObservedResponse::json(&json!({
            "model_alias": "synth-alias",
            "default_generation_settings": {
                "n_ctx": 262144,
                "params": {"seed": 42, "temperature": 1.0}
            },
            "chat_template": "synthetic chat template ".repeat(60),
            "chat_template_caps": {"supports_tools": true}
        })),
    );
    let refused = recheck_observations(&qualification, &runner, &api_policy(), &inputs);
    assert_eq!(
        refused.failure.as_ref().unwrap().kind,
        ObservationFailureKind::Missing
    );
    assert!(refused.changed[0].contains("/props"));

    // Endpoint, model and declared-identity changes refuse even without a new
    // observation.
    let mut moved_runner = runner.clone();
    moved_runner.endpoint = format!("{}/v2", server.url());
    let drift = observed_drift(&qualification, &moved_runner, &api_policy(), &observations);
    assert!(drift.changed.contains(&"endpoint".to_owned()));
    let mut renamed = runner.clone();
    renamed.model = "fixture-model-y".to_owned();
    let drift = observed_drift(&qualification, &renamed, &api_policy(), &observations);
    assert!(drift.changed.contains(&"model".to_owned()));
    let mut redeclared = runner.clone();
    redeclared.identity.quantization = None;
    let drift = observed_drift(&qualification, &redeclared, &api_policy(), &observations);
    assert!(drift.changed.contains(&"identity.quantization".to_owned()));
    assert!(drift.blocks_comparisons());
}

#[test]
fn api_observed_collection_failure_prevents_any_launch_and_stays_inspectable() {
    let server = ObservationServer::start(ObservedResponse::status(503));
    let f = Fixture::new();
    let files = ApiClientFiles::new();
    prepare_api_observation(&f, &server.url(), &files);

    // Required fetch/status failure suspends the attempt before launch.
    let row = f.run("success", 1);
    assert_eq!(row["status"], "failed");
    assert_eq!(row["observation_failure"]["kind"], "status");
    assert_eq!(row["observation_failure"]["source"], "/props");
    assert_eq!(row["observation_failure"]["detail"], "HTTP 503");
    assert!(!f.case.join("fixture-call.json").exists());
    assert!(!evidence(&row).join("started.json").exists());
    assert!(!row.to_string().contains("private fixture body"));

    // An unparsable document is a parse failure, not a missing optional field.
    server.set(
        "/props",
        ObservedResponse::raw(
            "HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot-json",
        ),
    );
    let row = f.run("success", 1);
    assert_eq!(row["observation_failure"]["kind"], "parse");
    assert!(!f.case.join("fixture-call.json").exists());

    // A required field present but unreadable is distinguishable as well.
    server.set(
        "/props",
        ObservedResponse::json(&json!({
            "build_info": {"nested": true},
            "model_alias": "synth-alias",
            "default_generation_settings": {
                "n_ctx": 262144,
                "params": {"seed": 42, "temperature": 1.0}
            },
            "chat_template": "synthetic chat template ".repeat(60),
            "chat_template_caps": {"supports_tools": true}
        })),
    );
    let row = f.run("success", 1);
    assert_eq!(row["observation_failure"]["kind"], "unreadable");
    assert_eq!(
        row["observation_failure"]["detail"],
        "required field 'server.build' is not a bounded scalar"
    );

    // Chunked framing is decoded through the bounded transport.
    server.set(
        "/props",
        ObservedResponse::chunked(&serde_json::to_string(&props("b-synth-chunked")).unwrap()),
    );
    server.set("/v1/models", ObservedResponse::json(&models()));
    let out = f
        .command("success")
        .env("HARNESS_OUTCOME_SOLUTION", "chunked solution\n")
        .output()
        .unwrap();
    let row = f.check(out, 0);
    assert_eq!(row["status"], "completed");
    assert_eq!(row["observed_api_verified"], true);
    assert_eq!(
        row["observed_api"]["fields"]["server.build"]["value"],
        "b-synth-chunked"
    );
}

#[test]
fn api_observed_attempt_observation_drift_fails_the_attempt() {
    // The second observation (after the attempt) sees a changed server fact;
    // the attempt cannot enter an API-observed qualification.
    let server = ObservationServer::start(ObservedResponse::status(404));
    serve_sources(&server, "b-synth-1");
    let f = Fixture::new();
    let files = ApiClientFiles::new();
    prepare_api_observation(&f, &server.url(), &files);
    server.set_request(2, ObservedResponse::json(&props("b-synth-2")));
    let row = f.run("success", 1);
    assert_eq!(row["status"], "failed");
    assert_eq!(row["observed_api_verified"], false);
    assert_eq!(
        row["observed_api"]["fields"]["server.build"]["value"],
        "b-synth-1"
    );
    assert_eq!(
        row["observed_api_after"]["fields"]["server.build"]["value"],
        "b-synth-2"
    );
    assert_eq!(row["observation_drift"], json!(["observed.server.build"]));
    assert!(has_error(&row, "api_observation_drift"));
    let runner = recorded_runner(&row);
    let observations: ApiObservations =
        serde_json::from_value(row["observed_api"].clone()).unwrap();
    let mut a1 = observation(&row, "digest-a");
    a1.attempt_id.push_str("#1");
    let mut a2 = a1.clone();
    a2.attempt_id.push_str("#2");
    assert!(!a1.completed);
    let result = qualify_api_observed(&runner, &api_policy(), &observations, &[a1, a2]).unwrap();
    assert!(result.blocks_comparisons());
    assert_eq!(result.unfinished_attempts.len(), 2);

    // A failed post-observation is retained distinguishably as well.
    let server = ObservationServer::start(ObservedResponse::status(404));
    serve_sources(&server, "b-synth-1");
    server.set_request(2, ObservedResponse::status(500));
    let f = Fixture::new();
    let files = ApiClientFiles::new();
    prepare_api_observation(&f, &server.url(), &files);
    let row = f.run("success", 1);
    assert_eq!(row["status"], "failed");
    assert_eq!(row["observed_api_verified"], false);
    assert_eq!(row["observation_after_failure"]["kind"], "status");
    assert!(has_error(&row, "api_observation_unavailable"));
    assert!(row["observed_api"].is_object());
}

#[test]
fn api_observed_qualification_requires_tools_verified_metadata_and_equal_output() {
    let server = ObservationServer::start(ObservedResponse::status(404));
    serve_sources(&server, "b-synth-1");
    let f = Fixture::new();
    let files = ApiClientFiles::new();
    prepare_api_observation(&f, &server.url(), &files);

    // Text-only and model-mismatched attempts stay blocked even though their
    // declared observations were collected successfully.
    let text_only = f.run("text-only", 0);
    let wrong_model = f.run("wrong-model", 0);
    assert_eq!(text_only["tool_operations"], 0);
    assert_eq!(wrong_model["observed_model_metadata_verified"], false);
    let runner = recorded_runner(&text_only);
    let observations: ApiObservations =
        serde_json::from_value(text_only["observed_api"].clone()).unwrap();
    let mut a = observation(&text_only, "digest-a");
    a.attempt_id.push_str("#1");
    let mut b = observation(&wrong_model, "digest-a");
    b.attempt_id.push_str("#2");
    let result = qualify_api_observed(&runner, &api_policy(), &observations, &[a, b]).unwrap();
    assert!(result.blocks_comparisons());
    assert_eq!(result.tool_exchange_missing.len(), 1);
    assert!(result.tool_exchange_missing[0].ends_with("#1"));
    assert_eq!(result.unverified_attempts.len(), 1);
    assert!(result.unverified_attempts[0].ends_with("#2"));
    // No retained observation is dropped by a blocked result.
    assert!(result.missing_observations.is_empty());

    let run = |content: &str| -> (Value, String) {
        let out = f
            .command("success")
            .env("HARNESS_OUTCOME_SOLUTION", content)
            .output()
            .unwrap();
        let row = f.check(out, 0);
        let digest = hash_file(&f.case.join("solution.txt")).unwrap();
        (row, digest)
    };
    let (first, first_digest) = run("solution-a\n");
    let (second, second_digest) = run("solution-b\n");
    assert_ne!(first_digest, second_digest);
    let mut a = observation(&first, &first_digest);
    a.attempt_id.push_str("#3");
    let mut b = observation(&second, &second_digest);
    b.attempt_id.push_str("#4");
    let observations: ApiObservations =
        serde_json::from_value(first["observed_api"].clone()).unwrap();
    let result = qualify_api_observed(&runner, &api_policy(), &observations, &[a, b]).unwrap();
    assert!(result.blocks_comparisons());
    assert_eq!(result.divergent_outputs, vec!["solution.txt"]);

    // A required output that is absent from a repeat is named, not ignored.
    let mut a = observation(&first, &first_digest);
    a.attempt_id = "api-missing-output-a".to_owned();
    let mut b = a.clone();
    b.attempt_id = "api-missing-output-b".to_owned();
    b.outputs.clear();
    let result = qualify_api_observed(&runner, &api_policy(), &observations, &[a, b]).unwrap();
    assert_eq!(result.missing_outputs, vec!["solution.txt"]);
    assert!(result.blocks_comparisons());

    // Identical repeats under an unchanged observed identity qualify.
    let (third, third_digest) = run("solution-b\n");
    assert_eq!(second_digest, third_digest);
    let mut a = observation(&second, &second_digest);
    a.attempt_id.push_str("#5");
    let mut b = observation(&third, &third_digest);
    b.attempt_id.push_str("#6");
    let result = qualify_api_observed(&runner, &api_policy(), &observations, &[a, b]).unwrap();
    assert!(result.qualified(), "{result:?}");
}

#[test]
fn observations_mode_collects_model_free_without_process_dispatch() {
    let server = ObservationServer::start(ObservedResponse::status(404));
    serve_sources(&server, "b-synth-1");
    let f = Fixture::new();
    let files = ApiClientFiles::new();
    let request = f.root.path().join("observations-request.json");
    fs::write(
        &request,
        serde_json::to_vec(&json!({
            "runner": api_runner(&server.url()),
            "plan": serde_json::to_value(api_plan()).unwrap(),
            "client_inputs": client_inputs_value(&files),
        }))
        .unwrap(),
    )
    .unwrap();
    let out = mode_command("--observations", &request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "collected");
    assert_eq!(
        value["observations"]["fields"]["server.build"]["value"],
        "b-synth-1"
    );
    assert_eq!(
        value["observations"]["fields"]["server.model_id"]["value"],
        "synth-alias"
    );
    assert_eq!(
        value["observations"]["fields"]["profile"]["value"],
        hash_file(&files.profile).unwrap()
    );
    assert_eq!(
        value["observations"]["unknown_optional"]["limits.weights"],
        "not reported"
    );
    // The mode never launches the native launcher and keeps no attempt root.
    assert!(value["evidence_root"].is_null());
    assert!(!f.case.join("fixture-call.json").exists());
    assert_eq!(server.targets(), vec!["GET /props", "GET /v1/models"]);

    // A required field that disappears is a reported failure, not a pass.
    server.set(
        "/props",
        ObservedResponse::json(&json!({
            "model_alias": "synth-alias",
            "default_generation_settings": {
                "n_ctx": 262144,
                "params": {"seed": 42, "temperature": 1.0}
            },
            "chat_template": "synthetic chat template ".repeat(60),
            "chat_template_caps": {"supports_tools": true}
        })),
    );
    let out = mode_command("--observations", &request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "failed");
    assert_eq!(value["observation_failure"]["kind"], "missing");
    assert!(
        value["observation_failure"]["source"]
            .as_str()
            .unwrap()
            .contains("/props")
    );
    assert!(!f.case.join("fixture-call.json").exists());
}

/// The declared bearer auth is never ambient: an https plan that names an
/// auth client input refuses clearly before any request when it was not
/// supplied, uses the TLS transport when it was, and never echoes the
/// material through the real entry point.
#[test]
fn declared_bearer_auth_is_explicit_and_never_echoed() {
    let f = Fixture::new();
    let files = ApiClientFiles::new();
    let auth = f.root.path().join("route.key");
    fs::write(&auth, "sk-synthetic-route-token\n").unwrap();
    let request = f.root.path().join("auth-observations.json");
    let body = |inputs: Value| {
        json!({
            "runner": api_runner("https://127.0.0.1:65500"),
            "plan": {
                "requests": [{
                    "path": "/v1/models",
                    "fields": [
                        {"name": "server.model_id", "pointer": "/data/0/id", "required": true}
                    ]
                }],
                "required_client_inputs": ["catalogue"],
                "bearer_auth": "route-key"
            },
            "client_inputs": inputs
        })
    };

    // The named auth input is missing: a clear refusal before transport.
    fs::write(
        &request,
        serde_json::to_vec(&body(
            json!([{"name": "catalogue", "path": files.catalogue}]),
        ))
        .unwrap(),
    )
    .unwrap();
    let out = mode_command("--observations", &request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "failed");
    assert_eq!(value["observation_failure"]["kind"], "client-input");
    assert_eq!(value["observation_failure"]["source"], "route-key");
    assert_eq!(
        value["observation_failure"]["detail"],
        "declared bearer auth client input was not supplied"
    );
    assert!(!f.case.join("fixture-call.json").exists());

    // With the declared input supplied the declared https transport is used;
    // the closed port is a bounded transport failure that echoes no material.
    fs::write(
        &request,
        serde_json::to_vec(&body(json!([
            {"name": "catalogue", "path": files.catalogue},
            {"name": "route-key", "path": auth}
        ])))
        .unwrap(),
    )
    .unwrap();
    let out = mode_command("--observations", &request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["observation_failure"]["kind"], "transport");
    assert_eq!(
        value["observation_failure"]["detail"],
        "https endpoint could not be reached"
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains("sk-synthetic-route-token"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("sk-synthetic-route-token"));
    assert!(!f.case.join("fixture-call.json").exists());
}

/// Observes an explicitly declared external HTTPS route through the real
/// entry point. Run only with private paths supplied through the environment
/// (never committed):
///
/// ```text
/// CODEX_HARNESS_OBSERVATION_REQUEST=<private --observations request JSON>
/// cargo test --locked -p codex-harness --test outcome_run -- --ignored
/// ```
///
/// The private request declares the endpoint, model, plan (including
/// `bearer_auth` naming one of its explicit private client inputs) and all
/// paths. The test performs no model call: it collects the declared fields
/// once through the model-free `--observations` mode, requires every declared
/// required field to be observed, and checks that the declared bearer
/// material is never echoed.
#[test]
#[ignore = "requires CODEX_HARNESS_OBSERVATION_REQUEST naming a private observations request file"]
fn declared_external_https_observation_collects_through_the_real_entry_point() {
    let request = PathBuf::from(
        std::env::var_os("CODEX_HARNESS_OBSERVATION_REQUEST").expect(
            "set CODEX_HARNESS_OBSERVATION_REQUEST to a private --observations request file",
        ),
    );
    let declaration: Value = read(&request);
    let auth_name = declaration["plan"]["bearer_auth"]
        .as_str()
        .expect("the declared plan must name its bearer auth client input")
        .to_owned();
    let cwd = request.parent().unwrap().to_path_buf();
    let resolved = |path: &Value| {
        let path = PathBuf::from(path.as_str().expect("a declared client input path"));
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    let auth_path = declaration["client_inputs"]
        .as_array()
        .expect("the request declares its client inputs")
        .iter()
        .find(|input| input["name"].as_str() == Some(auth_name.as_str()))
        .map(|input| resolved(&input["path"]))
        .expect("the declared bearer auth input must be supplied");
    let material =
        fs::read_to_string(&auth_path).expect("the declared bearer auth input is readable");
    assert!(!material.trim().is_empty());

    let out = mode_command("--observations", &request, &cwd)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "collected");
    let observations = &value["observations"];
    assert!(
        observations["endpoint"]
            .as_str()
            .unwrap()
            .starts_with("https://")
    );
    assert!(!observations["plan_digest"].as_str().unwrap().is_empty());
    for source in declaration["plan"]["requests"]
        .as_array()
        .expect("the plan declares its sources")
    {
        for field in source["fields"]
            .as_array()
            .expect("a source declares fields")
        {
            if field["required"] == true {
                let name = field["name"].as_str().unwrap();
                assert!(
                    !observations["fields"][name].is_null(),
                    "required declared field {name} was not observed"
                );
            }
        }
    }
    // The declared bearer material is used for the request and never echoed.
    let rendered = format!(
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!rendered.contains(material.trim()));
}

#[test]
fn qualify_and_recheck_modes_use_retained_attempts_model_free() {
    let server = ObservationServer::start(ObservedResponse::status(404));
    serve_sources(&server, "b-synth-1");
    let f = Fixture::new();
    let files = ApiClientFiles::new();
    prepare_api_observation(&f, &server.url(), &files);
    let run = |content: &str| -> (Value, String) {
        let out = f
            .command("success")
            .env("HARNESS_OUTCOME_SOLUTION", content)
            .output()
            .unwrap();
        let row = f.check(out, 0);
        let digest = hash_file(&f.case.join("solution.txt")).unwrap();
        (row, digest)
    };
    let (first, first_digest) = run("retained solution\n");
    let (second, second_digest) = run("retained solution\n");
    let call_before = fs::read(f.case.join("fixture-call.json")).unwrap();
    // Two retained attempts carry their own attempt identities; the fixture
    // deliberately reuses one thread id for both runs.
    let mut first_attempt = first.clone();
    first_attempt["thread_id"] = json!("00000000-1111-2222-3333-444444444401");
    let mut second_attempt = second.clone();
    second_attempt["thread_id"] = json!("00000000-1111-2222-3333-444444444402");
    let qualify_body = |attempts: Value| -> Value {
        json!({
            "runner": api_runner(&server.url()),
            "policy": serde_json::to_value(api_policy()).unwrap(),
            "observations": first["observed_api"],
            "attempts": attempts,
        })
    };
    let qualified_request = f.root.path().join("qualify-request.json");
    fs::write(
        &qualified_request,
        serde_json::to_vec(&qualify_body(json!([
            {"result": first_attempt, "outputs": {"solution.txt": first_digest}},
            {"result": second_attempt, "outputs": {"solution.txt": second_digest}}
        ])))
        .unwrap(),
    )
    .unwrap();
    let targets_before = server.targets().len();
    let out = mode_command("--qualify", &qualified_request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "qualified");
    assert_eq!(value["qualification"]["mode"], "api-observed");
    assert_eq!(value["qualification"]["observed_repeats"], 2);
    // Qualification consumes retained evidence only: no HTTP and no dispatch.
    assert_eq!(server.targets().len(), targets_before);
    assert_eq!(
        fs::read(f.case.join("fixture-call.json")).unwrap(),
        call_before
    );
    let qualified = value["qualification"].clone();

    // Divergent retained outputs block through the same mode.
    let blocked_request = f.root.path().join("qualify-blocked-request.json");
    fs::write(
        &blocked_request,
        serde_json::to_vec(&qualify_body(json!([
            {"result": first_attempt, "outputs": {"solution.txt": first_digest}},
            {"result": second_attempt, "outputs": {"solution.txt": "0".repeat(64)}}
        ])))
        .unwrap(),
    )
    .unwrap();
    let out = mode_command("--qualify", &blocked_request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "blocked");
    assert_eq!(
        value["qualification"]["divergent_outputs"],
        json!(["solution.txt"])
    );

    // An already-extracted attempt record is accepted as well.
    let extracted_request = f.root.path().join("qualify-extracted-request.json");
    let extracted_first = observation(&first_attempt, &first_digest);
    let extracted_second = observation(&second_attempt, &second_digest);
    fs::write(
        &extracted_request,
        serde_json::to_vec(&qualify_body(json!([
            serde_json::to_value(&extracted_first).unwrap(),
            serde_json::to_value(&extracted_second).unwrap(),
        ])))
        .unwrap(),
    )
    .unwrap();
    let out = mode_command("--qualify", &extracted_request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "qualified");

    let recheck_body = |qualification: &Value| -> Value {
        json!({
            "qualification": qualification,
            "runner": api_runner(&server.url()),
            "policy": serde_json::to_value(api_policy()).unwrap(),
            "client_inputs": client_inputs_value(&files),
        })
    };
    let recheck_request = f.root.path().join("recheck-request.json");
    let write_recheck = |body: &Value| {
        fs::write(&recheck_request, serde_json::to_vec(body).unwrap()).unwrap();
    };
    write_recheck(&recheck_body(&qualified));
    let out = mode_command("--recheck", &recheck_request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "unchanged");
    assert_eq!(value["drift"]["drifted"], false);

    // Observed server drift refuses through the CLI.
    serve_sources(&server, "b-synth-2");
    write_recheck(&recheck_body(&qualified));
    let out = mode_command("--recheck", &recheck_request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "drifted");
    assert!(
        value["drift"]["changed"]
            .as_array()
            .unwrap()
            .contains(&json!("observed.server.build"))
    );

    // Regression: a retained record edited to match the changed server while
    // keeping the old digest refuses; matching fresh fields cannot launder it.
    let mut tampered = qualified.clone();
    tampered["observations"]["fields"]["server.build"]["value"] = json!("b-synth-2");
    write_recheck(&recheck_body(&tampered));
    let out = mode_command("--recheck", &recheck_request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        value["drift"]["changed"]
            .as_array()
            .unwrap()
            .contains(&json!("retained.observation-digest")),
        "{value}"
    );

    // A wrong retained mode and a re-hashed record missing a required retained
    // fact refuse as well.
    let mut wrong_mode = qualified.clone();
    wrong_mode["mode"] = json!("full-material");
    write_recheck(&recheck_body(&wrong_mode));
    let out = mode_command("--recheck", &recheck_request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        value["drift"]["changed"]
            .as_array()
            .unwrap()
            .contains(&json!("retained.mode"))
    );
    let mut redigested = qualified.clone();
    let mut observations: ApiObservations =
        serde_json::from_value(redigested["observations"].clone()).unwrap();
    observations.fields.remove("profile");
    redigested["observations"] = serde_json::to_value(&observations).unwrap();
    redigested["observation_digest"] = json!(observations.digest().unwrap());
    write_recheck(&recheck_body(&redigested));
    let out = mode_command("--recheck", &recheck_request, f.root.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        value["drift"]["changed"]
            .as_array()
            .unwrap()
            .contains(&json!("retained.missing-observation.profile"))
    );
    // No mode above dispatched the launcher again.
    assert_eq!(
        fs::read(f.case.join("fixture-call.json")).unwrap(),
        call_before
    );
}

#[test]
fn observation_modes_reject_invalid_input_and_keep_help_clear() {
    let f = Fixture::new();
    let missing = f.root.path().join("absent.json");
    let missing_text = missing.to_str().unwrap();
    for args in [
        vec![
            "outcome-run",
            "--observations",
            missing_text,
            "--run-model-probes",
        ],
        vec![
            "outcome-run",
            "--qualify",
            missing_text,
            "--recheck",
            missing_text,
        ],
        vec![
            "outcome-run",
            "--observations",
            missing_text,
            "--request",
            missing_text,
        ],
        vec![
            "outcome-run",
            "--observations",
            missing_text,
            "--observations",
            missing_text,
        ],
        vec!["outcome-run", "--observations", missing_text],
        vec!["outcome-run", "--qualify", missing_text],
        vec!["outcome-run", "--recheck", missing_text],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(&args)
            .current_dir(f.root.path())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(!f.case.join("fixture-call.json").exists(), "{args:?}");
    }

    // Unknown fields, an unrelated schema and malformed JSON all refuse.
    for body in [
        json!({"runner": "x", "plan": {}, "extra": true}),
        json!({"nope": 1}),
    ] {
        let path = f.root.path().join("invalid-observations.json");
        fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
        let out = mode_command("--observations", &path, f.root.path())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(!f.case.join("fixture-call.json").exists());
    }
    let malformed = f.root.path().join("malformed.json");
    fs::write(&malformed, b"{").unwrap();
    let out = mode_command("--observations", &malformed, f.root.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));

    // The help names the model-free modes and their property.
    let out = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["outcome-run", "--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    for needle in [
        "--observations PATH",
        "--qualify PATH",
        "--recheck PATH",
        "without launching a process",
    ] {
        assert!(help.contains(needle), "{needle}");
    }
}
