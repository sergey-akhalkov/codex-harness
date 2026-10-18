//! Native dispatch, short real-operation, calibration and nuisance-control
//! checks split from `improvement_comparison`.
#![cfg(windows)]

#[path = "fixtures/improvement_comparison_common.rs"]
mod improvement_comparison_common;
use improvement_comparison_common::*;

use harness_core::build_identity;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

/// One successful model-free measured pair through the ordinary control-backed
/// dispatch route: real visible dispatch, native frontend attachment, the
/// host's own lifecycle receipt, settlement from that receipt, independent
/// acceptance and a published Beads decision - no seeded success receipt.
#[test]
fn one_real_control_backed_pair_settles_through_native_observation() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("native-pair");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_real_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

    // First resume: preparation plus the real baseline dispatch.
    let resume = fixture.resume_dispatched(&[]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let cursor = fixture.cursor();
    let baseline = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["role"] == "baseline")
        .cloned()
        .unwrap_or_else(|| {
            panic!("the visible owner accepted the baseline attempt: {cursor}\n{output}")
        });
    let generation = baseline["binding"]["generation"]
        .as_str()
        .unwrap_or_else(|| {
            panic!("the accepted dispatch recorded its generation: {baseline}\n{output}")
        });
    assert_eq!(baseline["state"], "started", "{cursor}");
    let baseline_receipt = attempt_receipt(&fixture, "base-1");
    let record = wait_for_terminal_receipt(&baseline_receipt);
    assert_eq!(
        record["observation"]["state"], "completed",
        "the controlled conversation completed through the real host: {record}"
    );
    assert!(
        !record["observation"]["session"].is_null(),
        "the host recorded the native session: {record}"
    );

    // A stale generation is refused through the real receipt, then restored.
    let real_bytes = fs::read(&baseline_receipt).unwrap();
    let mut stale: Value = serde_json::from_slice(&real_bytes).unwrap();
    stale["originatingLead"]["runGeneration"] = json!("stale-generation");
    fs::write(
        &baseline_receipt,
        serde_json::to_vec_pretty(&stale).unwrap(),
    )
    .unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let cursor = fixture.cursor();
    let baseline_attempt = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == "base-1")
        .unwrap();
    assert!(
        baseline_attempt["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("generation")),
        "a stale generation is refused from the real receipt: {cursor}"
    );
    assert_eq!(
        cursor["comparison"]["baseline"]["accepted"],
        Value::Null,
        "no comparison result is derived from the stale generation"
    );
    fs::write(&baseline_receipt, &real_bytes).unwrap();

    // The harness's own trusted-project addition to the workspace this dispatch
    // allocated is accepted, but every other change - including an unrelated
    // trusted workspace - still blocks the post-attempt consumption instead of
    // entering the comparison.
    let arm_config = fixture.arm_dir("baseline").join("home").join("config.toml");
    let served = fs::read_to_string(&arm_config).unwrap();
    assert!(
        served.contains("trust_level = \"trusted\""),
        "the real dispatch trusted its bound workspace: {served}"
    );
    // The trusted entry names exactly the workspace the accepted dispatch
    // recorded on its own attempt, so the narrowed rule has real authority to
    // check against.
    let recorded_checkout = fixture.cursor()["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["role"] == "baseline")
        .and_then(|attempt| attempt["checkout"].as_str())
        .map(str::to_owned)
        .unwrap_or_default();
    assert!(
        served
            .to_ascii_lowercase()
            .contains(&recorded_checkout.to_ascii_lowercase()),
        "the arm configuration carries the workspace the accepted dispatch recorded ({recorded_checkout}): {served}"
    );
    let unrelated_trust =
        format!("{served}\n[projects.'c:\\unrelated-workspace']\ntrust_level = \"trusted\"\n");
    let changed_level = served.replace("trust_level = \"trusted\"", "trust_level = \"untrusted\"");
    let extra_setting = format!("{served}\n[fixture-drift]\nvalue = 1\n");
    let model_drift = served.replace("model = \"fixture-glyph-1\"", "model = \"fixture-glyph-2\"");
    for (name, mutated) in [
        ("an unrelated trusted-project workspace", unrelated_trust),
        ("a changed trust level", changed_level),
        ("an extra configuration key", extra_setting),
        ("model drift in the arm configuration", model_drift),
    ] {
        fs::write(&arm_config, &mutated).unwrap();
        let resume = fixture.resume();
        let output = text(&resume);
        assert!(resume.status.success(), "{name}: {output}");
        assert!(
            output.contains("the arm configuration changed since preparation"),
            "{name} still refuses consumption: {output}"
        );
        assert_eq!(
            fixture.cursor()["comparison"]["baseline"]["accepted"],
            Value::Null,
            "{name} cannot enter the comparison"
        );
    }
    fs::write(&arm_config, &served).unwrap();

    // Restoring the accepted generation settles the baseline from its own
    // receipt, runs the independent oracle, and dispatches the candidate.
    let resume = fixture.resume_dispatched(&[]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "the real baseline solution passed the frozen oracle: {status}\n{output}"
    );
    let candidate_receipt = attempt_receipt(&fixture, "cand-1");
    let record = wait_for_terminal_receipt(&candidate_receipt);
    assert_eq!(record["observation"]["state"], "completed", "{record}");

    // The candidate settles, is independently checked and the frozen policy
    // publishes its decision.
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );
    let comments =
        harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, &fixture.card)
            .unwrap();
    let records = harness_core::benefit_gate::parse_gate_comments(&comments);
    let assessment = harness_core::benefit_gate::assess(&records, &fixture.card)
        .expect("a decision is published");
    assert!(
        assessment
            .latest
            .revisions
            .as_deref()
            .is_some_and(|revisions| revisions == format!("{base}..{}", checkout.revision)),
        "the published decision carries the exact evaluated revisions: {comments:?}"
    );
    // Both arms settled from real host receipts, and each arm's committed
    // solution was verified independently; nothing was seeded.
    let cursor = fixture.cursor();
    for (id, _role) in [("base-1", "baseline"), ("cand-1", "candidate")] {
        let attempt = cursor["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|attempt| attempt["id"] == id)
            .unwrap();
        assert_eq!(attempt["state"], "completed", "{id}: {cursor}");
        assert!(
            attempt["retained"]["receipt_sha256"].is_string(),
            "the attempt settled from its own retained receipt: {cursor}"
        );
    }
    for arm in ["baseline", "candidate"] {
        let oracle: Value =
            serde_json::from_slice(&fs::read(fixture.arm_dir(arm).join("oracle.json")).unwrap())
                .unwrap();
        assert_eq!(oracle["executed"], true, "{arm}: {oracle}");
        assert_eq!(oracle["checker_executed"], true, "{arm}: {oracle}");
        assert_eq!(oracle["passed"], true, "{arm}: {oracle}");
        assert!(
            status["comparison"][arm]["revision"].is_string(),
            "{arm}: the verified revision is retained: {status}"
        );
    }
    assert!(!generation.is_empty());
}

#[test]
fn baseline_qualification_frames_the_frozen_task_with_distinct_native_profiles() {
    baseline_workflow_case(true, None);
}

#[test]
fn baseline_qualification_requires_the_independent_oracle_despite_lead_acceptance() {
    baseline_workflow_case(false, None);
}

#[test]
fn baseline_qualification_consumes_an_independent_semantic_reviewer() {
    baseline_workflow_case(true, Some(SemanticReviewCase::Accept));
}

#[test]
fn baseline_qualification_rejects_a_substantive_semantic_finding() {
    baseline_workflow_case(true, Some(SemanticReviewCase::Reject));
}

#[test]
fn baseline_qualification_corrects_one_malformed_reviewer_result() {
    baseline_workflow_case(true, Some(SemanticReviewCase::MalformedThenAccept));
}

