#![cfg(windows)]

#[path = "fixtures/improvement_comparison_common.rs"]
mod improvement_comparison_common;
use improvement_comparison_common::*;

use harness_core::{
    build_identity,
    process_service::{self, ServiceProcess},
    task_worktree,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Output,
};

const LEAD_SESSION: &str = "12345678-1234-4234-8234-123456789031";
const EXECUTOR_SESSION: &str = "12345678-1234-4234-8234-123456789032";
const REVIEW_SESSION: &str = "12345678-1234-4234-8234-123456789033";
const RECOVERY_SESSION: &str = "12345678-1234-4234-8234-123456789034";
const INITIAL_REVIEW: &str = "semantic-review-1";
const RECOVERY_REVIEW: &str = "semantic-review-1-recovery-1";
const CORRECTION_REVIEW: &str = "semantic-review-2";

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
    let fixture = Fixture::new("qualification-reviewer-recovery");
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

fn semantic_acceptance(fixture: &Fixture) -> String {
    let spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    let run = spec["run"].as_str().expect("the fixture run identity");
    format!("{{\"run_id\":\"{run}-semantic-review\",\"findings\":[],\"unresolved_issues\":[]}}")
}

fn directory_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                if entry.file_name() != std::ffi::OsStr::new(".git") {
                    pending.push(path);
                }
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    files
}

