#![cfg(windows)]

#[path = "fixtures/improvement_comparison_common.rs"]
mod improvement_comparison_common;
use improvement_comparison_common::*;

use harness_core::build_identity;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const LEAD_SESSION: &str = "12345678-1234-4234-8234-123456789031";
const EXECUTOR_SESSION: &str = "12345678-1234-4234-8234-123456789032";
const REVIEW_SESSION: &str = "12345678-1234-4234-8234-123456789033";
const PACKET_INPUTS: [&str; 5] = [
    "task.txt",
    "criteria.txt",
    "submission.txt",
    "actions.json",
    "solution.txt",
];

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
    let fixture = Fixture::new("qualification-packet-recovery");
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

fn packet_source(fixture: &Fixture) -> PathBuf {
    fixture
        .run
        .join("baseline-qualification/reviewer")
        .join("source")
}

fn read_packet_inputs(source: &Path) -> BTreeMap<String, Vec<u8>> {
    PACKET_INPUTS
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                fs::read(source.join(name)).expect("the packet input is present"),
            )
        })
        .collect()
}

fn write_incomplete_packet(fixture: &Fixture, inputs: &BTreeMap<String, Vec<u8>>) -> PathBuf {
    let source = packet_source(fixture);
    if source.exists() {
        fs::remove_dir_all(&source).unwrap();
    }
    fs::create_dir_all(&source).unwrap();
    for name in PACKET_INPUTS {
        fs::write(
            source.join(name),
            inputs.get(name).expect("the retained input bytes"),
        )
        .unwrap();
    }
    git(&source, &["init", "-q", "--initial-branch=main"]);
    git(
        &source,
        &[
            "remote",
            "add",
            "origin",
            source.to_str().expect("the synthetic packet path is UTF-8"),
        ],
    );
    assert_eq!(
        git_output(&source, &["remote", "get-url", "origin"]),
        source.to_str().expect("the synthetic packet path is UTF-8")
    );
    assert_eq!(
        git_output(&source, &["symbolic-ref", "--short", "HEAD"]),
        "main"
    );
    assert!(git_output(&source, &["ls-files"]).is_empty());
    assert!(git_output(&source, &["for-each-ref", "--format=%(refname)"]).is_empty());
    assert!(!source.join("manifest.json").exists());
    let head = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .current_dir(&source)
        .output()
        .unwrap();
    assert!(
        !head.status.success(),
        "the preparation must have no commit"
    );
    assert!(head.stdout.is_empty(), "the preparation must have no HEAD");
    source
}

fn assert_packet_inputs(source: &Path, inputs: &BTreeMap<String, Vec<u8>>) {
    for name in PACKET_INPUTS {
        assert_eq!(
            fs::read(source.join(name)).unwrap(),
            inputs[name],
            "the packet input {name} must retain its exact bytes"
        );
    }
}

fn artifact_paths(attempt: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    for key in ["receipt", "result", "detail"] {
        if let Some(path) = attempt[key].as_str() {
            paths.push(path.to_owned());
        }
    }
    if let Some(path) = attempt["retained"]["receipt"].as_str() {
        paths.push(path.to_owned());
    }
    paths
}

fn artifact_bytes(paths: &[String]) -> Vec<(String, Option<Vec<u8>>)> {
    paths
        .iter()
        .map(|path| (path.clone(), fs::read(path).ok()))
        .collect()
}

fn assert_preparation_refused(fixture: &Fixture, ready: &Value, expected: &str) {
    fixture.write_cursor(ready);
    let acceptance = semantic_acceptance(fixture);
    let output = resume_turn(
        fixture,
        "lead-acceptance",
        REVIEW_SESSION,
        "preparation-refused",
        &acceptance,
        "needs-correction",
    );
    assert!(!output.status.success(), "{}", text(&output));
    assert!(text(&output).contains(expected), "{}", text(&output));
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 3, "{cursor}");
    assert!(
        !attempts
            .iter()
            .any(|attempt| attempt["role"] == "semantic-review"),
        "{cursor}"
    );
}

fn run_ordinary_workflow(fixture: &Fixture) -> Value {
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
            ("HARNESS_IMPROVEMENT_FIXTURE_TURN_SUFFIX", "lead-frame-1"),
            ("HARNESS_IMPROVEMENT_FIXTURE_RESULT", &assignment),
            (
                "HARNESS_IMPROVEMENT_FIXTURE_SOLUTION_TEXT",
                "needs-correction",
            ),
        ],
    );
    assert!(started.status.success(), "{}", text(&started));
    assert_eq!(wait_last(fixture)["observation"]["state"], "completed");

    let executor = resume_turn(
        fixture,
        "executor",
        EXECUTOR_SESSION,
        "executor-1",
        "Implemented the task and committed the scoped solution.",
        "solved",
    );
    assert!(executor.status.success(), "{}", text(&executor));
    assert_eq!(wait_last(fixture)["observation"]["state"], "completed");

    let review = resume_turn(
        fixture,
        "lead-review",
        LEAD_SESSION,
        "lead-review-1",
        "ACCEPT: The scoped result and applicable checks satisfy the original task.",
        "needs-correction",
    );
    assert!(review.status.success(), "{}", text(&review));
    assert_eq!(wait_last(fixture)["observation"]["state"], "completed");

    let terminal = fixture.cursor();
    assert_eq!(
        terminal["attempts"].as_array().unwrap().len(),
        3,
        "{terminal}"
    );
    terminal
}