#[test]
fn baseline_qualification_refuses_an_unknown_semantic_judgment() {
    baseline_workflow_case(true, Some(SemanticReviewCase::Unknown));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SemanticReviewCase {
    Accept,
    Reject,
    MalformedThenAccept,
    Unknown,
}

fn baseline_workflow_case(
    independent_acceptance_passes: bool,
    review_case: Option<SemanticReviewCase>,
) {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("qualification-native-frame");
    let launcher = std::path::PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
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
    fs::write(&config, "model = 'gpt-6-astra'\nmodel_provider = 'openai'\nmodel_reasoning_effort = 'xhigh'\n[profiles.ds]\nmodel = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n").unwrap();
    let prompt = fixture.spec.with_file_name("qualification-task.txt");
    fs::write(&prompt, "Read the frozen add-workload OpenSpec task and frame its implementation inside solution.txt and sleep_ms. Preserve its independent acceptance.").unwrap();
    let review_run_id = format!("{}-semantic-review", spec["run"].as_str().unwrap());
    let reviewer = review_case.map(|case| {
        let config = fixture
            .spec
            .with_file_name("qualification-review-config.toml");
        fs::write(
            &config,
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
        let accepted = format!(
            "{{\"run_id\":\"{review_run_id}\",\"findings\":[],\"unresolved_issues\":[]}}"
        );
        // The rejection finding binds a declared physical location in its
        // evidence but paraphrases the content instead of quoting the line.
        let initial = match case {
            SemanticReviewCase::Accept => accepted.clone(),
            SemanticReviewCase::Reject => format!(
                "{{\"run_id\":\"{review_run_id}\",\"findings\":[{{\"path\":\"solution.txt\",\"line\":1,\"description\":\"the submitted content does not demonstrate the frozen workload result\",\"evidence\":\"solution.txt:1 is the only submitted content and shows no implemented result\"}}],\"unresolved_issues\":[]}}"
            ),
            SemanticReviewCase::MalformedThenAccept => format!(
                "{{\"run_id\":\"{review_run_id}\",\"findings\":[{{\"path\":\"solution.txt\",\"line\":1,\"description\":\"the submitted content does not demonstrate the frozen workload result\"}}],\"unresolved_issues\":[]}}"
            ),
            SemanticReviewCase::Unknown => format!(
                "{{\"run_id\":\"{review_run_id}\",\"findings\":[],\"unresolved_issues\":[\"the frozen task criterion cannot be judged from the supplied packet\"]}}"
            ),
        };
        let corrected =
            (case == SemanticReviewCase::MalformedThenAccept).then(|| accepted.clone());
        (config, criteria, initial, corrected)
    });
    spec["local_runner"] = Value::Null;
    spec["qualification"] = Value::Null;
    spec["baseline_qualification"] = json!({
        "schema": 1,
        "task": previous["task"],
        "task_prompt": {"path": prompt, "sha256": build_identity::hash_file(&prompt).unwrap()},
        "runtime": {
            "state": fixture.state, "build": fixture.state.join("builds/h-build"),
            "label": "accepted-native-runtime", "upstream": fixture.upstream,
            "native_config": {"path": config, "sha256": build_identity::hash_file(&config).unwrap()},
            "private_inputs": []
        },
        "acceptance": previous["acceptance"],
        "workflow": {"schema":1,"lead_profile":"default","executor_profile":"ds","max_corrections":1,"capacity":2},
        "report": fixture.spec.with_file_name("qualification-result.txt")
    });
    if let Some((config, criteria, _, _)) = &reviewer {
        spec["baseline_qualification"]["semantic_review"] = json!({
            "schema": 1,
            "runtime": {
                "state": fixture.state,
                "build": fixture.state.join("builds/h-build"),
                "label": "accepted-semantic-review-runtime",
                "upstream": fixture.upstream,
                "native_config": {
                    "path": config,
                    "sha256": build_identity::hash_file(config).unwrap()
                },
                "private_inputs": []
            },
            "profile": "ds",
            "criteria": {
                "path": criteria,
                "sha256": build_identity::hash_file(criteria).unwrap()
            },
            "sources": ["solution.txt"],
            "max_corrections": 1
        });
    }
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    let assignment = json!({
        "schema":1, "objective":"Implement the frozen workload in the bound checkout.",
        "inputs":[], "outputs":["solution.txt"], "invariants":["Preserve frozen acceptance."],
        "acceptance":["The independent workload check must pass."], "consumer":"qualification controller", "escalate":[]
    }).to_string();
    let environment = [
        CONTROL_CHILD_MODE,
        ("HARNESS_IMPROVEMENT_FIXTURE_SCENARIO", "measured-workflow"),
        ("HARNESS_IMPROVEMENT_FIXTURE_ROLE", "lead-frame"),
        (
            "HARNESS_IMPROVEMENT_FIXTURE_SESSION",
            "12345678-1234-4234-8234-123456789001",
        ),
        ("HARNESS_IMPROVEMENT_FIXTURE_TURN_SUFFIX", "frame"),
        ("HARNESS_IMPROVEMENT_FIXTURE_RESULT", assignment.as_str()),
    ];
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
        &environment,
    );
    assert!(started.status.success(), "{}", text(&started));
    let cursor = fixture.cursor();
    assert_eq!(cursor["phase"], "baseline-qualification", "{cursor}");
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 1, "{cursor}");
    let attempt = &cursor["attempts"][0];
    assert_eq!(attempt["role"], "lead-frame");
    assert_eq!(attempt["profile"], "default");
    assert_eq!(attempt["model"], "gpt-6-astra");
    assert_eq!(attempt["reasoning_effort"], "xhigh");
    let receipt = std::path::PathBuf::from(attempt["receipt"].as_str().unwrap());
    let terminal = wait_for_terminal_receipt(&receipt);
    assert_eq!(terminal["observation"]["state"], "completed", "{terminal}");

    let report_path = fixture.spec.with_file_name("qualification-result.txt");
    fs::write(&report_path, "unrelated report must survive").unwrap();
    let collision = fixture.resume();
    assert!(!collision.status.success(), "{}", text(&collision));
    assert!(
        text(&collision).contains("report already exists"),
        "{}",
        text(&collision)
    );
    assert_eq!(
        fs::read_to_string(&report_path).unwrap(),
        "unrelated report must survive"
    );
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 1);
    fs::remove_file(&report_path).unwrap();

    let original = fs::read(&receipt).unwrap();
    let mut foreign: Value = serde_json::from_slice(&original).unwrap();
    foreign["originatingLead"]["runGeneration"] = json!("unrelated-generation");
    fs::write(&receipt, serde_json::to_vec(&foreign).unwrap()).unwrap();
    let refused = fixture.resume();
    assert!(refused.status.success(), "{}", text(&refused));
    assert!(fixture.cursor()["baseline_qualification"]["workflow"]["lead"].is_null());
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 1);
    fs::write(&receipt, original).unwrap();

    let resume_turn_with_suffix = |role: &str, result: &str, suffix: &str| {
        let output = fixture.improve_with_env(
            &["resume", "--run", fixture.run.to_str().unwrap()],
            &[
                CONTROL_CHILD_MODE,
                ("HARNESS_IMPROVEMENT_FIXTURE_SCENARIO", "measured-workflow"),
                ("HARNESS_IMPROVEMENT_FIXTURE_ROLE", role),
                ("HARNESS_IMPROVEMENT_FIXTURE_RESULT", result),
                (
                    "HARNESS_IMPROVEMENT_FIXTURE_SESSION",
                    if role == "lead-review" {
                        "12345678-1234-4234-8234-123456789001"
                    } else if role == "lead-acceptance" {
                        "12345678-1234-4234-8234-123456789003"
                    } else {
                        "12345678-1234-4234-8234-123456789002"
                    },
                ),
                ("HARNESS_IMPROVEMENT_FIXTURE_TURN_SUFFIX", suffix),
                (
                    "HARNESS_IMPROVEMENT_FIXTURE_SOLUTION_TEXT",
                    if role == "correction" {
                        if independent_acceptance_passes {
                            "solved"
                        } else {
                            "still-broken"
                        }
                    } else {
                        "needs-correction"
                    },
                ),
            ],
        );
        assert!(output.status.success(), "{}", text(&output));
        output
    };
    let resume_turn = |role: &str, result: &str| {
        let suffix = format!(
            "attempt-{}",
            fixture.cursor()["attempts"].as_array().unwrap().len() + 1
        );
        resume_turn_with_suffix(role, result, &suffix)
    };
    let wait_last = || {
        let cursor = fixture.cursor();
        let attempt = cursor["attempts"].as_array().unwrap().last().unwrap();
        let receipt = std::path::PathBuf::from(attempt["receipt"].as_str().unwrap());
        let terminal = wait_for_terminal_receipt(&receipt);
        assert_eq!(terminal["observation"]["state"], "completed", "{terminal}");
    };
    resume_turn(
        "executor",
        "Implemented the task and committed the scoped solution.",
    );
    let cursor = fixture.cursor();
    let qualification = &cursor["baseline_qualification"];
    assert_eq!(qualification["lead_binding"]["model"], "gpt-6-astra");
    assert_eq!(qualification["executor_binding"]["model"], "deepseek-flash");
    assert_eq!(qualification["executor_binding"]["reasoning_effort"], "max");
    assert_eq!(qualification["workflow"]["lead"]["profile"], "default");
    assert!(
        qualification["workflow"]["lead"]["session"]
            .as_str()
            .is_some()
    );
    assert_eq!(qualification["workflow"]["executor"], Value::Null);
    assert!(qualification["accepted"].is_null());
    let retained = std::path::PathBuf::from(
        cursor["attempts"][0]["retained"]["result"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(fs::read_to_string(retained).unwrap(), assignment);
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 2);
    assert_eq!(cursor["attempts"][1]["profile"], "ds");
    assert_eq!(cursor["attempts"][1]["model"], "deepseek-flash");
    wait_last();
    resume_turn(
        "lead-review",
        "CORRECT: Address the one concrete scoped correction and rerun the check.",
    );
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 3);
    wait_last();
    let lead_session =
        fixture.cursor()["baseline_qualification"]["workflow"]["lead"]["session"].clone();
    resume_turn(
        "correction",
        "Applied the concrete correction and verified the task.",
    );
    let cursor = fixture.cursor();
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 4);
    assert_eq!(cursor["attempts"][2]["binding"]["session"], lead_session);
    let executor_session =
        cursor["baseline_qualification"]["workflow"]["executor"]["session"].clone();
    wait_last();
    resume_turn(
        "lead-review",
        "ACCEPT: The scoped result and applicable checks satisfy the original task.",
    );
    let cursor = fixture.cursor();
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 5);
    assert_eq!(
        cursor["attempts"][3]["binding"]["session"],
        executor_session
    );
    wait_last();
    if let Some((_, _, initial_result, corrected_result)) = &reviewer {
        resume_turn("lead-acceptance", initial_result);
        let cursor = fixture.cursor();
        assert_eq!(cursor["attempts"].as_array().unwrap().len(), 6, "{cursor}");
        let review_attempt = &cursor["attempts"][5];
        assert_eq!(review_attempt["role"], "semantic-review", "{cursor}");
        assert_eq!(review_attempt["profile"], "ds", "{cursor}");
        assert_eq!(review_attempt["model"], "deepseek-flash", "{cursor}");
        assert_eq!(review_attempt["state"], "started", "{cursor}");
        wait_last();
        if let Some(corrected) = corrected_result {
            let suffix = format!(
                "attempt-{}",
                fixture.cursor()["attempts"].as_array().unwrap().len() + 1
            );
            resume_turn_with_suffix("lead-acceptance", corrected, &suffix);
            let cursor = fixture.cursor();
            assert_eq!(cursor["attempts"].as_array().unwrap().len(), 7, "{cursor}");
            let corrected_attempt = &cursor["attempts"][6];
            assert_eq!(corrected_attempt["role"], "semantic-review", "{cursor}");
            assert_eq!(corrected_attempt["state"], "started", "{cursor}");
            wait_last();
        }
    }
    if review_case == Some(SemanticReviewCase::Unknown) {
        let (_, _, unknown_result, _) = reviewer.as_ref().unwrap();
        let refused = fixture.resume();
        assert!(!refused.status.success(), "{}", text(&refused));
        assert!(
            text(&refused).contains("unresolved issues"),
            "{}",
            text(&refused)
        );
        let cursor = fixture.cursor();
        assert_eq!(cursor["attempts"].as_array().unwrap().len(), 6, "{cursor}");
        let review_attempt = &cursor["attempts"][5];
        assert_eq!(review_attempt["state"], "completed", "{cursor}");
        assert_eq!(
            review_attempt["binding"]["session"], "12345678-1234-4234-8234-123456789003",
            "{cursor}"
        );
        let retained =
            std::path::PathBuf::from(review_attempt["retained"]["result"].as_str().unwrap());
        assert_eq!(
            fs::read_to_string(&retained).unwrap(),
            unknown_result.as_str()
        );
        let qualification = &cursor["baseline_qualification"];
        assert!(qualification["accepted"].is_null(), "{cursor}");
        assert!(qualification["row"].is_null(), "{cursor}");
        assert!(qualification["oracle"].is_null(), "{cursor}");
        assert_eq!(
            qualification["semantic_review"]["judgment_kind"], "unknown",
            "{cursor}"
        );
        assert_eq!(qualification["semantic_review"]["findings"], 0, "{cursor}");
        assert_eq!(
            qualification["semantic_review"]["unresolved_issues"], 1,
            "{cursor}"
        );
        let repeated = fixture.resume();
        assert!(!repeated.status.success(), "{}", text(&repeated));
        assert!(
            text(&repeated).contains("unresolved issues"),
            "{}",
            text(&repeated)
        );
        let repeated_cursor = fixture.cursor();
        assert_eq!(
            repeated_cursor["attempts"].as_array().unwrap().len(),
            6,
            "{repeated_cursor}"
        );
        let repeated_attempt = &repeated_cursor["attempts"][5];
        let repeated_retained =
            std::path::PathBuf::from(repeated_attempt["retained"]["result"].as_str().unwrap());
        assert_eq!(
            fs::read_to_string(&repeated_retained).unwrap(),
            unknown_result.as_str()
        );
        println!(
            "semantic-review evidence: case=Unknown run_id={review_run_id} attempts=1 session=12345678-1234-4234-8234-123456789003 retained={} judgment=unknown accepted=null row=none",
            retained.display()
        );
        let stopped = fixture.stop();
        assert!(stopped.status.success(), "{}", text(&stopped));
        return;
    }
    let expected_attempts = match review_case {
        None => 5,
        Some(SemanticReviewCase::MalformedThenAccept) => 7,
        Some(_) => 6,
    };
    let oracle_workspace = std::path::PathBuf::from(
        fixture.cursor()["baseline_qualification"]["task_workspace"]
            .as_str()
            .unwrap(),
    );
    let preserved = oracle_workspace.join("unrelated.txt");
    fs::write(&preserved, "preserve before acceptance").unwrap();
    let changed = fixture.resume();
    assert!(!changed.status.success(), "{}", text(&changed));
    assert!(
        text(&changed).contains("oracle workspace changed"),
        "{}",
        text(&changed)
    );
    assert_eq!(
        fs::read_to_string(&preserved).unwrap(),
        "preserve before acceptance"
    );
    let changed_cursor = fixture.cursor();
    assert!(changed_cursor["baseline_qualification"]["oracle"].is_null());
    assert_eq!(
        changed_cursor["attempts"].as_array().unwrap().len(),
        expected_attempts
    );
    if let Some((_, _, initial_result, corrected_result)) = &reviewer {
        let final_index = if corrected_result.is_some() { 6 } else { 5 };
        let final_result = corrected_result.as_ref().unwrap_or(initial_result);
        let review_attempt = &changed_cursor["attempts"][final_index];
        assert_eq!(review_attempt["state"], "completed", "{changed_cursor}");
        assert_eq!(
            review_attempt["binding"]["session"], "12345678-1234-4234-8234-123456789003",
            "{changed_cursor}"
        );
        let retained =
            std::path::PathBuf::from(review_attempt["retained"]["result"].as_str().unwrap());
        assert_eq!(fs::read_to_string(retained).unwrap(), final_result.as_str());
        let expected_kind = if review_case == Some(SemanticReviewCase::Reject) {
            "reject"
        } else {
            "accept"
        };
        assert_eq!(
            changed_cursor["baseline_qualification"]["semantic_review"]["judgment_kind"],
            expected_kind,
            "{changed_cursor}"
        );
        if corrected_result.is_some() {
            let first_attempt = &changed_cursor["attempts"][5];
            assert_eq!(first_attempt["state"], "completed", "{changed_cursor}");
            assert_eq!(
                first_attempt["binding"]["session"], "12345678-1234-4234-8234-123456789003",
                "{changed_cursor}"
            );
            let first_retained =
                std::path::PathBuf::from(first_attempt["retained"]["result"].as_str().unwrap());
            assert_eq!(
                fs::read_to_string(first_retained).unwrap(),
                initial_result.as_str()
            );
        }
    }
    fs::remove_file(&preserved).unwrap();
    let settled = fixture.resume();
    assert!(settled.status.success(), "{}", text(&settled));
    let cursor = fixture.cursor();
    assert_eq!(cursor["attempts"][4]["binding"]["session"], lead_session);
    let workflow = &cursor["baseline_qualification"]["workflow"];
    assert_eq!(workflow["completed"], true);
    assert_eq!(workflow["lead_accepted"], true);
    assert_eq!(workflow["corrections"].as_array().unwrap().len(), 1);
    assert_eq!(cursor["phase"], "baseline-qualification-recorded");
    let review_rejects = review_case == Some(SemanticReviewCase::Reject);
    let expected_accepted = independent_acceptance_passes && !review_rejects;
    assert_eq!(
        cursor["baseline_qualification"]["accepted"],
        expected_accepted
    );
    assert_ne!(lead_session, executor_session);
    let row_path =
        std::path::PathBuf::from(cursor["baseline_qualification"]["row"].as_str().unwrap());
    let row_bytes = fs::read(&row_path).unwrap();
    let row: Value = serde_json::from_slice(&row_bytes).unwrap();
    let expected_native_runs = if review_case.is_some() { 3 } else { 2 };
    let expected_responses = match review_case {
        None => 5,
        Some(SemanticReviewCase::MalformedThenAccept) => 7,
        Some(_) => 6,
    };
    let expected_total_tokens = expected_responses * 120;
    assert_eq!(
        row["native_runs"].as_array().unwrap().len(),
        expected_native_runs
    );
    assert_eq!(
        row["usage"]["total_tokens"], expected_total_tokens,
        "each distinct participant response is charged once: {row}"
    );
    assert_eq!(row["usage"]["response_count"], expected_responses, "{row}");
    assert_eq!(
        row["checks"][0]["passed"], independent_acceptance_passes,
        "{row}"
    );
    assert_eq!(row["accepted"], expected_accepted, "{row}");
    if review_case.is_some() {
        let semantic = row["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["id"] == "independent-semantic-review")
            .expect("the row retains the independent semantic judgment");
        assert_eq!(semantic["executed"], true, "{row}");
        assert_eq!(semantic["passed"], !review_rejects, "{row}");
        assert_eq!(
            semantic["findings"],
            if review_rejects { 1 } else { 0 },
            "{row}"
        );
        assert_eq!(semantic["unresolved_issues"], 0, "{row}");
        assert_eq!(
            cursor["baseline_qualification"]["semantic_review"]["judgment_kind"],
            if review_rejects { "reject" } else { "accept" }
        );
    }
    let oracle_path =
        std::path::PathBuf::from(cursor["baseline_qualification"]["oracle"].as_str().unwrap());
    let oracle_bytes = fs::read(&oracle_path).unwrap();
    let report =
        std::path::PathBuf::from(cursor["baseline_qualification"]["report"].as_str().unwrap());
    assert_eq!(
        fs::read_to_string(report).unwrap(),
        "Applied the concrete correction and verified the task."
    );
    let repeated = fixture.resume();
    assert!(repeated.status.success(), "{}", text(&repeated));
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        expected_attempts
    );
    assert_eq!(fs::read(&row_path).unwrap(), row_bytes);
    assert_eq!(fs::read(oracle_path).unwrap(), oracle_bytes);
    if let Some(case) = review_case {
        let reviewer_attempts: Vec<&Value> = cursor["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|attempt| attempt["role"] == "semantic-review")
            .collect();
        println!(
            "semantic-review evidence: case={case:?} run_id={review_run_id} attempts={} sessions={} retained={} native_runs={} responses={} tokens={} judgment={} accepted={} row={}",
            reviewer_attempts.len(),
            reviewer_attempts
                .iter()
                .map(|attempt| attempt["binding"]["session"].as_str().unwrap_or("?"))
                .collect::<Vec<_>>()
                .join(","),
            reviewer_attempts
                .iter()
                .map(|attempt| attempt["retained"]["result"].as_str().unwrap_or("?"))
                .collect::<Vec<_>>()
                .join(","),
            expected_native_runs,
            expected_responses,
            expected_total_tokens,
            if review_rejects { "reject" } else { "accept" },
            expected_accepted,
            row_path.display()
        );
    }
    let stopped = fixture.stop();
    assert!(stopped.status.success(), "{}", text(&stopped));
}

