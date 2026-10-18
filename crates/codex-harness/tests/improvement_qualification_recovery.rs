#![cfg(windows)]

#[path = "fixtures/improvement_comparison_common.rs"]
mod improvement_comparison_common;
use improvement_comparison_common::*;

use harness_core::build_identity;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Output,
};

const FIRST_SESSION: &str = "12345678-1234-4234-8234-123456789011";
const SECOND_SESSION: &str = "12345678-1234-4234-8234-123456789012";
const REFUSED_SESSION: &str = "12345678-1234-4234-8234-123456789013";

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
        "Read the frozen add-workload OpenSpec task and frame its implementation inside solution.txt and sleep_ms. Preserve its independent acceptance.",
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
        "workflow": {"schema":1,"lead_profile":"default","executor_profile":"ds","max_corrections":1,"capacity":2},
        "report": fixture.spec.with_file_name("qualification-result.txt")
    });
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    fixture
}

fn start_fixture(fixture: &Fixture, assignment: &str) -> Output {
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
            ("HARNESS_IMPROVEMENT_FIXTURE_RESULT", assignment),
        ],
    )
}

fn resume_fixture(fixture: &Fixture, session: &str, assignment: &str) -> Output {
    fixture.improve_with_env(
        &["resume", "--run", fixture.run.to_str().unwrap()],
        &[
            CONTROL_CHILD_MODE,
            ("HARNESS_IMPROVEMENT_FIXTURE_SCENARIO", "measured-workflow"),
            ("HARNESS_IMPROVEMENT_FIXTURE_ROLE", "lead-frame"),
            ("HARNESS_IMPROVEMENT_FIXTURE_SESSION", session),
            ("HARNESS_IMPROVEMENT_FIXTURE_TURN_SUFFIX", "recovery-frame"),
            ("HARNESS_IMPROVEMENT_FIXTURE_RESULT", assignment),
        ],
    )
}

fn install_legacy_failure(
    receipt: &Path,
    completed: &Value,
    mutate: impl FnOnce(&mut Value),
) -> Value {
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
    mutate(&mut value);
    if let Some(detail) = detail {
        fs::write(detail, []).unwrap();
    }
    if let Some(result) = result {
        let _ = fs::remove_file(result);
    }
    fs::write(receipt, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    value
}

fn assert_refused_without_retry(fixture: &Fixture, output: &Output) {
    assert!(output.status.success(), "{}", text(output));
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 1, "{cursor}\n{}", text(output));
    assert_eq!(attempts[0]["id"], "lead-frame-1", "{cursor}");
    assert_eq!(
        cursor["baseline_qualification"]["workflow"]["lead_frame"], "lead-frame-1",
        "{cursor}"
    );
    assert!(
        attempts[0]["binding"].get("pre_submission").is_none(),
        "refused evidence must not be recorded as pre-submission: {cursor}"
    );
}