fn install_native_startup_failure(receipt: &Path, completed: &Value, finished_ms: u64) -> Value {
    let mut value = completed.clone();
    let session = value["observation"]["session"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned();
    let observation = value["observation"].as_object_mut().unwrap();
    let detail = observation
        .get("detail")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let result = observation
        .get("result")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let cause = format!(
        "the native frontend did not attach to thread {session} within 30s; console caption is \"CReview (ds)\"; bounded console text: \"  Hooks need review\n  1 hook is new or changed.\n  Hooks can run outside the sandbox after you trust them.\n› 2. Trust all and continue\n  3. Continue without trusting (hooks won't run)\". The assignment was not submitted. The owned child tree was terminated; no process remained"
    );
    observation.insert("reason".to_owned(), Value::Null);
    observation.insert("state".to_owned(), json!("failed"));
    observation.insert("submission".to_owned(), json!("not-attempted"));
    observation.insert("turn".to_owned(), Value::Null);
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
    observation.insert("updatedMs".to_owned(), json!(finished_ms));
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
    value
}

fn failed_attempt_cursor(
    ready: &Value,
    retained_receipt: &Path,
    retained_bytes: &[u8],
    finished_ms: u64,
) -> Value {
    let mut cursor = ready.clone();
    let attempt = &mut cursor["attempts"][3];
    attempt["state"] = json!("failed");
    attempt["updated_ms"] = json!(finished_ms);
    attempt["reason"] = json!("synthetic native frontend startup failure before submission");
    attempt["binding"]["pre_submission"] = json!("native-marker");
    attempt["retained"] = json!({
        "receipt": retained_receipt,
        "receipt_sha256": build_identity::hash_bytes(retained_bytes),
        "result": null,
        "result_sha256": null,
        "note": "the terminal result file was already gone when the attempt settled",
        "retained_ms": unix_ms_now(),
    });
    cursor
}

fn live_host() -> Value {
    let user = process_service::current_user().unwrap();
    let program = std::env::current_exe().unwrap();
    let identity = ServiceProcess::observe(std::process::id(), &program, 0, &user)
        .unwrap()
        .identity();
    json!({
        "pid": identity.pid,
        "created": identity.creation_time,
        "program": program.to_string_lossy(),
    })
}

fn assert_reviewer_blocked(fixture: &Fixture, cursor: &Value, expected: &str) {
    fixture.write_cursor(cursor);
    let output = resume_turn(
        fixture,
        "lead-acceptance",
        RECOVERY_SESSION,
        "blocked-reviewer",
        &semantic_acceptance(fixture),
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
            .any(|attempt| attempt["id"] == RECOVERY_REVIEW),
        "{refused}"
    );
    assert!(
        !attempts
            .iter()
            .any(|attempt| attempt["id"] == CORRECTION_REVIEW),
        "{refused}"
    );
}

#[test]
fn verified_native_pre_submission_reviewer_failure_recovers_once_without_replaying_ordinary_work() {
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
            ("HARNESS_IMPROVEMENT_FIXTURE_SESSION", LEAD_SESSION),
            ("HARNESS_IMPROVEMENT_FIXTURE_TURN_SUFFIX", "frame-1"),
            ("HARNESS_IMPROVEMENT_FIXTURE_RESULT", &assignment),
            (
                "HARNESS_IMPROVEMENT_FIXTURE_SOLUTION_TEXT",
                "needs-correction",
            ),
        ],
    );
    assert!(started.status.success(), "{}", text(&started));
    assert_eq!(wait_last(&fixture)["observation"]["state"], "completed");

    let executor = resume_turn(
        &fixture,
        "executor",
        EXECUTOR_SESSION,
        "executor-1",
        "Implemented the task and committed the scoped solution.",
        "solved",
    );
    assert!(executor.status.success(), "{}", text(&executor));
    assert_eq!(wait_last(&fixture)["observation"]["state"], "completed");

    let review = resume_turn(
        &fixture,
        "lead-review",
        LEAD_SESSION,
        "lead-review-1",
        "ACCEPT: The scoped result and applicable checks satisfy the original task.",
        "needs-correction",
    );
    assert!(review.status.success(), "{}", text(&review));
    assert_eq!(wait_last(&fixture)["observation"]["state"], "completed");

    let acceptance = semantic_acceptance(&fixture);
    let dispatched = resume_turn(
        &fixture,
        "lead-acceptance",
        REVIEW_SESSION,
        INITIAL_REVIEW,
        &acceptance,
        "needs-correction",
    );
    assert!(dispatched.status.success(), "{}", text(&dispatched));
    let completed = wait_last(&fixture);
    assert_eq!(
        completed["observation"]["state"], "completed",
        "{completed}"
    );
    assert_eq!(
        completed["observation"]["session"], REVIEW_SESSION,
        "{completed}"
    );

    let ready = fixture.cursor();
    let attempts = ready["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 4, "{ready}");
    assert_eq!(attempts[3]["id"], INITIAL_REVIEW, "{ready}");
    assert_eq!(attempts[3]["role"], "semantic-review", "{ready}");
    assert_eq!(attempts[3]["state"], "started", "{ready}");
    assert_eq!(
        ready["baseline_qualification"]["semantic_review"]["initial_attempt"], INITIAL_REVIEW,
        "{ready}"
    );
    let ordinary = attempts[..3].to_vec();
    let initial = attempts[3].clone();
    let initial_owner = initial["owner"].as_str().unwrap().to_owned();
    let receipt_path = PathBuf::from(initial["receipt"].as_str().unwrap());
    let assignment_path = fixture
        .run
        .join("assignments")
        .join(format!("{INITIAL_REVIEW}.json"));
    let assignment_bytes = fs::read(&assignment_path).unwrap();
    let source_root = fixture.run.join("baseline-qualification/reviewer/source");
    let manifest_path = source_root.join("manifest.json");
    let manifest_bytes = fs::read(&manifest_path).unwrap();
    let packet_bytes = directory_bytes(&source_root);
    let reviewer_home = PathBuf::from(
        ready["baseline_qualification"]["semantic_review"]["home"]
            .as_str()
            .unwrap(),
    );
    let reviewer_slot = initial["binding"]["slot"].as_u64().unwrap();
    let slot_state = task_worktree::pool_state_dir(&reviewer_home, &source_root).unwrap();
    let slot_record_path = slot_state.join(format!("slot-{reviewer_slot}.json"));
    let original_slot_record = fs::read(&slot_record_path).unwrap();
    let reviewer_lease_path = slot_state.join(format!("lease-{reviewer_slot}.json"));
    let started_ms = initial["started_ms"].as_u64().unwrap();
    let finished_ms = unix_ms_now() as u64;
    assert!(finished_ms >= started_ms, "{ready}");

    let failure = install_native_startup_failure(&receipt_path, &completed, finished_ms);
    let failure_bytes = serde_json::to_vec_pretty(&failure).unwrap();
    let retained_path = fixture
        .run
        .join("evidence")
        .join(INITIAL_REVIEW)
        .join("receipt.json");
    fs::create_dir_all(retained_path.parent().unwrap()).unwrap();
    fs::write(&retained_path, &failure_bytes).unwrap();
    let base = failed_attempt_cursor(&ready, &retained_path, &failure_bytes, finished_ms);

    let mut generic = base.clone();
    generic["attempts"][3]["binding"]
        .as_object_mut()
        .unwrap()
        .remove("pre_submission");
    assert_reviewer_blocked(
        &fixture,
        &generic,
        "no exact verified pre-submission startup evidence",
    );

    let mut active_receipt = failure.clone();
    active_receipt["observation"]["state"] = json!("running");
    active_receipt["observation"]["submission"] = Value::Null;
    active_receipt["observation"]["cause"] = Value::Null;
    active_receipt["observation"]["host"] = live_host();
    active_receipt["observation"]["updatedMs"] = json!(unix_ms_now());
    let active_bytes = serde_json::to_vec_pretty(&active_receipt).unwrap();
    fs::write(&retained_path, &active_bytes).unwrap();
    let mut active = base.clone();
    active["attempts"][3]["retained"]["receipt_sha256"] =
        json!(build_identity::hash_bytes(&active_bytes));
    assert_reviewer_blocked(&fixture, &active, "retained a non-failed terminal state");

    let mut unknown_receipt = failure.clone();
    unknown_receipt["observation"]["state"] = json!("running");
    unknown_receipt["observation"]["submission"] = Value::Null;
    unknown_receipt["observation"]["cause"] = Value::Null;
    unknown_receipt["observation"]["host"] = Value::Null;
    unknown_receipt["observation"]["updatedMs"] = json!(unix_ms_now() - 120_000);
    let unknown_bytes = serde_json::to_vec_pretty(&unknown_receipt).unwrap();
    fs::write(&retained_path, &unknown_bytes).unwrap();
    let mut unknown = base.clone();
    unknown["attempts"][3]["retained"]["receipt_sha256"] =
        json!(build_identity::hash_bytes(&unknown_bytes));
    assert_reviewer_blocked(&fixture, &unknown, "retained a non-failed terminal state");

    let mut submitted_receipt = failure.clone();
    submitted_receipt["observation"]["turn"] = json!("turn-submitted-before-lost-attachment");
    let submitted_bytes = serde_json::to_vec_pretty(&submitted_receipt).unwrap();
    fs::write(&retained_path, &submitted_bytes).unwrap();
    let mut submitted = base.clone();
    submitted["attempts"][3]["retained"]["receipt_sha256"] =
        json!(build_identity::hash_bytes(&submitted_bytes));
    assert_reviewer_blocked(
        &fixture,
        &submitted,
        "retained pre-submission evidence that does not match its frozen binding",
    );

    let mut foreign_receipt = failure.clone();
    foreign_receipt["originatingLead"]["runGeneration"] = json!("foreign-reviewer-generation");
    let foreign_bytes = serde_json::to_vec_pretty(&foreign_receipt).unwrap();
    fs::write(&retained_path, &foreign_bytes).unwrap();
    let mut foreign = base.clone();
    foreign["attempts"][3]["retained"]["receipt_sha256"] =
        json!(build_identity::hash_bytes(&foreign_bytes));
    assert_reviewer_blocked(&fixture, &foreign, "does not belong to the frozen dispatch");

    fs::write(&retained_path, &failure_bytes).unwrap();
    let host = live_host();
    fs::write(
        &reviewer_lease_path,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "owner": initial_owner,
            "index": reviewer_slot,
            "path": initial["checkout"],
            "pid": host["pid"],
            "created": host["created"],
            "program": host["program"],
        }))
        .unwrap(),
    )
    .unwrap();
    assert_reviewer_blocked(&fixture, &base, "is already live in slot");
    fs::remove_file(&reviewer_lease_path).unwrap();

    let mut changed_generation: Value = serde_json::from_slice(&original_slot_record).unwrap();
    changed_generation["handle"] =
        json!("0000000000000000000000000000000000000000000000000000000000000000");
    fs::write(
        &slot_record_path,
        serde_json::to_vec_pretty(&changed_generation).unwrap(),
    )
    .unwrap();
    assert_reviewer_blocked(&fixture, &base, "was reassigned or its generation changed");
    fs::write(&slot_record_path, &original_slot_record).unwrap();

    let mut missing = base.clone();
    missing["attempts"][3]["retained"] = Value::Null;
    assert_reviewer_blocked(&fixture, &missing, "retained terminal evidence is missing");

    fs::write(&retained_path, b"corrupt retained receipt\n").unwrap();
    assert_reviewer_blocked(&fixture, &base, "does not verify");
    fs::write(&retained_path, &failure_bytes).unwrap();

    let mut retargeted = base.clone();
    retargeted["attempts"][3]["receipt"] = json!(fixture.run.join("foreign-receipt.json"));
    assert_reviewer_blocked(
        &fixture,
        &retargeted,
        "no longer points at its accepted native receipt",
    );

    let mut fallback = base.clone();
    fallback["attempts"][3]["model"] = json!("fallback-model");
    assert_reviewer_blocked(
        &fixture,
        &fallback,
        "no longer matches the frozen profile/model/provider/effort",
    );

    fs::write(&assignment_path, b"{}\n").unwrap();
    assert_reviewer_blocked(
        &fixture,
        &base,
        "the frozen semantic review assignment changed",
    );
    fs::write(&assignment_path, &assignment_bytes).unwrap();

    fs::write(&retained_path, &failure_bytes).unwrap();
    fs::write(&receipt_path, &failure_bytes).unwrap();
    fixture.write_cursor(&base);
    let recovered = resume_turn(
        &fixture,
        "lead-acceptance",
        RECOVERY_SESSION,
        RECOVERY_REVIEW,
        &acceptance,
        "needs-correction",
    );
    assert!(recovered.status.success(), "{}", text(&recovered));

    let after_dispatch = fixture.cursor();
    let attempts_after = after_dispatch["attempts"].as_array().unwrap();
    assert_eq!(attempts_after.len(), 5, "{after_dispatch}");
    assert_eq!(attempts_after[..3], ordinary[..], "{after_dispatch}");
    let failed_after = &attempts_after[3];
    assert_eq!(failed_after["id"], INITIAL_REVIEW, "{after_dispatch}");
    assert_eq!(failed_after["state"], "failed", "{after_dispatch}");
    assert_eq!(
        failed_after["binding"]["pre_submission"], "native-marker",
        "{after_dispatch}"
    );
    assert_eq!(
        failed_after["retained"]["receipt_sha256"],
        json!(build_identity::hash_bytes(&failure_bytes)),
        "{after_dispatch}"
    );
    assert_eq!(fs::read(&retained_path).unwrap(), failure_bytes);
    assert_eq!(
        failed_after["profile"], initial["profile"],
        "{after_dispatch}"
    );
    assert_eq!(failed_after["model"], initial["model"], "{after_dispatch}");
    assert_eq!(
        failed_after["model_provider"], initial["model_provider"],
        "{after_dispatch}"
    );
    assert_eq!(
        failed_after["reasoning_effort"], initial["reasoning_effort"],
        "{after_dispatch}"
    );
    let recovery = &attempts_after[4];
    assert_eq!(recovery["id"], RECOVERY_REVIEW, "{after_dispatch}");
    assert_eq!(recovery["role"], "semantic-review", "{after_dispatch}");
    assert_eq!(recovery["state"], "started", "{after_dispatch}");
    // The replacement continues the failed reviewer durable task in place:
    // same owner, slot and checkout, but its own dispatch generation and a
    // fresh native conversation.
    assert_eq!(
        recovery["owner"].as_str(),
        Some(initial_owner.as_str()),
        "{after_dispatch}"
    );
    assert_eq!(
        recovery["binding"]["slot"], initial["binding"]["slot"],
        "{after_dispatch}"
    );
    assert_eq!(
        recovery["binding"]["owner"], initial["binding"]["owner"],
        "{after_dispatch}"
    );
    assert_eq!(
        recovery["checkout"], initial["checkout"],
        "{after_dispatch}"
    );
    assert_ne!(
        recovery["binding"]["generation"], initial["binding"]["generation"],
        "{after_dispatch}"
    );
    // The fresh conversation is asserted from the terminal receipt below,
    // because the acceptance snapshot may predate the first observation.
    assert_eq!(
        failed_after["binding"]["generation"], initial["binding"]["generation"],
        "{after_dispatch}"
    );
    assert_eq!(recovery["model"], initial["model"], "{after_dispatch}");
    assert_eq!(
        attempts_after
            .iter()
            .filter(|attempt| attempt["id"] == RECOVERY_REVIEW)
            .count(),
        1,
        "{after_dispatch}"
    );
    assert!(
        !attempts_after
            .iter()
            .any(|attempt| attempt["id"] == CORRECTION_REVIEW),
        "{after_dispatch}"
    );
    let semantic_state = &after_dispatch["baseline_qualification"]["semantic_review"];
    assert_eq!(
        semantic_state["initial_attempt"], RECOVERY_REVIEW,
        "{after_dispatch}"
    );
    assert_eq!(
        semantic_state["correction_attempt"],
        Value::Null,
        "{after_dispatch}"
    );
    assert_eq!(semantic_state["correction_used"], false, "{after_dispatch}");

    let recovery_receipt_path = PathBuf::from(recovery["receipt"].as_str().unwrap());
    let recovered_completed = wait_for_terminal_receipt(&recovery_receipt_path);
    assert_eq!(
        recovered_completed["observation"]["state"], "completed",
        "{recovered_completed}"
    );
    assert_eq!(
        recovered_completed["observation"]["session"], RECOVERY_SESSION,
        "{recovered_completed}"
    );

    let settled = fixture.resume();
    assert!(settled.status.success(), "{}", text(&settled));
    let repeat = fixture.resume();
    assert!(repeat.status.success(), "{}", text(&repeat));

    let final_cursor = fixture.cursor();
    assert_eq!(
        final_cursor["phase"], "baseline-qualification-recorded",
        "{final_cursor}"
    );
    let final_attempts = final_cursor["attempts"].as_array().unwrap();
    assert_eq!(final_attempts.len(), 5, "{final_cursor}");
    assert_eq!(final_attempts[..3], ordinary[..], "{final_cursor}");
    assert_eq!(&final_attempts[3], failed_after, "{final_cursor}");
    assert_eq!(fs::read(&retained_path).unwrap(), failure_bytes);
    assert_eq!(fs::read(&assignment_path).unwrap(), assignment_bytes);
    assert_eq!(fs::read(&manifest_path).unwrap(), manifest_bytes);
    assert_eq!(directory_bytes(&source_root), packet_bytes);
    assert_eq!(
        final_cursor["baseline_qualification"]["accepted"], true,
        "{final_cursor}"
    );

    let row_path = PathBuf::from(
        final_cursor["baseline_qualification"]["row"]
            .as_str()
            .expect("the accepted workflow records its accounting row"),
    );
    let row: Value = serde_json::from_slice(&fs::read(&row_path).unwrap()).unwrap();
    assert_eq!(row["accepted"], true, "{row}");
    let interventions = row["interventions"].as_array().unwrap();
    assert_eq!(interventions.len(), 1, "{row}");
    assert_eq!(interventions[0]["attempt_id"], INITIAL_REVIEW, "{row}");
    assert_eq!(
        interventions[0]["replacement_attempt"], RECOVERY_REVIEW,
        "{row}"
    );
    assert_eq!(interventions[0]["pre_submission"], "native-marker", "{row}");
    assert_eq!(
        interventions[0]["evidence_sha256"],
        json!(build_identity::hash_bytes(&failure_bytes)),
        "{row}"
    );
    assert_eq!(
        interventions[0]["started_at"].as_f64(),
        Some(started_ms as f64 / 1000.0),
        "{row}"
    );
    assert_eq!(
        interventions[0]["ended_at"].as_f64(),
        Some(finished_ms as f64 / 1000.0),
        "{row}"
    );
    let native_runs = row["native_runs"].as_array().unwrap();
    let sessions: Vec<&str> = native_runs
        .iter()
        .filter_map(|run| run["session"].as_str())
        .collect();
    assert!(sessions.contains(&RECOVERY_SESSION), "{row}");
    assert!(!sessions.contains(&REVIEW_SESSION), "{row}");
    assert!(sessions.contains(&LEAD_SESSION), "{row}");
    assert!(sessions.contains(&EXECUTOR_SESSION), "{row}");
    assert_eq!(native_runs.len(), 3, "{row}");
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

    let interventions = row["interventions"].as_array().unwrap();
    assert_eq!(interventions.len(), 1, "{row}");
    let intervention = &interventions[0];
    assert_eq!(intervention["attempt_id"], INITIAL_REVIEW, "{row}");
    assert_eq!(
        intervention["kind"], "verified-pre-submission-reviewer-recovery",
        "{row}"
    );
    assert_eq!(intervention["role"], "semantic-review", "{row}");
    assert_eq!(intervention["state"], "failed", "{row}");
    assert_eq!(intervention["pre_submission"], "native-marker", "{row}");
    assert_eq!(
        intervention["replacement_attempt"], RECOVERY_REVIEW,
        "{row}"
    );
    assert_eq!(
        intervention["evidence_sha256"],
        json!(build_identity::hash_bytes(&failure_bytes)),
        "{row}"
    );
    let expected_started = started_ms as f64 / 1000.0;
    let expected_ended = finished_ms as f64 / 1000.0;
    assert_eq!(
        intervention["started_at"].as_f64().unwrap(),
        expected_started,
        "{row}"
    );
    assert_eq!(
        intervention["ended_at"].as_f64().unwrap(),
        expected_ended,
        "{row}"
    );
    assert!(expected_ended > expected_started, "{row}");
}