/// An existing kit workload already carries its own executor declaration; the
/// native dispatch must select that configured profile, bind the qualified
/// local route under it and leave the frozen task tree unchanged.
#[test]
fn native_dispatch_uses_the_existing_executor_declaration() {
    let _serial = INSTALL.lock().unwrap();
    let declaration = "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"default\"\nexecutor_profiles = [\"workload-executor\"]\nmax_concurrent_executors = 1\nvote_threshold = 2\nincubator_size_cap = 32\nfeedback_batch_limit = 8\n";
    let fixture = Fixture::with_workload_declaration("declared", Some(declaration));
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_real_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

    let resume = fixture.resume_dispatched(&[]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let cursor = fixture.cursor();
    let baseline = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["role"] == "baseline")
        .cloned()
        .unwrap_or_else(|| {
            panic!("the task tree's own executor profile dispatched: {cursor}\n{output}")
        });
    assert_eq!(
        baseline["profile"], "workload-executor",
        "the dispatch records the task tree's own executor profile: {baseline}\n{output}"
    );
    assert_eq!(baseline["state"], "started", "{cursor}\n{output}");

    // The frozen declaration is consumed, never rewritten, and the prepared
    // arm home mirrors the qualified local route under that exact profile.
    let dispatch_checkout = fixture.arm_dir("baseline").join("checkout");
    let status = git_output(&dispatch_checkout, &["status", "--porcelain"]);
    assert_eq!(
        status.trim(),
        "",
        "the frozen task tree stays unchanged: {status}"
    );
    assert_eq!(
        fs::read_to_string(dispatch_checkout.join("global/orchestration.toml"))
            .unwrap()
            .replace("\r\n", "\n"),
        declaration,
        "the frozen executor declaration stays unchanged"
    );
    let config =
        fs::read_to_string(fixture.arm_dir("baseline").join("home").join("config.toml")).unwrap();
    let profile = config
        .split("[profiles.workload-executor]")
        .nth(1)
        .unwrap_or_else(|| panic!("the arm home binds the declared profile: {config}"));
    assert!(
        profile.contains("model = \"fixture-glyph-1\""),
        "the profile binds the qualified model: {config}"
    );
    assert!(
        profile.contains("model_provider = \"local\""),
        "the profile binds the qualified provider: {config}"
    );
    assert!(
        profile.contains("model_reasoning_effort = \"low\""),
        "the profile binds the qualified effort: {config}"
    );

    // The real conversation still completes through the host's own receipt.
    let receipt = attempt_receipt(&fixture, "base-1");
    let record = wait_for_terminal_receipt(&receipt);
    assert_eq!(record["observation"]["state"], "completed", "{record}");
}

/// Ordinary managed commands in each comparison arm consume that arm's
/// installation on the initial dispatch and on a later controller process,
/// even when both inherit a conflicting PATH. Unqualified names and explicit
/// `.exe` names are both executed. No model request is made.
#[test]
fn selected_arm_tools_resolve_on_dispatch_and_later_resume() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("arm-tools");
    let baseline_tool = compile_identity(&fixture.root, "baseline-id", "baseline");
    let candidate_tool = compile_identity(&fixture.root, "candidate-id", "candidate");
    let foreign_initial = compile_identity(&fixture.root, "foreign-initial-id", "foreign-initial");
    let foreign_resume = compile_identity(&fixture.root, "foreign-resume-id", "foreign-resume");
    let foreign_rtk = compile_identity(&fixture.root, "foreign-rtk-id", "foreign-rtk");
    let unrelated = compile_relative(
        &fixture.root,
        "unrelated-id",
        "unrelated",
        "marker-data.txt",
    );
    let foreign1 = fixture.root.join("foreign-initial");
    let foreign2 = fixture.root.join("foreign-resume");
    let unrelated_dir = fixture.root.join("unrelated-tools");
    for directory in [&foreign1, &foreign2, &unrelated_dir] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::copy(&foreign_initial, foreign1.join("codex-harness.exe")).unwrap();
    fs::copy(&foreign_rtk, foreign1.join("harness-rtk.exe")).unwrap();
    fs::copy(&foreign_resume, foreign2.join("codex-harness.exe")).unwrap();
    fs::copy(&foreign_rtk, foreign2.join("harness-rtk.exe")).unwrap();
    fs::copy(&unrelated, unrelated_dir.join("marker-tool.exe")).unwrap();
    fs::write(
        unrelated_dir.join("marker-data.txt"),
        "unrelated-sibling-sentinel",
    )
    .unwrap();

    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_identity_builds(
        &checkout,
        &fs::read(&baseline_tool).unwrap(),
        &fs::read(&candidate_tool).unwrap(),
    );
    fixture.start_with_ready_candidate(&checkout, false);

    let allow = fixture
        .root
        .to_str()
        .expect("fixture root is Unicode")
        .to_owned();
    let baseline_receipt = fixture.root.join("baseline-tools.json");
    let candidate_receipt = fixture.root.join("candidate-tools.json");
    write_tool_probe(&fixture.run, "baseline", &baseline_receipt, &allow);
    write_tool_probe(&fixture.run, "candidate", &candidate_receipt, &allow);
    let initial_path = prefixed_path(&[&foreign1, &unrelated_dir]);
    let resume = fixture.resume_in_process(&[("PATH", initial_path.as_str())]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("PowerShell 7."),
        "the dispatch prepared the owner shell: {output}"
    );
    assert!(
        baseline_receipt.is_file(),
        "the initial dispatch did not host the baseline tool probe: {}\n{output}",
        fs::read_to_string(fixture.run.join("tool-probes").join("baseline-trace.txt"))
            .unwrap_or_else(|error| format!("no trace: {error}"))
    );
    let baseline = tool_receipt(&baseline_receipt);
    assert_tool(
        &baseline,
        "codex-harness",
        "arm-tool-identity:baseline",
        "foreign-initial",
    );
    assert_tool(
        &baseline,
        "codex-harness.exe",
        "arm-tool-identity:baseline",
        "foreign-initial",
    );
    assert_tool(
        &baseline,
        "marker-tool",
        "arm-tool-identity:unrelated",
        "foreign-initial",
    );
    assert_relative_tool(&baseline, "marker-tool", "unrelated-sibling-sentinel");
    assert_shell(&baseline);

    let resume_path = prefixed_path(&[&foreign2, &unrelated_dir]);
    let resume = fixture.resume_in_process(&[("PATH", resume_path.as_str())]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        candidate_receipt.is_file(),
        "the later controller did not host the candidate tool probe: {output}"
    );
    let candidate = tool_receipt(&candidate_receipt);
    assert_tool(
        &candidate,
        "codex-harness",
        "arm-tool-identity:candidate",
        "foreign-resume",
    );
    assert_tool(
        &candidate,
        "codex-harness.exe",
        "arm-tool-identity:candidate",
        "foreign-resume",
    );
    assert_tool(
        &candidate,
        "marker-tool",
        "arm-tool-identity:unrelated",
        "foreign-resume",
    );
    assert_relative_tool(&candidate, "marker-tool", "unrelated-sibling-sentinel");
    assert_shell(&candidate);
    assert_ne!(
        baseline["pid"], candidate["pid"],
        "resume must be a later controller process: baseline {baseline} candidate {candidate}"
    );

    // A selected command removed after the hosted turn is drift. The next
    // resume must preserve that attempt and refuse a benefit decision.
    fs::remove_file(
        fixture
            .arm_dir("candidate")
            .join("home/harness/bin/codex-harness.exe"),
    )
    .unwrap();
    let drifted = fixture.resume_in_process(&[]);
    let output = text(&drifted);
    assert!(drifted.status.success(), "{output}");
    assert!(
        output.contains("no longer consumes its prepared runtime")
            || output.contains("changed since preparation")
            || output.contains("does not point at its prepared source"),
        "post-attempt command drift was accepted: {output}"
    );
    let status = fixture.status_json();
    assert_ne!(status["phase"], "decision-recorded", "{status}");
    assert_eq!(
        status["comparison"]["candidate"]["accepted"],
        Value::Null,
        "drift must not authorize a benefit decision: {status}"
    );
}

/// An advertised token-workflow component that the lifecycle owner cannot
/// prepare refuses readiness before a hosted turn. A foreign copy on PATH is
/// not consumed as a substitute.
#[test]
fn advertised_token_workflow_refuses_readiness_before_dispatch() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("arm-tools-unprepared");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fs::write(
        fixture.proj.join("global/rtk.json"),
        br#"{"version":"0.48.0","executableSha256":"abc"}"#,
    )
    .unwrap();
    let foreign = fixture.root.join("foreign-rtk");
    fs::create_dir_all(&foreign).unwrap();
    let foreign_tool = compile_identity(&fixture.root, "foreign-rtk-tool", "foreign-rtk");
    fs::copy(&foreign_tool, foreign.join("harness-rtk.exe")).unwrap();
    fixture.start_with_ready_candidate(&checkout, false);
    let receipt = fixture.root.join("should-not-run.json");
    write_tool_probe(
        &fixture.run,
        "baseline",
        &receipt,
        fixture.root.to_str().unwrap(),
    );
    let path = prefixed_path(&[&foreign]);
    let resume = fixture.resume_in_process(&[("PATH", path.as_str())]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("token-workflow") && output.contains("readiness is refused"),
        "an unprepared advertised component was treated as ready: {output}"
    );
    assert!(
        !receipt.is_file(),
        "readiness refusal still hosted a turn: {}",
        fs::read_to_string(&receipt).unwrap_or_default()
    );
    let status = fixture.status_json();
    assert_ne!(status["phase"], "decision-recorded", "{status}");
    assert!(
        status["comparison"]["baseline"]["accepted"] != true,
        "an unprepared arm authorized a benefit: {status}"
    );
}

