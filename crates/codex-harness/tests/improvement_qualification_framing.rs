#![cfg(windows)]

#[path = "fixtures/improvement_comparison_common.rs"]
mod improvement_comparison_common;
use improvement_comparison_common::*;

use harness_core::build_identity;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Output};

const FIRST_SESSION: &str = "12345678-1234-4234-8234-123456789021";
const SECOND_SESSION: &str = "12345678-1234-4234-8234-123456789022";
const REFUSED_SESSION: &str = "12345678-1234-4234-8234-123456789023";

/// The incident's ordinary framing defect: both path fields are JSON objects
/// instead of the required plain string arrays. The exact bytes are retained
/// unchanged and are never normalized into a success.
fn malformed_assignment() -> String {
    r#"{
  "schema": 1,
  "objective": "Report the frozen workload state without editing files.",
  "inputs": {"paths": []},
  "outputs": {"paths": []},
  "invariants": ["Preserve frozen acceptance."],
  "acceptance": ["Report the observed workload state."],
  "consumer": "qualification controller",
  "escalate": []
}"#
    .to_owned()
}

fn valid_read_only_assignment() -> String {
    json!({
        "schema": 1,
        "objective": "Report the frozen workload state without editing files.",
        "inputs": [],
        "outputs": [],
        "invariants": ["Preserve frozen acceptance."],
        "acceptance": ["Report the observed workload state."],
        "consumer": "qualification controller",
        "escalate": []
    })
    .to_string()
}

fn read_only_scope_violation() -> String {
    json!({
        "schema": 1,
        "objective": "Report the frozen workload state without editing files.",
        "inputs": [],
        "outputs": ["solution.txt"],
        "invariants": ["Preserve frozen acceptance."],
        "acceptance": ["Report the observed workload state."],
        "consumer": "qualification controller",
        "escalate": []
    })
    .to_string()
}

fn prepare_fixture(name: &str) -> Fixture {
    let fixture = Fixture::new(name);
    let launcher = PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
    fixture_build(
        &fixture.state,
        "h-build",
        &fixture.proj,
        &launcher,
        "baseline",
    );
    fixture.write_spec(&[], None);
    let mut spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    let previous = spec["comparison"].take();
    let config = fixture
        .spec
        .with_file_name("qualification-native-config.toml");
    fs::write(
        &config,
        "model = 'gpt-6-astra'\nmodel_provider = 'openai'\nmodel_reasoning_effort = 'xhigh'\n[profiles.ds]\nmodel = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    let prompt = fixture.spec.with_file_name("qualification-task.txt");
    fs::write(
        &prompt,
        "Read the frozen add-workload OpenSpec task and report its state. Preserve its independent acceptance.",
    )
    .unwrap();
    spec["local_runner"] = Value::Null;
    spec["qualification"] = Value::Null;
    let mut task = previous["task"].clone();
    task["read_only"] = json!(true);
    task["writable_scope"] = json!([]);
    spec["baseline_qualification"] = json!({
        "schema": 1,
        "task": task,
        "task_prompt": {"path": prompt, "sha256": build_identity::hash_file(&prompt).unwrap()},
        "runtime": {
            "state": fixture.state,
            "build": fixture.state.join("builds/h-build"),
            "label": "accepted-native-runtime",
            "upstream": fixture.upstream,
            "native_config": {"path": config, "sha256": build_identity::hash_file(&config).unwrap()},
            "private_inputs": []
        },
        "acceptance": previous["acceptance"].clone(),
        "workflow": {"schema":1,"lead_profile":"default","executor_profile":"ds","max_corrections":1,"capacity":2},
        "report": fixture.spec.with_file_name("qualification-result.txt")
    });
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    fixture
}

fn start_fixture(fixture: &Fixture, result: &str) -> Output {
    fixture.improve_with_env(
        &[
            "start",
            "--run",
            fixture.run.to_str().unwrap(),
            "--spec",
            fixture.spec.to_str().unwrap(),
            "--supervision",
            "once",
        ],
        &[
            CONTROL_CHILD_MODE,
            ("HARNESS_IMPROVEMENT_FIXTURE_SCENARIO", "measured-workflow"),
            ("HARNESS_IMPROVEMENT_FIXTURE_ROLE", "lead-frame"),
            ("HARNESS_IMPROVEMENT_FIXTURE_SESSION", FIRST_SESSION),
            ("HARNESS_IMPROVEMENT_FIXTURE_TURN_SUFFIX", "frame-1"),
            ("HARNESS_IMPROVEMENT_FIXTURE_RESULT", result),
        ],
    )
}

fn resume_fixture(
    fixture: &Fixture,
    role: &str,
    session: &str,
    suffix: &str,
    result: &str,
) -> Output {
    fixture.improve_with_env(
        &["resume", "--run", fixture.run.to_str().unwrap()],
        &[
            CONTROL_CHILD_MODE,
            ("HARNESS_IMPROVEMENT_FIXTURE_SCENARIO", "measured-workflow"),
            ("HARNESS_IMPROVEMENT_FIXTURE_ROLE", role),
            ("HARNESS_IMPROVEMENT_FIXTURE_SESSION", session),
            ("HARNESS_IMPROVEMENT_FIXTURE_TURN_SUFFIX", suffix),
            ("HARNESS_IMPROVEMENT_FIXTURE_RESULT", result),
        ],
    )
}

fn wait_attempt(fixture: &Fixture, index: usize) -> Value {
    let cursor = fixture.cursor();
    let attempt = cursor["attempts"]
        .as_array()
        .expect("attempts")
        .get(index)
        .cloned()
        .expect("the attempt exists");
    let receipt = PathBuf::from(attempt["receipt"].as_str().expect("attempt receipt"));
    let terminal = wait_for_terminal_receipt(&receipt);
    assert_eq!(
        terminal["observation"]["state"], "completed",
        "the fixture turn must complete before the controller consumes it: {terminal}"
    );
    attempt
}

fn rollout_records(fixture: &Fixture, session: &str) -> Vec<Value> {
    let path = fixture
        .run
        .join("baseline-qualification/home/sessions/2026/10/01")
        .join(format!("rollout-{session}.jsonl"));
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read the native rollout {}: {error}", path.display()))
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "token_usage_record")
        .collect()
}

