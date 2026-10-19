//! Ordinary review consumes one retained completion through the public CLI.
#![cfg(windows)]

#[path = "fixtures/improvement_comparison_common.rs"]
mod improvement_comparison_common;
use improvement_comparison_common::*;

use harness_core::{
    build_identity,
    delivery_reflection::{Purpose, SourceIdentity},
    task_worktree::{self, AssignmentSubmission, CompletionFacts, SlotAssignmentBinding},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Output};

const EXECUTOR_SESSION: &str = "12345678-1234-4234-8234-123456789023";
const REVIEW_SESSION: &str = "12345678-1234-4234-8234-123456789024";
const REVIEW_ID: &str = "ordinary-review-fixture";
const RUN: &str = "ordinary-review-source";
const TURN: &str = "ordinary-source-turn";

struct OrdinarySetup {
    fixture: Fixture,
    actions: PathBuf,
    declaration: PathBuf,
    review: PathBuf,
    retained_result: PathBuf,
    manifest: PathBuf,
}

impl OrdinarySetup {
    fn prepare(&self) -> Output {
        self.fixture.improve(&[
            "review",
            "--prepare",
            "--review",
            self.review.to_str().unwrap(),
            "--declaration",
            self.declaration.to_str().unwrap(),
        ])
    }

    fn resume(&self, result: &str, suffix: &str) -> Output {
        let review = self.review.to_str().unwrap();
        self.fixture.improve_with_env(
            &["review", "--resume", "--review", review],
            &[
                CONTROL_CHILD_MODE,
                ("HARNESS_IMPROVEMENT_FIXTURE_SCENARIO", "measured-workflow"),
                ("HARNESS_IMPROVEMENT_FIXTURE_ROLE", "lead-acceptance"),
                ("HARNESS_IMPROVEMENT_FIXTURE_SESSION", REVIEW_SESSION),
                ("HARNESS_IMPROVEMENT_FIXTURE_TURN_SUFFIX", suffix),
                ("HARNESS_IMPROVEMENT_FIXTURE_RESULT", result),
            ],
        )
    }
}