/// Different prepared component inventories follow one recorded preparation
/// method, so the controller does not report a preparation-policy mismatch.
/// The component owner's installed-configuration edit remains a config-identity
/// mismatch. A genuinely different recorded method remains incomparable, and
/// independent acceptance still runs.
#[test]
fn different_component_inventories_share_one_preparation_method() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("component-method");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let mut checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    let vendor = b"staged-rtk-for-comparison-arm";
    let adapter = b"staged-adapter-for-comparison-arm";
    for (relative, bytes) in [
        (
            "crates/harness-rtk/Cargo.toml",
            &b"[package]\nname = \"harness-rtk\"\nversion = \"0.0.0\"\n"[..],
        ),
        ("crates/harness-rtk/src/main.rs", &b"fn main() {}\n"[..]),
    ] {
        let path = checkout.path.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
    }
    fs::write(
        checkout.path.join("global/rtk.json"),
        serde_json::to_vec(&json!({
            "version": "0.48.0",
            "executableSha256": build_identity::hash_bytes(vendor),
        }))
        .unwrap(),
    )
    .unwrap();
    git(&checkout.path, &["add", "."]);
    git(
        &checkout.path,
        &[
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "user.name=Fixture",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "advertise optional token workflow",
        ],
    );
    checkout.revision = git_output(&checkout.path, &["rev-parse", "HEAD"]);
    let identity =
        harness_core::token_workflow_lifecycle::component_source_identity(&checkout.path)
            .expect("the candidate source advertises a preparable token-workflow component");
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_flag = stop.clone();
    let home = fixture.arm_dir("candidate").join("home");
    let staged_identity = identity.clone();
    let watcher = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        while !stop_flag.load(std::sync::atomic::Ordering::Relaxed)
            && std::time::Instant::now() < deadline
        {
            if home.is_dir() {
                stage_token_workflow_package(&home, &staged_identity, vendor, adapter);
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    });
    let installed = fixture.resume();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    watcher
        .join()
        .expect("the component staging watcher finishes");
    let output = text(&installed);
    assert!(installed.status.success(), "{output}");
    assert!(
        fixture.arm_dir("candidate").join("runtime.json").is_file()
            && fixture.arm_dir("baseline").join("runtime.json").is_file(),
        "both arms were not prepared: {output}"
    );

    let baseline_runtime: harness_core::improvement_runtime::ArmRuntime = serde_json::from_slice(
        &fs::read(fixture.arm_dir("baseline").join("runtime.json")).unwrap(),
    )
    .unwrap();
    let candidate_runtime: harness_core::improvement_runtime::ArmRuntime = serde_json::from_slice(
        &fs::read(fixture.arm_dir("candidate").join("runtime.json")).unwrap(),
    )
    .unwrap();
    assert!(
        baseline_runtime.components.is_empty(),
        "the baseline source advertises no optional component: {baseline_runtime:?}"
    );
    let component = candidate_runtime
        .components
        .iter()
        .find(|component| component.name == "token-workflow")
        .expect("the candidate treatment's optional component was prepared");
    assert_eq!(component.status, "Token workflow connected");
    assert!(!component.state_sha256.is_empty());
    for name in ["rtk.exe", "harness-rtk.exe"] {
        let link = component
            .links
            .iter()
            .find(|link| link.name == name)
            .unwrap_or_else(|| panic!("{name} was not retained"));
        assert_eq!(
            build_identity::hash_file(&link.source).unwrap(),
            link.sha256
        );
        assert!(!link.sha256.is_empty());
    }
    for arm in ["baseline", "candidate"] {
        let receipt = load_json(&fixture.arm_dir(arm).join("preparation-method.json"));
        assert_eq!(receipt["schema"], 1, "{arm} method receipt: {receipt}");
        assert_eq!(
            receipt["method"], "owner-install/v1",
            "{arm} method receipt: {receipt}"
        );
        let config = fs::read_to_string(fixture.arm_dir(arm).join("home/config.toml")).unwrap();
        assert!(
            config.contains("fixture-glyph-1"),
            "{arm} lost the declared model: {config}"
        );
    }

    settle_dispatched_pair(&fixture);
    let report = load_json(&fixture.run.join("comparison/report.json"));
    let reasons = comparison_reasons(&report);
    assert!(
        !reasons
            .iter()
            .any(|reason| reason.contains("preparation_policy")),
        "different component inventories were treated as a preparation-policy mismatch: {reasons:?}\n{report}"
    );
    // The component owner enables its features in the installed configuration.
    // That digest remains a matched field; this test must not bypass it.
    assert_eq!(
        reasons,
        vec!["mismatch:config_identity".to_owned()],
        "the shared method removed a gate other than preparation policy: {reasons:?}"
    );
    assert_eq!(report["comparisons"][0]["comparable"], false, "{reasons:?}");
    assert_ne!(
        baseline_runtime
            .configuration
            .as_ref()
            .map(|item| &item.sha256),
        candidate_runtime
            .configuration
            .as_ref()
            .map(|item| &item.sha256),
        "the config gate did not observe the component owner's feature edit"
    );
    let baseline_row = load_json(&fixture.arm_dir("baseline").join("row.json"));
    let candidate_row = load_json(&fixture.arm_dir("candidate").join("row.json"));
    assert_eq!(
        baseline_row["matched"]["preparation_policy"],
        "method:owner-install/v1"
    );
    assert_eq!(
        baseline_row["matched"]["preparation_policy"],
        candidate_row["matched"]["preparation_policy"]
    );
    assert_eq!(
        baseline_row["treatment"]["components"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        candidate_row["treatment"]["components"][0]["name"],
        "token-workflow"
    );
    let retained_links = candidate_row["treatment"]["components"][0]["links"]
        .as_array()
        .expect("the accounting row retains the prepared links");
    assert_eq!(retained_links.len(), component.links.len());
    for link in &component.links {
        assert!(
            retained_links.iter().any(|retained| {
                retained["name"] == link.name && retained["sha256"] == link.sha256
            }),
            "the accounting row dropped prepared link {}: {retained_links:?}",
            link.name
        );
    }
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "{status}"
    );
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );
    let decision = status["comparison"]["decision"].as_str().unwrap_or("");
    assert!(
        !decision.contains("outcome=adopt"),
        "a config mismatch still authorized adoption: {status}"
    );
    assert_eq!(git_output(&fixture.proj, &["rev-parse", "HEAD"]), base);

    let mut changed = candidate_row.clone();
    changed["matched"]["preparation_policy"] = json!("method:owner-install/v0");
    let changed_report = harness_core::outcome_report::summarize_attempts(&[baseline_row, changed])
        .expect("the accounting owner still compares a changed method");
    let changed_reasons = comparison_reasons(&changed_report);
    assert!(
        changed_reasons
            .iter()
            .any(|reason| reason == "mismatch:preparation_policy"),
        "a changed preparation method stayed comparable: {changed_reasons:?}"
    );
    assert_eq!(changed_report["comparisons"][0]["comparable"], false);
}

/// An arm whose retained preparation method differs is incomparable even when
/// both inventories were prepared by the same controller and independently
/// accepted. The method receipt is not rewritten on resume.
#[test]
fn changed_preparation_method_remains_incomparable() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("changed-method");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let installed = fixture.resume();
    assert!(installed.status.success(), "{}", text(&installed));
    let receipt = fixture.arm_dir("candidate").join("preparation-method.json");
    let mut method = load_json(&receipt);
    assert_eq!(method["method"], "owner-install/v1", "{method}");
    method["method"] = json!("owner-install/v0");
    fs::write(&receipt, serde_json::to_vec_pretty(&method).unwrap()).unwrap();
    settle_dispatched_pair(&fixture);
    let retained = load_json(&receipt);
    assert_eq!(
        retained["method"], "owner-install/v0",
        "resume rewrote the recorded preparation method: {retained}"
    );
    let report = load_json(&fixture.run.join("comparison/report.json"));
    let reasons = comparison_reasons(&report);
    assert!(
        reasons
            .iter()
            .any(|reason| reason == "mismatch:preparation_policy"),
        "a changed actual preparation method was comparable: {reasons:?}"
    );
    assert_eq!(report["comparisons"][0]["comparable"], false, "{reasons:?}");
    assert_eq!(
        load_json(&fixture.arm_dir("baseline").join("row.json"))["matched"]["preparation_policy"],
        "method:owner-install/v1"
    );
    assert_eq!(
        load_json(&fixture.arm_dir("candidate").join("row.json"))["matched"]["preparation_policy"],
        "method:owner-install/v0"
    );
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "{status}"
    );
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );
    let decision = status["comparison"]["decision"].as_str().unwrap_or("");
    assert!(
        !decision.contains("outcome=adopt"),
        "an incomparable method still authorized adoption: {status}"
    );
    assert_eq!(git_output(&fixture.proj, &["rev-parse", "HEAD"]), base);
}