fn retained_result_path(attempt: &Value) -> PathBuf {
    PathBuf::from(
        attempt["retained"]["result"]
            .as_str()
            .unwrap_or_else(|| panic!("the retained result path is missing: {attempt}")),
    )
}

fn assert_contract(prompt: &str) {
    for needle in [
        "Return exactly one JSON object",
        "Assignment schema 1 is strict and rejects unknown fields",
        "\"inputs\" and \"outputs\" must be JSON arrays of plain checkout-relative path strings, never objects",
        "\"outputs\" must be the empty array []",
        "at most 262144 bytes",
        "at most 16384 bytes",
        "at most 64 entries",
        "at most 240 bytes",
        "at most 512 bytes",
    ] {
        assert!(
            prompt.contains(needle),
            "the framing prompt is missing {needle:?}: {prompt}"
        );
    }
}

fn assert_retained_attempts(fixture: &Fixture, attempts: usize) {
    let cursor = fixture.cursor();
    let recorded = cursor["attempts"].as_array().expect("attempts");
    assert_eq!(
        recorded.len(),
        attempts,
        "no further model turn may be dispatched: {cursor}"
    );
    assert!(
        recorded
            .iter()
            .all(|attempt| attempt["binding"]["session"].as_str() != Some(REFUSED_SESSION)),
        "the refusal session must never appear in a dispatch binding: {cursor}"
    );
    assert!(
        recorded
            .iter()
            .all(|attempt| attempt["role"] != json!("executor")),
        "no executor may be dispatched from an unaccepted correction: {cursor}"
    );
    assert_eq!(
        cursor["baseline_qualification"]["workflow"]["executor_attempt"],
        Value::Null,
        "the executor binding stays empty: {cursor}"
    );
}

fn refusal_text(fixture: &Fixture, output: &Output) -> String {
    let cursor = fixture.cursor();
    format!(
        "{}\ncondition={}\n{}",
        text(output),
        cursor["baseline_qualification"]["condition"],
        cursor
    )
}

