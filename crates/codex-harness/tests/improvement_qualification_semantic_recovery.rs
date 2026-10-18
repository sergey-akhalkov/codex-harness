#![cfg(windows)]

#[path = "fixtures/improvement_comparison_common.rs"]
mod improvement_comparison_common;
use improvement_comparison_common::*;

use harness_core::{
    build_identity,
    process_service::{self, ServiceProcess},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Output,
};

const FIRST_SESSION: &str = "12345678-1234-4234-8234-123456789021";
const SECOND_SESSION: &str = "12345678-1234-4234-8234-123456789022";
const EXECUTOR_SESSION: &str = "12345678-1234-4234-8234-123456789023";
const REVIEW_SESSION: &str = "12345678-1234-4234-8234-123456789024";

fn assignment() -> String {
    json!({
        "schema": 1,
        "objective": "Implement the frozen workload in the bound checkout.",
        "inputs": [],
        "outputs": ["solution.txt"],
        "invariants": ["Preserve frozen acceptance."],
        "acceptance": ["The independent workload check must pass."],
        "consumer": "qualification controller",
        "escalate": []
    })
    .to_string()
}

fn prepare_fixture() -> Fixture {
    let fixture = Fixture::new("qualification-semantic-recovery");
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
        "Read the frozen add-workload OpenSpec task and frame its implementation inside solution.txt and sleep_ms. Preserve its independent acceptance.",
    )
    .unwrap();
    let review_config = fixture
        .spec
        .with_file_name("qualification-review-config.toml");
    fs::write(
        &review_config,
        "sandbox_mode = 'read-only'\napproval_policy = 'never'\nmodel = 'gpt-6-astra'\nmodel_provider = 'openai'\nmodel_reasoning_effort = 'xhigh'\n[profiles.ds]\nmodel = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    let criteria = fixture
        .spec
        .with_file_name("qualification-review-criteria.txt");
    fs::write(
        &criteria,
        "The submitted result must implement the frozen task and preserve its declared acceptance.\n",
    )
    .unwrap();
    spec["local_runner"] = Value::Null;
    spec["qualification"] = Value::Null;
    spec["baseline_qualification"] = json!({
        "schema": 1,
        "task": previous["task"],
        "task_prompt": {"path": prompt, "sha256": build_identity::hash_file(&prompt).unwrap()},
        "runtime": {
            "state": fixture.state,
            "build": fixture.state.join("builds/h-build"),
            "label": "accepted-native-runtime",
            "upstream": fixture.upstream,
            "native_config": {"path": config, "sha256": build_identity::hash_file(&config).unwrap()},
            "private_inputs": []
        },
        "acceptance": previous["acceptance"],
        "workflow": {"schema":1,"lead_profile":"default","executor_profile":"ds","max_corrections":1,"capacity":6},
        "report": fixture.spec.with_file_name("qualification-result.txt"),
        "semantic_review": {
            "schema": 1,
            "runtime": {
                "state": fixture.state,
                "build": fixture.state.join("builds/h-build"),
                "label": "accepted-semantic-review-runtime",
                "upstream": fixture.upstream,
                "native_config": {
                    "path": review_config,
                    "sha256": build_identity::hash_file(&review_config).unwrap()
                },
                "private_inputs": []
            },
            "profile": "ds",
            "criteria": {
                "path": criteria,
                "sha256": build_identity::hash_file(&criteria).unwrap()
            },
            "sources": ["solution.txt"],
            "max_corrections": 1
        }
    });
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    fixture
}

fn resume_turn(
    fixture: &Fixture,
    role: &str,
    session: &str,
    suffix: &str,
    result: &str,
    solution: &str,
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
            ("HARNESS_IMPROVEMENT_FIXTURE_SOLUTION_TEXT", solution),
        ],
    )
}

fn wait_last(fixture: &Fixture) -> Value {
    let cursor = fixture.cursor();
    let attempt = cursor["attempts"]
        .as_array()
        .and_then(|attempts| attempts.last())
        .expect("the dispatched attempt is recorded");
    let receipt = PathBuf::from(attempt["receipt"].as_str().expect("the attempt receipt"));
    wait_for_terminal_receipt(&receipt)
}