/// The full short path: the directed-measurement gate admits the run, both
/// arms execute the declared real operation (a genuine build/check cycle
/// with unchanged-input reuse, changed-input invalidation and a real built
/// output), the frozen checker independently accepts the baseline and fails
/// the candidate's wrong workload result, the decision is published and the
/// rejected candidate leaves the accepted baseline unchanged with every
/// artifact retained. No model conversation is opened anywhere.
#[test]
fn a_short_real_operation_pair_reaches_a_supported_rejection_and_restores_the_baseline() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, checkout) = short_operation_fixture(
        "short-operation",
        "solved",
        "broken",
        &["{workspace}", "{runtime}", "{target}"],
    );
    let head_before = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let resumed = resume_short_operation(&fixture);
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    assert!(
        output.contains("direct operation completed through the real heavy-command route"),
        "{output}"
    );
    // The directed initial-measurement gate admitted the run before any arm
    // ran, and its receipt binds the declared operation the arms executed.
    let measurement = load_json(&fixture.run.join("measurement-receipt.json"));
    assert_eq!(
        measurement["scope"]["workload"]["operation"],
        "cargo build --offline --manifest-path build/Cargo.toml",
        "{measurement}"
    );
    // The declared model qualification record is absent, yet both measured
    // arms ran: the direct-operation unit performs no model dispatch and
    // requires no unrelated model qualification.
    assert!(!fixture.root.join("absent-qualification.json").exists());

    // Both measured arms are real executions with genuine retained outputs.
    for (arm, expected_exit) in [("baseline", 0), ("candidate", 0)] {
        let receipt = load_json(&fixture.arm_dir(arm).join("operation-receipt.json"));
        assert_eq!(receipt["arm"], json!(arm), "{receipt}");
        assert_eq!(receipt["method"], "real-operation", "{receipt}");
        assert_eq!(
            receipt["workspace"].as_str().unwrap_or_default(),
            fixture
                .arm_dir(arm)
                .join("checkout")
                .to_string_lossy()
                .as_ref(),
            "{receipt}"
        );
        assert_eq!(receipt["status"], "exited", "{receipt}");
        assert_eq!(receipt["exit_code"], json!(expected_exit), "{receipt}");
        assert_eq!(receipt["model_calls"], 0, "{receipt}");
        assert_eq!(receipt["model_metrics"], "inapplicable", "{receipt}");
        assert!(
            receipt["elapsed_seconds"].as_f64().unwrap_or(0.0) > 0.0,
            "the execution time is the heavy owner's measured wall clock: {receipt}"
        );
        assert_eq!(
            receipt["program_sha256"].as_str().unwrap_or_default().len(),
            64,
            "{receipt}"
        );
        let stdout = receipt["admitted"]["streams"]["stdout"]["path"]
            .as_str()
            .expect("retained stdout");
        assert!(Path::new(stdout).is_file(), "{receipt}");
        let report = fs::read_to_string(
            fixture
                .arm_dir(arm)
                .join("operation-target")
                .join("report.json"),
        )
        .expect("the operation's own report is retained");
        assert!(report.contains("\"second_fresh\":1"), "{arm}: {report}");
        assert!(
            report.contains("\"first_compiled\":true"),
            "{arm}: {report}"
        );
        assert!(
            report.contains("\"third_compiled\":true"),
            "the changed input genuinely recompiled: {arm}: {report}"
        );
    }

    // Independent acceptance through the frozen oracle: the baseline's real
    // build/check result passes; the candidate's wrong workload result fails.
    let baseline_oracle = load_json(&fixture.arm_dir("baseline").join("oracle.json"));
    let candidate_oracle = load_json(&fixture.arm_dir("candidate").join("oracle.json"));
    assert_eq!(
        baseline_oracle["checker_executed"], true,
        "{baseline_oracle}"
    );
    assert_eq!(baseline_oracle["passed"], true, "{baseline_oracle}");
    assert_eq!(
        candidate_oracle["checker_executed"], true,
        "{candidate_oracle}"
    );
    assert_eq!(candidate_oracle["passed"], false, "{candidate_oracle}");
    // No duplicate feature implementation: the two arms executed the same
    // existing operation, no workload implementation was carried, and no
    // fabricated implementation record was written to either hypothesis card.
    for card in [&fixture.card, &fixture.workload_card] {
        let comments =
            harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, card).unwrap();
        assert!(
            comments
                .iter()
                .all(|comment| !comment.starts_with("hypothesis-implementation")),
            "{card}: {comments:?}"
        );
    }

    // The comparison decided and the rejection was consumed: the accepted
    // baseline is unchanged, nothing was integrated or activated, and the
    // candidate, its failed acceptance and the measured scope stay retained.
    let status = fixture.status_json();
    assert_eq!(status["phase"], "idle", "{status}\n{output}");
    assert!(
        status["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("leaves the baseline unchanged"),
        "{status}"
    );
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], false,
        "{status}"
    );
    assert!(!fixture.run.join("integration.json").is_file());
    assert!(!fixture.run.join("activation.json").is_file());
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        head_before
    );
    assert!(
        checkout.path.is_dir(),
        "the candidate checkout is preserved"
    );
    let lineage = load_json(&fixture.run.join("lineage.json"));
    assert_eq!(lineage["decision"], "reject", "{lineage}");
    let reconcile = load_json(&fixture.run.join("reconcile.json"));
    assert_eq!(reconcile["outcome"], "reject", "{reconcile}");
    assert!(
        fixture.proj.join("openspec/changes/add-synthetic").is_dir(),
        "an unadopted change is never synchronized into the main specs"
    );
    assert!(fixture.run.join("comparison/decision.json").is_file());
    assert!(fixture.run.join("comparison/report.json").is_file());
    assert!(fixture.run.join("comparison/evaluation.json").is_file());

    // No conversation was opened and no model work ran for any phase.
    let cursor = fixture.cursor();
    assert!(
        cursor["attempts"].as_array().unwrap().is_empty(),
        "the short path requires no model conversation: {cursor}"
    );
    assert!(
        cursor["effects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|effect| effect["kind"].as_str().unwrap_or_default() != "dispatch-accepted"),
        "no conversation is ever dispatched on the direct-operation route: {cursor}"
    );
    assert!(
        cursor["effects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|effect| effect["kind"].as_str().unwrap_or_default() != "comparison-arm-refused"),
        "{cursor}"
    );
    let baseline_receipt =
        fs::read(fixture.arm_dir("baseline").join("operation-receipt.json")).unwrap();
    // A repeated resume reuses the retained executions and publishes nothing
    // new: unchanged input never replays a completed operation.
    let again = resume_short_operation(&fixture);
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(
        fs::read(fixture.arm_dir("baseline").join("operation-receipt.json")).unwrap(),
        baseline_receipt,
        "the retained baseline execution is reused, never replayed"
    );
    let effects = fixture.cursor()["effects"].as_array().unwrap().clone();
    let count = |kind: &str| {
        effects
            .iter()
            .filter(|effect| effect["kind"] == kind)
            .count()
    };
    assert_eq!(count("comparison-arm-accepted"), 2, "{effects:?}");
    assert_eq!(count("comparison-decision-recorded"), 1, "{effects:?}");
}

/// A settled arm is reused while all declared inputs match, and a rewritten
/// operation declaration refuses reuse with the exact changed field: the
/// retained execution is preserved and remeasurement is required rather than
/// silently reusing evidence that no longer binds the run's declaration.
#[test]
fn a_settled_short_operation_is_reused_only_while_its_declared_inputs_match() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, _checkout) = short_operation_fixture(
        "short-operation-reuse",
        "solved",
        "solved",
        &["{workspace}", "{runtime}", "{target}"],
    );
    // The reuse case is about retained executions, not about a margin: an
    // unreachable declared effect makes the final verdict independent of the
    // two operations' wall-clock noise while the accounting stays intact.
    let mut policy = load_json(&fixture.policy);
    policy["meaningfulEffectPercent"] = json!(1000.0);
    fs::write(&fixture.policy, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
    let first = resume_short_operation(&fixture);
    assert!(first.status.success(), "{}", text(&first));
    let baseline_path = fixture.arm_dir("baseline").join("operation-receipt.json");
    let baseline_receipt = fs::read(&baseline_path).unwrap();
    assert!(
        !fixture
            .arm_dir("candidate")
            .join("operation-receipt.json")
            .is_file(),
        "one arm is executed per advance"
    );

    // Unchanged inputs: the retained baseline execution is reused and only
    // the candidate's own operation runs.
    let second = resume_short_operation(&fixture);
    assert!(second.status.success(), "{}", text(&second));
    assert_eq!(
        fs::read(&baseline_path).unwrap(),
        baseline_receipt,
        "unchanged inputs reuse the retained execution"
    );
    assert!(
        fixture
            .arm_dir("candidate")
            .join("operation-receipt.json")
            .is_file()
    );

    // A rewritten declaration cannot reuse the completed candidate: the
    // exact changed field is named and the retained result is untouched.
    let declaration_path = fixture.run.join("operation.json");
    let original = fs::read(&declaration_path).unwrap();
    let mut declaration: Value = serde_json::from_slice(&original).unwrap();
    declaration["arguments"] = json!(operation_arguments(&["--redeclared"]));
    fs::write(
        &declaration_path,
        serde_json::to_vec_pretty(&declaration).unwrap(),
    )
    .unwrap();
    let candidate_path = fixture.arm_dir("candidate").join("operation-receipt.json");
    let candidate_receipt = fs::read(&candidate_path).unwrap();
    let third = resume_short_operation(&fixture);
    let output = text(&third);
    assert!(third.status.success(), "{output}");
    assert!(output.contains("cannot be reused"), "{output}");
    assert!(output.contains("arguments"), "{output}");
    assert_eq!(
        fs::read(&candidate_path).unwrap(),
        candidate_receipt,
        "the retained execution is preserved, never replayed"
    );
    assert!(
        !fixture
            .run
            .join("comparison")
            .join("decision.json")
            .is_file(),
        "changed conditions publish no decision"
    );

    // Restoring the exact declaration revalidates the retained executions
    // and the pair settles without remeasurement.
    fs::write(&declaration_path, &original).unwrap();
    let fourth = resume_short_operation(&fixture);
    assert!(fourth.status.success(), "{}", text(&fourth));
    assert!(fixture.run.join("comparison/decision.json").is_file());
    assert_eq!(fs::read(&baseline_path).unwrap(), baseline_receipt);
    assert_eq!(fs::read(&candidate_path).unwrap(), candidate_receipt);
    // Both arms passed their independent checks, so the model-free pair forms
    // a comparable unit on the operation's own measured work: the model
    // dimensions stay explicitly inapplicable (never a measured zero). Real
    // durations vary run to run, so which supported verdict the measured
    // difference reaches (adopt or reject against the declared threshold) is
    // not fixed here; the assertions below bind the record instead of a
    // timing value.
    let decision = load_json(&fixture.run.join("comparison/decision.json"));
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    assert_eq!(
        decision["decision"], evaluation["decision"],
        "the published decision is the evaluated verdict: {decision}\n{evaluation}"
    );
    let verdict = evaluation["decision"].as_str().unwrap_or_default();
    assert!(
        verdict == "adopt" || verdict == "reject",
        "both real operations were independently accepted, so the verdict is supported, never inconclusive: {decision}\n{evaluation}"
    );
    assert_eq!(
        evaluation["acceptedTasks"], 2,
        "both real operations were independently accepted: {evaluation}"
    );
    // The model-free pair is a first-class measured unit: its comparability
    // rests on the declared identities and the operation's own recorded work,
    // and its model metrics stay inapplicable rather than a measured zero.
    assert_eq!(evaluation["matched"], 1, "{evaluation}");
    assert_eq!(
        evaluation["coverage"].as_str().unwrap_or_default(),
        "time+method:real-operation model-metrics:inapplicable operation-work:duration+exit+inputs; complete-pairs:1; variation:unmeasured",
        "the unmeasured model dimensions are visible limits, never measured zero: {evaluation}"
    );
    assert_eq!(
        evaluation["quality"], "unchanged",
        "independent acceptance held on both arms: {evaluation}"
    );
    // The model-free distinction is recorded whichever way the timing falls:
    // the operation's own work covers the non-time dimensions and the model
    // metrics stay inapplicable rather than becoming a measured zero.
    assert_eq!(
        evaluation["coverage"].as_str().unwrap_or_default(),
        "time+method:real-operation model-metrics:inapplicable operation-work:duration+exit+inputs; complete-pairs:1; variation:unmeasured",
        "the coverage names the real-operation method with inapplicable model metrics: {evaluation}"
    );
    assert!(
        evaluation["variation"]["basis"]
            .as_str()
            .unwrap_or_default()
            .contains(
                "the declared method executes no model call, so its model metrics are inapplicable rather than a measured zero"
            ),
        "the recorded variation basis carries the model-free distinction: {evaluation}"
    );
    // The verdict must follow the record's own measured work: an adoption
    // needs a positive reduction clearing the declared meaningful threshold;
    // a non-adoption either names the un-met threshold or a material time
    // regression beyond the declared tolerance.
    let reasons = evaluation["reasons"]
        .as_array()
        .expect("the evaluation records its reasons")
        .iter()
        .map(|reason| reason.as_str().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("; ");
    let baseline_seconds = evaluation["baselineSeconds"]
        .as_f64()
        .expect("the accepted pair records the baseline duration");
    let candidate_seconds = evaluation["candidateSeconds"]
        .as_f64()
        .expect("the accepted pair records the candidate duration");
    assert!(
        baseline_seconds > 0.0 && candidate_seconds > 0.0,
        "the pair records real measured durations: {evaluation}"
    );
    let policy = load_json(&fixture.policy);
    let meaningful = policy["meaningfulEffectPercent"]
        .as_f64()
        .expect("the fixture policy declares its meaningful effect");
    let tolerance = policy["tolerancePercent"]
        .as_f64()
        .expect("the fixture policy declares its tolerance");
    let effect_percent = (baseline_seconds - candidate_seconds) / baseline_seconds * 100.0;
    if verdict == "adopt" {
        assert!(
            effect_percent >= meaningful,
            "the adoption clears the declared meaningful effect on the recorded work: {evaluation}"
        );
        assert!(
            reasons.contains("model metrics are inapplicable"),
            "the adoption names the model-free distinction: {evaluation}"
        );
    } else if reasons.contains("materially regressed the primary time metric") {
        assert!(
            effect_percent < -tolerance,
            "the recorded material regression is beyond the declared tolerance: {evaluation}"
        );
    } else {
        assert!(
            reasons.contains("does not meet the predeclared meaningful threshold"),
            "the non-adoption states the measured limit precisely: {evaluation}"
        );
        assert!(
            effect_percent >= -tolerance && effect_percent < meaningful,
            "the non-adoption is grounded in the recorded work below the declared threshold: {evaluation}"
        );
    }
    let effects = fixture.cursor()["effects"].as_array().unwrap().clone();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| effect["kind"] == "comparison-arm-accepted")
            .count(),
        2,
        "exactly one executed settlement per arm: {:?}",
        fixture.cursor()
    );
}

/// A failed real operation is retained with its own exit status and streams,
/// the comparison stops without a decision, and a resumed pass never replays
/// the failed execution.
#[test]
fn a_failed_short_operation_is_retained_and_never_replayed() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, _checkout) = short_operation_fixture(
        "short-operation-failure",
        "solved",
        "solved",
        &["{workspace}", "{runtime}", "{target}", "--fail-build"],
    );
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let first = resume_short_operation(&fixture);
    let output = text(&first);
    assert!(first.status.success(), "{output}");
    assert!(output.contains("did not enter the comparison"), "{output}");

    let status = fixture.status_json();
    assert_eq!(status["phase"], "blocked", "{status}\n{output}");
    let condition = status["condition"].as_str().unwrap_or_default();
    assert!(condition.contains("exit_code"), "{condition}");
    assert!(
        status["comparison"]["baseline"]["condition"].is_string(),
        "{status}"
    );
    // The genuine build failure is retained with its own exit status and the
    // real Cargo failure output; no counter is simulated.
    let receipt_path = fixture.arm_dir("baseline").join("operation-receipt.json");
    let receipt = load_json(&receipt_path);
    assert_eq!(receipt["status"], "exited", "{receipt}");
    assert_ne!(receipt["exit_code"], 0, "{receipt}");
    assert_eq!(receipt["model_calls"], 0, "{receipt}");
    let stderr_path = receipt["admitted"]["streams"]["stderr"]["path"]
        .as_str()
        .expect("retained stderr");
    let stderr = fs::read_to_string(stderr_path).unwrap();
    assert!(
        stderr.contains("cargo build failed"),
        "the failing build's own output is retained: {stderr}"
    );
    assert!(!fixture.arm_dir("baseline").join("oracle.json").is_file());
    assert!(!fixture.run.join("comparison/decision.json").is_file());
    assert!(
        fixture.cursor()["attempts"].as_array().unwrap().is_empty(),
        "{}",
        fixture.cursor()
    );

    // A resumed pass re-reads the retained failure instead of replaying it.
    let receipt_bytes = fs::read(&receipt_path).unwrap();
    let second = resume_short_operation(&fixture);
    let output = text(&second);
    assert!(second.status.success(), "{output}");
    assert_eq!(status_json_phase(&fixture), "blocked");
    assert_eq!(
        fs::read(&receipt_path).unwrap(),
        receipt_bytes,
        "the failed execution is never replayed"
    );
    assert_eq!(
        fixture.cursor()["effects"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|effect| effect["kind"] == "comparison-arm-accepted")
            .count(),
        0
    );
}

