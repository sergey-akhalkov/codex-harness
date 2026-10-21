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
    resume_run(
        fixture,
        &fixture.run,
        role,
        session,
        suffix,
        result,
        solution,
    )
}

fn resume_run(
    fixture: &Fixture,
    run: &Path,
    role: &str,
    session: &str,
    suffix: &str,
    result: &str,
    solution: &str,
) -> Output {
    fixture.improve_with_env(
        &["resume", "--run", run.to_str().unwrap()],
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
    wait_last_run(&fixture.run)
}

fn wait_last_run(run: &Path) -> Value {
    let cursor = run_cursor(run);
    let attempt = cursor["attempts"]
        .as_array()
        .and_then(|attempts| attempts.last())
        .expect("the dispatched attempt is recorded");
    let receipt = PathBuf::from(attempt["receipt"].as_str().expect("the attempt receipt"));
    wait_for_terminal_receipt(&receipt)
}

fn semantic_acceptance(fixture: &Fixture) -> String {
    semantic_acceptance_for(&fixture.spec)
}

fn semantic_acceptance_for(spec_path: &Path) -> String {
    let spec: Value = serde_json::from_slice(&fs::read(spec_path).unwrap()).unwrap();
    let run = spec["run"].as_str().expect("the fixture run identity");
    format!("{{\"run_id\":\"{run}-semantic-review\",\"findings\":[],\"unresolved_issues\":[]}}")
}

fn run_cursor(run: &Path) -> Value {
    serde_json::from_slice(&fs::read(run.join("cursor.json")).unwrap()).unwrap()
}

fn read_value(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// Path spelling is not identity: retained receipts may be serialized through
/// either separator. Compare the actual files the cursors name.
fn assert_same_retained_path(actual: &Value, expected: &Path, label: &str) {
    let actual = Path::new(
        actual
            .as_str()
            .unwrap_or_else(|| panic!("{label} is not a path: {actual}")),
    );
    assert_eq!(
        fs::canonicalize(actual).unwrap(),
        fs::canonicalize(expected).unwrap(),
        "{label}"
    );
}

fn assert_refused_before_effects(adopting: &AdoptingRun, output: &Output, needle: &str) {
    let output = text(output);
    assert!(
        !output.contains("prepare-only: model-free preparation complete"),
        "{output}"
    );
    assert!(output.contains(needle), "{output}");
    assert!(!adopting.report.exists(), "the refused run wrote a report");
    assert!(
        !adopting
            .run
            .join("baseline-qualification/workload")
            .exists(),
        "the refused run froze a task copy"
    );
    assert!(
        !adopting
            .run
            .join("baseline-qualification/checkout")
            .exists(),
        "the refused run created a checkout"
    );
    if adopting.run.join("cursor.json").is_file() {
        assert_eq!(
            run_cursor(&adopting.run)["attempts"],
            json!([]),
            "the refused run dispatched an attempt"
        );
    }
}

struct AdoptingRun {
    run: PathBuf,
    spec: PathBuf,
    report: PathBuf,
}

/// A sequential run gives every task, checkout, oracle, report and participant
/// context a fresh path while both runtime declarations name the completed
/// origin receipt and terminal cursor by exact identity.
fn adopting_run(fixture: &Fixture, name: &str, origin_cursor: &Path) -> AdoptingRun {
    let origin = read_value(origin_cursor);
    let ordinary_runtime = path_field(&origin, "/baseline_qualification/runtime").to_path_buf();
    let reviewer_runtime =
        path_field(&origin, "/baseline_qualification/semantic_review/runtime").to_path_buf();
    let mut spec = read_value(&fixture.spec);
    let run = fixture.root.join("runs").join(name);
    let report = fixture.root.join(format!("{name}-result.txt"));

    let mut request = read_value(&fixture.request);
    request["case_root"] = json!(fixture.root.join(format!("{name}-workspace")));
    let request_path = fixture.root.join(format!("{name}-acceptance.json"));
    fs::write(&request_path, serde_json::to_vec_pretty(&request).unwrap()).unwrap();

    spec["run"] = json!(format!("qualification-{name}"));
    spec["baseline_qualification"]["report"] = json!(report);
    spec["baseline_qualification"]["acceptance"] = json!({
        "request": request_path,
        "request_sha256": build_identity::hash_file(&request_path).unwrap()
    });
    let ordinary_reuse = json!({
        "runtime": {
            "path": ordinary_runtime,
            "sha256": build_identity::hash_file(&ordinary_runtime).unwrap()
        },
        "cursor": {
            "path": origin_cursor,
            "sha256": build_identity::hash_file(origin_cursor).unwrap()
        }
    });
    let reviewer_reuse = json!({
        "runtime": {
            "path": reviewer_runtime,
            "sha256": build_identity::hash_file(&reviewer_runtime).unwrap()
        },
        "cursor": {
            "path": origin_cursor,
            "sha256": build_identity::hash_file(origin_cursor).unwrap()
        }
    });
    spec["baseline_qualification"]["runtime"]["reuse"] = ordinary_reuse;
    spec["baseline_qualification"]["semantic_review"]["runtime"]["reuse"] = reviewer_reuse;

    let spec_path = fixture.root.join(format!("run-spec-{name}.json"));
    fs::write(&spec_path, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    AdoptingRun {
        run,
        spec: spec_path,
        report,
    }
}

fn start_adopting(fixture: &Fixture, adopting: &AdoptingRun) -> Output {
    fixture.improve(&[
        "start",
        "--prepare-only",
        "--run",
        adopting.run.to_str().unwrap(),
        "--spec",
        adopting.spec.to_str().unwrap(),
    ])
}

fn mutate_spec(adopting: &AdoptingRun, mutate: impl FnOnce(&mut Value)) {
    let mut spec = read_value(&adopting.spec);
    mutate(&mut spec);
    fs::write(&adopting.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
}

fn complete_qualification(
    fixture: &Fixture,
    run: &Path,
    spec: &Path,
    lead_session: &str,
    executor_session: &str,
    review_session: &str,
) {
    let lead = resume_run(
        fixture,
        run,
        "lead-frame",
        lead_session,
        "frame-1",
        &assignment(),
        "needs-correction",
    );
    assert!(lead.status.success(), "{}", text(&lead));
    assert_eq!(wait_last_run(run)["observation"]["state"], "completed");

    let executor = resume_run(
        fixture,
        run,
        "executor",
        executor_session,
        "executor-1",
        "Implemented the task and committed the scoped solution.",
        "solved",
    );
    assert!(executor.status.success(), "{}", text(&executor));
    assert_eq!(wait_last_run(run)["observation"]["state"], "completed");

    let review = resume_run(
        fixture,
        run,
        "lead-review",
        lead_session,
        "lead-review-1",
        "ACCEPT: The scoped result and applicable checks satisfy the original task.",
        "needs-correction",
    );
    assert!(review.status.success(), "{}", text(&review));
    assert_eq!(wait_last_run(run)["observation"]["state"], "completed");

    let semantic = resume_run(
        fixture,
        run,
        "lead-acceptance",
        review_session,
        "semantic-review-1",
        &semantic_acceptance_for(spec),
        "needs-correction",
    );
    assert!(semantic.status.success(), "{}", text(&semantic));
    assert_eq!(wait_last_run(run)["observation"]["state"], "completed");

    let settled = fixture.improve(&["resume", "--run", run.to_str().unwrap()]);
    assert!(settled.status.success(), "{}", text(&settled));
    let cursor = run_cursor(run);
    assert_eq!(
        cursor["phase"], "baseline-qualification-recorded",
        "{cursor}"
    );
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

fn completed_fixture(_name: &str) -> Fixture {
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
    complete_qualification(
        &fixture,
        &fixture.run,
        &fixture.spec,
        LEAD_SESSION,
        EXECUTOR_SESSION,
        REVIEW_SESSION,
    );
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["phase"], "baseline-qualification-recorded",
        "{cursor}"
    );
    assert_eq!(
        cursor["baseline_qualification"]["workflow"]["completed"], true,
        "{cursor}"
    );
    fixture
}

fn adoption_claim_path(runtime: &Path) -> PathBuf {
    let name = runtime.file_name().unwrap().to_string_lossy();
    runtime.with_file_name(format!("{name}.adoption.json"))
}

fn adoption_claim_statuses(runtime: &Path, run: &str) -> Vec<String> {
    let path = adoption_claim_path(runtime);
    if !path.is_file() {
        return Vec::new();
    }
    read_value(&path)["claims"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|claim| claim["run"].as_str() == Some(run))
        .map(|claim| claim["status"].as_str().unwrap().to_owned())
        .collect()
}

fn trusted_hash_count(config: &str) -> usize {
    config.match_indices("trusted_hash").count()
}

/// Update both role declarations' retained file digest after the test mutates
/// one exact file. The digest itself remains the authority; the test only keeps
/// the caller declaration current so the intended gate is exercised.
fn refresh_reuse_digest(spec_path: &Path, reuse_pointer: &str, field: &str, sha256: &str) {
    let mut spec = read_value(spec_path);
    spec.pointer_mut(reuse_pointer).unwrap()[field]["sha256"] = json!(sha256);
    fs::write(spec_path, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
}

/// Change the one native hook approval already present, or add a changed
/// approval for this arm's exact hook key when the retained configuration has
/// none yet. Either form must stop matching the prepared configuration.
fn mutate_hook_trust(text: &str, key: &str) -> String {
    if let Some(start) = text.find("trusted_hash = \"") {
        let value_start = start + "trusted_hash = \"".len();
        let value_end = value_start + text[value_start..].find('"').unwrap();
        let mut changed = String::with_capacity(text.len());
        changed.push_str(&text[..value_start]);
        changed.push_str("sha256:changed");
        changed.push_str(&text[value_end..]);
        changed
    } else {
        format!("{text}\n[hooks.state.'{key}']\ntrusted_hash = \"sha256:changed\"\n")
    }
}

/// Copy one completed origin cursor and its frozen spec into a new directory so
/// provenance-link refusals can be exercised without touching the origin.
fn synthetic_origin_cursor(fixture: &Fixture, name: &str) -> PathBuf {
    let directory = fixture.root.join(format!("synthetic-{name}"));
    fs::create_dir_all(&directory).unwrap();
    let cursor = directory.join("cursor.json");
    fs::copy(fixture.run.join("cursor.json"), &cursor).unwrap();
    fs::copy(fixture.run.join("spec.json"), directory.join("spec.json")).unwrap();
    cursor
}

fn write_synthetic_adoption(cursor_path: &Path, adoption: Value) {
    let mut cursor = read_value(cursor_path);
    cursor["baseline_qualification"]["adoption"] = adoption;
    fs::write(cursor_path, serde_json::to_vec_pretty(&cursor).unwrap()).unwrap();
}

/// A run that failed after acquiring its exclusive claim keeps that claim until
/// it settles; tests that intentionally reach post-claim gates must release it
/// before the next adopter runs.
fn settle_refused_claims(fixture: &Fixture, adopting: &AdoptingRun, ordinary_runtime: &Path) {
    let run = read_value(&adopting.spec)["run"]
        .as_str()
        .unwrap()
        .to_owned();
    if !adoption_claim_statuses(ordinary_runtime, &run)
        .iter()
        .any(|status| status == "active")
    {
        return;
    }
    let stopped = fixture.improve(&["stop", "--run", adopting.run.to_str().unwrap()]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    assert_eq!(
        adoption_claim_statuses(ordinary_runtime, &run),
        ["stopped"],
        "the refused run did not settle its claim"
    );
}

#[test]
fn sequential_adoption_reuses_homes_and_preserves_provenance() {
    let _serial = INSTALL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = completed_fixture("sequential-adoption");
    let origin = fixture.cursor();
    let origin_checkout = path_field(&origin["baseline_qualification"], "/checkout").to_path_buf();
    let origin_runtime = path_field(&origin["baseline_qualification"], "/runtime").to_path_buf();
    let origin_reviewer_runtime = path_field(
        &origin["baseline_qualification"]["semantic_review"],
        "/runtime",
    )
    .to_path_buf();
    let ordinary_home = path_field(&origin["baseline_qualification"], "/home").to_path_buf();
    let reviewer_home = path_field(
        &origin["baseline_qualification"]["semantic_review"],
        "/home",
    )
    .to_path_buf();
    let ordinary_config = ordinary_home.join("config.toml");
    let reviewer_config = reviewer_home.join("config.toml");
    let ordinary_installation = ordinary_home.join("harness/installation.json");
    let reviewer_installation = reviewer_home.join("harness/installation.json");
    let origin_cursor = fixture.run.join("cursor.json");
    let origin_cursor_bytes = fs::read(&origin_cursor).unwrap();
    let origin_spec_bytes = fs::read(fixture.run.join("spec.json")).unwrap();
    let origin_row = path_field(&origin["baseline_qualification"], "/row").to_path_buf();
    let origin_row_bytes = fs::read(&origin_row).unwrap();
    let ordinary_config_bytes = fs::read(&ordinary_config).unwrap();
    let reviewer_config_bytes = fs::read(&reviewer_config).unwrap();
    let ordinary_installation_bytes = fs::read(&ordinary_installation).unwrap();
    let reviewer_installation_bytes = fs::read(&reviewer_installation).unwrap();
    let origin_runtime_bytes = fs::read(&origin_runtime).unwrap();
    let origin_reviewer_runtime_bytes = fs::read(&origin_reviewer_runtime).unwrap();

    let second = adopting_run(&fixture, "sequential-second", &origin_cursor);
    let started = start_adopting(&fixture, &second);
    assert!(started.status.success(), "{}", text(&started));
    let output = text(&started);
    assert!(output.contains("adopted unchanged"), "{output}");
    assert!(
        output.contains("no installation, connection or trust write"),
        "{output}"
    );
    let prepared = run_cursor(&second.run);
    assert!(
        prepared["attempts"].as_array().is_some_and(Vec::is_empty),
        "{prepared}"
    );
    let qualification = &prepared["baseline_qualification"];
    assert_same_retained_path(
        &qualification["runtime"],
        &origin_runtime,
        "ordinary runtime",
    );
    assert_same_retained_path(&qualification["home"], &ordinary_home, "ordinary home");
    assert_same_retained_path(
        &qualification["semantic_review"]["runtime"],
        &origin_reviewer_runtime,
        "reviewer runtime",
    );
    assert_same_retained_path(
        &qualification["semantic_review"]["home"],
        &reviewer_home,
        "reviewer home",
    );
    for field in ["checkout", "task_copy"] {
        let path = path_field(qualification, &format!("/{field}"));
        assert!(
            path.starts_with(&second.run),
            "{field} is not fresh: {}",
            path.display()
        );
        assert!(path.exists(), "{field} is missing: {}", path.display());
    }
    let task_workspace = path_field(qualification, "/task_workspace");
    assert_eq!(
        fs::canonicalize(task_workspace).unwrap(),
        fs::canonicalize(fixture.root.join("sequential-second-workspace")).unwrap(),
        "the oracle workspace is not the fresh declared case root"
    );
    assert!(qualification["report"].is_null(), "{prepared}");
    assert!(qualification["oracle"].is_null(), "{prepared}");
    assert!(!second.report.exists());
    assert!(
        !second
            .run
            .join("baseline-qualification/runtime.json")
            .exists()
    );
    assert!(
        !second
            .run
            .join("baseline-qualification/reviewer/runtime.json")
            .exists()
    );
    assert_eq!(fs::read(&ordinary_config).unwrap(), ordinary_config_bytes);
    assert_eq!(fs::read(&reviewer_config).unwrap(), reviewer_config_bytes);
    assert_eq!(
        fs::read(&ordinary_installation).unwrap(),
        ordinary_installation_bytes
    );
    assert_eq!(
        fs::read(&reviewer_installation).unwrap(),
        reviewer_installation_bytes
    );
    assert_eq!(fs::read(&origin_runtime).unwrap(), origin_runtime_bytes);
    assert_eq!(
        fs::read(&origin_reviewer_runtime).unwrap(),
        origin_reviewer_runtime_bytes
    );
    assert_eq!(
        adoption_claim_statuses(&origin_runtime, "qualification-sequential-second"),
        ["active"]
    );
    assert_eq!(
        adoption_claim_statuses(&origin_reviewer_runtime, "qualification-sequential-second"),
        ["active"]
    );

    // An explicit stop settles the same-owner claim without retiring a home;
    // a later resume reactivates it and verifies consumption without installing.
    let stopped = fixture.improve(&["stop", "--run", second.run.to_str().unwrap()]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    assert!(
        text(&stopped).contains("status=stopped"),
        "{}",
        text(&stopped)
    );
    assert_eq!(
        adoption_claim_statuses(&origin_runtime, "qualification-sequential-second"),
        ["stopped"]
    );
    let before_resume = fs::read_to_string(&ordinary_config).unwrap();
    let lead = resume_run(
        &fixture,
        &second.run,
        "lead-frame",
        "12345678-1234-4234-8234-123456789051",
        "frame-1",
        &assignment(),
        "needs-correction",
    );
    assert!(lead.status.success(), "{}", text(&lead));
    assert_eq!(
        wait_last_run(&second.run)["observation"]["state"],
        "completed"
    );
    assert_eq!(
        adoption_claim_statuses(&origin_runtime, "qualification-sequential-second"),
        ["active"]
    );
    assert_eq!(
        fs::read(&ordinary_installation).unwrap(),
        ordinary_installation_bytes
    );
    assert_eq!(
        fs::read(&reviewer_installation).unwrap(),
        reviewer_installation_bytes
    );
    assert!(
        !second
            .run
            .join("baseline-qualification/runtime.json")
            .exists()
    );
    let after_resume = fs::read_to_string(&ordinary_config).unwrap();
    assert!(
        after_resume.starts_with(&before_resume),
        "adopted configuration was rewritten instead of only accepting workspace trust"
    );
    assert_eq!(
        trusted_hash_count(&after_resume),
        trusted_hash_count(&before_resume),
        "resume changed the approved hook trust"
    );

    let executor = resume_run(
        &fixture,
        &second.run,
        "executor",
        "12345678-1234-4234-8234-123456789052",
        "executor-1",
        "Implemented the task and committed the scoped solution.",
        "solved",
    );
    assert!(executor.status.success(), "{}", text(&executor));
    assert_eq!(
        wait_last_run(&second.run)["observation"]["state"],
        "completed"
    );
    let review = resume_run(
        &fixture,
        &second.run,
        "lead-review",
        "12345678-1234-4234-8234-123456789051",
        "lead-review-1",
        "ACCEPT: The scoped result and applicable checks satisfy the original task.",
        "needs-correction",
    );
    assert!(review.status.success(), "{}", text(&review));
    assert_eq!(
        wait_last_run(&second.run)["observation"]["state"],
        "completed"
    );
    let semantic = resume_run(
        &fixture,
        &second.run,
        "lead-acceptance",
        "12345678-1234-4234-8234-123456789053",
        "semantic-review-1",
        &semantic_acceptance_for(&second.spec),
        "needs-correction",
    );
    assert!(semantic.status.success(), "{}", text(&semantic));
    assert_eq!(
        wait_last_run(&second.run)["observation"]["state"],
        "completed"
    );
    let settled = fixture.improve(&["resume", "--run", second.run.to_str().unwrap()]);
    assert!(settled.status.success(), "{}", text(&settled));
    let second_cursor = run_cursor(&second.run);
    assert_eq!(
        second_cursor["phase"], "baseline-qualification-recorded",
        "{second_cursor}"
    );
    assert_eq!(
        adoption_claim_statuses(&origin_runtime, "qualification-sequential-second"),
        ["completed"]
    );
    assert_eq!(
        adoption_claim_statuses(&origin_reviewer_runtime, "qualification-sequential-second"),
        ["completed"]
    );
    assert_eq!(fs::read(&origin_cursor).unwrap(), origin_cursor_bytes);
    assert_eq!(
        fs::read(fixture.run.join("spec.json")).unwrap(),
        origin_spec_bytes
    );
    assert_eq!(fs::read(&origin_row).unwrap(), origin_row_bytes);
    assert_eq!(
        fs::read(&ordinary_installation).unwrap(),
        ordinary_installation_bytes
    );
    assert_eq!(
        fs::read(&reviewer_installation).unwrap(),
        reviewer_installation_bytes
    );

    // The third run adopts the same retained receipt through the second run's
    // completed cursor. Its provenance must therefore include both checkouts.
    let second_checkout =
        path_field(&second_cursor["baseline_qualification"], "/checkout").to_path_buf();
    let second_cursor_path = second.run.join("cursor.json");
    let second_cursor_bytes = fs::read(&second_cursor_path).unwrap();
    let second_spec_bytes = fs::read(second.run.join("spec.json")).unwrap();
    let second_row = path_field(&second_cursor["baseline_qualification"], "/row").to_path_buf();
    let second_row_bytes = fs::read(&second_row).unwrap();
    let third = adopting_run(&fixture, "sequential-third", &second_cursor_path);
    let started = start_adopting(&fixture, &third);
    assert!(started.status.success(), "{}", text(&started));
    let third_cursor = run_cursor(&third.run);
    assert_eq!(
        third_cursor["phase"], "baseline-qualification",
        "{third_cursor}"
    );
    assert_same_retained_path(
        &third_cursor["baseline_qualification"]["runtime"],
        &origin_runtime,
        "third ordinary runtime",
    );
    assert_same_retained_path(
        &third_cursor["baseline_qualification"]["home"],
        &ordinary_home,
        "third ordinary home",
    );
    assert!(!third.report.exists());
    assert!(
        !third
            .run
            .join("baseline-qualification/runtime.json")
            .exists()
    );
    let provenance = third_cursor["baseline_qualification"]["adoption"]["trusted_workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| PathBuf::from(path.as_str().unwrap()))
        .collect::<Vec<_>>();
    assert!(provenance.contains(&origin_checkout), "{provenance:?}");
    assert!(provenance.contains(&second_checkout), "{provenance:?}");
    assert_eq!(fs::read(&origin_cursor).unwrap(), origin_cursor_bytes);
    assert_eq!(fs::read(&second_cursor_path).unwrap(), second_cursor_bytes);
    assert_eq!(
        fs::read(second.run.join("spec.json")).unwrap(),
        second_spec_bytes
    );
    assert_eq!(fs::read(&second_row).unwrap(), second_row_bytes);
    assert_eq!(
        fs::read(&ordinary_installation).unwrap(),
        ordinary_installation_bytes
    );
    assert_eq!(
        fs::read(&reviewer_installation).unwrap(),
        reviewer_installation_bytes
    );

    let stopped = fixture.improve(&["stop", "--run", third.run.to_str().unwrap()]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    assert_eq!(
        adoption_claim_statuses(&origin_runtime, "qualification-sequential-third"),
        ["stopped"]
    );
    assert_eq!(fs::read(&origin_cursor).unwrap(), origin_cursor_bytes);
    assert_eq!(fs::read(&second_cursor_path).unwrap(), second_cursor_bytes);
    assert_eq!(fs::read(&origin_row).unwrap(), origin_row_bytes);
    assert_eq!(fs::read(&second_row).unwrap(), second_row_bytes);
}

#[test]
fn adoption_refusals_precede_effects_and_preserve_existing_owners() {
    let _serial = INSTALL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixture = completed_fixture("adoption-refusals");
    let origin_cursor = fixture.run.join("cursor.json");
    let origin_spec = fixture.run.join("spec.json");
    let origin_cursor_bytes = fs::read(&origin_cursor).unwrap();
    let origin_spec_bytes = fs::read(&origin_spec).unwrap();
    let origin = read_value(&origin_cursor);
    let ordinary_runtime = path_field(&origin, "/baseline_qualification/runtime").to_path_buf();
    let reviewer_runtime =
        path_field(&origin, "/baseline_qualification/semantic_review/runtime").to_path_buf();
    let ordinary_runtime_sha = build_identity::hash_file(&ordinary_runtime).unwrap();
    let reviewer_runtime_sha = build_identity::hash_file(&reviewer_runtime).unwrap();
    let ordinary_runtime_bytes = fs::read(&ordinary_runtime).unwrap();
    let reviewer_runtime_bytes = fs::read(&reviewer_runtime).unwrap();
    let ordinary_home = path_field(&origin, "/baseline_qualification/home").to_path_buf();
    let reviewer_home =
        path_field(&origin, "/baseline_qualification/semantic_review/home").to_path_buf();
    let config = ordinary_home.join("config.toml");
    let reviewer_config = reviewer_home.join("config.toml");
    let config_bytes = fs::read(&config).unwrap();
    let reviewer_config_bytes = fs::read(&reviewer_config).unwrap();
    let installation = ordinary_home.join("harness/installation.json");
    let reviewer_installation = reviewer_home.join("harness/installation.json");
    let installation_bytes = fs::read(&installation).unwrap();
    let reviewer_installation_bytes = fs::read(&reviewer_installation).unwrap();
    let origin_cursor_sha = build_identity::hash_file(&origin_cursor).unwrap();
    let spec_digest = origin["spec_digest"].as_str().unwrap().to_owned();
    let declaration_digest = origin["baseline_qualification"]["declaration_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let origin_attempts = origin["attempts"].clone();

    macro_rules! refuse_spec {
        ($name:literal, $needle:literal, $mutate:expr) => {{
            let adopting = adopting_run(&fixture, $name, &origin_cursor);
            mutate_spec(&adopting, $mutate);
            let output = start_adopting(&fixture, &adopting);
            assert_refused_before_effects(&adopting, &output, $needle);
            assert_eq!(
                fs::read(&config).unwrap(),
                config_bytes,
                "{} changed the native configuration",
                $name
            );
            assert_eq!(
                fs::read(&installation).unwrap(),
                installation_bytes,
                "{} changed the installation",
                $name
            );
        }};
    }

    // A changed variant, client, private input or owned configuration source
    // must not borrow the origin's consent.
    refuse_spec!(
        "refuse-label",
        "frozen non-reuse inputs",
        |spec: &mut Value| spec["baseline_qualification"]["runtime"]["label"] =
            json!("changed-label")
    );
    refuse_spec!(
        "refuse-build",
        "frozen non-reuse inputs",
        |spec: &mut Value| spec["baseline_qualification"]["runtime"]["build"] = json!(fixture.proj)
    );
    refuse_spec!(
        "refuse-upstream",
        "frozen non-reuse inputs",
        |spec: &mut Value| spec["baseline_qualification"]["runtime"]["upstream"] =
            json!(fixture.request)
    );
    let review_config = fixture
        .spec
        .with_file_name("qualification-review-config.toml");
    let review_config_sha = build_identity::hash_file(&review_config).unwrap();
    refuse_spec!(
        "refuse-native-config",
        "frozen non-reuse inputs",
        |spec: &mut Value| spec["baseline_qualification"]["runtime"]["native_config"] =
            json!({"path": &review_config, "sha256": &review_config_sha})
    );
    refuse_spec!(
        "refuse-private-inputs",
        "frozen non-reuse inputs",
        |spec: &mut Value| spec["baseline_qualification"]["runtime"]["private_inputs"] =
            json!([{"source": &fixture.request, "destination": "additional-private.json"}])
    );

    // A different participant route is a different consent decision even when
    // the retained home and receipt are unchanged.
    refuse_spec!(
        "refuse-reviewer-binding",
        "differs from the completed origin",
        |spec: &mut Value| spec["baseline_qualification"]["semantic_review"]["profile"] =
            json!("default")
    );
    {
        let mut changed = origin.clone();
        changed["baseline_qualification"]["executor_binding"]["model_provider"] =
            json!("foreign-provider");
        let changed_bytes = serde_json::to_vec_pretty(&changed).unwrap();
        fs::write(&origin_cursor, &changed_bytes).unwrap();
        let adopting = adopting_run(&fixture, "refuse-executor-binding", &origin_cursor);
        let output = start_adopting(&fixture, &adopting);
        assert_refused_before_effects(&adopting, &output, "differs from the completed origin");
        assert_eq!(fs::read(&origin_cursor).unwrap(), changed_bytes);
        fs::write(&origin_cursor, &origin_cursor_bytes).unwrap();
    }

    // Exact retained identities are caller inputs, not suggestions.
    {
        let adopting = adopting_run(&fixture, "refuse-cursor-digest", &origin_cursor);
        mutate_spec(&adopting, |spec: &mut Value| {
            for pointer in [
                "/baseline_qualification/runtime/reuse",
                "/baseline_qualification/semantic_review/runtime/reuse",
            ] {
                spec.pointer_mut(pointer).unwrap()["cursor"]["sha256"] = json!("0".repeat(64));
            }
        });
        let output = start_adopting(&fixture, &adopting);
        assert_refused_before_effects(
            &adopting,
            &output,
            "adopted qualification origin cursor changed",
        );
        assert_eq!(fs::read(&origin_cursor).unwrap(), origin_cursor_bytes);
    }

    for (name, state) in [
        ("refuse-active-attempt", "started"),
        ("refuse-unknown-attempt", "unknown"),
    ] {
        let mut changed = origin.clone();
        changed["attempts"][0]["state"] = json!(state);
        let changed_bytes = serde_json::to_vec_pretty(&changed).unwrap();
        fs::write(&origin_cursor, &changed_bytes).unwrap();
        let adopting = adopting_run(&fixture, name, &origin_cursor);
        let output = start_adopting(&fixture, &adopting);
        assert_refused_before_effects(&adopting, &output, "effect is not terminal");
        assert_eq!(fs::read(&origin_cursor).unwrap(), changed_bytes);
        fs::write(&origin_cursor, &origin_cursor_bytes).unwrap();
    }

    // The frozen spec and retained receipt are both digest-bound.
    {
        let adopting = adopting_run(&fixture, "refuse-changed-spec", &origin_cursor);
        let mut changed = read_value(&origin_spec);
        changed["hypothesis_item"] = json!("changed-origin-hypothesis");
        let changed = serde_json::to_vec_pretty(&changed).unwrap();
        fs::write(&origin_spec, &changed).unwrap();
        let output = start_adopting(&fixture, &adopting);
        assert_refused_before_effects(&adopting, &output, "spec changed after the origin started");
        assert_eq!(fs::read(&origin_spec).unwrap(), changed);
        fs::write(&origin_spec, &origin_spec_bytes).unwrap();
    }
    {
        let adopting = adopting_run(&fixture, "refuse-changed-receipt", &origin_cursor);
        let mut receipt = read_value(&ordinary_runtime);
        receipt["label"] = json!("changed-receipt-label");
        let changed = serde_json::to_vec_pretty(&receipt).unwrap();
        fs::write(&ordinary_runtime, &changed).unwrap();
        refresh_reuse_digest(
            &adopting.spec,
            "/baseline_qualification/runtime/reuse",
            "runtime",
            &build_identity::hash_file(&ordinary_runtime).unwrap(),
        );
        let output = start_adopting(&fixture, &adopting);
        assert_refused_before_effects(
            &adopting,
            &output,
            "adopted runtime label does not match the frozen declaration",
        );
        assert_eq!(
            fs::read(&ordinary_runtime).unwrap(),
            changed,
            "the refused run rewrote the retained receipt"
        );
        fs::write(&ordinary_runtime, &ordinary_runtime_bytes).unwrap();
    }

    // Only the exact retained hook and workspace approvals may remain in the
    // occupied home configuration.
    let runtime_receipt = read_value(&ordinary_runtime);
    let hook_key = runtime_receipt["components"]
        .as_array()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component["name"].as_str() == Some("token-workflow"))
        })
        .and_then(|component| component["links"].as_array())
        .and_then(|links| {
            links
                .iter()
                .find(|link| link["name"].as_str() == Some("hooks.json"))
        })
        .and_then(|link| link["destination"].as_str())
        .map(|destination| format!("{destination}:pre_tool_use:0:0"))
        .unwrap_or_else(|| "D:\\foreign\\hooks.json:pre_tool_use:0:0".to_owned());
    let refuse_config = |name: &str, needle: &str, mutate: &dyn Fn(&str) -> String| {
        let adopting = adopting_run(&fixture, name, &origin_cursor);
        let original = String::from_utf8(fs::read(&config).unwrap()).unwrap();
        let changed = mutate(&original);
        fs::write(&config, changed.as_bytes()).unwrap();
        let output = start_adopting(&fixture, &adopting);
        assert_refused_before_effects(&adopting, &output, needle);
        assert_eq!(
            fs::read(&config).unwrap(),
            changed.as_bytes(),
            "{name} rewrote the native configuration"
        );
        assert_eq!(
            fs::read(&installation).unwrap(),
            installation_bytes,
            "{name} changed the installation"
        );
        settle_refused_claims(&fixture, &adopting, &ordinary_runtime);
        fs::write(&config, &config_bytes).unwrap();
    };
    refuse_config(
        "refuse-config-drift",
        "arm configuration changed since preparation",
        &|text: &str| format!("{text}\n# changed after preparation\n"),
    );
    refuse_config(
        "refuse-extra-workspace-trust",
        "arm configuration changed since preparation",
        &|text: &str| {
            format!(
                "{text}\n[projects.'D:\\foreign-adoption-workspace']\ntrust_level = \"trusted\"\n"
            )
        },
    );
    refuse_config(
        "refuse-changed-hook-trust",
        "arm configuration changed since preparation",
        &|text: &str| mutate_hook_trust(text, &hook_key),
    );
    refuse_config(
        "refuse-extra-hook-trust",
        "arm configuration changed since preparation",
        &|text: &str| {
            format!(
                "{text}\n[hooks.state.'D:\\foreign\\hooks.json:pre_tool_use:0:0']\ntrusted_hash = \"sha256:foreign\"\n"
            )
        },
    );

    // The ordinary and reviewer homes are separate ownership domains.
    {
        let adopting = adopting_run(&fixture, "refuse-cross-role-ordinary", &origin_cursor);
        mutate_spec(&adopting, |spec: &mut Value| {
            spec["baseline_qualification"]["runtime"]["reuse"]["runtime"] =
                json!({"path": &reviewer_runtime, "sha256": &reviewer_runtime_sha});
        });
        let output = start_adopting(&fixture, &adopting);
        assert_refused_before_effects(&adopting, &output, "cross-role home sharing is refused");
    }
    {
        let adopting = adopting_run(&fixture, "refuse-cross-role-reviewer", &origin_cursor);
        mutate_spec(&adopting, |spec: &mut Value| {
            spec["baseline_qualification"]["semantic_review"]["runtime"]["reuse"]["runtime"] =
                json!({"path": &ordinary_runtime, "sha256": &ordinary_runtime_sha});
        });
        let output = start_adopting(&fixture, &adopting);
        assert_refused_before_effects(&adopting, &output, "cross-role home sharing is refused");
    }

    // Provenance must be derivable from frozen attempts. Missing, edited or
    // foreign links refuse; a self-referential record cannot retain a matching
    // digest, so it fails identity verification before traversal.
    let refuse_provenance = |name: &str, needle: &str, adoption: Value| {
        let cursor = synthetic_origin_cursor(&fixture, name);
        write_synthetic_adoption(&cursor, adoption);
        let adopting = adopting_run(&fixture, name, &cursor);
        let output = start_adopting(&fixture, &adopting);
        assert_refused_before_effects(&adopting, &output, needle);
        assert_eq!(fs::read(&origin_cursor).unwrap(), origin_cursor_bytes);
        assert_eq!(fs::read(&origin_spec).unwrap(), origin_spec_bytes);
        assert_eq!(fs::read(&ordinary_runtime).unwrap(), ordinary_runtime_bytes);
    };
    refuse_provenance(
        "refuse-missing-provenance",
        "missing or not an absolute retained file",
        json!({
            "role": "ordinary",
            "runtime": {"path": &ordinary_runtime, "sha256": &ordinary_runtime_sha},
            "origin_cursor": {
                "path": fixture.root.join("absent-provenance/cursor.json"),
                "sha256": "0".repeat(64)
            },
            "origin_spec_digest": &spec_digest,
            "origin_declaration_digest": &declaration_digest,
            "trusted_workspaces": []
        }),
    );
    refuse_provenance(
        "refuse-provenance-spec-digest",
        "origin spec digest changed",
        json!({
            "role": "ordinary",
            "runtime": {"path": &ordinary_runtime, "sha256": &ordinary_runtime_sha},
            "origin_cursor": {"path": &origin_cursor, "sha256": &origin_cursor_sha},
            "origin_spec_digest": "0".repeat(64),
            "origin_declaration_digest": &declaration_digest,
            "trusted_workspaces": []
        }),
    );
    refuse_provenance(
        "refuse-provenance-declaration-digest",
        "origin declaration changed",
        json!({
            "role": "ordinary",
            "runtime": {"path": &ordinary_runtime, "sha256": &ordinary_runtime_sha},
            "origin_cursor": {"path": &origin_cursor, "sha256": &origin_cursor_sha},
            "origin_spec_digest": &spec_digest,
            "origin_declaration_digest": "0".repeat(64),
            "trusted_workspaces": []
        }),
    );
    refuse_provenance(
        "refuse-edited-provenance",
        "provenance changed; derive it again",
        json!({
            "role": "ordinary",
            "runtime": {"path": &ordinary_runtime, "sha256": &ordinary_runtime_sha},
            "origin_cursor": {"path": &origin_cursor, "sha256": &origin_cursor_sha},
            "origin_spec_digest": &spec_digest,
            "origin_declaration_digest": &declaration_digest,
            "trusted_workspaces": []
        }),
    );
    refuse_provenance(
        "refuse-foreign-provenance",
        "provenance changed; derive it again",
        json!({
            "role": "ordinary",
            "runtime": {"path": &ordinary_runtime, "sha256": &ordinary_runtime_sha},
            "origin_cursor": {"path": &origin_cursor, "sha256": &origin_cursor_sha},
            "origin_spec_digest": &spec_digest,
            "origin_declaration_digest": &declaration_digest,
            "trusted_workspaces": [&fixture.proj]
        }),
    );
    {
        let cursor = synthetic_origin_cursor(&fixture, "self-provenance");
        let sha_before_adoption = build_identity::hash_file(&cursor).unwrap();
        write_synthetic_adoption(
            &cursor,
            json!({
                "role": "ordinary",
                "runtime": {"path": &ordinary_runtime, "sha256": &ordinary_runtime_sha},
                "origin_cursor": {"path": &cursor, "sha256": &sha_before_adoption},
                "origin_spec_digest": &spec_digest,
                "origin_declaration_digest": &declaration_digest,
                "trusted_workspaces": []
            }),
        );
        let adopting = adopting_run(&fixture, "refuse-self-provenance", &cursor);
        let output = start_adopting(&fixture, &adopting);
        assert_refused_before_effects(&adopting, &output, "retained file changed");
    }

    // A second adopter must refuse before its own task, checkout or dispatch
    // exists and must leave the first owner's claim and artifacts intact.
    let first = adopting_run(&fixture, "concurrent-first", &origin_cursor);
    let started = start_adopting(&fixture, &first);
    assert!(started.status.success(), "{}", text(&started));
    let first_cursor_bytes = fs::read(first.run.join("cursor.json")).unwrap();
    let claim_ledger = adoption_claim_path(&ordinary_runtime);
    let claim_ledger_bytes = fs::read(&claim_ledger).unwrap();
    let second = adopting_run(&fixture, "concurrent-second", &origin_cursor);
    let refused = start_adopting(&fixture, &second);
    assert_refused_before_effects(
        &second,
        &refused,
        "actively adopted by run qualification-concurrent-first",
    );
    assert_eq!(
        fs::read(&claim_ledger).unwrap(),
        claim_ledger_bytes,
        "the refused second adopter rewrote the claim ledger"
    );
    assert_eq!(
        fs::read(first.run.join("cursor.json")).unwrap(),
        first_cursor_bytes,
        "the first adopter's artifacts changed"
    );
    let stopped = fixture.improve(&["stop", "--run", first.run.to_str().unwrap()]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    assert_eq!(
        adoption_claim_statuses(&ordinary_runtime, "qualification-concurrent-first"),
        ["stopped"]
    );
    assert_eq!(
        adoption_claim_statuses(&reviewer_runtime, "qualification-concurrent-first"),
        ["stopped"]
    );

    // An interrupted preparation whose adoption state was not persisted still
    // settles through its frozen declaration, leaving no permanent lock.
    let interrupted = adopting_run(&fixture, "interrupted-claim", &origin_cursor);
    let started = start_adopting(&fixture, &interrupted);
    assert!(started.status.success(), "{}", text(&started));
    let mut cursor = read_value(&interrupted.run.join("cursor.json"));
    cursor["baseline_qualification"]
        .as_object_mut()
        .unwrap()
        .remove("adoption");
    if let Some(review) = cursor["baseline_qualification"]["semantic_review"].as_object_mut() {
        review.remove("adoption");
    }
    fs::write(
        interrupted.run.join("cursor.json"),
        serde_json::to_vec_pretty(&cursor).unwrap(),
    )
    .unwrap();
    let stopped = fixture.improve(&["stop", "--run", interrupted.run.to_str().unwrap()]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    assert!(
        text(&stopped).contains("status=stopped"),
        "{}",
        text(&stopped)
    );
    assert_eq!(
        adoption_claim_statuses(&ordinary_runtime, "qualification-interrupted-claim"),
        ["stopped"]
    );
    assert_eq!(
        adoption_claim_statuses(&reviewer_runtime, "qualification-interrupted-claim"),
        ["stopped"]
    );
    let recovery = adopting_run(&fixture, "interrupted-recovery", &origin_cursor);
    let started = start_adopting(&fixture, &recovery);
    assert!(started.status.success(), "{}", text(&started));
    let stopped = fixture.improve(&["stop", "--run", recovery.run.to_str().unwrap()]);
    assert!(stopped.status.success(), "{}", text(&stopped));

    assert_eq!(fs::read(&origin_cursor).unwrap(), origin_cursor_bytes);
    assert_eq!(fs::read(&origin_spec).unwrap(), origin_spec_bytes);
    assert_eq!(fs::read(&ordinary_runtime).unwrap(), ordinary_runtime_bytes);
    assert_eq!(fs::read(&reviewer_runtime).unwrap(), reviewer_runtime_bytes);
    assert_eq!(fs::read(&config).unwrap(), config_bytes);
    assert_eq!(fs::read(&reviewer_config).unwrap(), reviewer_config_bytes);
    assert_eq!(fs::read(&installation).unwrap(), installation_bytes);
    assert_eq!(
        fs::read(&reviewer_installation).unwrap(),
        reviewer_installation_bytes
    );
    assert_eq!(run_cursor(&fixture.run)["attempts"], origin_attempts);
}