fn ordinary_setup(name: &str) -> OrdinarySetup {
    ordinary_setup_with(name, ActionEvidenceMode::RetainedSidecars)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActionEvidenceMode {
    /// Already-retained sidecars are supplied directly, as previously
    /// supported inputs.
    RetainedSidecars,
    /// The evidence root starts empty; a synthetic native pool receipt and
    /// stream stand in for the completed executor dispatch.
    NativePool,
}

fn native_ordinary_setup(name: &str) -> OrdinarySetup {
    ordinary_setup_with(name, ActionEvidenceMode::NativePool)
}

fn ordinary_setup_with(name: &str, mode: ActionEvidenceMode) -> OrdinarySetup {
    let fixture = Fixture::new(name);
    let launcher = PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
    fixture_build(
        &fixture.state,
        "h-build",
        &fixture.proj,
        &launcher,
        "baseline",
    );
    let prompt = fixture.root.join("ordinary-task.txt");
    let criteria = fixture.root.join("ordinary-criteria.txt");
    let config = fixture.root.join("ordinary-reviewer.toml");
    let result = fixture.root.join("ordinary-result.txt");
    fs::write(
        &prompt,
        "Inspect the frozen solution.txt and report its current state.\n",
    )
    .unwrap();
    fs::write(
        &criteria,
        "The report must accurately describe the frozen solution.txt.\n",
    )
    .unwrap();
    fs::write(&result, "Completed the requested inspection.\n").unwrap();
    fs::write(
        &config,
        "sandbox_mode = 'read-only'\napproval_policy = 'never'\nmodel = 'gpt-6-astra'\nmodel_provider = 'openai'\nmodel_reasoning_effort = 'xhigh'\n[profiles.ds]\nmodel = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    let assignment_sha256 = build_identity::hash_bytes(b"ordinary-review-assignment");
    let source = SourceIdentity {
        project_id: "ordinary-review-fixture".into(),
        beads_dir: fixture.wl.join(".beads").to_string_lossy().into_owned(),
        database: "ordinary-review-fixture".into(),
        repo_root: fixture.wl.to_string_lossy().into_owned(),
        branch: git_output(&fixture.wl, &["branch", "--show-current"]),
        commit: fixture.workload_revision.clone(),
    };
    let state_dir = task_worktree::pool_state_dir(&fixture.home, &fixture.wl).unwrap();
    let completion = task_worktree::retain_completion(
        &state_dir,
        &CompletionFacts {
            run_generation: RUN.into(),
            slot: 1,
            owner: "ordinary-review-fixture".into(),
            checkout: fixture.wl.clone(),
            task: Some("ordinary-task".into()),
            handle: Some("ordinary-task-handle".into()),
            assignment: Some(SlotAssignmentBinding {
                source,
                issue: "ordinary-task".into(),
                assignment_sha256: assignment_sha256.clone(),
                purpose: Some(Purpose::Ordinary.as_str().into()),
                purpose_source: "board".into(),
            }),
            submitted_assignment: Some(AssignmentSubmission::structured(assignment_sha256)),
            role: Some("executor".into()),
            state: "completed".into(),
            exit_code: Some(0),
            session: Some(EXECUTOR_SESSION.into()),
            turn: Some(TURN.into()),
            result: Some(result),
        },
    )
    .unwrap();
    assert!(
        completion.eligibility.eligible,
        "{:?}",
        completion.eligibility
    );
    let retained_result = completion.result.as_ref().unwrap().locator.clone();

    let actions = fixture.root.join("retained-actions");
    fs::create_dir(&actions).unwrap();
    let events = format!(
        "{}\n{}\n",
        json!({"method":"item/completed","params":{"turnId":TURN,"item":{
            "id":"command-1","type":"commandExecution","command":"git status --short",
            "status":"completed","exitCode":0,"aggregatedOutput":""
        }}}),
        json!({"method":"turn/completed","params":{"turn":{"id":TURN,"status":"completed"}}})
    );
    match mode {
        ActionEvidenceMode::RetainedSidecars => {
            let raw_path = actions.join(format!("{RUN}.jsonl"));
            fs::write(&raw_path, &events).unwrap();
            fs::write(
                actions.join(format!("{RUN}.json")),
                serde_json::to_vec_pretty(&json!({
                    "schema":1,"attempt":RUN,"role":"executor","session":EXECUTOR_SESSION,
                    "source":raw_path,"captured":true,"source_bytes":events.len(),
                    "retained_bytes":events.len(),
                    "source_sha256":build_identity::hash_bytes(events.as_bytes()),
                    "truncated":false,"observed_events":2,"observed_malformed":0,
                    "observed_rounds":1,"observed_turn":TURN,"note":null
                }))
                .unwrap(),
            )
            .unwrap();
        }
        ActionEvidenceMode::NativePool => {
            fs::create_dir_all(&state_dir).unwrap();
            let detail = state_dir.join("stream-1.jsonl");
            fs::write(&detail, &events).unwrap();
            fs::write(
                state_dir.join("spawn-1.json"),
                serde_json::to_vec_pretty(&json!({
                    "slot":{"index":1,"owner":"ordinary-review-fixture"},
                    "originatingLead":{"runGeneration":RUN},
                    "observation":{
                        "schema":1,"coverage":"native","state":"completed","exitCode":0,
                        "session":EXECUTOR_SESSION,"turn":TURN,"events":2,"messages":0,
                        "toolCalls":1,"rounds":1,"malformed":0,
                        "detail":detail,"result":retained_result.clone(),"updatedMs":0
                    }
                }))
                .unwrap(),
            )
            .unwrap();
        }
    }

    let declaration = fixture.root.join("ordinary-review.json");
    let review = fixture.root.join("ordinary-review");
    let input = json!({
        "schema":1,"review":"ordinary-review-fixture",
        "task":{"source":fixture.wl,"revision":fixture.workload_revision,
            "name":"ordinary-task","read_only":true,"writable_scope":[]},
        "task_prompt":{"path":prompt,"sha256":build_identity::hash_file(&prompt).unwrap()},
        "completion":{"codex_home":fixture.home,"run_generation":RUN,"executor_profile":"ds"},
        "lead_session":null,"action_evidence_root":actions,
        "semantic_review":{
            "schema":1,"runtime":{"state":fixture.state,"build":fixture.state.join("builds/h-build"),
                "label":"ordinary-review-runtime","upstream":fixture.upstream,
                "native_config":{"path":config,"sha256":build_identity::hash_file(&config).unwrap()},
                "private_inputs":[]},
            "profile":"ds","criteria":{"path":criteria,"sha256":build_identity::hash_file(&criteria).unwrap()},
            "sources":["solution.txt"],"max_corrections":1
        }
    });
    fs::write(&declaration, serde_json::to_vec_pretty(&input).unwrap()).unwrap();
    OrdinarySetup {
        fixture,
        actions,
        declaration,
        review,
        retained_result,
        manifest: state_dir
            .join("completions")
            .join(RUN)
            .join("completion.json"),
    }
}

#[test]
fn retained_ordinary_completion_prepares_once_without_model_dispatch() {
    let _guard = INSTALL.lock().unwrap();
    let fixture = Fixture::new("ordinary-review-entry");
    let launcher = PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
    fixture_build(
        &fixture.state,
        "h-build",
        &fixture.proj,
        &launcher,
        "baseline",
    );
    let prompt = fixture.root.join("ordinary-task.txt");
    let criteria = fixture.root.join("ordinary-criteria.txt");
    let config = fixture.root.join("ordinary-reviewer.toml");
    let result = fixture.root.join("ordinary-result.txt");
    fs::write(
        &prompt,
        "Inspect the frozen solution.txt and report its current state.\n",
    )
    .unwrap();
    fs::write(
        &criteria,
        "The report must accurately describe the frozen solution.txt.\n",
    )
    .unwrap();
    fs::write(&result, "Completed the requested inspection.\n").unwrap();
    fs::write(
        &config,
        "sandbox_mode = 'read-only'\napproval_policy = 'never'\nmodel = 'gpt-6-astra'\nmodel_provider = 'openai'\nmodel_reasoning_effort = 'xhigh'\n[profiles.ds]\nmodel = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    let assignment_sha256 = build_identity::hash_bytes(b"ordinary-review-assignment");
    let source = SourceIdentity {
        project_id: "ordinary-review-fixture".into(),
        beads_dir: fixture.wl.join(".beads").to_string_lossy().into_owned(),
        database: "ordinary-review-fixture".into(),
        repo_root: fixture.wl.to_string_lossy().into_owned(),
        branch: git_output(&fixture.wl, &["branch", "--show-current"]),
        commit: fixture.workload_revision.clone(),
    };
    let state_dir = task_worktree::pool_state_dir(&fixture.home, &fixture.wl).unwrap();
    let completion = task_worktree::retain_completion(
        &state_dir,
        &CompletionFacts {
            run_generation: RUN.into(),
            slot: 1,
            owner: "ordinary-review-fixture".into(),
            checkout: fixture.wl.clone(),
            task: Some("ordinary-task".into()),
            handle: Some("ordinary-task-handle".into()),
            assignment: Some(SlotAssignmentBinding {
                source,
                issue: "ordinary-task".into(),
                assignment_sha256: assignment_sha256.clone(),
                purpose: Some(Purpose::Ordinary.as_str().into()),
                purpose_source: "board".into(),
            }),
            submitted_assignment: Some(AssignmentSubmission::structured(assignment_sha256)),
            role: Some("executor".into()),
            state: "completed".into(),
            exit_code: Some(0),
            session: Some(EXECUTOR_SESSION.into()),
            turn: Some(TURN.into()),
            result: Some(result),
        },
    )
    .unwrap();
    assert!(
        completion.eligibility.eligible,
        "{:?}",
        completion.eligibility
    );

    // These are already retained action facts. Review must not rediscover a
    // mutable pool stream or start the original executor to obtain them.
    let actions = fixture.root.join("retained-actions");
    fs::create_dir(&actions).unwrap();
    let raw = format!(
        "{}\n{}\n",
        json!({"method":"item/completed","params":{"turnId":TURN,"item":{
            "id":"command-1","type":"commandExecution","command":"git status --short",
            "status":"completed","exitCode":0,"aggregatedOutput":""
        }}}),
        json!({"method":"turn/completed","params":{"turn":{"id":TURN,"status":"completed"}}})
    );
    let raw_path = actions.join(format!("{RUN}.jsonl"));
    fs::write(&raw_path, &raw).unwrap();
    fs::write(
        actions.join(format!("{RUN}.json")),
        serde_json::to_vec_pretty(&json!({
            "schema":1,"attempt":RUN,"role":"executor","session":EXECUTOR_SESSION,
            "source":raw_path,"captured":true,"source_bytes":raw.len(),
            "retained_bytes":raw.len(),"source_sha256":build_identity::hash_bytes(raw.as_bytes()),
            "truncated":false,"observed_events":2,"observed_malformed":0,
            "observed_rounds":1,"observed_turn":TURN,"note":null
        }))
        .unwrap(),
    )
    .unwrap();

    let declaration = fixture.root.join("ordinary-review.json");
    let review = fixture.root.join("ordinary-review");
    let input = json!({
        "schema":1,"review":"ordinary-review-fixture",
        "task":{"source":fixture.wl,"revision":fixture.workload_revision,
            "name":"ordinary-task","read_only":true,"writable_scope":[]},
        "task_prompt":{"path":prompt,"sha256":build_identity::hash_file(&prompt).unwrap()},
        "completion":{"codex_home":fixture.home,"run_generation":RUN,"executor_profile":"ds"},
        "lead_session":null,"action_evidence_root":actions,
        "semantic_review":{
            "schema":1,"runtime":{"state":fixture.state,"build":fixture.state.join("builds/h-build"),
                "label":"ordinary-review-runtime","upstream":fixture.upstream,
                "native_config":{"path":config,"sha256":build_identity::hash_file(&config).unwrap()},
                "private_inputs":[]},
            "profile":"ds","criteria":{"path":criteria,"sha256":build_identity::hash_file(&criteria).unwrap()},
            "sources":["solution.txt"],"max_corrections":1
        }
    });
    let input_bytes = serde_json::to_vec_pretty(&input).unwrap();
    fs::write(&declaration, &input_bytes).unwrap();
    let args = [
        "review",
        "--prepare",
        "--review",
        review.to_str().unwrap(),
        "--declaration",
        declaration.to_str().unwrap(),
    ];
    let first = fixture.improve(&args);
    assert!(
        first.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(String::from_utf8_lossy(&first.stdout).contains("improve review --resume --review"));
    assert_eq!(
        fs::read(review.join("declaration.json")).unwrap(),
        input_bytes
    );
    let state_bytes = fs::read(review.join("state.json")).unwrap();
    let state: Value = serde_json::from_slice(&state_bytes).unwrap();
    assert!(state["cursor"]["attempts"].as_array().unwrap().is_empty());
    let repeated = fixture.improve(&args);
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(fs::read(review.join("state.json")).unwrap(), state_bytes);
    assert_eq!(fs::read(&declaration).unwrap(), input_bytes);
}

fn assert_no_attempts(setup: &OrdinarySetup) {
    let state: Value =
        serde_json::from_slice(&fs::read(setup.review.join("state.json")).unwrap()).unwrap();
    assert!(state["cursor"]["attempts"].as_array().unwrap().is_empty());
}

fn assert_prepared(setup: &OrdinarySetup) {
    let output = setup.prepare();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_no_attempts(setup);
}

fn review_state(setup: &OrdinarySetup) -> Value {
    serde_json::from_slice(&fs::read(setup.review.join("state.json")).unwrap()).unwrap()
}

fn review_attempts(setup: &OrdinarySetup) -> Vec<Value> {
    review_state(setup)["cursor"]["attempts"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn attempt_receipt_path(attempt: &Value) -> PathBuf {
    PathBuf::from(
        attempt["receipt"]
            .as_str()
            .or_else(|| attempt["retained"]["receipt"].as_str())
            .expect("the review attempt records a receipt"),
    )
}

fn dispatch_review(
    setup: &OrdinarySetup,
    result: &str,
    suffix: &str,
    expected_id: &str,
) -> (PathBuf, Value) {
    let output = setup.resume(result, suffix);
    assert!(output.status.success(), "{}", text(&output));
    let attempts = review_attempts(setup);
    let attempt = attempts
        .iter()
        .find(|attempt| attempt["id"] == expected_id)
        .unwrap_or_else(|| panic!("missing review attempt {expected_id}: {attempts:?}"));
    let receipt = attempt_receipt_path(attempt);
    let completed = wait_for_terminal_receipt(&receipt);
    (receipt, completed)
}

fn accepted_judgment() -> String {
    format!("{{\"run_id\":\"{REVIEW_ID}\",\"findings\":[],\"unresolved_issues\":[]}}")
}

fn rejected_judgment() -> String {
    format!(
        "{{\"run_id\":\"{REVIEW_ID}\",\"findings\":[{{\"path\":\"submission.txt\",\"line\":1,\"description\":\"the submission does not demonstrate the requested result\",\"evidence\":\"submission.txt:1 records the submitted result\"}}],\"unresolved_issues\":[]}}"
    )
}

fn unknown_judgment() -> String {
    format!(
        "{{\"run_id\":\"{REVIEW_ID}\",\"findings\":[],\"unresolved_issues\":[\"the frozen packet does not expose enough information to judge the task\"]}}"
    )
}

fn assert_review_settled(setup: &OrdinarySetup, expected: &str) {
    let before = review_attempts(setup);
    let output = setup.resume(&accepted_judgment(), "settled-repeat");
    assert!(output.status.success(), "{}", text(&output));
    let rendered = text(&output);
    assert!(rendered.contains(expected), "{rendered}");
    assert_eq!(review_attempts(setup).len(), before.len());
}

fn interrupted_receipt(completed: &Value) -> Value {
    let mut value = completed.clone();
    let observation = value["observation"]
        .as_object_mut()
        .expect("the fixture receipt records an observation");
    observation.insert("state".to_owned(), json!("interrupted"));
    observation.insert("cause".to_owned(), json!("synthetic interruption"));
    observation.insert("exitCode".to_owned(), Value::Null);
    observation.insert("result".to_owned(), Value::Null);
    observation.insert("updatedMs".to_owned(), json!(unix_ms_now()));
    value
}

fn running_receipt(completed: &Value) -> Value {
    let mut value = completed.clone();
    let observation = value["observation"]
        .as_object_mut()
        .expect("the fixture receipt records an observation");
    observation.insert("state".to_owned(), json!("running"));
    observation.insert("host".to_owned(), Value::Null);
    observation.insert("exitCode".to_owned(), Value::Null);
    observation.insert("cause".to_owned(), Value::Null);
    observation.insert("result".to_owned(), Value::Null);
    observation.insert("updatedMs".to_owned(), json!(unix_ms_now()));
    value
}

fn assert_resume_refused(setup: &OrdinarySetup, state: &[u8], expected: &str) {
    let output = setup.resume(&accepted_judgment(), "frozen-input-refused");
    let rendered = text(&output);
    assert!(!output.status.success(), "{rendered}");
    assert!(rendered.contains(expected), "{rendered}");
    assert!(review_attempts(setup).is_empty(), "{rendered}");
    assert_eq!(
        fs::read(setup.review.join("state.json")).unwrap(),
        state,
        "a refused frozen-input check must not change review state"
    );
}

#[test]
fn changed_declaration_is_refused_after_preparation() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-declaration-drift");
    assert_prepared(&setup);
    let mut declaration: Value =
        serde_json::from_slice(&fs::read(&setup.declaration).unwrap()).unwrap();
    declaration["review"] = json!("ordinary-review-fixture-changed");
    fs::write(
        &setup.declaration,
        serde_json::to_vec_pretty(&declaration).unwrap(),
    )
    .unwrap();
    let refused = setup.prepare();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("declaration changed after preparation"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert_no_attempts(&setup);
}

#[test]
fn altered_retained_result_is_refused_before_preparation_effects() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-result-drift");
    assert_prepared(&setup);
    fs::write(&setup.retained_result, "changed retained result\n").unwrap();
    let refused = setup.prepare();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("retained result artifact"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert_no_attempts(&setup);
}

#[test]
fn changed_completion_source_identity_is_refused() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-source-drift");
    assert_prepared(&setup);
    let mut manifest: Value = serde_json::from_slice(&fs::read(&setup.manifest).unwrap()).unwrap();
    manifest["source"]["revision"] = json!("ffffffffffffffffffffffffffffffffffffffff");
    fs::write(
        &setup.manifest,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let refused = setup.prepare();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("Git revision"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert_no_attempts(&setup);
}

#[test]
fn missing_action_evidence_is_an_explicit_gap() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-missing-actions");
    fs::remove_file(setup.actions.join(format!("{RUN}.json"))).unwrap();
    let refused = setup.prepare();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("action view is missing or incomplete"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert_no_attempts(&setup);
}

#[test]
fn truncated_action_evidence_is_an_explicit_gap() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-truncated-actions");
    fs::write(setup.actions.join(format!("{RUN}.jsonl")), b"{}\n").unwrap();
    let refused = setup.prepare();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("action view is missing or incomplete"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert_no_attempts(&setup);
}

#[test]
fn mismatched_action_evidence_is_an_explicit_gap() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-mismatched-actions");
    let sidecar = setup.actions.join(format!("{RUN}.json"));
    let mut record: Value = serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
    record["session"] = json!("different-session");
    fs::write(&sidecar, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    let refused = setup.prepare();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("action view is missing or incomplete"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    let state: Value =
        serde_json::from_slice(&fs::read(setup.review.join("state.json")).unwrap()).unwrap();
    assert!(
        state["cursor"]["condition"]
            .as_str()
            .is_some_and(|condition| condition.contains("does not bind")),
        "{state}"
    );
    assert_no_attempts(&setup);
}

#[test]
fn retained_action_copy_survives_loss_of_original_capture_path() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-retained-origin");
    let sidecar_path = fs::read_dir(&setup.actions)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .expect("retained action sidecar");
    let mut sidecar: Value = serde_json::from_slice(&fs::read(&sidecar_path).unwrap()).unwrap();
    // The existing capture owner records the original live detail locator,
    // while evidence consumption reads the immutable adjacent JSONL copy.
    let original = setup.fixture.root.join("original-native-detail.jsonl");
    fs::write(
        &original,
        fs::read(sidecar_path.with_extension("jsonl")).unwrap(),
    )
    .unwrap();
    sidecar["source"] = json!(original);
    fs::write(&sidecar_path, serde_json::to_vec(&sidecar).unwrap()).unwrap();
    fs::remove_file(&original).unwrap();
    assert_prepared(&setup);
}

#[test]
fn native_pool_completion_captures_action_evidence_without_prewritten_sidecars() {
    let _guard = INSTALL.lock().unwrap();
    let setup = native_ordinary_setup("ordinary-review-native-capture");
    let sidecar = setup.actions.join(format!("{RUN}.json"));
    let raw = setup.actions.join(format!("{RUN}.jsonl"));
    assert!(
        !sidecar.exists() && !raw.exists(),
        "the native case must start without prewritten action sidecars"
    );
    let pool_dir = task_worktree::pool_state_dir(&setup.fixture.home, &setup.fixture.wl).unwrap();
    let pool_stream = pool_dir.join("stream-1.jsonl");
    let pool_bytes = fs::read(&pool_stream).unwrap();

    let first = setup.prepare();
    assert!(
        first.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let record: Value = serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
    assert_eq!(record["attempt"], RUN);
    assert_eq!(record["role"], "executor");
    assert_eq!(record["session"], EXECUTOR_SESSION);
    assert_eq!(record["observed_turn"], TURN);
    assert_eq!(record["observed_events"], 2);
    assert_eq!(record["observed_rounds"], 1);
    assert_eq!(record["captured"], true);
    assert_eq!(record["truncated"], false);
    assert_eq!(fs::read(&raw).unwrap(), pool_bytes);
    assert_no_attempts(&setup);
    assert!(!review_state(&setup)["semantic_review"].is_null());

    // Release the recorded slot to a later dispatch. Repreparation must reuse
    // the captured packet and never reread the replaced receipt or stream.
    let state_bytes = fs::read(setup.review.join("state.json")).unwrap();
    let sidecar_bytes = fs::read(&sidecar).unwrap();
    let raw_bytes = fs::read(&raw).unwrap();
    fs::write(
        pool_dir.join("spawn-1.json"),
        serde_json::to_vec_pretty(&json!({
            "slot":{"index":1,"owner":"later-owner"},
            "originatingLead":{"runGeneration":"later-generation"},
            "observation":{
                "schema":1,"coverage":"native","state":"completed","exitCode":0,
                "session":"later-session","turn":"later-turn","events":1,
                "messages":0,"toolCalls":0,"rounds":1,"malformed":0,
                "detail":pool_stream,"result":setup.retained_result.clone(),"updatedMs":0
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(&pool_stream, b"{\"later\":true}\n").unwrap();

    let repeated = setup.prepare();
    assert!(
        repeated.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&repeated.stdout),
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(fs::read(&sidecar).unwrap(), sidecar_bytes);
    assert_eq!(fs::read(&raw).unwrap(), raw_bytes);
    assert_eq!(
        fs::read(setup.review.join("state.json")).unwrap(),
        state_bytes
    );
    assert_no_attempts(&setup);
}

#[test]
fn native_capture_refuses_mismatched_receipt_before_review_effects() {
    let _guard = INSTALL.lock().unwrap();

    let generation = native_ordinary_setup("ordinary-review-native-wrong-generation");
    let pool_dir =
        task_worktree::pool_state_dir(&generation.fixture.home, &generation.fixture.wl).unwrap();
    let receipt = pool_dir.join("spawn-1.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    value["originatingLead"]["runGeneration"] = json!("later-generation");
    fs::write(&receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    let refused = generation.prepare();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("does not bind run generation"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(!generation.review.join("state.json").exists());
    assert!(!generation.actions.join(format!("{RUN}.json")).exists());

    let turn = native_ordinary_setup("ordinary-review-native-wrong-turn");
    let pool_dir = task_worktree::pool_state_dir(&turn.fixture.home, &turn.fixture.wl).unwrap();
    let receipt = pool_dir.join("spawn-1.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    value["observation"]["turn"] = json!("different-turn");
    fs::write(&receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    let refused = turn.prepare();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("does not bind run generation"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(!turn.review.join("state.json").exists());
    assert!(!turn.actions.join(format!("{RUN}.json")).exists());

    let lost = native_ordinary_setup("ordinary-review-native-lost-receipt");
    let pool_dir = task_worktree::pool_state_dir(&lost.fixture.home, &lost.fixture.wl).unwrap();
    fs::remove_file(pool_dir.join("spawn-1.json")).unwrap();
    let refused = lost.prepare();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("is not completed"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(!lost.review.join("state.json").exists());
    assert!(!lost.actions.join(format!("{RUN}.json")).exists());
}

#[test]
fn native_capture_records_gap_for_missing_or_truncated_stream() {
    let _guard = INSTALL.lock().unwrap();

    let missing = native_ordinary_setup("ordinary-review-native-missing-stream");
    let pool_dir =
        task_worktree::pool_state_dir(&missing.fixture.home, &missing.fixture.wl).unwrap();
    fs::remove_file(pool_dir.join("stream-1.jsonl")).unwrap();
    let refused = missing.prepare();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("action view is missing or incomplete"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    let record: Value =
        serde_json::from_slice(&fs::read(missing.actions.join(format!("{RUN}.json"))).unwrap())
            .unwrap();
    assert_eq!(record["captured"], false);
    assert!(
        record["note"]
            .as_str()
            .is_some_and(|note| note.contains("unavailable")),
        "{record}"
    );
    assert_no_attempts(&missing);

    let truncated = native_ordinary_setup("ordinary-review-native-truncated-stream");
    let pool_dir =
        task_worktree::pool_state_dir(&truncated.fixture.home, &truncated.fixture.wl).unwrap();
    let receipt = pool_dir.join("spawn-1.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    value["observation"]["detailTruncated"] = json!(true);
    fs::write(&receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    let refused = truncated.prepare();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("action view is missing or incomplete"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    let record: Value =
        serde_json::from_slice(&fs::read(truncated.actions.join(format!("{RUN}.json"))).unwrap())
            .unwrap();
    assert_eq!(record["captured"], true);
    assert_eq!(record["truncated"], true);
    assert_no_attempts(&truncated);
}

#[test]
fn accepted_judgment_settles_once_and_repeats_without_dispatch() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-accepted");
    assert_prepared(&setup);
    let (receipt, completed) = dispatch_review(
        &setup,
        &accepted_judgment(),
        "accepted-1",
        "semantic-review-1",
    );
    assert_eq!(completed["observation"]["state"], "completed");

    let settled = setup.resume(&accepted_judgment(), "accepted-settle");
    let rendered = text(&settled);
    assert!(settled.status.success(), "{rendered}");
    assert!(rendered.contains("ordinary review accepted"), "{rendered}");

    let attempts = review_attempts(&setup);
    assert_eq!(attempts.len(), 1, "{attempts:?}");
    assert_eq!(attempts[0]["id"], "semantic-review-1");
    assert_eq!(attempts[0]["binding"]["session"], REVIEW_SESSION);
    assert_ne!(attempts[0]["binding"]["session"], EXECUTOR_SESSION);
    let state = review_state(&setup);
    assert_eq!(
        state["semantic_review"]["judgment_kind"], "accept",
        "{state}"
    );
    let retained = PathBuf::from(
        attempts[0]["retained"]["receipt"]
            .as_str()
            .expect("the accepted attempt retains its receipt"),
    );
    assert_eq!(fs::read(&retained).unwrap(), fs::read(&receipt).unwrap());

    assert_review_settled(&setup, "ordinary review accepted");
}

#[test]
fn rejected_judgment_settles_as_rejected_and_is_not_replayed() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-rejected");
    assert_prepared(&setup);
    dispatch_review(
        &setup,
        &rejected_judgment(),
        "rejected-1",
        "semantic-review-1",
    );

    let settled = setup.resume(&rejected_judgment(), "rejected-settle");
    let rendered = text(&settled);
    assert!(settled.status.success(), "{rendered}");
    assert!(rendered.contains("ordinary review rejected"), "{rendered}");
    assert_eq!(
        review_state(&setup)["semantic_review"]["judgment_kind"],
        "reject"
    );
    assert_eq!(review_attempts(&setup).len(), 1);

    assert_review_settled(&setup, "ordinary review rejected");
}

#[test]
fn unknown_judgment_refuses_and_repeats_without_dispatch() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-unknown");
    assert_prepared(&setup);
    dispatch_review(
        &setup,
        &unknown_judgment(),
        "unknown-1",
        "semantic-review-1",
    );

    let refused = setup.resume(&unknown_judgment(), "unknown-refused");
    let rendered = text(&refused);
    assert!(!refused.status.success(), "{rendered}");
    assert!(rendered.contains("unresolved issues"), "{rendered}");
    assert_eq!(
        review_state(&setup)["semantic_review"]["judgment_kind"],
        "unknown"
    );
    assert_eq!(review_attempts(&setup).len(), 1);

    let repeated = setup.resume(&unknown_judgment(), "unknown-repeat");
    let repeated_rendered = text(&repeated);
    assert!(!repeated.status.success(), "{repeated_rendered}");
    assert!(
        repeated_rendered.contains("unresolved issues"),
        "{repeated_rendered}"
    );
    assert_eq!(review_attempts(&setup).len(), 1);
}

#[test]
fn live_review_receipt_is_pending_without_duplicate_dispatch() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-live");
    assert_prepared(&setup);
    let (receipt, completed) =
        dispatch_review(&setup, &accepted_judgment(), "live-1", "semantic-review-1");
    let terminal_bytes = fs::read(&receipt).unwrap();
    fs::write(
        &receipt,
        serde_json::to_vec_pretty(&running_receipt(&completed)).unwrap(),
    )
    .unwrap();

    let pending = setup.resume(&accepted_judgment(), "live-pending");
    let rendered = text(&pending);
    assert!(pending.status.success(), "{rendered}");
    assert!(rendered.contains("ordinary review pending"), "{rendered}");
    assert_eq!(review_attempts(&setup).len(), 1, "{rendered}");

    // Restore the terminal bytes without dispatching another reviewer. The
    // same attempt must settle from its retained evidence.
    fs::write(&receipt, terminal_bytes).unwrap();
    let settled = setup.resume(&accepted_judgment(), "live-settled");
    let settled_rendered = text(&settled);
    assert!(settled.status.success(), "{settled_rendered}");
    assert!(
        settled_rendered.contains("ordinary review accepted"),
        "{settled_rendered}"
    );
    assert_eq!(review_attempts(&setup).len(), 1);
}

#[test]
fn interrupted_receipt_reconciles_without_replay() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-interrupted");
    assert_prepared(&setup);
    let (receipt, completed) = dispatch_review(
        &setup,
        &accepted_judgment(),
        "interrupted-1",
        "semantic-review-1",
    );
    fs::write(
        &receipt,
        serde_json::to_vec_pretty(&interrupted_receipt(&completed)).unwrap(),
    )
    .unwrap();

    let refused = setup.resume(&accepted_judgment(), "interrupted-refused");
    let rendered = text(&refused);
    assert!(!refused.status.success(), "{rendered}");
    assert!(rendered.contains("Interrupted"), "{rendered}");
    assert!(rendered.contains("not replayed"), "{rendered}");
    let attempts = review_attempts(&setup);
    assert_eq!(attempts.len(), 1, "{attempts:?}");
    assert_eq!(attempts[0]["state"], "interrupted", "{attempts:?}");
    let retained = PathBuf::from(
        attempts[0]["retained"]["receipt"]
            .as_str()
            .expect("the interrupted attempt retains its receipt"),
    );
    assert!(retained.is_file(), "{}", retained.display());

    let repeated = setup.resume(&accepted_judgment(), "interrupted-repeat");
    let repeated_rendered = text(&repeated);
    assert!(!repeated.status.success(), "{repeated_rendered}");
    assert!(
        repeated_rendered.contains("Interrupted"),
        "{repeated_rendered}"
    );
    assert_eq!(review_attempts(&setup).len(), 1);
}

#[test]
fn malformed_first_judgment_uses_one_bounded_correction() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-correction");
    assert_prepared(&setup);
    dispatch_review(
        &setup,
        "not a semantic judgment",
        "correction-1",
        "semantic-review-1",
    );

    let correction = setup.resume(&accepted_judgment(), "correction-2");
    let rendered = text(&correction);
    assert!(correction.status.success(), "{rendered}");
    assert!(rendered.contains("semantic-review-2"), "{rendered}");
    let attempts = review_attempts(&setup);
    assert_eq!(attempts.len(), 2, "{attempts:?}");
    assert_eq!(attempts[1]["id"], "semantic-review-2");
    let correction_receipt = attempt_receipt_path(&attempts[1]);
    wait_for_terminal_receipt(&correction_receipt);

    let settled = setup.resume(&accepted_judgment(), "correction-settle");
    let settled_rendered = text(&settled);
    assert!(settled.status.success(), "{settled_rendered}");
    assert!(
        settled_rendered.contains("ordinary review accepted"),
        "{settled_rendered}"
    );
    let state = review_state(&setup);
    assert_eq!(
        state["semantic_review"]["judgment_kind"], "accept",
        "{state}"
    );
    assert_eq!(state["semantic_review"]["correction_used"], true, "{state}");
    assert_eq!(review_attempts(&setup).len(), 2);
}

#[test]
fn frozen_input_tampering_is_refused_before_review_effects() {
    let _guard = INSTALL.lock().unwrap();
    let setup = ordinary_setup("ordinary-review-frozen-inputs");
    assert_prepared(&setup);
    let state_before = fs::read(setup.review.join("state.json")).unwrap();

    let declaration_copy = setup.review.join("declaration.json");
    let declaration_bytes = fs::read(&declaration_copy).unwrap();
    let mut declaration: Value = serde_json::from_slice(&declaration_bytes).unwrap();
    declaration["review"] = json!("ordinary-review-fixture-tampered");
    fs::write(
        &declaration_copy,
        serde_json::to_vec_pretty(&declaration).unwrap(),
    )
    .unwrap();
    assert_resume_refused(&setup, &state_before, "does not bind this declaration");
    fs::write(&declaration_copy, declaration_bytes).unwrap();

    let prompt = setup.fixture.root.join("ordinary-task.txt");
    let prompt_bytes = fs::read(&prompt).unwrap();
    fs::write(&prompt, "tampered prompt\n").unwrap();
    assert_resume_refused(&setup, &state_before, "ordinary task prompt");
    fs::write(&prompt, prompt_bytes).unwrap();

    let criteria = setup.fixture.root.join("ordinary-criteria.txt");
    let criteria_bytes = fs::read(&criteria).unwrap();
    fs::write(&criteria, "tampered criteria\n").unwrap();
    assert_resume_refused(&setup, &state_before, "semantic review criteria");
    fs::write(&criteria, criteria_bytes).unwrap();

    let result_bytes = fs::read(&setup.retained_result).unwrap();
    fs::write(&setup.retained_result, "tampered completion result\n").unwrap();
    assert_resume_refused(&setup, &state_before, "retained result artifact");
    fs::write(&setup.retained_result, result_bytes).unwrap();

    let runtime_path = setup.review.join("reviewer/runtime.json");
    let runtime_bytes = fs::read(&runtime_path).unwrap();
    let mut runtime: Value = serde_json::from_slice(&runtime_bytes).unwrap();
    runtime["label"] = json!("tampered-reviewer-runtime");
    fs::write(&runtime_path, serde_json::to_vec_pretty(&runtime).unwrap()).unwrap();
    assert_resume_refused(&setup, &state_before, "retained semantic reviewer runtime");
    fs::write(&runtime_path, runtime_bytes).unwrap();
}