#[test]
fn a_warmer_second_arm_blocks_the_pair_until_the_owned_state_is_corrected() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, _checkout) = short_operation_fixture(
        "nuisance-warm-second-arm",
        "solved",
        "wrong",
        &["{workspace}", "{runtime}", "{target}"],
    );
    write_nuisance_policy(&fixture.policy, &fixture_nuisance_plan());
    // `continuous` drives the pair to its outcome or blocking condition in
    // one invocation, as the declared comparison expects.
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();

    let scratch = fixture
        .run
        .join("comparison")
        .join("candidate")
        .join("operation-target");
    // While the baseline arm is being measured, the second arm's owned
    // scratch receives inherited output - exactly the warmer-second-arm
    // condition the frozen plan must not absorb silently. The write happens
    // after the comparison is prepared (so it is not a partial preparation)
    // and before the candidate arm starts.
    let first = std::thread::scope(|scope| {
        let running = scope.spawn(|| resume_short_operation(&fixture));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(180);
        // The frozen bindings appear at the end of the model-free preparation,
        // while both arms are installed and long before the candidate arm
        // starts. Writing afterwards cannot be mistaken for a partial
        // preparation, and the candidate's own start check still runs later.
        let bindings = fixture.run.join("comparison").join("bindings.json");
        while !bindings.is_file() {
            assert!(
                std::time::Instant::now() < until,
                "the comparison was never prepared"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        fs::create_dir_all(&scratch).unwrap();
        fs::write(scratch.join("warm-leftover.bin"), b"inherited\n").unwrap();
        running.join().unwrap()
    });
    let output = text(&first);
    assert!(first.status.success(), "{output}");
    assert!(
        output.contains("would start from non-empty-owned-state"),
        "the warmed second arm is refused: {output}"
    );
    assert!(
        output.contains("no shared or unrelated state was cleared"),
        "{output}"
    );
    assert_eq!(
        fs::read(scratch.join("warm-leftover.bin")).unwrap(),
        b"inherited\n",
        "the inherited owned state is preserved, never cleared"
    );
    assert_eq!(status_json_phase(&fixture), "blocked");
    assert!(
        !fixture.run.join("comparison/decision.json").is_file(),
        "no decision is published from violated conditions"
    );
    let start = load_json(&fixture.run.join("comparison/candidate/nuisance-start.json"));
    assert_eq!(start["declared"], "owned-cold", "{start}");
    assert_eq!(start["violated"], true, "{start}");
    assert!(
        start["observed"]
            .as_str()
            .is_some_and(|observed| observed.contains("non-empty")),
        "{start}"
    );
    // The frozen plan was already bound before the arms: the declared plan,
    // its realized order and the unobserved shared state were recorded first.
    let receipt_path = fixture.run.join("comparison/nuisance-preflight.json");
    let receipt = load_json(&receipt_path);
    assert_eq!(receipt["realized_first"], "baseline-first", "{receipt}");
    assert_eq!(receipt["controller_order"], "baseline-first", "{receipt}");
    assert_eq!(receipt["shared_state"], "unobserved", "{receipt}");
    assert_eq!(receipt["plan"]["faults"], "observed-effect", "{receipt}");
    assert_eq!(receipt["plan"]["retries"], "policy-stopping", "{receipt}");
    assert!(
        receipt["resets"]
            .as_str()
            .is_some_and(|resets| resets.starts_with("none:")),
        "the receipt proves no state beyond owned observation is touched: {receipt}"
    );

    // Correcting the owned condition and resuming exercises the pair. The
    // recorded block is retried by the single-step controller, which
    // re-observes the corrected owned state before the arm starts.
    fs::remove_dir_all(&scratch).unwrap();
    fs::remove_file(fixture.run.join("supervision.json")).unwrap();
    for _ in 0..4 {
        if fixture.run.join("comparison/decision.json").is_file() {
            break;
        }
        let step = resume_short_operation(&fixture);
        assert!(step.status.success(), "{}", text(&step));
    }
    assert_eq!(status_json_phase(&fixture), "decision-recorded");
    let candidate_start = load_json(&fixture.run.join("comparison/candidate/nuisance-start.json"));
    assert_eq!(candidate_start["violated"], false, "{candidate_start}");

    // Both measured arms carry the frozen declaration, the observed initial
    // condition and the load disclosure into the report; the unobserved
    // shared state and the retained order exposure stay explicit in the
    // published evaluation.
    for arm in ["baseline", "candidate"] {
        let row = load_json(&fixture.arm_dir(arm).join("row.json"));
        assert_eq!(
            row["nuisance"]["order"]["realized"], "baseline-first",
            "{row}"
        );
        assert_eq!(
            row["nuisance"]["initial"]["observed"], "empty-or-absent-owned-state",
            "{row}"
        );
        assert_eq!(
            row["nuisance"]["load"]["observed"]["admission_evidence"], "unobserved",
            "{row}"
        );
        assert_eq!(row["nuisance"]["load"]["rule"], "recorded", "{row}");
    }
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    let coverage = evaluation["coverage"].as_str().unwrap_or_default();
    assert!(
        coverage.contains("nuisance-shared-unobserved"),
        "{evaluation}"
    );
    assert!(
        coverage.contains("nuisance-order-exposure-retained"),
        "{evaluation}"
    );

    // Recovery reuses the frozen receipt unchanged.
    let frozen = fs::read(&receipt_path).unwrap();
    let third = resume_short_operation(&fixture);
    assert!(third.status.success(), "{}", text(&third));
    assert_eq!(
        fs::read(&receipt_path).unwrap(),
        frozen,
        "the nuisance-control receipt is reused unchanged on resume"
    );
}

#[test]
fn an_arm_order_this_controller_cannot_realize_is_refused_before_results() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, _checkout) = short_operation_fixture(
        "nuisance-unrealizable-order",
        "solved",
        "solved",
        &["{workspace}", "{runtime}", "{target}"],
    );
    // The predeclared seed realizes the candidate first. This sequential
    // controller runs the baseline first and refuses to silently substitute.
    let mut plan = fixture_nuisance_plan();
    plan.order = harness_core::improvement_policy::OrderRule::Randomized;
    plan.seed = Some(1);
    write_nuisance_policy(&fixture.policy, &plan);
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();

    let resume = resume_short_operation(&fixture);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("realizes candidate-first")
            && output.contains("will not silently substitute a different order"),
        "{output}"
    );
    assert_eq!(status_json_phase(&fixture), "blocked");
    assert!(
        !fixture
            .run
            .join("comparison/nuisance-preflight.json")
            .is_file(),
        "an unrealizable order is not frozen into a receipt"
    );
    assert!(
        !fixture.arm_dir("baseline").join("row.json").is_file()
            && !fixture.arm_dir("candidate").join("row.json").is_file(),
        "no arm is measured under an unrealized order"
    );
}

#[test]
fn a_failed_measured_execution_is_classified_and_follows_the_frozen_retry_rule() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, _checkout) = short_operation_fixture(
        "nuisance-execution-failure",
        "solved",
        "solved",
        &["{workspace}", "{runtime}", "{target}", "--fail-build"],
    );
    write_nuisance_policy(&fixture.policy, &fixture_nuisance_plan());
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let first = resume_short_operation(&fixture);
    let output = text(&first);
    assert!(first.status.success(), "{output}");
    assert!(output.contains("did not enter the comparison"), "{output}");

    // The fault is classified by its observed effect: the operation started
    // and failed, so it is not an eligible transport-only idle wait, and the
    // frozen retry rule records that it is not replayed.
    let fault_path = fixture.arm_dir("baseline").join("fault.json");
    let fault = load_json(&fault_path);
    assert_eq!(fault["class"], "execution-failure", "{fault}");
    assert_ne!(fault["class"], "transport-only-refusal", "{fault}");
    assert_eq!(fault["retry"]["rule"], "policy-stopping", "{fault}");
    assert_eq!(fault["retry"]["max_attempts_per_arm"], 1, "{fault}");
    assert_eq!(fault["retry"]["replayed"], false, "{fault}");

    // The genuine failure is retained with its own exit status and never
    // replayed; no row and no comparison decision are fabricated from it.
    let receipt_path = fixture.arm_dir("baseline").join("operation-receipt.json");
    let receipt_bytes = fs::read(&receipt_path).unwrap();
    let receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
    assert_eq!(receipt["status"], "exited", "{receipt}");
    assert_ne!(receipt["exit_code"], 0, "{receipt}");
    let start = load_json(&fixture.run.join("comparison/baseline/nuisance-start.json"));
    assert_eq!(start["violated"], false, "{start}");
    assert!(!fixture.arm_dir("baseline").join("row.json").is_file());
    assert!(!fixture.run.join("comparison/decision.json").is_file());

    let second = resume_short_operation(&fixture);
    assert!(second.status.success(), "{}", text(&second));
    assert_eq!(
        fs::read(&receipt_path).unwrap(),
        receipt_bytes,
        "the failed execution is never replayed"
    );
    assert_eq!(
        load_json(&fault_path)["class"],
        "execution-failure",
        "the classification stays bound to the frozen plan"
    );
}

/// The background-contention counterexample: an unrelated owned heavy command
/// occupies the isolated account slot while the baseline arm runs. The
/// recorded contention stays visible on that arm, no utilization-based time
/// correction is invented for it, the retained attempts keep their raw and
/// admitted evidence, and the unsupported causal claim stays inconclusive
/// instead of publishing a false gain.
#[test]
fn recorded_background_contention_withholds_the_unsupported_verdict() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("nuisance-contention");
    write_nuisance_policy(&fixture.policy, &fixture_nuisance_plan());
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_real_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

    // One unrelated owned holder occupies the only slot of the isolated
    // account the measured arms use.
    let account = fixture.root.join("isolated-heavy-account");
    harness_core::heavy_command::prepare(&account).unwrap();
    let mut budget = harness_core::heavy_command::Budget::read(&account).unwrap();
    budget.max_concurrent_trees = 1;
    harness_core::heavy_command::Budget::write(&account, &budget).unwrap();
    let started = fixture.root.join("holder-started.txt");
    let launch = env!("CARGO_BIN_EXE_harness-launch-fixture");
    let mut holder = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    holder
        .arg("heavy")
        .arg("--account")
        .arg(&account)
        .arg("--attempt")
        .arg("holder-other")
        .arg("--")
        .arg(launch)
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "180000")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &started);
    let mut holder = holder.spawn().expect("isolated holder starts");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !started.exists() {
        assert!(
            std::time::Instant::now() < until,
            "holder did not acquire the isolated slot"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // The baseline arm is dispatched while the unrelated holder blocks the
    // shared slot; the holder is released after the arm reached its heavy
    // command, exactly as the existing owned-contention check does.
    let account_text = account.display().to_string();
    let marker = fixture.root.join("heavy-started.txt");
    let marker_text = marker.display().to_string();
    let release = std::thread::spawn({
        let marker = marker.clone();
        move || {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(240);
            while !marker.exists() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            if marker.exists() {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            let _ = holder.kill();
            let _ = holder.wait();
        }
    });
    let mut command = Command::new(manager());
    command
        .arg("improve")
        .args(["resume", "--run", fixture.run.to_str().unwrap()])
        .env_remove("HARNESS_EXECUTOR_SESSION")
        .env_remove("HARNESS_EXECUTOR_FIXTURE_MODE")
        .env_remove("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE")
        .env_remove("HARNESS_EXECUTOR_RUN")
        .env_remove("HARNESS_ORIGINATING_LEAD")
        .env_remove("HARNESS_LEAD_THREAD")
        .env_remove("HARNESS_LEAD_RECIPIENT")
        .env_remove("WT_SESSION")
        .env(CONTROL_CHILD_MODE.0, CONTROL_CHILD_MODE.1)
        .env("HARNESS_IMPROVEMENT_FIXTURE_HEAVY", "1")
        .env("CODEX_HARNESS_HEAVY_ACCOUNT", &account_text)
        .env("HARNESS_IMPROVEMENT_FIXTURE_HEAVY_MARKER", &marker_text)
        .env("HARNESS_IMPROVEMENT_FIXTURE_HEAVY_PROGRAM", launch)
        .env(
            "HARNESS_IMPROVEMENT_FIXTURE_HEAVY_CLI",
            env!("CARGO_BIN_EXE_codex-harness"),
        )
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "1000");
    let resume = command.output().expect("baseline resume");
    let output = text(&resume);
    assert!(
        marker.exists(),
        "the fixture did not reach the owned heavy command: {output}"
    );
    assert!(resume.status.success(), "{output}");
    let _ = release.join();
    let receipt = attempt_receipt(&fixture, "base-1");
    let record = wait_for_terminal_receipt(&receipt);
    assert_eq!(
        record["observation"]["state"], "completed",
        "the visible fixture conversation did not complete: {record}"
    );

    // The baseline settles, then the candidate arm is dispatched without the
    // holder (the account slot is free again) and settles too.
    let settle = fixture.resume_in_process(&[]);
    assert!(settle.status.success(), "settlement: {}", text(&settle));
    let candidate_receipt = attempt_receipt(&fixture, "cand-1");
    let candidate_record = wait_for_terminal_receipt(&candidate_receipt);
    assert_eq!(
        candidate_record["observation"]["state"], "completed",
        "{candidate_record}"
    );
    let settle = fixture.resume_in_process(&[]);
    assert!(settle.status.success(), "decision: {}", text(&settle));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");

    // The baseline arm's own row records the observed unrelated contention;
    // the candidate arm's row records none, and neither is repaired.
    let baseline = load_json(&fixture.arm_dir("baseline").join("row.json"));
    let candidate = load_json(&fixture.arm_dir("candidate").join("row.json"));
    assert_eq!(
        baseline["nuisance"]["load"]["observed"]["unrelated_wait"], true,
        "{baseline}"
    );
    assert_eq!(
        baseline["nuisance"]["load"]["observed"]["admission_evidence"], "recorded",
        "{baseline}"
    );
    assert_eq!(
        candidate["nuisance"]["load"]["observed"]["unrelated_wait"], false,
        "{candidate}"
    );
    // The raw observed time and the admitted evidence stay retained.
    let report = load_json(&fixture.run.join("comparison/report.json"));
    assert!(
        report["attempts"][0]["infrastructure"]["observed_seconds"]
            .as_f64()
            .is_some_and(|seconds| seconds > 0.0),
        "{report}"
    );

    // The asymmetric contention could explain the observed difference without
    // any declared work-efficiency binding: the causal claim is withheld and
    // named, rather than corrected by an invented utilization factor.
    let evaluation: Value = load_json(&fixture.run.join("comparison/evaluation.json"));
    assert_eq!(evaluation["decision"], "inconclusive", "{evaluation}");
    assert!(
        evaluation["reasons"]
            .as_array()
            .is_some_and(|reasons| reasons.iter().any(|reason| reason
                .as_str()
                .is_some_and(|reason| reason.contains(
                    "unrelated external heavy command was measured over exactly one arm"
                )))),
        "{evaluation}"
    );
    let decision = load_json(&fixture.run.join("comparison/decision.json"));
    assert_eq!(decision["decision"], "inconclusive", "{decision}");
}

/// A refusal that happened after the measured work started is classified by
/// its observed effect: the changed or unverifiable response cannot be
/// repaired by subtracting its wall duration, the original attempt stays
/// retained, and the frozen retry rule records that it is never replayed.
#[test]
fn an_unverifiable_measured_response_is_classified_and_not_repaired() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("nuisance-unverified-response");
    write_nuisance_policy(&fixture.policy, &fixture_nuisance_plan());
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    // The conversation ran and produced a response, but the observed model
    // facts do not match the declared client: the trajectory is unverifiable,
    // so the arm is refused instead of entering the comparison.
    let session = session_id("nuisance-unverified-session");
    fixture.simulate_arm("baseline", "baseline", "solved", 50, 5.0, 2, 3, &session);
    rewrite_rollout(&fixture, "baseline", &session, "another-model", "low");
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("recorded model another-model"), "{output}");

    let fault_path = fixture.arm_dir("baseline").join("fault.json");
    let fault = load_json(&fault_path);
    assert_eq!(fault["class"], "work-started-unverified", "{fault}");
    assert_ne!(fault["class"], "transport-only-refusal", "{fault}");
    assert_eq!(fault["retry"]["rule"], "policy-stopping", "{fault}");
    assert_eq!(fault["retry"]["replayed"], false, "{fault}");
    // The attempt and its refusal stay retained; no comparison row, oracle
    // result or decision is fabricated from the unverifiable trajectory.
    assert_eq!(
        fixture.cursor()["comparison"]["baseline"]["accepted"],
        Value::Null
    );
    assert!(!fixture.arm_dir("baseline").join("row.json").is_file());
    assert!(!fixture.run.join("comparison/decision.json").is_file());

    // A resumed pass re-reads the retained refusal instead of replaying the
    // arm or repairing its trajectory.
    let second = fixture.resume();
    assert!(second.status.success(), "{}", text(&second));
    assert_eq!(
        load_json(&fault_path)["class"],
        "work-started-unverified",
        "the classification stays bound to the frozen plan"
    );
}