/// Makes the recorded correction receipt describe a live turn without changing
/// its native identity. Resume must observe this as in-flight, never replay it.
fn install_active_receipt(receipt: &PathBuf, terminal: &Value) {
    let mut value = terminal.clone();
    value["observation"]["state"] = json!("running");
    value["observation"]["host"] = Value::Null;
    value["observation"]["exitCode"] = Value::Null;
    value["observation"]["updatedMs"] = json!(unix_ms_now());
    fs::write(receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

#[test]
fn malformed_frame_is_corrected_once_in_the_same_lead_session() {
    let _serial = INSTALL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = prepare_fixture("qualification-framing-correction");
    let malformed = malformed_assignment();
    let started = start_fixture(&fixture, &malformed);
    assert!(started.status.success(), "{}", text(&started));

    let first = wait_attempt(&fixture, 0);
    assert_eq!(first["id"], "lead-frame-1");
    assert_eq!(first["role"], "lead-frame");
    let first_generation = first["binding"]["generation"]
        .as_str()
        .expect("the original generation")
        .to_owned();

    let valid = valid_read_only_assignment();
    let corrected = resume_fixture(
        &fixture,
        "lead-frame",
        FIRST_SESSION,
        "correction-1",
        &valid,
    );
    assert!(corrected.status.success(), "{}", text(&corrected));

    let mut cursor = fixture.cursor();
    assert_eq!(
        cursor["baseline_qualification"]["workflow"]["lead_frame"], "lead-frame-1",
        "{cursor}"
    );
    assert_eq!(
        cursor["baseline_qualification"]["workflow"]["lead_frame_correction"],
        "lead-frame-correction-1",
        "{cursor}"
    );
    let attempts = cursor["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 2, "{cursor}");
    let original = &attempts[0];
    let correction = &attempts[1];
    assert_eq!(original["id"], "lead-frame-1", "{cursor}");
    assert_eq!(original["state"], "completed", "{cursor}");
    assert_eq!(
        original["binding"]["session"], FIRST_SESSION,
        "the original accepted turn keeps its exact resumed session: {cursor}"
    );
    assert_eq!(correction["id"], "lead-frame-correction-1", "{cursor}");
    assert_eq!(correction["role"], "lead-frame", "{cursor}");
    assert_eq!(correction["state"], "started", "{cursor}");
    assert_eq!(
        correction["binding"]["slot"], original["binding"]["slot"],
        "{cursor}"
    );
    assert_eq!(
        correction["binding"]["owner"], original["binding"]["owner"],
        "{cursor}"
    );
    assert_ne!(
        correction["binding"]["generation"],
        json!(first_generation),
        "the correction is a new native generation: {cursor}"
    );
    assert!(
        original["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("deterministic assignment format defect")),
        "the original invalid frame records its deterministic defect: {cursor}"
    );

    let original_prompt =
        fs::read_to_string(original["assignment"].as_str().expect("original prompt")).unwrap();
    let correction_prompt = fs::read_to_string(
        correction["assignment"]
            .as_str()
            .expect("correction prompt"),
    )
    .unwrap();
    assert_contract(&original_prompt);
    assert_contract(&correction_prompt);
    assert!(
        original_prompt.contains(
            "Read the frozen add-workload OpenSpec task and report its state. Preserve its independent acceptance."
        ),
        "the original prompt carries the frozen task instructions, not an answer: {original_prompt}"
    );
    assert!(
        correction_prompt.contains("invalid type: map, expected a sequence at line 4 column 12"),
        "the correction names the native validation error: {correction_prompt}"
    );
    assert!(
        correction_prompt.contains("single bounded format correction")
            && correction_prompt.contains("Do not implement changes"),
        "the correction is explicitly format-only: {correction_prompt}"
    );

    let original_result = retained_result_path(original);
    assert_eq!(
        fs::read_to_string(&original_result).unwrap(),
        malformed,
        "the original invalid result is preserved byte for byte"
    );
    assert_eq!(
        original["retained"]["result_sha256"],
        json!(build_identity::hash_file(&original_result).unwrap())
    );

    let correction_receipt = PathBuf::from(
        correction["receipt"]
            .as_str()
            .expect("the correction receipt"),
    );
    let correction_terminal = wait_for_terminal_receipt(&correction_receipt);
    assert_eq!(
        correction_terminal["observation"]["session"], FIRST_SESSION,
        "the fixture must serve the exact requested session: {correction_terminal}"
    );

    let sidecar = fixture
        .run
        .join("baseline-qualification/action-evidence/lead-frame-1.json");
    let action: Value = serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
    assert_eq!(action["attempt"], "lead-frame-1", "{action}");
    assert_eq!(action["role"], "lead-frame", "{action}");
    assert_eq!(action["session"], FIRST_SESSION, "{action}");
    assert_eq!(action["captured"], true, "{action}");
    assert!(
        action["retained_bytes"].as_u64().unwrap_or(0) > 0,
        "the original visible stream was snapshotted before pool reuse: {action}"
    );

    let usage = rollout_records(&fixture, FIRST_SESSION);
    assert_eq!(
        usage.len(),
        2,
        "both lead turns keep their own native cost record: {usage:?}"
    );
    let responses: Vec<&str> = usage
        .iter()
        .filter_map(|record| record["payload"]["response_id"].as_str())
        .collect();
    assert!(responses.iter().any(|id| id.contains("frame-1")));
    assert!(responses.iter().any(|id| id.contains("correction-1")));
    for record in &usage {
        assert!(
            record["payload"]["usage"]["total_tokens"]
                .as_u64()
                .is_some(),
            "each retained turn carries complete native usage: {record}"
        );
    }

    let executor = resume_fixture(
        &fixture,
        "executor",
        SECOND_SESSION,
        "executor-1",
        "executor: completed the assigned read-only task",
    );
    assert!(executor.status.success(), "{}", text(&executor));
    cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 3, "{cursor}");
    let original = &attempts[0];
    let correction = &attempts[1];
    let executor = &attempts[2];
    assert_eq!(original["retained"]["result"], json!(original_result));
    assert_eq!(correction["state"], "completed", "{cursor}");
    assert_eq!(
        correction["binding"]["session"], FIRST_SESSION,
        "the settled correction resumed the exact recorded lead session: {cursor}"
    );
    let correction_result = retained_result_path(correction);
    assert_eq!(
        correction["retained"]["result_sha256"],
        json!(build_identity::hash_file(&correction_result).unwrap())
    );
    let corrected_document: Value =
        serde_json::from_slice(&fs::read(&correction_result).unwrap()).unwrap();
    assert_eq!(
        corrected_document["outputs"],
        json!([]),
        "a read-only task keeps an empty output scope"
    );
    assert_eq!(executor["id"], "executor-1", "{cursor}");
    assert_eq!(executor["role"], "executor", "{cursor}");
    assert_eq!(executor["state"], "started", "{cursor}");
    let executor_receipt =
        PathBuf::from(executor["receipt"].as_str().expect("the executor receipt"));
    let executor_terminal = wait_for_terminal_receipt(&executor_receipt);
    assert_eq!(
        executor_terminal["observation"]["session"], SECOND_SESSION,
        "the ordinary executor was served by its exact session: {executor_terminal}"
    );
    assert_eq!(
        cursor["baseline_qualification"]["workflow"]["executor_attempt"], "executor-1",
        "{cursor}"
    );
    assert_eq!(
        executor["assignment"], correction["retained"]["result"],
        "the ordinary executor consumes the valid corrected assignment: {cursor}"
    );
}

#[test]
fn second_malformed_correction_stops_without_a_third_lead_turn() {
    let _serial = INSTALL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = prepare_fixture("qualification-framing-second-malformed");
    let malformed = malformed_assignment();
    let started = start_fixture(&fixture, &malformed);
    assert!(started.status.success(), "{}", text(&started));
    let first = wait_attempt(&fixture, 0);
    assert_eq!(first["id"], "lead-frame-1");

    let invalid_scope = read_only_scope_violation();
    let corrected = resume_fixture(
        &fixture,
        "lead-frame",
        FIRST_SESSION,
        "correction-1",
        &invalid_scope,
    );
    assert!(corrected.status.success(), "{}", text(&corrected));
    let correction = wait_attempt(&fixture, 1);
    assert_eq!(correction["id"], "lead-frame-correction-1");

    let valid = valid_read_only_assignment();
    let stopped = resume_fixture(
        &fixture,
        "lead-frame",
        REFUSED_SESSION,
        "third-frame",
        &valid,
    );
    let text = refusal_text(&fixture, &stopped);
    assert!(stopped.status.success(), "{text}");
    assert_retained_attempts(&fixture, 2);
    assert!(
        text.contains("also invalid") && text.contains("outputs must be []"),
        "the second malformed correction is an explicit retained stop: {text}"
    );

    let cursor = fixture.cursor();
    assert_eq!(
        cursor["baseline_qualification"]["workflow"]["lead_frame_correction"],
        "lead-frame-correction-1",
        "{cursor}"
    );
    let attempts = cursor["attempts"].as_array().expect("attempts");
    let original_result = retained_result_path(&attempts[0]);
    assert_eq!(fs::read_to_string(original_result).unwrap(), malformed);
    let correction_result = retained_result_path(&attempts[1]);
    let corrected_document: Value =
        serde_json::from_slice(&fs::read(&correction_result).unwrap()).unwrap();
    assert_eq!(
        corrected_document["outputs"],
        json!(["solution.txt"]),
        "the invalid correction result is preserved, not normalized"
    );
    assert!(
        attempts[1]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("deterministic assignment format defect")),
        "the correction attempt records its deterministic defect: {cursor}"
    );
    assert_eq!(
        rollout_records(&fixture, FIRST_SESSION).len(),
        2,
        "both lead turns keep their costs even after the retained stop: {cursor}"
    );
    assert_eq!(
        attempts[0]["binding"]["session"],
        json!(FIRST_SESSION),
        "the original exact session stays recorded: {cursor}"
    );
    assert_eq!(
        attempts[1]["binding"]["session"],
        json!(FIRST_SESSION),
        "the correction exact session stays recorded: {cursor}"
    );
}

#[test]
fn missing_unknown_and_changed_correction_state_refuses_without_dispatch() {
    let _serial = INSTALL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = prepare_fixture("qualification-framing-refusals");
    let malformed = malformed_assignment();
    let started = start_fixture(&fixture, &malformed);
    assert!(started.status.success(), "{}", text(&started));
    let first = wait_attempt(&fixture, 0);
    assert_eq!(first["id"], "lead-frame-1");

    let correction_document = malformed_assignment();
    let corrected = resume_fixture(
        &fixture,
        "lead-frame",
        FIRST_SESSION,
        "correction-1",
        &correction_document,
    );
    assert!(corrected.status.success(), "{}", text(&corrected));
    let correction = wait_attempt(&fixture, 1);
    let correction_receipt = PathBuf::from(
        correction["receipt"]
            .as_str()
            .expect("the correction receipt"),
    );
    let correction_terminal = wait_for_terminal_receipt(&correction_receipt);
    assert_eq!(
        correction_terminal["observation"]["state"], "completed",
        "{correction_terminal}"
    );
    let correction_receipt_bytes = fs::read(&correction_receipt).unwrap();

    // Settle the malformed correction so it can never reach an executor while
    // the refusal variants reuse its exact retained evidence.
    let valid = valid_read_only_assignment();
    let settled = resume_fixture(
        &fixture,
        "lead-frame",
        REFUSED_SESSION,
        "settle-invalid",
        &valid,
    );
    let settled_text = refusal_text(&fixture, &settled);
    assert!(settled.status.success(), "{settled_text}");
    assert!(
        settled_text.contains("also invalid"),
        "the malformed correction is a retained stop: {settled_text}"
    );
    let mut baseline = fixture.cursor();
    assert_eq!(
        baseline["attempts"].as_array().expect("attempts").len(),
        2,
        "{baseline}"
    );
    assert_eq!(baseline["attempts"][1]["state"], "completed", "{baseline}");
    assert_eq!(
        baseline["baseline_qualification"]["workflow"]["executor_attempt"],
        Value::Null,
        "the refused control dispatch must not create an executor attempt: {baseline}"
    );
    baseline["baseline_qualification"]["condition"] = Value::Null;
    baseline["condition"] = Value::Null;

    let original_result = retained_result_path(&baseline["attempts"][0]);
    let original_bytes = fs::read(&original_result).unwrap();
    let correction_result = retained_result_path(&baseline["attempts"][1]);
    let correction_bytes = fs::read(&correction_result).unwrap();

    // Missing attempt after the durable intent.
    let mut missing = baseline.clone();
    missing["attempts"]
        .as_array_mut()
        .expect("attempts")
        .retain(|attempt| attempt["id"] != "lead-frame-correction-1");
    fixture.write_cursor(&missing);
    let refused = resume_fixture(
        &fixture,
        "lead-frame",
        REFUSED_SESSION,
        "third-frame",
        &valid,
    );
    let text = refusal_text(&fixture, &refused);
    assert!(refused.status.success(), "{text}");
    assert_retained_attempts(&fixture, 1);
    assert!(
        text.contains("attempt record is missing"),
        "a recorded correction intent without its attempt refuses: {text}"
    );
    assert_eq!(fs::read(&original_result).unwrap(), original_bytes);

    // Unknown outcome.
    install_active_receipt(&correction_receipt, &correction_terminal);
    let mut unknown = baseline.clone();
    unknown["attempts"][1]["state"] = json!("unknown");
    fixture.write_cursor(&unknown);
    let refused = resume_fixture(
        &fixture,
        "lead-frame",
        REFUSED_SESSION,
        "third-frame",
        &valid,
    );
    let text = refusal_text(&fixture, &refused);
    assert!(refused.status.success(), "{text}");
    assert_retained_attempts(&fixture, 2);
    assert!(
        text.contains("is Unknown"),
        "an unknown correction outcome refuses without replay: {text}"
    );

    // In-flight turn.
    install_active_receipt(&correction_receipt, &correction_terminal);
    let mut started_again = baseline.clone();
    started_again["attempts"][1]["state"] = json!("started");
    fixture.write_cursor(&started_again);
    let refused = resume_fixture(
        &fixture,
        "lead-frame",
        REFUSED_SESSION,
        "third-frame",
        &valid,
    );
    let text = refusal_text(&fixture, &refused);
    assert!(refused.status.success(), "{text}");
    assert_retained_attempts(&fixture, 2);
    assert!(
        text.contains("resume observes its exact native outcome without replay"),
        "an in-flight correction is observed, not dispatched again: {text}"
    );
    assert_eq!(
        fixture.cursor()["attempts"][1]["state"],
        "started",
        "the in-flight correction stays retained: {text}"
    );

    // Restore the terminal receipt so the completed-correction variants use
    // their unchanged evidence rather than the synthetic live observation.
    fs::write(&correction_receipt, &correction_receipt_bytes).unwrap();

    // Missing generation/binding on the completed correction.
    let mut unbound = baseline.clone();
    unbound["attempts"][1]["binding"] = Value::Null;
    fixture.write_cursor(&unbound);
    let refused = resume_fixture(
        &fixture,
        "lead-frame",
        REFUSED_SESSION,
        "third-frame",
        &valid,
    );
    let text = refusal_text(&fixture, &refused);
    assert!(refused.status.success(), "{text}");
    assert_retained_attempts(&fixture, 2);
    assert!(
        text.contains("no exact native participant"),
        "a correction without its native binding refuses: {text}"
    );

    // Changed original binding.
    let mut changed_original = baseline.clone();
    changed_original["attempts"][0]["binding"]["session"] = json!("foreign-original-session");
    fixture.write_cursor(&changed_original);
    let refused = resume_fixture(
        &fixture,
        "lead-frame",
        REFUSED_SESSION,
        "third-frame",
        &valid,
    );
    let text = refusal_text(&fixture, &refused);
    assert!(refused.status.success(), "{text}");
    assert_retained_attempts(&fixture, 2);
    assert!(
        text.contains("instead of the recorded lead"),
        "a changed original binding refuses before executor dispatch: {text}"
    );

    // Changed correction binding.
    let mut changed_correction = baseline.clone();
    changed_correction["attempts"][1]["binding"]["session"] = json!("foreign-correction-session");
    fixture.write_cursor(&changed_correction);
    let refused = resume_fixture(
        &fixture,
        "lead-frame",
        REFUSED_SESSION,
        "third-frame",
        &valid,
    );
    let text = refusal_text(&fixture, &refused);
    assert!(refused.status.success(), "{text}");
    assert_retained_attempts(&fixture, 2);
    assert!(
        text.contains("instead of the recorded lead"),
        "a changed correction binding refuses before executor dispatch: {text}"
    );

    assert_eq!(
        fs::read(&original_result).unwrap(),
        original_bytes,
        "the original retained result is never erased or rewritten"
    );
    assert_eq!(
        fs::read(&correction_result).unwrap(),
        correction_bytes,
        "the correction retained result is never erased or rewritten"
    );
}