fn harvest_complete_packet(fixture: &Fixture) -> BTreeMap<String, Vec<u8>> {
    let output = fixture.improve_with_env(
        &["resume", "--run", fixture.run.to_str().unwrap()],
        &[CONTROL_CHILD_MODE, ("HARNESS_EXECUTOR_SESSION", "1")],
    );
    assert!(!output.status.success(), "{}", text(&output));
    assert!(
        text(&output).contains("executor sessions cannot dispatch executors"),
        "{}",
        text(&output)
    );
    let source = packet_source(fixture);
    assert!(source.join("manifest.json").is_file());
    assert!(git_output(&source, &["rev-parse", "HEAD"]).len() >= 40);
    read_packet_inputs(&source)
}

fn retained_pre_packet_state(harvested: Value) -> Value {
    let mut ready = harvested;
    ready["attempts"]
        .as_array_mut()
        .unwrap()
        .retain(|attempt| attempt["role"] != "semantic-review");
    ready["condition"] = Value::Null;
    let semantic = ready["baseline_qualification"]["semantic_review"]
        .as_object_mut()
        .expect("the retained semantic review preparation");
    for field in [
        "source",
        "manifest",
        "manifest_sha256",
        "submission_revision",
        "review_revision",
        "participant",
        "initial_attempt",
        "correction_attempt",
        "judgment",
        "judgment_sha256",
        "judgment_kind",
        "findings",
        "unresolved_issues",
    ] {
        semantic.insert(field.to_owned(), Value::Null);
    }
    semantic.insert("correction_used".to_owned(), json!(false));
    ready
}

