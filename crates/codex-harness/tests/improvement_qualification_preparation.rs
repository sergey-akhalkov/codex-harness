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

const LEAD_SESSION: &str = "12345678-1234-4234-8234-123456789041";
const EXECUTOR_SESSION: &str = "12345678-1234-4234-8234-123456789042";
const REVIEW_SESSION: &str = "12345678-1234-4234-8234-123456789043";

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
    let fixture = Fixture::new("qualification-preparation");
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

fn path_field<'a>(value: &'a Value, pointer: &str) -> &'a Path {
    Path::new(
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("missing path at {pointer}: {value}")),
    )
}

#[test]
fn prepare_only_keeps_frozen_identity_then_resume_completes_acceptance() {
    let _serial = INSTALL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = prepare_fixture();
    let started = fixture.improve(&[
        "start",
        "--prepare-only",
        "--run",
        fixture.run.to_str().unwrap(),
        "--spec",
        fixture.spec.to_str().unwrap(),
    ]);
    assert!(started.status.success(), "{}", text(&started));

    let output = text(&started);
    assert!(
        output.contains(
            "prepare-only: model-free preparation complete; no model attempt was dispatched"
        ),
        "{output}"
    );
    assert!(
        output.contains("native first-use or hook consent"),
        "{output}"
    );
    assert!(
        output.contains(&format!(
            "next: codex-harness improve resume --run \"{}\"",
            fixture.run.display()
        )),
        "{output}"
    );

    let prepared = fixture.cursor();
    assert_eq!(prepared["phase"], "baseline-qualification", "{prepared}");
    assert_eq!(fixture.status_json()["supervision"], "once");
    assert!(
        prepared["attempts"].as_array().is_some_and(Vec::is_empty),
        "{prepared}"
    );
    let assignments = fixture.run.join("assignments");
    assert!(
        !assignments.exists() || fs::read_dir(&assignments).unwrap().next().is_none(),
        "preparation wrote an assignment file"
    );

    let qualification = &prepared["baseline_qualification"];
    let ordinary_home = path_field(qualification, "/home");
    let reviewer_home = path_field(qualification, "/semantic_review/home");
    let checkout = path_field(qualification, "/checkout");
    let task_copy = path_field(qualification, "/task_copy");
    let task_workspace = path_field(qualification, "/task_workspace");
    let runtime_path = path_field(qualification, "/runtime");
    let reviewer_runtime_path = path_field(qualification, "/semantic_review/runtime");
    for (label, path) in [
        ("ordinary home", ordinary_home),
        ("reviewer home", reviewer_home),
        ("dispatch checkout", checkout),
        ("frozen task copy", task_copy),
        ("oracle workspace", task_workspace),
    ] {
        assert!(path.exists(), "{label} is missing at {}", path.display());
        assert!(
            output.contains(&path.display().to_string()),
            "{label} is not reported in:\n{output}"
        );
    }
    assert_ne!(ordinary_home, reviewer_home);
    assert!(runtime_path.is_file());
    assert!(reviewer_runtime_path.is_file());
    assert!(checkout.join("global/orchestration.toml").is_file());
    assert!(!qualification["prepared_ms"].is_null(), "{prepared}");

    let prepared_ms = qualification["prepared_ms"].clone();
    let ordinary_runtime = fs::read(runtime_path).unwrap();
    let reviewer_runtime = fs::read(reviewer_runtime_path).unwrap();
    let cursor_path = fixture.run.join("cursor.json");
    let cursor_before_drift = fs::read(&cursor_path).unwrap();
    let prompt = fixture.spec.with_file_name("qualification-task.txt");
    let prompt_bytes = fs::read(&prompt).unwrap();
    fs::write(&prompt, b"changed task instructions\n").unwrap();
    let drifted = fixture.improve(&["resume", "--run", fixture.run.to_str().unwrap()]);
    assert!(!drifted.status.success(), "{}", text(&drifted));
    assert!(text(&drifted).contains("task prompt"), "{}", text(&drifted));
    assert_eq!(fs::read(&cursor_path).unwrap(), cursor_before_drift);
    assert!(
        fixture.cursor()["attempts"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    fs::write(&prompt, prompt_bytes).unwrap();

    let lead = resume_turn(
        &fixture,
        "lead-frame",
        LEAD_SESSION,
        "frame-1",
        &assignment(),
        "needs-correction",
    );
    assert!(lead.status.success(), "{}", text(&lead));
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(
        wait_last(&fixture)["observation"]["state"],
        "completed",
        "{}",
        text(&lead)
    );
    let after_lead = fixture.cursor();
    assert_eq!(
        after_lead["baseline_qualification"]["home"],
        qualification["home"]
    );
    assert_eq!(
        after_lead["baseline_qualification"]["checkout"],
        qualification["checkout"]
    );
    assert_eq!(
        after_lead["baseline_qualification"]["semantic_review"]["home"],
        qualification["semantic_review"]["home"]
    );
    assert_eq!(
        after_lead["baseline_qualification"]["prepared_ms"],
        prepared_ms
    );
    assert_eq!(fs::read(runtime_path).unwrap(), ordinary_runtime);
    assert_eq!(fs::read(reviewer_runtime_path).unwrap(), reviewer_runtime);

    let executor = resume_turn(
        &fixture,
        "executor",
        EXECUTOR_SESSION,
        "executor-1",
        "Implemented the task and committed the scoped solution.",
        "solved",
    );
    assert!(executor.status.success(), "{}", text(&executor));
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 2);
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
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 3);
    assert_eq!(wait_last(&fixture)["observation"]["state"], "completed");

    let acceptance = semantic_acceptance(&fixture);
    let semantic = resume_turn(
        &fixture,
        "lead-acceptance",
        REVIEW_SESSION,
        "semantic-review-1",
        &acceptance,
        "needs-correction",
    );
    assert!(semantic.status.success(), "{}", text(&semantic));
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 4, "{cursor}");
    assert_eq!(attempts[3]["role"], "semantic-review", "{cursor}");
    assert_eq!(attempts[3]["model"], "deepseek-flash", "{cursor}");
    assert_eq!(wait_last(&fixture)["observation"]["state"], "completed");

    let settled = fixture.resume();
    assert!(settled.status.success(), "{}", text(&settled));
    let accepted = fixture.cursor();
    assert_eq!(
        accepted["phase"], "baseline-qualification-recorded",
        "{accepted}"
    );
    assert_eq!(
        accepted["baseline_qualification"]["workflow"]["completed"], true,
        "{accepted}"
    );
    assert_eq!(
        accepted["baseline_qualification"]["accepted"], true,
        "{accepted}"
    );
    assert_eq!(
        accepted["baseline_qualification"]["semantic_review"]["judgment_kind"], "accept",
        "{accepted}"
    );
    assert_eq!(
        accepted["baseline_qualification"]["prepared_ms"],
        prepared_ms
    );
    assert_eq!(fs::read(runtime_path).unwrap(), ordinary_runtime);
    assert_eq!(fs::read(reviewer_runtime_path).unwrap(), reviewer_runtime);
    let row_path = path_field(&accepted["baseline_qualification"], "/row");
    let row: Value = serde_json::from_slice(&fs::read(row_path).unwrap()).unwrap();
    assert_eq!(row["accepted"], true, "{row}");
    let native_runs = row["native_runs"].as_array().unwrap();
    assert_eq!(native_runs.len(), 3, "{row}");
    assert_eq!(
        native_runs
            .iter()
            .map(|run| run["attempts"].as_array().unwrap().len())
            .sum::<usize>(),
        4,
        "{row}"
    );
}