/// Identical runtimes plus one known external idle delay: the raw accounting
/// keeps the delay, the adjusted view excludes exactly its verified blocked
/// interval once, no deduction is invented for the unblocked arm, and the
/// queue-only difference is not reported as a gain or a regression.
#[test]
fn calibration_idle_delay_is_retained_and_deducted_without_a_false_effect() {
    let _serial = INSTALL.lock().unwrap();
    let delay_seconds = 6.0;
    let delay_ms = 6_000;
    let session = session_id("calibration-delay-baseline");
    let mut baseline = ControlledArm::new("baseline", "baseline", &session, "solved");
    baseline.sleep_ms = 3_000;
    baseline.started_offset_seconds = 9.0;
    baseline.window_seconds = 20.0;
    baseline.commands = vec![controlled_command(
        "call_heavy_blocked_1",
        delay_ms + 1_500,
        1_500,
        true,
    )];
    let candidate_session = session_id("calibration-delay-candidate");
    let mut candidate = ControlledArm::new("candidate", "candidate", &candidate_session, "solved");
    candidate.sleep_ms = 3_000;
    candidate.started_offset_seconds = 3.0;
    candidate.window_seconds = 12.0;
    candidate.commands = vec![controlled_command("call_build_1", 3_500, 1_500, false)];
    let fixture = controlled_pair("calibration-delay", true, baseline, candidate);

    let report = load_json(&fixture.run.join("comparison/report.json"));
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    let decision = load_json(&fixture.run.join("comparison/decision.json"));
    let baseline_attempt = attempt_of_arm(&report, "baseline");
    let candidate_attempt = attempt_of_arm(&report, "candidate");
    let baseline_infra = &baseline_attempt["infrastructure"];
    let candidate_infra = &candidate_attempt["infrastructure"];

    // Raw retention: the observed elapsed keeps the injected delay and is
    // never rewritten by the adjustment.
    let baseline_observed = baseline_infra["observed_seconds"]
        .as_f64()
        .unwrap_or_else(|| panic!("the baseline observed time is retained: {report}"));
    let candidate_observed = candidate_infra["observed_seconds"]
        .as_f64()
        .unwrap_or_else(|| panic!("the candidate observed time is retained: {report}"));
    assert!(
        baseline_observed - candidate_observed > delay_seconds - 2.0,
        "the raw observed difference must retain the injected delay: {baseline_infra} {candidate_infra}"
    );
    let unit = &report["units"][0];
    let raw_delta = unit["effect"]["baseline_seconds"]
        .as_f64()
        .zip(unit["effect"]["candidate_seconds"].as_f64())
        .map(|(baseline, candidate)| baseline - candidate);
    assert!(
        raw_delta.is_some_and(|delta| delta > delay_seconds - 2.0),
        "the retained unit effect keeps the raw difference: {unit}"
    );

    // Bounded adjustment: only the verified blocked interval is excluded,
    // once, and the identity observed = adjusted + excluded holds exactly.
    let deducted = baseline_infra["deductible_seconds"]
        .as_f64()
        .unwrap_or_else(|| panic!("the deduction is retained: {baseline_infra}"));
    assert!(
        (delay_seconds - 0.1..=delay_seconds).contains(&deducted),
        "the deductible portion must be the verified blocked interval, not less or more: {baseline_infra}"
    );
    assert_eq!(
        candidate_infra["deductible_ns"], 0,
        "an unblocked arm must keep no deduction: {candidate_infra}"
    );
    let adjusted_high = baseline_infra["adjusted_high_seconds"].as_f64().unwrap();
    let adjusted_low = baseline_infra["adjusted_low_seconds"].as_f64().unwrap();
    let unresolved = baseline_infra["unresolved_seconds"].as_f64().unwrap();
    assert!(
        (baseline_observed - deducted - adjusted_high).abs() < 1e-6,
        "the raw time must stay adjusted + excluded: {baseline_infra}"
    );
    assert!(
        (adjusted_high - adjusted_low - unresolved).abs() < 1e-6 && unresolved < 0.2,
        "the observed error bound stays visible and small: {baseline_infra}"
    );
    assert!(
        baseline_observed - adjusted_high > 5.0,
        "the injected interval must be excluded once: {baseline_infra}"
    );
    assert!(
        (adjusted_high - candidate_observed).abs() < 4.0,
        "the adjusted baseline must return to the identical-work time within the observed bound: {baseline_infra} {candidate_infra}"
    );

    // The attribution record binds the deduction to the predeclared rule,
    // lineage, cause and retained evidence; nothing is claimed unbound.
    let attribution = &baseline_attempt["attribution"];
    assert_eq!(
        attribution["rule_version"],
        harness_core::infrastructure_accounting::RULE_VERSION,
        "{attribution}"
    );
    assert_eq!(
        attribution["lineage"],
        harness_core::infrastructure_accounting::MEASUREMENT_LINEAGE,
        "{attribution}"
    );
    assert_eq!(attribution["replay"], "reproduced", "{attribution}");
    assert_eq!(
        attribution["reconciliation"]["elapsed"]["status"], "consistent",
        "{attribution}"
    );
    assert!(
        attribution["exclusions"].as_array().is_some_and(|items| {
            items.iter().any(|item| {
                item["kind"] == "deducted" && item["cause"] == "unrelated-external-blocking"
            })
        }),
        "the deduction keeps its cause and evidence: {attribution}"
    );

    // No false effect: the queue-only difference cannot become a gain or a
    // regression, and the decision names the attribution instead.
    assert_ne!(
        decision["decision"], "adopt",
        "a queue-only difference must not be adopted: {decision}"
    );
    assert_eq!(decision["decision"], "inconclusive", "{decision}");
    let reasons = evaluation_reasons(&evaluation);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("attributed to excluded external waiting")),
        "the verdict must name the queue-only attribution: {reasons:?}"
    );
    assert!(
        !reasons
            .iter()
            .any(|reason| reason.contains("materially regressed")),
        "the delay must not be reported as a regression: {reasons:?}"
    );
}

/// Real added work stays attributable: an arm that records a redundant build
/// and polling commands keeps their cost in both raw and adjusted accounting,
/// nothing is deducted for its own granted work, and the added burden cannot
/// be normalized into a supported improvement.
#[test]
fn calibration_controls_preserve_extra_work_without_normalizing_it_away() {
    let _serial = INSTALL.lock().unwrap();
    let baseline = granted_arm(
        "baseline",
        "calibration-extra-baseline",
        2.0,
        &[("call_build_1", 2_500, 1_000)],
    );
    let candidate = granted_arm(
        "candidate",
        "calibration-extra-candidate",
        7.0,
        &[
            ("call_build_1", 7_500, 5_500),
            ("call_build_extra_1", 5_000, 3_500),
            ("call_poll_1", 2_500, 2_000),
        ],
    );
    let fixture = controlled_pair("calibration-extra-work", true, baseline, candidate);

    let report = load_json(&fixture.run.join("comparison/report.json"));
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    let decision = load_json(&fixture.run.join("comparison/decision.json"));
    let baseline_attempt = attempt_of_arm(&report, "baseline");
    let candidate_attempt = attempt_of_arm(&report, "candidate");
    for (name, attempt) in [
        ("baseline", baseline_attempt),
        ("candidate", candidate_attempt),
    ] {
        let infra = &attempt["infrastructure"];
        assert_eq!(
            infra["deductible_ns"], 0,
            "{name}: granted own work must not be deducted: {attempt}"
        );
        assert_eq!(
            infra["coverage"], "measured",
            "{name}: the granted-work trace is fully measured: {attempt}"
        );
        assert!(
            (infra["adjusted_high_seconds"].as_f64().unwrap()
                - infra["observed_seconds"].as_f64().unwrap())
            .abs()
                < 1e-6,
            "{name}: adjusted time keeps the own-work cost: {attempt}"
        );
    }
    // The added build and polling work stays visible in the arm's own
    // counters and in the measured time.
    let baseline_observed = baseline_attempt["infrastructure"]["observed_seconds"]
        .as_f64()
        .unwrap();
    let candidate_observed = candidate_attempt["infrastructure"]["observed_seconds"]
        .as_f64()
        .unwrap();
    assert!(
        candidate_observed - baseline_observed > 1.5,
        "the extra admitted work must stay in the measured time: {baseline_attempt} {candidate_attempt}"
    );
    assert_eq!(
        candidate_attempt["tool_operations"], 3,
        "{candidate_attempt}"
    );
    assert_eq!(baseline_attempt["tool_operations"], 1, "{baseline_attempt}");
    let activity = candidate_attempt["infrastructure_capture"]["activity"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        activity.len(),
        3,
        "every recorded command stays placed: {candidate_attempt}"
    );
    // The added burden is a measured regression of the treatment, never an
    // adopted improvement.
    assert_eq!(decision["decision"], "reject", "{decision}");
    let reasons = evaluation_reasons(&evaluation);
    assert!(
        reasons.iter().any(|reason| reason.contains("regressed")
            || reason.contains("does not meet the predeclared meaningful threshold")),
        "the added burden decides the verdict: {reasons:?}"
    );
    let selected = &report["units"][0]["infrastructure_effect"];
    assert!(
        selected["candidate_high_seconds"].as_f64().unwrap()
            > selected["baseline_low_seconds"].as_f64().unwrap(),
        "the adjusted view keeps the added cost: {selected}"
    );
}