#[test]
fn interrupted_precommit_packet_recovers_once_and_preserves_refusals() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = prepare_fixture();
    let terminal = run_ordinary_workflow(&fixture);
    assert_eq!(terminal["attempts"].as_array().unwrap().len(), 3);

    // Harvest the exact bytes the packet owner generates without dispatching a
    // reviewer. The executor-session marker is refused by the visible dispatch
    // owner only after the accepted workflow and packet are persisted.
    let inputs = harvest_complete_packet(&fixture);
    let ready = retained_pre_packet_state(fixture.cursor());
    assert_eq!(ready["attempts"].as_array().unwrap().len(), 3, "{ready}");
    assert_eq!(
        ready["baseline_qualification"]["workflow"]["lead_accepted"], true,
        "{ready}"
    );
    let semantic = &ready["baseline_qualification"]["semantic_review"];
    for field in [
        "source",
        "manifest",
        "manifest_sha256",
        "submission_revision",
        "review_revision",
        "participant",
        "initial_attempt",
        "correction_attempt",
        "judgment",
        "judgment_kind",
    ] {
        assert!(semantic[field].is_null(), "{field} is null in {ready}");
    }
    assert!(semantic["binding"].is_object(), "{ready}");

    let ordinary_attempts = ready["attempts"].as_array().unwrap().clone();
    let mut ordinary_paths = Vec::new();
    for attempt in &ordinary_attempts {
        ordinary_paths.extend(artifact_paths(attempt));
    }
    let ordinary_artifacts = artifact_bytes(&ordinary_paths);

    fixture.write_cursor(&ready);
    let source = write_incomplete_packet(&fixture, &inputs);
    assert_packet_inputs(&source, &inputs);

    let mut changed = inputs.clone();
    changed.get_mut("actions.json").unwrap()[0] ^= 0x01;
    write_incomplete_packet(&fixture, &changed);
    assert_preparation_refused(
        &fixture,
        &ready,
        "differs from the current frozen packet bytes",
    );
    assert_eq!(
        fs::read(source.join("actions.json")).unwrap(),
        changed["actions.json"]
    );

    let mut malformed = inputs.clone();
    malformed.insert("submission.txt".to_owned(), b"{\n".to_vec());
    write_incomplete_packet(&fixture, &malformed);
    assert_preparation_refused(
        &fixture,
        &ready,
        "differs from the current frozen packet bytes",
    );
    assert_eq!(fs::read(source.join("submission.txt")).unwrap(), b"{\n");

    write_incomplete_packet(&fixture, &inputs);
    fs::write(source.join("unexpected.txt"), b"foreign preparation\n").unwrap();
    assert_preparation_refused(&fixture, &ready, "unexpected path");
    assert_eq!(
        fs::read(source.join("unexpected.txt")).unwrap(),
        b"foreign preparation\n"
    );
    assert_packet_inputs(&source, &inputs);

    write_incomplete_packet(&fixture, &inputs);
    let mut add = vec!["add", "--"];
    add.extend(PACKET_INPUTS);
    git(&source, &add);
    git(
        &source,
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "-q",
            "-m",
            "unexpected pre-manifest commit",
        ],
    );
    let committed = git_output(&source, &["rev-parse", "HEAD"]);
    assert_preparation_refused(&fixture, &ready, "commit without a manifest");
    assert_eq!(git_output(&source, &["rev-parse", "HEAD"]), committed);
    assert!(!source.join("manifest.json").exists());
    assert_packet_inputs(&source, &inputs);

    fixture.write_cursor(&ready);
    write_incomplete_packet(&fixture, &inputs);
    let dispatched = resume_turn(
        &fixture,
        "lead-acceptance",
        REVIEW_SESSION,
        "semantic-review-1",
        &semantic_acceptance(&fixture),
        "needs-correction",
    );
    assert!(dispatched.status.success(), "{}", text(&dispatched));
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 4, "{cursor}");
    assert_eq!(&attempts[..3], ordinary_attempts.as_slice(), "{cursor}");
    assert_eq!(artifact_bytes(&ordinary_paths), ordinary_artifacts);
    let semantic = attempts.last().expect("the reviewer attempt");
    assert_eq!(semantic["id"], "semantic-review-1", "{cursor}");
    assert_eq!(semantic["role"], "semantic-review", "{cursor}");
    assert_eq!(semantic["state"], "started", "{cursor}");

    let review = &cursor["baseline_qualification"]["semantic_review"];
    assert_eq!(PathBuf::from(review["source"].as_str().unwrap()), source);
    assert_eq!(review["initial_attempt"], "semantic-review-1");
    let head = git_output(&source, &["rev-parse", "HEAD"]);
    assert_eq!(review["review_revision"], head);
    assert_eq!(
        review["manifest_sha256"],
        build_identity::hash_file(&source.join("manifest.json")).unwrap()
    );
    assert_packet_inputs(&source, &inputs);

    let reviewer = wait_last(&fixture);
    assert_eq!(reviewer["observation"]["state"], "completed", "{reviewer}");
    assert_eq!(
        reviewer["observation"]["session"], REVIEW_SESSION,
        "{reviewer}"
    );
    let semantic_paths = artifact_paths(semantic);
    let semantic_artifacts = artifact_bytes(&semantic_paths);

    // A bound packet remains strict: changing an input after dispatch refuses
    // before the retained reviewer result can be consumed or replayed.
    let actions_path = source.join("actions.json");
    let actions_bytes = fs::read(&actions_path).unwrap();
    fs::write(&actions_path, b"{\"changed\":true}\n").unwrap();
    let refused = fixture.resume();
    assert!(!refused.status.success(), "{}", text(&refused));
    assert!(
        text(&refused).contains("changed after the packet was committed"),
        "{}",
        text(&refused)
    );
    let refused_cursor = fixture.cursor();
    assert_eq!(refused_cursor["attempts"].as_array().unwrap().len(), 4);
    assert_eq!(artifact_bytes(&semantic_paths), semantic_artifacts);
    assert_eq!(git_output(&source, &["rev-parse", "HEAD"]), head);
    fs::write(&actions_path, &actions_bytes).unwrap();

    let settled = fixture.resume();
    assert!(settled.status.success(), "{}", text(&settled));
    let settled_cursor = fixture.cursor();
    assert_eq!(
        settled_cursor["baseline_qualification"]["semantic_review"]["judgment_kind"], "accept",
        "{settled_cursor}"
    );
    let settled_attempts = settled_cursor["attempts"].as_array().unwrap();
    assert_eq!(settled_attempts.len(), 4, "{settled_cursor}");
    assert_eq!(&settled_attempts[..3], ordinary_attempts.as_slice());
    assert_eq!(artifact_bytes(&ordinary_paths), ordinary_artifacts);
    assert_eq!(artifact_bytes(&semantic_paths), semantic_artifacts);

    let repeated = fixture.resume();
    assert!(repeated.status.success(), "{}", text(&repeated));
    let repeated_cursor = fixture.cursor();
    let repeated_attempts = repeated_cursor["attempts"].as_array().unwrap();
    assert_eq!(repeated_attempts.len(), 4, "{repeated_cursor}");
    assert_eq!(&repeated_attempts[..3], ordinary_attempts.as_slice());
    assert_eq!(artifact_bytes(&ordinary_paths), ordinary_artifacts);
    assert_eq!(artifact_bytes(&semantic_paths), semantic_artifacts);
}