#[test]
fn prepare_only_refuses_incompatible_modes_before_run_state_is_created() {
    let _serial = INSTALL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let with_baseline = prepare_fixture();
    let continuous = with_baseline.improve(&[
        "start",
        "--prepare-only",
        "--supervision",
        "continuous",
        "--run",
        with_baseline.run.to_str().unwrap(),
        "--spec",
        with_baseline.spec.to_str().unwrap(),
    ]);
    assert!(!continuous.status.success(), "{}", text(&continuous));
    assert!(
        text(&continuous).contains("single-step preparation"),
        "{}",
        text(&continuous)
    );
    assert!(!with_baseline.run.join("spec.json").exists());
    assert!(!with_baseline.run.join("cursor.json").exists());

    let without_baseline = Fixture::new("qualification-preparation-no-baseline");
    let missing = without_baseline.improve(&[
        "start",
        "--prepare-only",
        "--supervision",
        "once",
        "--run",
        without_baseline.run.to_str().unwrap(),
        "--spec",
        without_baseline.spec.to_str().unwrap(),
    ]);
    assert!(!missing.status.success(), "{}", text(&missing));
    assert!(
        text(&missing).contains("baseline_qualification declaration"),
        "{}",
        text(&missing)
    );
    assert!(!without_baseline.run.join("spec.json").exists());
    assert!(!without_baseline.run.join("cursor.json").exists());
}