fn install_legacy_failure(receipt: &Path, completed: &Value) {
    let mut value = completed.clone();
    let session = value["observation"]["session"]
        .as_str()
        .expect("the fixture receipt records the native session")
        .to_owned();
    let owner = value["slot"]["owner"]
        .as_str()
        .expect("the fixture receipt records the dispatch owner")
        .to_owned();
    let observation = value["observation"]
        .as_object_mut()
        .expect("the fixture receipt records an observation");
    let detail = observation
        .get("detail")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let result = observation
        .get("result")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let cause = format!(
        "the native frontend did not attach to thread {session} within 30s; console caption is \"CLead (default) - {owner}\"; bounded console text: \"  Hooks need review\\n  1 hook is new or changed.\\n  Hooks can run outside the sandbox after you trust them.\\n  1. Review hooks\\n› 2. Trust all and continue\\n  3. Continue without trusting (hooks won't run)\\n  enter confirm · esc skip\". The assignment was not submitted. Remedy: resolve the reported frontend startup condition, then retry through the owning dispatcher; the owned child tree was terminated; no process remained"
    );
    observation.insert("reason".to_owned(), Value::Null);
    observation.insert("state".to_owned(), json!("failed"));
    observation.insert("turn".to_owned(), Value::Null);
    observation.insert("submission".to_owned(), Value::Null);
    observation.insert("previousSession".to_owned(), Value::Null);
    observation.insert("exitCode".to_owned(), Value::Null);
    observation.insert("events".to_owned(), json!(0));
    observation.insert("messages".to_owned(), json!(0));
    observation.insert("toolCalls".to_owned(), json!(0));
    observation.insert("rounds".to_owned(), json!(0));
    observation.insert("toolOperationCounts".to_owned(), json!({}));
    observation.insert("malformed".to_owned(), json!(0));
    observation.insert("cause".to_owned(), json!(cause));
    observation.insert("result".to_owned(), Value::Null);
    observation.insert("updatedMs".to_owned(), json!(unix_ms_now() - 60_000));
    observation.insert("activity".to_owned(), json!([]));
    observation.insert("detailTruncated".to_owned(), json!(false));
    observation.insert("activityTruncated".to_owned(), json!(false));
    observation.insert("activityGap".to_owned(), json!(false));
    observation.insert("clock".to_owned(), Value::Null);
    if let Some(detail) = detail {
        fs::write(detail, []).unwrap();
    }
    if let Some(result) = result {
        let _ = fs::remove_file(result);
    }
    fs::write(receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn write_running_receipt(receipt: &Path, completed: &Value, live_host: bool) -> Value {
    let user = process_service::current_user().unwrap();
    let program = std::env::current_exe().unwrap();
    let identity = ServiceProcess::observe(std::process::id(), &program, 0, &user)
        .unwrap()
        .identity();
    let host = json!({
        "pid": identity.pid,
        "created": if live_host {
            identity.creation_time
        } else {
            identity.creation_time.saturating_add(1)
        },
        "program": program.to_string_lossy(),
    });
    let mut value = completed.clone();
    let observation = value["observation"].as_object_mut().unwrap();
    observation.insert("state".to_owned(), json!("running"));
    observation.insert("host".to_owned(), host.clone());
    observation.insert("updatedMs".to_owned(), json!(unix_ms_now()));
    observation.insert("exitCode".to_owned(), Value::Null);
    observation.insert("cause".to_owned(), Value::Null);
    observation.insert("result".to_owned(), Value::Null);
    fs::write(receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    host
}

fn semantic_acceptance(fixture: &Fixture) -> String {
    let spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    let run = spec["run"].as_str().expect("the fixture run identity");
    format!("{{\"run_id\":\"{run}-semantic-review\",\"findings\":[],\"unresolved_issues\":[]}}")
}

fn assert_blocked(fixture: &Fixture, cursor: &Value, expected: &str) {
    fixture.write_cursor(cursor);
    let acceptance = semantic_acceptance(fixture);
    let output = resume_turn(
        fixture,
        "lead-acceptance",
        REVIEW_SESSION,
        "blocked-review",
        &acceptance,
        "needs-correction",
    );
    assert!(!output.status.success(), "{}", text(&output));
    assert!(text(&output).contains(expected), "{}", text(&output));
    let refused = fixture.cursor();
    let attempts = refused["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 4, "{refused}");
    assert!(
        !attempts
            .iter()
            .any(|attempt| attempt["role"] == "semantic-review"),
        "{refused}"
    );
    let source = fixture.run.join("baseline-qualification/reviewer/source");
    if source.exists() {
        fs::remove_dir_all(source).unwrap();
    }
}

#[test]
fn verified_pre_submission_failure_keeps_action_evidence_and_reaches_semantic_review() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = prepare_fixture();
    let assignment = assignment();
    let started = fixture.improve_with_env(
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
            ("HARNESS_IMPROVEMENT_FIXTURE_RESULT", &assignment),
            (
                "HARNESS_IMPROVEMENT_FIXTURE_SOLUTION_TEXT",
                "needs-correction",
            ),
        ],
    );
    assert!(started.status.success(), "{}", text(&started));

    let cursor = fixture.cursor();
    let first = cursor["attempts"]
        .as_array()
        .and_then(|attempts| attempts.first())
        .cloned()
        .expect("the first lead frame is recorded");
    let first_receipt = PathBuf::from(first["receipt"].as_str().expect("first receipt"));
    let completed = wait_for_terminal_receipt(&first_receipt);
    assert_eq!(
        completed["observation"]["state"], "completed",
        "{completed}"
    );

    let mut cursor = fixture.cursor();
    cursor["attempts"][0]["updated_ms"] = json!(unix_ms_now() - 60_000);
    fixture.write_cursor(&cursor);
    install_legacy_failure(&first_receipt, &completed);

    let recovered = resume_turn(
        &fixture,
        "lead-frame",
        SECOND_SESSION,
        "recovery-frame",
        &assignment,
        "needs-correction",
    );
    assert!(recovered.status.success(), "{}", text(&recovered));
    let recovered_cursor = fixture.cursor();
    let attempts = recovered_cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2, "{recovered_cursor}");
    let failed = attempts[0].clone();
    assert_eq!(failed["id"], "lead-frame-1", "{recovered_cursor}");
    assert_eq!(failed["state"], "failed", "{recovered_cursor}");
    assert_eq!(
        failed["binding"]["pre_submission"], "legacy-frontend-attachment",
        "{recovered_cursor}"
    );
    let retained_path = PathBuf::from(
        failed["retained"]["receipt"]
            .as_str()
            .expect("the failed frame retained its terminal receipt"),
    );
    let retained_bytes = fs::read(&retained_path).unwrap();
    let second_receipt = PathBuf::from(attempts[1]["receipt"].as_str().expect("second receipt"));
    let second_completed = wait_for_terminal_receipt(&second_receipt);
    assert_eq!(second_completed["observation"]["state"], "completed");

    let executor = resume_turn(
        &fixture,
        "executor",
        EXECUTOR_SESSION,
        "executor-1",
        "Implemented the task and committed the scoped solution.",
        "solved",
    );
    assert!(executor.status.success(), "{}", text(&executor));
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 3);
    assert_eq!(wait_last(&fixture)["observation"]["state"], "completed");

    let review = resume_turn(
        &fixture,
        "lead-review",
        SECOND_SESSION,
        "lead-review-1",
        "ACCEPT: The scoped result and applicable checks satisfy the original task.",
        "needs-correction",
    );
    assert!(review.status.success(), "{}", text(&review));
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 4);
    assert_eq!(wait_last(&fixture)["observation"]["state"], "completed");

    let mut missing_proof = fixture.cursor();
    missing_proof["attempts"][0]["binding"]
        .as_object_mut()
        .unwrap()
        .remove("pre_submission");
    assert_blocked(
        &fixture,
        &missing_proof,
        "a lead or executor turn is unresolved",
    );
    // The refusal above follows the retained U2 shape: the controller settled
    // the accepted workflow (recording completion) before the semantic packet
    // refused the unresolved participant. That completed state is the base for
    // every other refusal case and for the successful reviewer dispatch.
    let mut ready = fixture.cursor();
    assert_eq!(
        ready["baseline_qualification"]["workflow"]["completed"], true,
        "{ready}"
    );
    assert_eq!(
        ready["baseline_qualification"]["workflow"]["lead_accepted"], true,
        "{ready}"
    );
    ready["attempts"][0]["binding"]["pre_submission"] = json!("legacy-frontend-attachment");
    assert_eq!(ready["attempts"][0], failed, "{ready}");
    assert_eq!(fs::read(&retained_path).unwrap(), retained_bytes);

    let mut missing_binding = ready.clone();
    missing_binding["attempts"][0]["binding"] = Value::Null;
    assert_blocked(
        &fixture,
        &missing_binding,
        "a lead or executor turn is unresolved",
    );

    let failed_receipt_bytes = fs::read(&first_receipt).unwrap();

    let mut active = ready.clone();
    let active_host = write_running_receipt(&first_receipt, &completed, true);
    active["attempts"][0]["state"] = json!("started");
    active["attempts"][0]["binding"]["host"] = active_host;
    assert_blocked(&fixture, &active, "a lead or executor turn is unresolved");
    fs::write(&first_receipt, &failed_receipt_bytes).unwrap();

    let mut unknown = ready.clone();
    let unknown_host = write_running_receipt(&first_receipt, &completed, false);
    unknown["attempts"][0]["state"] = json!("unknown");
    unknown["attempts"][0]["binding"]["host"] = unknown_host;
    assert_blocked(&fixture, &unknown, "a lead or executor turn is unresolved");
    fs::write(&first_receipt, &failed_receipt_bytes).unwrap();

    let mut missing_retained = ready.clone();
    missing_retained["attempts"][0]["retained"] = Value::Null;
    assert_blocked(
        &fixture,
        &missing_retained,
        "retained terminal evidence is missing",
    );

    fs::write(&retained_path, b"corrupt retained receipt\n").unwrap();
    assert_blocked(&fixture, &ready, "does not verify");
    fs::write(&retained_path, &retained_bytes).unwrap();

    fixture.write_cursor(&ready);
    let acceptance = semantic_acceptance(&fixture);
    let dispatched = resume_turn(
        &fixture,
        "lead-acceptance",
        REVIEW_SESSION,
        "semantic-review-1",
        &acceptance,
        "needs-correction",
    );
    assert!(dispatched.status.success(), "{}", text(&dispatched));
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 5, "{cursor}");
    assert_eq!(attempts[0], failed, "{cursor}");
    assert_eq!(fs::read(&retained_path).unwrap(), retained_bytes);
    let semantic = &attempts[4];
    assert_eq!(semantic["role"], "semantic-review", "{cursor}");
    assert_eq!(semantic["state"], "started", "{cursor}");
    assert_eq!(semantic["model"], "deepseek-flash", "{cursor}");

    let actions_path = fixture
        .run
        .join("baseline-qualification/reviewer/source/actions.json");
    let actions: Value = serde_json::from_slice(&fs::read(&actions_path).unwrap()).unwrap();
    assert_eq!(actions["action_evidence_complete"], true, "{actions}");
    let participants = actions["participants"].as_array().unwrap();
    let participant_ids: Vec<_> = participants
        .iter()
        .filter_map(|participant| participant["attempt"].as_str())
        .collect();
    assert_eq!(
        participant_ids,
        ["lead-frame-2", "executor-1", "lead-review-1"],
        "{actions}"
    );
    let coverage = actions["action_coverage"].as_array().unwrap();
    assert_eq!(coverage.len(), participants.len(), "{actions}");
    for (participant, coverage) in participants.iter().zip(coverage) {
        assert_eq!(coverage["attempt"], participant["attempt"], "{actions}");
        assert_eq!(coverage["complete"], true, "{actions}");
        assert!(
            coverage["turn"]
                .as_str()
                .is_some_and(|turn| !turn.is_empty()),
            "{actions}"
        );
    }
    assert!(
        coverage
            .iter()
            .all(|entry| entry["attempt"] != "lead-frame-1"),
        "{actions}"
    );

    let reviewer = wait_last(&fixture);
    assert_eq!(reviewer["observation"]["state"], "completed", "{reviewer}");
    assert_eq!(
        reviewer["observation"]["session"], REVIEW_SESSION,
        "{reviewer}"
    );

    let settled = fixture.resume();
    assert!(settled.status.success(), "{}", text(&settled));
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["phase"], "baseline-qualification-recorded",
        "{cursor}"
    );
    let workflow = &cursor["baseline_qualification"]["workflow"];
    assert_eq!(workflow["completed"], true, "{cursor}");
    assert_eq!(workflow["lead_accepted"], true, "{cursor}");
    assert_eq!(
        cursor["baseline_qualification"]["accepted"], true,
        "{cursor}"
    );
    assert_eq!(
        cursor["baseline_qualification"]["semantic_review"]["judgment_kind"], "accept",
        "{cursor}"
    );
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 5, "{cursor}");
    assert_eq!(attempts[0], failed, "{cursor}");
    assert_eq!(fs::read(&retained_path).unwrap(), retained_bytes);

    let row_path = PathBuf::from(
        cursor["baseline_qualification"]["row"]
            .as_str()
            .expect("the accepted workflow records its accounting row"),
    );
    let row: Value = serde_json::from_slice(&fs::read(&row_path).unwrap()).unwrap();
    assert_eq!(row["accepted"], true, "{row}");
    let native_runs = row["native_runs"].as_array().unwrap();
    assert_eq!(native_runs.len(), 3, "{row}");
    let mut sessions: Vec<&str> = native_runs
        .iter()
        .filter_map(|run| run["session"].as_str())
        .collect();
    sessions.sort_unstable();
    let mut expected_sessions = [SECOND_SESSION, EXECUTOR_SESSION, REVIEW_SESSION];
    expected_sessions.sort_unstable();
    assert_eq!(sessions, expected_sessions, "{row}");
    assert!(!sessions.contains(&FIRST_SESSION), "{row}");
    let responses = row["usage"]["response_count"].as_u64().unwrap();
    assert!(
        responses >= 4,
        "the four completed turns are charged: {row}"
    );
    assert_eq!(
        row["usage"]["total_tokens"].as_u64(),
        Some(responses * 120),
        "the failed zero-turn dispatch contributes no usage: {row}"
    );
}