/// A real cache benefit remains an attributable treatment effect: an arm that
/// avoids a redundant rebuild is measurably faster in raw and adjusted
/// accounting, nothing is deducted for either arm's granted work, and the
/// supported effect is preserved through the decision.
#[test]
fn calibration_controls_preserve_a_real_cache_benefit() {
    let _serial = INSTALL.lock().unwrap();
    let baseline = granted_arm(
        "baseline",
        "calibration-cache-baseline",
        9.0,
        &[
            ("call_build_1", 9_500, 7_500),
            ("call_rebuild_1", 7_000, 5_500),
        ],
    );
    let candidate = granted_arm(
        "candidate",
        "calibration-cache-candidate",
        2.0,
        &[("call_build_1", 2_500, 1_500)],
    );
    let fixture = controlled_pair("calibration-cache-benefit", true, baseline, candidate);

    let report = load_json(&fixture.run.join("comparison/report.json"));
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    let decision = load_json(&fixture.run.join("comparison/decision.json"));
    let baseline_attempt = attempt_of_arm(&report, "baseline");
    let candidate_attempt = attempt_of_arm(&report, "candidate");
    for (name, attempt) in [
        ("baseline", baseline_attempt),
        ("candidate", candidate_attempt),
    ] {
        assert_eq!(
            attempt["infrastructure"]["deductible_ns"], 0,
            "{name}: a granted build is not a deductable wait: {attempt}"
        );
    }
    assert_eq!(
        baseline_attempt["tool_operations"], 2,
        "the baseline records its redundant rebuild: {baseline_attempt}"
    );
    assert_eq!(
        candidate_attempt["tool_operations"], 1,
        "the candidate records the avoided rebuild: {candidate_attempt}"
    );
    let selected = &report["units"][0]["infrastructure_effect"];
    assert!(
        selected["candidate_high_seconds"].as_f64().unwrap()
            < selected["baseline_low_seconds"].as_f64().unwrap(),
        "the avoided work must be visible in the adjusted view: {selected}"
    );
    assert_eq!(
        decision["decision"], "adopt",
        "the supported cache benefit must survive the decision: {evaluation} {decision}"
    );
}

/// A quality failure stays an acceptance failure: a faster candidate whose
/// solution fails the independent oracle cannot be adopted, whatever its
/// measured time says, and the retained failure stays inspectable.
#[test]
fn calibration_controls_keep_a_quality_failure_as_rejection() {
    let _serial = INSTALL.lock().unwrap();
    let baseline = granted_arm(
        "baseline",
        "calibration-quality-baseline",
        3.0,
        &[("call_build_1", 3_500, 1_500)],
    );
    let mut candidate = granted_arm(
        "candidate",
        "calibration-quality-candidate",
        1.0,
        &[("call_build_1", 1_500, 500)],
    );
    candidate.solution = "broken";
    let fixture = controlled_pair("calibration-quality-failure", true, baseline, candidate);

    let report = load_json(&fixture.run.join("comparison/report.json"));
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    let decision = load_json(&fixture.run.join("comparison/decision.json"));
    let candidate_attempt = attempt_of_arm(&report, "candidate");
    assert_eq!(
        candidate_attempt["checks"][0]["passed"], false,
        "the failing acceptance stays retained: {candidate_attempt}"
    );
    let oracle = load_json(&fixture.arm_dir("candidate").join("oracle.json"));
    assert_eq!(oracle["checker_executed"], true, "{oracle}");
    assert_eq!(oracle["passed"], false, "{oracle}");
    assert_eq!(decision["decision"], "reject", "{decision}");
    let reasons = evaluation_reasons(&evaluation);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("independent acceptance failed")),
        "the acceptance failure decides the verdict: {reasons:?}"
    );
    assert_eq!(
        evaluation["quality"], "unmeasurable",
        "a failed candidate acceptance is an unmeasurable quality result, never a benefit: {evaluation}"
    );
}

/// Unusable evidence cannot claim a complete correction: a dropped measured
/// bound and an unverified ownership association each keep the raw delay
/// visible, produce no deduction, widen the adjusted range with an explicit
/// gap and leave the decision inconclusive instead of inventing precision.
#[test]
fn calibration_controls_expose_a_dropped_boundary_and_unverified_ownership() {
    let _serial = INSTALL.lock().unwrap();
    let baseline = delayed_arm(
        "baseline",
        "calibration-defect-boundary-baseline",
        6_000,
        ControlledDefect::BoundaryDropped,
    );
    let candidate = delayed_arm(
        "candidate",
        "calibration-defect-ownership-candidate",
        6_000,
        ControlledDefect::OwnershipUnverified,
    );
    let fixture = controlled_pair("calibration-defects-one", true, baseline, candidate);

    let report = load_json(&fixture.run.join("comparison/report.json"));
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    let decision = load_json(&fixture.run.join("comparison/decision.json"));
    let baseline_infra = &attempt_of_arm(&report, "baseline")["infrastructure"];
    let candidate_infra = &attempt_of_arm(&report, "candidate")["infrastructure"];
    // The raw delay is retained even though nothing may be corrected.
    for (name, infra) in [("baseline", baseline_infra), ("candidate", candidate_infra)] {
        assert!(
            infra["observed_seconds"].as_f64().unwrap() > 6.0,
            "{name}: the raw delay stays visible: {infra}"
        );
        assert_eq!(
            infra["deductible_ns"], 0,
            "{name}: unusable evidence must not deduct: {infra}"
        );
        assert_ne!(
            infra["coverage"], "measured",
            "{name}: the correction cannot claim measured coverage: {infra}"
        );
        assert!(
            infra["unresolved_seconds"].as_f64().unwrap() > 0.0,
            "{name}: the missing correction stays a visible unresolved bound: {infra}"
        );
        assert_ne!(infra["proven_zero_queue"], true, "{name}: {infra}");
    }
    let baseline_gaps = baseline_infra["gaps"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        baseline_gaps.iter().any(|gap| gap
            .as_str()
            .is_some_and(|gap| gap.contains("endpoint_unknown"))),
        "the dropped measured bound is named: {baseline_infra}"
    );
    let candidate_gaps = candidate_infra["gaps"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        candidate_gaps.iter().any(|gap| gap
            .as_str()
            .is_some_and(|gap| gap.contains("queue_not_independently_blocked"))),
        "the unverified ownership keeps the queue uncovered and named: {candidate_infra}"
    );
    assert_eq!(decision["decision"], "inconclusive", "{decision}");
    let reasons = evaluation_reasons(&evaluation);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("attribution gaps could move")),
        "the verdict states the attribution gap: {reasons:?}"
    );
}

/// A record from another clock domain and an overflowed trace both stop the
/// correction: nothing is deducted outside the attempt's clock domain, the
/// truncation stays explicit, and no complete correction or zero missing cost
/// is claimed.
#[test]
fn calibration_controls_expose_a_misaligned_clock_and_overflowed_detail() {
    let _serial = INSTALL.lock().unwrap();
    let baseline = delayed_arm(
        "baseline",
        "calibration-defect-clock-baseline",
        6_000,
        ControlledDefect::ClockMisaligned,
    );
    let mut candidate = granted_arm(
        "candidate",
        "calibration-overflow-candidate",
        2.0,
        &[("call_build_1", 2_500, 1_500)],
    );
    candidate.overflow = true;
    let fixture = controlled_pair("calibration-defects-two", true, baseline, candidate);

    let report = load_json(&fixture.run.join("comparison/report.json"));
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    let decision = load_json(&fixture.run.join("comparison/decision.json"));
    let baseline_infra = &attempt_of_arm(&report, "baseline")["infrastructure"];
    let candidate_infra = &attempt_of_arm(&report, "candidate")["infrastructure"];
    assert!(
        baseline_infra["observed_seconds"].as_f64().unwrap() > 6.0,
        "the raw delay stays visible: {baseline_infra}"
    );
    assert_eq!(
        baseline_infra["deductible_ns"], 0,
        "another clock domain cannot authorize a deduction: {baseline_infra}"
    );
    let gaps = baseline_infra["gaps"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        gaps.iter()
            .any(|gap| gap.as_str().is_some_and(|gap| gap.contains("wrong_domain"))),
        "the misaligned clock domain is named: {baseline_infra}"
    );
    assert_eq!(
        candidate_infra["detail_overflow"], true,
        "{candidate_infra}"
    );
    assert_eq!(
        candidate_infra["activity_overflow"], true,
        "{candidate_infra}"
    );
    let overflow_gaps = candidate_infra["gaps"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        overflow_gaps.iter().any(|gap| gap
            .as_str()
            .is_some_and(|gap| gap.contains("raw_detail_overflow")))
            && overflow_gaps.iter().any(|gap| gap
                .as_str()
                .is_some_and(|gap| gap.contains("compact_activity_overflow"))),
        "the overflow is named, not silently repaired: {candidate_infra}"
    );
    assert_eq!(decision["decision"], "inconclusive", "{decision}");
    let reasons = evaluation_reasons(&evaluation);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("attribution gaps could move")),
        "the verdict states the attribution gap: {reasons:?}"
    );
}

/// Calibration results are not inherited across changed inputs: a policy
/// rewritten after its declaration refuses every dependent advance, and a
/// retained adjusted view that no longer reduces from its retained native
/// trace (whether the collector input or the view changed) is contaminated
/// evidence that cannot support an adoption.
#[test]
fn calibration_reuse_is_invalidated_by_changed_inputs() {
    let _serial = INSTALL.lock().unwrap();
    let baseline = granted_arm(
        "baseline",
        "calibration-reuse-baseline",
        5.0,
        &[
            ("call_build_1", 5_500, 3_500),
            ("call_rebuild_1", 3_000, 1_500),
        ],
    );
    let candidate = granted_arm(
        "candidate",
        "calibration-reuse-candidate",
        2.0,
        &[("call_build_1", 2_500, 1_500)],
    );
    // Declare the calibration binding before the run, then prove a changed
    // calibration contract cannot govern the retained evidence.
    let fixture = Fixture::new("calibration-reuse");
    let binding = harness_core::infrastructure_accounting::binding_clause(
        harness_core::infrastructure_accounting::MetricView::WorkEfficiency,
        harness_core::infrastructure_accounting::Mechanism::None,
    );
    let mut policy: Value = load_json(&fixture.policy);
    policy["uncertainty"] = json!(binding);
    fs::write(&fixture.policy, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let dispatch = fixture.resume();
    assert!(dispatch.status.success(), "{}", text(&dispatch));

    // A rewritten calibration identity after the declaration refuses the
    // dependent advance instead of being silently re-frozen.
    let mut rewritten: Value = load_json(&fixture.policy);
    rewritten["uncertainty"] = json!("unknown evidence stays inconclusive");
    fs::write(
        &fixture.policy,
        serde_json::to_vec_pretty(&rewritten).unwrap(),
    )
    .unwrap();
    let refused = fixture.resume();
    let output = text(&refused);
    assert!(refused.status.success(), "{output}");
    assert!(
        output.contains("policy changed after its declaration"),
        "a changed calibration identity must refuse the advance: {output}"
    );
    // Restoring the declared policy resumes the same retained evidence.
    fs::write(&fixture.policy, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
    seed_controlled_arm(&fixture, &baseline);
    let settle = fixture.resume();
    assert!(settle.status.success(), "{}", text(&settle));

    // The completed baseline row retains the calibration view the declared
    // rule derived from its native capture - the retained result a later
    // resume may only reuse while its collector, rule and clock inputs stay
    // valid. The capture is then changed (the granted admission is retained
    // as an unrelated wait without measured endpoints), so the retained view
    // no longer reduces from the changed input. The decision must refuse it
    // instead of inheriting the earlier calibration.
    let row_path = fixture.arm_dir("baseline").join("row.json");
    let mut row: Value = load_json(&row_path);
    row["infrastructure"] = harness_core::outcome_report::replay_attempt(&row)
        .expect("the retained native capture reduces under the declared rule");
    row["infrastructure_capture"]["admissions"][0]["class"] = json!("unrelated_wait");
    fs::write(&row_path, serde_json::to_vec_pretty(&row).unwrap()).unwrap();
    seed_controlled_arm(&fixture, &candidate);
    let decided = fixture.resume();
    let output = text(&decided);
    assert!(decided.status.success(), "{output}");
    let report = load_json(&fixture.run.join("comparison/report.json"));
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    let decision = load_json(&fixture.run.join("comparison/decision.json"));
    let baseline_attempt = attempt_of_arm(&report, "baseline");
    assert_eq!(
        baseline_attempt["attribution"]["replay"], "mismatch",
        "the changed retained evidence must not reproduce: {baseline_attempt}"
    );
    assert_ne!(decision["decision"], "adopt", "{decision}");
    assert_eq!(decision["decision"], "inconclusive", "{decision}");
    let reasons = evaluation_reasons(&evaluation);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("does not reproduce")),
        "the contaminated evidence is named as the reason: {reasons:?}"
    );
    assert!(
        baseline_attempt["infrastructure"]["observed_seconds"]
            .as_f64()
            .is_some(),
        "the raw observed evidence stays retained: {baseline_attempt}"
    );
}