#[test]
fn verified_pre_submission_failure_is_recovered_as_a_separate_lead_generation() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = prepare_fixture("qualification-pre-submission-recovery");
    let assignment = assignment();
    let started = start_fixture(&fixture, &assignment);
    assert!(started.status.success(), "{}", text(&started));

    let mut cursor = fixture.cursor();
    let first = cursor["attempts"]
        .as_array()
        .expect("the first lead frame was recorded")
        .first()
        .cloned()
        .expect("the first lead frame exists");
    assert_eq!(first["id"], "lead-frame-1", "{cursor}");
    assert_eq!(first["state"], "started", "{cursor}");
    let first_receipt = PathBuf::from(first["receipt"].as_str().expect("first receipt"));
    let completed: Value = wait_for_terminal_receipt(&first_receipt);
    assert_eq!(
        completed["observation"]["state"], "completed",
        "{completed}"
    );
    let first_owner = first["binding"]["owner"].as_str().unwrap().to_owned();
    let first_generation = first["binding"]["generation"].as_str().unwrap().to_owned();
    let first_session = completed["observation"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(first_session, FIRST_SESSION, "{completed}");
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 1, "{cursor}");

    // The failed dispatch happened long before this resume. Reconciliation
    // must not restart the configured retry window from the later read.
    cursor["attempts"][0]["updated_ms"] = json!(unix_ms_now() - 60_000);
    fixture.write_cursor(&cursor);
    let original_cursor = fixture.cursor();
    // Refusal counterexamples run before the supported recovery: they must
    // never dispatch, and the recovered dispatch below legitimately appends
    // the retry slot's own trust entry, which a reset cursor would no longer
    // authorize.
    let refused = original_cursor;

    fixture.write_cursor(&refused);
    install_legacy_failure(&first_receipt, &completed, |value| {
        value["observation"]["cause"] =
            json!("fixture provider error: synthetic failure before any turn");
    });
    assert_refused_without_retry(
        &fixture,
        &resume_fixture(&fixture, REFUSED_SESSION, &assignment),
    );

    fixture.write_cursor(&refused);
    install_legacy_failure(&first_receipt, &completed, |value| {
        value["observation"]["submission"] = json!("not-attempted");
        value["observation"]["turn"] = json!("turn-observed-before-settlement");
        value["observation"]["events"] = json!(1);
        value["observation"]["messages"] = json!(1);
        value["observation"]["rounds"] = json!(1);
    });
    assert_refused_without_retry(
        &fixture,
        &resume_fixture(&fixture, REFUSED_SESSION, &assignment),
    );

    fixture.write_cursor(&refused);
    let legacy = install_legacy_failure(&first_receipt, &completed, |_| {});
    let detail = PathBuf::from(
        legacy["observation"]["detail"]
            .as_str()
            .expect("the fixture receipt records its detail file"),
    );
    fs::write(&detail, b"{\"type\":\"turn.started\"}\n").unwrap();
    assert_refused_without_retry(
        &fixture,
        &resume_fixture(&fixture, REFUSED_SESSION, &assignment),
    );

    fixture.write_cursor(&refused);
    install_legacy_failure(&first_receipt, &completed, |value| {
        value["originatingLead"]["runGeneration"] = json!("foreign-generation");
    });
    assert_refused_without_retry(
        &fixture,
        &resume_fixture(&fixture, REFUSED_SESSION, &assignment),
    );

    // Restore the exact legacy attachment failure and resume: the retained
    // generation is settled as failed and only a separately bound retry is
    // dispatched.
    fixture.write_cursor(&refused);
    let legacy = install_legacy_failure(&first_receipt, &completed, |_| {});
    assert_eq!(legacy["originatingLead"]["runGeneration"], first_generation);
    let legacy_bytes = fs::read(&first_receipt).unwrap();

    let resumed = resume_fixture(&fixture, SECOND_SESSION, &assignment);
    assert!(resumed.status.success(), "{}", text(&resumed));
    let recovered = fixture.cursor();
    let attempts = recovered["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2, "{recovered}\n{}", text(&resumed));
    let retained_first = &attempts[0];
    assert_eq!(retained_first["id"], "lead-frame-1", "{recovered}");
    assert_eq!(retained_first["state"], "failed", "{recovered}");
    assert_eq!(
        retained_first["binding"]["pre_submission"], "legacy-frontend-attachment",
        "{recovered}"
    );
    assert_eq!(
        retained_first["binding"]["generation"], first_generation,
        "{recovered}"
    );
    assert_eq!(
        retained_first["binding"]["owner"], first_owner,
        "{recovered}"
    );
    assert_eq!(
        retained_first["binding"]["session"], first_session,
        "{recovered}"
    );
    assert!(
        retained_first["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("legacy classification")),
        "{recovered}"
    );
    let retained_receipt = PathBuf::from(
        retained_first["retained"]["receipt"]
            .as_str()
            .expect("the failed generation was retained"),
    );
    assert_eq!(fs::read(retained_receipt).unwrap(), legacy_bytes);

    let second = &attempts[1];
    assert_eq!(second["id"], "lead-frame-2", "{recovered}");
    assert_eq!(second["state"], "started", "{recovered}");
    assert_ne!(second["binding"]["owner"], first_owner, "{recovered}");
    assert_ne!(
        second["binding"]["generation"], first_generation,
        "{recovered}"
    );
    assert_ne!(second["binding"]["session"], first_session, "{recovered}");
    assert!(
        second["binding"]["session"].is_null()
            || second["binding"]["session"] == json!(SECOND_SESSION),
        "an observed session must be the separately dispatched one: {recovered}"
    );
    assert_eq!(
        recovered["baseline_qualification"]["workflow"]["lead_frame"], "lead-frame-2",
        "{recovered}"
    );
    let second_receipt = PathBuf::from(second["receipt"].as_str().expect("second receipt"));
    let second_terminal = wait_for_terminal_receipt(&second_receipt);
    assert_eq!(second_terminal["observation"]["state"], "completed");
    assert_eq!(
        second_terminal["observation"]["session"], SECOND_SESSION,
        "the retained retry runs the separately dispatched session: {second_terminal}"
    );
}
