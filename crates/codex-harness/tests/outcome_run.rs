//! Actual CLI and Rust process doubles. No live model, OAuth or global writes.
#![cfg(windows)]
use harness_core::{
    build_identity::hash_file,
    outcome_qualification::{
        QualificationAttempt, QualificationStatus, RepeatabilityPolicy, RunnerRecord, drift,
        qualification_attempt, qualify, smoke,
    },
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
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
