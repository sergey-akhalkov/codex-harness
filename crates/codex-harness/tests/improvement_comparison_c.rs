//! Declared-corroboration, comparison-decision and owned-heavy checks split
//! from `improvement_comparison` into the third deadline-sized target.
#![cfg(windows)]

#[path = "fixtures/improvement_comparison_common.rs"]
mod improvement_comparison_common;
use improvement_comparison_common::*;

use harness_core::build_identity;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

/// A missing policy, qualification or workload planning receipt blocks
/// before any comparison workspace, installation or model dispatch exists.
#[test]
fn comparison_preparation_requires_policy_qualification_and_workload_planning() {
    // No usable qualification record: the measured arms never begin.
    let fixture = Fixture::new("unqualified");
    fixture.write_spec(
        &[(
            "qualification",
            json!(fixture.root.join("missing-qualification.json")),
        )],
        None,
    );
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("qualification"), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["attempts"].as_array().unwrap().len(), 0, "{status}");
    assert!(
        !fixture.run.join("comparison").exists(),
        "an unqualified run creates no comparison workspace"
    );

    // An unreadable declaration is refused instead of being reinterpreted.
    let fixture = Fixture::new("bad-policy");
    fs::write(&fixture.policy, "{\"schema\": 9}\n").unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("policy"), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["attempts"].as_array().unwrap().len(), 0, "{status}");
    assert!(!fixture.run.join("comparison").exists(), "{status}");

    // A workload whose own change no longer qualifies blocks the preparation
    // even when the run inputs are otherwise complete.
    let fixture = Fixture::new("unplanned-workload");
    fs::remove_dir_all(fixture.wl.join("openspec/changes/add-workload")).unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("workload"), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["attempts"].as_array().unwrap().len(), 0, "{status}");
    assert!(!fixture.run.join("comparison").exists(), "{status}");

    // Changed frozen acceptance bytes are refused instead of being re-frozen.
    let fixture = Fixture::new("changed-request");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let mut request: Value = serde_json::from_slice(&fs::read(&fixture.request).unwrap()).unwrap();
    request["timeout_seconds"] = json!(30);
    fs::write(
        &fixture.request,
        serde_json::to_vec_pretty(&request).unwrap(),
    )
    .unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("changed since the run declared it"),
        "{output}"
    );
    let status = fixture.status_json();
    assert_eq!(status["attempts"].as_array().unwrap().len(), 0, "{status}");
    assert!(!fixture.run.join("comparison").exists(), "{status}");
}

/// A declared corroboration requirement with too few applicable retained units
/// stays explicitly inconclusive: the comparison selects through the driver's
/// merged owner before evaluating, attaches the run's own receipt to the
/// evaluated report and the frozen policy consumes it end-to-end instead of a
/// summary.
#[test]
fn declared_corroboration_insufficient_units_leave_the_broader_claim_inconclusive() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("corroboration-short");
    fs::write(
        &fixture.policy,
        serde_json::to_vec_pretty(&corroboration_scope_policy()).unwrap(),
    )
    .unwrap();
    fixture.write_spec(&[], None);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("corroboration-short-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("corroboration-short-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 50, 1.5, 1, 1, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));

    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    assert!(
        status["comparison"]["decision"]
            .as_str()
            .unwrap_or_default()
            .contains("outcome=inconclusive"),
        "{status}"
    );
    let evaluation: harness_core::improvement_policy::PolicyEvaluation =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/evaluation.json")).unwrap())
            .unwrap();
    let corroboration = evaluation
        .corroboration
        .clone()
        .expect("the consumed selection is recorded with the decision");
    assert_eq!(
        corroboration.status,
        harness_core::outcome_report::CorroborationState::Inconclusive,
        "{evaluation:?}"
    );
    assert!(!corroboration.ready, "{evaluation:?}");
    assert_eq!(corroboration.required_units, 1, "{evaluation:?}");
    assert!(
        evaluation
            .reasons
            .iter()
            .any(|reason| reason.contains("corroboration selection is inconclusive")),
        "{:?}",
        evaluation.reasons
    );
    let report = load_json(&fixture.run.join("comparison/report.json"));
    assert_eq!(report["corroboration"]["ready"], false, "{report}");
    assert_eq!(report["corroboration"]["requiredUnits"], 1, "{report}");
    let receipt = load_json(&fixture.run.join("corroboration.json"));
    assert_eq!(receipt["status"], "selected", "{receipt}");
    assert_eq!(receipt["required_units"], 1, "{receipt}");
}

/// A declared corroboration requirement with one applicable independent
/// retained unit available is attached to the evaluated report and supplies
/// the missing unit by identity, so the frozen policy reports the broader
/// claim with the adoption; the receipt carries identity and replay references
/// only, never the retained solution.
#[test]
fn declared_corroboration_sufficient_units_are_reported_with_the_adoption() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("corroboration-ready");
    // The prior completed real task is committed before the run spec is
    // re-declared, so the frozen base revision and the prepared builds agree
    // with the source the retained snapshot is taken from.
    fs::write(fixture.proj.join("prior-task.txt"), "prior real task\n").unwrap();
    git(&fixture.proj, &["add", "."]);
    git(
        &fixture.proj,
        &[
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "user.name=Fixture",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "prior task identity",
        ],
    );
    let prior_revision = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fs::write(
        &fixture.policy,
        serde_json::to_vec_pretty(&corroboration_scope_policy()).unwrap(),
    )
    .unwrap();
    fixture.write_spec(&[], None);
    let owner = admit_prior_hypothesis_card(&fixture, "bounded-output");
    let prior = prior_retained_task(
        &fixture,
        &prior_revision,
        &owner,
        "case-c",
        "bounded-output",
    );
    record_prior_retention_on_board(&fixture, &owner, &prior);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("corroboration-ready-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("corroboration-ready-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 50, 1.5, 1, 1, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));

    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    assert!(
        status["comparison"]["decision"]
            .as_str()
            .unwrap_or_default()
            .contains("outcome=adopt"),
        "{status}"
    );
    let comments = fixture.bd_comments(&fixture.card);
    assert!(comments.contains("outcome=adopt"), "{comments}");
    let evaluation: harness_core::improvement_policy::PolicyEvaluation =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/evaluation.json")).unwrap())
            .unwrap();
    let corroboration = evaluation
        .corroboration
        .clone()
        .expect("the consumed selection is recorded with the decision");
    assert_eq!(
        corroboration.status,
        harness_core::outcome_report::CorroborationState::Ready,
        "{evaluation:?}"
    );
    assert!(corroboration.ready, "{evaluation:?}");
    assert_eq!(corroboration.units[0].case_id, "case-c", "{evaluation:?}");
    assert_eq!(corroboration.units[0].owner, owner, "{evaluation:?}");
    let report = load_json(&fixture.run.join("comparison/report.json"));
    assert_eq!(report["corroboration"]["ready"], true, "{report}");
    assert_eq!(
        report["corroboration"]["units"][0]["caseId"], "case-c",
        "{report}"
    );
    let receipt = load_json(&fixture.run.join("corroboration.json"));
    assert_eq!(receipt["selection"]["status"], "ready", "{receipt}");
    assert_eq!(
        receipt["selection"]["units"][0]["caseId"], "case-c",
        "{receipt}"
    );
    assert_eq!(
        receipt["selection"]["units"][0]["owner"], owner,
        "{receipt}"
    );
    assert!(!receipt.to_string().contains("prior solution"), "{receipt}");
}

/// Missing measured counters stay visible: the declared rounds/tool evidence
/// is incomplete, so the frozen policy cannot adopt and the decision records
/// the missing coverage instead of assuming zero. The authoritative
/// counters are removed at their producer: the completed-turn counter and the
/// completed-tool-item counter of the retained receipt observation.
#[test]
fn missing_counter_evidence_stays_inconclusive() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("missing-counters");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("counter-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 50, 30.0, 2, 3, &session);
    let receipt_path = fixture.run.join("baseline-receipt.json");
    let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    receipt["observation"]
        .as_object_mut()
        .unwrap()
        .remove("toolCalls");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("counter-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 50, 1.0, 1, 1, &session);
    let receipt_path = fixture.run.join("candidate-receipt.json");
    let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    receipt["observation"]
        .as_object_mut()
        .unwrap()
        .remove("rounds");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    assert!(
        status["comparison"]["decision"]
            .as_str()
            .unwrap()
            .contains("outcome=inconclusive"),
        "{status}"
    );
    let comments = fixture.bd_comments(&fixture.card);
    assert!(comments.contains("outcome=inconclusive"), "{comments}");

    // A counter no source recorded stays absent (unknown), and the provenance
    // map names exactly the counters that were recorded.
    let baseline_row = load_json(&fixture.arm_dir("baseline").join("row.json"));
    let native = &baseline_row["native_runs"][0];
    assert!(native["tool_operations"].is_null(), "{baseline_row}");
    assert!(native["rounds"].is_number(), "{baseline_row}");
    assert_eq!(
        native["counter_sources"]["rounds"], "receipt-observation:completed-native-turns",
        "{baseline_row}"
    );
    assert_eq!(
        native["counter_sources"]["tool_calls"], "rollout:recorded-outer-calls",
        "{baseline_row}"
    );
    assert!(
        native["counter_sources"]["tool_operations"].is_null(),
        "the missing counter must not claim a source: {baseline_row}"
    );
    let candidate_row = load_json(&fixture.arm_dir("candidate").join("row.json"));
    let native = &candidate_row["native_runs"][0];
    assert!(native["rounds"].is_null(), "{candidate_row}");
    assert!(native["tool_operations"].is_number(), "{candidate_row}");
    assert!(
        native["counter_sources"]["rounds"].is_null(),
        "the missing round counter must not claim a source: {candidate_row}"
    );
}

/// Unrelated or swapped runtime builds cannot authorize a measured arm: the
/// explicit build inputs are refused before any installation or dispatch.
#[test]
fn unrelated_or_swapped_builds_cannot_authorize_a_measured_arm() {
    // Swapped sources: the baseline build records the candidate checkout and
    // the candidate build the project working tree.
    let fixture = Fixture::new("swapped-builds");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture_build(
        &fixture.state,
        "h-build",
        &checkout.path,
        &programs().launcher,
        "baseline",
    );
    fixture_build(
        &fixture.state,
        "ha-build",
        &fixture.proj,
        &programs().launcher,
        "candidate",
    );
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("does not match the frozen baseline source"),
        "a swapped baseline build is refused before any dispatch: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        0,
        "{output}"
    );
    assert!(
        !fixture
            .run
            .join("comparison/baseline/runtime.json")
            .exists(),
        "the refused build is never installed"
    );

    // An unrelated source: the candidate build is a valid build of a
    // different checkout and must not enter the comparison.
    let fixture = Fixture::new("unrelated-build");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    let unrelated = fixture.root.join("unrelated-kit");
    for directory in ["crates/one/src", "global/agents"] {
        fs::create_dir_all(unrelated.join(directory)).unwrap();
    }
    fs::write(
        unrelated.join("Cargo.toml"),
        "[package]\nname = \"unrelated\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    fs::write(unrelated.join("Cargo.lock"), "# unrelated lock\n").unwrap();
    fs::write(unrelated.join("crates/one/src/lib.rs"), "// unrelated\n").unwrap();
    fixture_build(
        &fixture.state,
        "h-build",
        &fixture.proj,
        &programs().launcher,
        "baseline",
    );
    fixture_build(
        &fixture.state,
        "ha-build",
        &unrelated,
        &programs().launcher,
        "candidate",
    );
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("does not match the frozen candidate source"),
        "an unrelated candidate build is refused before any dispatch: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        0,
        "{output}"
    );
    assert!(
        !fixture
            .run
            .join("comparison/candidate/runtime.json")
            .exists(),
        "the refused build is never installed"
    );
}

/// Workload B has its own durable card and its own removal authority: a
/// refusal blocks both measured arms before either dispatch, and the informed
/// approval unblocks them.
#[test]
fn workload_removal_decision_gates_both_measured_arms() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("workload-removal");
    let mut spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    spec["comparison"]["workload_removal"] =
        json!({"proposal": "workload-alpha", "target": "workload-target"});
    let comparison = spec["comparison"].take();
    fixture.write_spec(&[("comparison", comparison)], None);
    let proposed = fixture.feedback(&[
        "removal-propose",
        "--item",
        &fixture.workload_card,
        "--proposal",
        "workload-alpha",
        "--target",
        "workload-target",
        "--evidence",
        "evidence-b",
        "--loss",
        "workload-loss",
    ]);
    assert!(proposed.status.success(), "{}", text(&proposed));
    let refused = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.workload_card,
        "--decision",
        "refuse",
        "--proposal",
        "workload-alpha",
        "--target",
        "workload-target",
        "--loss",
        "workload-loss",
    ]);
    assert!(refused.status.success(), "{}", text(&refused));
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("workload")
            && (output.contains("refused by the user") || output.contains("declined")),
        "the workload removal refusal gates both arms: {output}"
    );
    let attempts = fixture.cursor()["attempts"].as_array().unwrap().len();
    assert_eq!(attempts, 0, "neither measured arm starts: {output}");

    // The informed approval unblocks the baseline arm first.
    let approved = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.workload_card,
        "--decision",
        "approve",
        "--proposal",
        "workload-alpha",
        "--target",
        "workload-target",
        "--actions",
        "experiment",
        "--loss",
        "workload-loss",
    ]);
    assert!(approved.status.success(), "{}", text(&approved));
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 1, "{output}");
    assert_eq!(attempts[0]["role"], "baseline");
}

/// The observed conversation must carry the declared model and effort: a
/// rollout that records a different model or effort refuses that arm instead
/// of entering the comparison as unverified evidence.
#[test]
fn wrong_observed_model_or_effort_refuses_the_arm() {
    let _serial = INSTALL.lock().unwrap();
    // Wrong observed model.
    let fixture = Fixture::new("wrong-model");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("wrong-model-session");
    fixture.simulate_arm("baseline", "baseline", "solved", 50, 5.0, 2, 3, &session);
    rewrite_rollout(&fixture, "baseline", &session, "another-model", "low");
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("recorded model another-model instead of the declared fixture-glyph-1"),
        "a wrong observed model refuses the arm: {output}"
    );
    assert_eq!(
        fixture.cursor()["comparison"]["baseline"]["accepted"],
        Value::Null
    );
    assert!(
        fixture.cursor()["comparison"]["baseline"]["condition"]
            .as_str()
            .is_some_and(|condition| condition.contains("another-model")),
        "the refusal is retained on the arm"
    );

    // Wrong observed reasoning effort.
    let fixture = Fixture::new("wrong-effort");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("wrong-effort-session");
    fixture.simulate_arm("baseline", "baseline", "solved", 50, 5.0, 2, 3, &session);
    rewrite_rollout(&fixture, "baseline", &session, "fixture-glyph-1", "xhigh");
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("recorded reasoning effort xhigh instead of the declared low"),
        "a wrong observed effort refuses the arm: {output}"
    );
    assert_eq!(
        fixture.cursor()["comparison"]["baseline"]["accepted"],
        Value::Null
    );
}

/// Controlled protocol observations through the same real path: a rollout that
/// records another model or effort refuses that arm, and nothing is adopted.
#[test]
fn real_control_dispatches_refuse_wrong_observed_model_and_effort() {
    let _serial = INSTALL.lock().unwrap();
    for (name, variable, wrong, declared) in [
        (
            "real-wrong-model",
            "HARNESS_IMPROVEMENT_FIXTURE_MODEL",
            "another-model",
            "fixture-glyph-1",
        ),
        (
            "real-wrong-effort",
            "HARNESS_IMPROVEMENT_FIXTURE_EFFORT",
            "xhigh",
            "low",
        ),
    ] {
        let fixture = Fixture::new(name);
        let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
        fixture.prepare_real_builds(&checkout);
        fixture.start_with_ready_candidate(&checkout, false);
        let resume = fixture.resume_dispatched(&[]);
        assert!(resume.status.success(), "{}", text(&resume));
        let baseline_receipt = attempt_receipt(&fixture, "base-1");
        wait_for_terminal_receipt(&baseline_receipt);
        // Settle the baseline and dispatch the candidate under the wrong
        // controlled observation.
        let resume = fixture.resume_dispatched(&[(variable, wrong)]);
        assert!(resume.status.success(), "{}", text(&resume));
        let candidate_receipt = attempt_receipt(&fixture, "cand-1");
        wait_for_terminal_receipt(&candidate_receipt);
        let resume = fixture.resume();
        let output = text(&resume);
        assert!(resume.status.success(), "{output}");
        assert!(
            output.contains(&format!(
                "recorded {} {wrong}",
                if variable.ends_with("MODEL") {
                    "model"
                } else {
                    "reasoning effort"
                }
            )) || output.contains(wrong),
            "the wrong observed {declared} is refused: {output}"
        );
        assert_eq!(
            fixture.cursor()["comparison"]["candidate"]["accepted"],
            Value::Null
        );
        assert!(
            !fixture
                .bd_comments(&fixture.card)
                .contains("benefit-gate v2"),
            "no decision is published from a refused arm"
        );
    }
}

/// Owned heavy contention plus the visible fixture conversation. The controller
/// captures the admission, the host observation and the report. This is not an
/// actual-model comparison. The fixture emits the selected route's opaque
/// process id and records the control owner's live OS link for the process it
/// spawned, so the deduction binds through verified identity, not pid equality.
#[test]
fn owned_heavy_contention_reaches_the_native_report() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("heavy-noise");
    let policy_text = fs::read_to_string(&fixture.policy).unwrap();
    let mut policy: Value = serde_json::from_str(&policy_text).unwrap();
    policy["uncertainty"] = json!(harness_core::infrastructure_accounting::binding_clause(
        harness_core::infrastructure_accounting::MetricView::WorkEfficiency,
        harness_core::infrastructure_accounting::Mechanism::None,
    ));
    fs::write(&fixture.policy, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_real_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

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
    let settle = fixture.resume_in_process(&[]);
    let settle_output = text(&settle);
    assert!(
        settle.status.success(),
        "settlement resume failed: {settle_output}\nfirst resume: {output}"
    );

    let evidence = fixture.run.join("comparison").join("queue-evidence");
    let mut documents = Vec::new();
    if evidence.is_dir() {
        for entry in fs::read_dir(&evidence).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                documents.extend(harness_core::heavy_command_trace::read_directory(&path).unwrap());
            }
        }
    }
    assert!(
        documents.iter().any(|view| matches!(
            harness_core::heavy_command_trace::interpret(view).delay,
            harness_core::heavy_command_trace::QueueDelay::UnrelatedWait { timing }
                if matches!(timing.endpoint, harness_core::heavy_command_trace::TimingBound::Measured(_))
        )),
        "the native admission did not record a measured unrelated wait: {documents:?}\n{output}"
    );
    let cursor = fixture.cursor();
    let row_path = cursor["comparison"]["baseline"]["row"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            fixture
                .run
                .join("comparison")
                .join("baseline")
                .join("row.json")
        });
    let row = fs::read(&row_path).unwrap_or_else(|error| {
        panic!(
            "the controller wrote no baseline row at {}: {error}; cursor={cursor}",
            row_path.display()
        )
    });
    let row: Value = serde_json::from_slice(&row).unwrap();
    assert!(
        row["infrastructure_capture"]["admissions"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "the controller did not attach the captured admission: {row}"
    );
    let summarized = harness_core::outcome_report::summarize_attempts(std::slice::from_ref(&row))
        .expect("native report");
    let requests = row["infrastructure_capture"]["requests"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        requests.iter().any(|request| {
            request["structural_single_tool"] == true
                && request["tool_call_id"] == "call_heavy_blocked_1"
                && request["command_id"] == "call_heavy_blocked_1"
                && request["start_ns"].is_null()
                && request["end_ns"].is_null()
                && request["wait_only"].is_null()
        }),
        "the native rollout was not correlated through call_id and turn_id: {requests:?}"
    );
    let activity = &row["infrastructure_capture"]["activity"];
    assert!(
        activity.as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item["id"] == "call_heavy_blocked_1" && item["placement"] == "source")
        }),
        "the blocked command must use the producer timestamp, not a receipt: {activity}"
    );
    let admissions = &row["infrastructure_capture"]["admissions"];
    assert!(
        admissions.as_array().is_some_and(|items| {
            items.iter().any(|item| {
                item["tool_call_id"] == "call_heavy_blocked_1"
                    && item["command_id"] == "call_heavy_blocked_1"
                    && item["class"] == "unrelated_wait"
            })
        }),
        "the recorded OS link did not bind the admission to the command item: {admissions}"
    );
    let unlabeled = documents.iter().any(|view| {
        matches!(
            view,
            harness_core::heavy_command_trace::EvidenceView::Record(document)
                if document.correlation.command_id.is_none()
                    && document.correlation.tool_call_id.is_none()
        )
    });
    assert!(
        unlabeled,
        "the public admission must not depend on a seeded command label"
    );
    let infra = &summarized["attempts"][0]["infrastructure"];
    assert!(infra["observed_seconds"].as_f64().is_some(), "{infra}");
    assert_ne!(infra["proven_zero_queue"], true);
    assert!(
        infra["deductible_ns"].as_u64().unwrap_or(0) > 0,
        "a serial agent blocked only on the unrelated holder must keep a conservative deduction: {infra}"
    );
    assert!(
        infra["adjusted_low_ns"].as_u64().unwrap_or(0)
            <= infra["adjusted_high_ns"].as_u64().unwrap_or(0),
        "{infra}"
    );
    assert!(
        infra["excluded_requests"]
            .as_array()
            .is_some_and(|ids| ids.is_empty()),
        "the task launch is not a wait-only request: {infra}"
    );
    assert!(
        infra["unresolved_ns"].as_u64().unwrap_or(0) > 0
            || infra["gaps"]
                .as_array()
                .is_some_and(|gaps| gaps.iter().any(|gap| {
                    gap.as_str().is_some_and(|gap| {
                        gap.contains("bracket")
                            || gap.contains("request_interval")
                            || gap.contains("endpoint")
                    })
                })),
        "boundary uncertainty must stay visible: {infra}"
    );
}

/// The same owned heavy contention without the control owner's OS link. The
/// item still carries the selected route's opaque producer id and the
/// admission still records its own ancestry, but no verified association
/// exists, so the admission must not bind and no time may be deducted. This is
/// the native missing-OS-identity counterexample, not an actual-model
/// comparison.
#[test]
fn owned_heavy_without_a_control_os_link_cannot_deduct() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("heavy-no-link");
    let policy_text = fs::read_to_string(&fixture.policy).unwrap();
    let mut policy: Value = serde_json::from_str(&policy_text).unwrap();
    policy["uncertainty"] = json!(harness_core::infrastructure_accounting::binding_clause(
        harness_core::infrastructure_accounting::MetricView::WorkEfficiency,
        harness_core::infrastructure_accounting::Mechanism::None,
    ));
    fs::write(&fixture.policy, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_real_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

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
        .env("HARNESS_IMPROVEMENT_FIXTURE_HEAVY_NO_OS_LINK", "1")
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
    let settle = fixture.resume_in_process(&[]);
    let settle_output = text(&settle);
    assert!(
        settle.status.success(),
        "settlement resume failed: {settle_output}\nfirst resume: {output}"
    );
    let evidence_dir = fixture
        .run
        .join("comparison")
        .join("queue-evidence")
        .join("base-1");
    let entries = fs::read_dir(&evidence_dir)
        .expect("the attempt evidence directory exists")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(
        entries.iter().any(|name| name.ends_with(".ancestry")),
        "the admission did not record its own ancestry: {entries:?}"
    );
    assert!(
        !entries.iter().any(|name| name.ends_with(".link")),
        "the negative control must not record an OS link: {entries:?}"
    );

    let cursor = fixture.cursor();
    let row_path = cursor["comparison"]["baseline"]["row"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            fixture
                .run
                .join("comparison")
                .join("baseline")
                .join("row.json")
        });
    let row: Value = serde_json::from_slice(&fs::read(&row_path).unwrap()).unwrap();
    let summarized = harness_core::outcome_report::summarize_attempts(std::slice::from_ref(&row))
        .expect("native report");
    let admissions = &row["infrastructure_capture"]["admissions"];
    assert!(
        admissions.as_array().is_some_and(|items| {
            items.iter().any(|item| {
                item["tool_call_id"].is_null()
                    && item["command_id"].is_null()
                    && item["class"] == "unrelated_wait"
            })
        }),
        "an opaque process id without an OS link must not bind the admission: {admissions}"
    );
    let infra = &summarized["attempts"][0]["infrastructure"];
    assert!(infra["observed_seconds"].as_f64().is_some(), "{infra}");
    assert_ne!(infra["proven_zero_queue"], true);
    assert!(
        infra["deductible_ns"].as_u64().unwrap_or(1) == 0,
        "missing OS identity must not create a deduction: {infra}"
    );
    assert!(
        infra["adjusted_low_ns"].as_u64().unwrap_or(0)
            <= infra["adjusted_high_ns"].as_u64().unwrap_or(0),
        "{infra}"
    );
}

/// A declared subtractive comparison records the removed burden and each
/// arm's retained consumption of it, and the accounting and policy gates
/// consume that record: an exercised removal with retained checks can adopt,
/// while a candidate that still consumes the removed burden stays
/// inconclusive instead of banking a faster result.
#[test]
fn subtractive_comparison_rows_carry_consumption_through_the_gates() {
    let _serial = INSTALL.lock().unwrap();

    // The candidate treatment removes the fixture skill the baseline arm
    // consumes.
    let fixture = Fixture::new("subtractive-rows");
    fixture.write_spec(
        &[(
            "removal",
            json!({"proposal": "proposal-skill", "target": "arm-skill"}),
        )],
        None,
    );
    let mut checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fs::remove_dir_all(checkout.path.join(".agents/skills/arm-skill")).unwrap();
    // Git does not track empty directories: keep the declared skills location
    // present in the candidate revision without the removed skill.
    fs::write(checkout.path.join(".agents/skills/.gitkeep"), "\n").unwrap();
    git(&checkout.path, &["add", "-A"]);
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
            "remove the fixture skill arm-skill",
        ],
    );
    checkout.revision = git_output(&checkout.path, &["rev-parse", "HEAD"]);
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, true);

    // First resume: preparation plus the refused pre-submission baseline
    // dispatch; the retained receipt then settles the baseline arm.
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("subtractive-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "{status}"
    );

    // The declared removal needs the informed decision before the candidate's
    // measured conversation; the approval covers the experiment stage.
    let proposed = fixture.feedback(&[
        "removal-propose",
        "--item",
        &fixture.card,
        "--proposal",
        "proposal-skill",
        "--target",
        "arm-skill",
        "--evidence",
        "evidence-1",
        "--loss",
        "synthetic-loss",
        "--preview",
        "preview-1",
    ]);
    assert!(proposed.status.success(), "{}", text(&proposed));
    let approved = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "approve",
        "--proposal",
        "proposal-skill",
        "--target",
        "arm-skill",
        "--actions",
        "experiment",
        "--loss",
        "synthetic-loss",
    ]);
    assert!(approved.status.success(), "{}", text(&approved));
    // One resume clears the recorded block, the next dispatches the approved
    // candidate conversation (refused before submission by the fixture
    // launcher) and the retained receipt settles it.
    for _ in 0..2 {
        let resume = fixture.resume();
        assert!(resume.status.success(), "{}", text(&resume));
    }
    let session = session_id("subtractive-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 50, 1.5, 1, 1, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );

    // The comparison rows carry the declared treatment and each arm's
    // consumption in the shapes the merged accounting consumes.
    let baseline_row: Value =
        serde_json::from_slice(&fs::read(fixture.arm_dir("baseline").join("row.json")).unwrap())
            .unwrap();
    let candidate_row: Value =
        serde_json::from_slice(&fs::read(fixture.arm_dir("candidate").join("row.json")).unwrap())
            .unwrap();
    assert_eq!(
        baseline_row["treatment"]["kind"], "subtraction",
        "{baseline_row}"
    );
    assert_eq!(
        baseline_row["treatment"]["removed"], "arm-skill",
        "{baseline_row}"
    );
    assert_eq!(
        baseline_row["consumption"]["capability"], "arm-skill",
        "{baseline_row}"
    );
    assert_eq!(
        baseline_row["consumption"]["status"], "consumed",
        "{baseline_row}"
    );
    assert_eq!(
        candidate_row["treatment"]["kind"], "subtraction",
        "{candidate_row}"
    );
    assert_eq!(
        candidate_row["treatment"]["removed"], "arm-skill",
        "{candidate_row}"
    );
    assert_eq!(
        candidate_row["consumption"]["capability"], "arm-skill",
        "{candidate_row}"
    );
    assert_eq!(
        candidate_row["consumption"]["status"], "absent",
        "{candidate_row}"
    );
    for row in [&baseline_row, &candidate_row] {
        assert!(
            row["consumption"]["evidence"]
                .as_str()
                .is_some_and(|evidence| !evidence.is_empty()),
            "the consumption record names its retained evidence: {row}"
        );
    }

    // The outcome report and the frozen policy consume the same record: the
    // unit is exercised with retained checks and the decision is bound to it.
    let report: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/report.json")).unwrap())
            .unwrap();
    for row in report["attempts"].as_array().unwrap() {
        assert_eq!(row["treatment_kind"], "subtractive", "{row}");
        assert_eq!(row["removed_burden"], "arm-skill", "{row}");
        assert_eq!(
            row["consumption_evidence"]["capability"], "arm-skill",
            "{row}"
        );
        assert_eq!(row["consumption_evidence"]["evidenced"], true, "{row}");
    }
    let unit = &report["units"][0];
    assert_eq!(unit["subtractive"], true, "{unit}");
    assert_eq!(unit["removed_burden"], "arm-skill", "{unit}");
    assert_eq!(unit["applicability"], "exercised", "{unit}");
    assert_eq!(unit["retained_checks"], true, "{unit}");
    assert_eq!(unit["evidence_complete"], true, "{unit}");
    assert_eq!(unit["positive_effect"], true, "{unit}");
    let evaluation: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/evaluation.json")).unwrap())
            .unwrap();
    assert_eq!(evaluation["decision"], "adopt", "{evaluation}");

    // Stale context after removal: the same declared removal with a candidate
    // that still consumes the skill records `consumed` on both arms, so a
    // faster result cannot pass the subtractive gate.
    let stale = Fixture::new("subtractive-stale");
    stale.write_spec(
        &[(
            "removal",
            json!({"proposal": "proposal-skill", "target": "arm-skill"}),
        )],
        None,
    );
    let checkout = stale.prepare_candidate(Some("// candidate implementation\n"));
    stale.prepare_builds(&checkout);
    stale.start_with_ready_candidate(&checkout, true);
    let resume = stale.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("subtractive-stale-baseline");
    stale.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let resume = stale.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let proposed = stale.feedback(&[
        "removal-propose",
        "--item",
        &stale.card,
        "--proposal",
        "proposal-skill",
        "--target",
        "arm-skill",
        "--evidence",
        "evidence-1",
        "--loss",
        "synthetic-loss",
        "--preview",
        "preview-1",
    ]);
    assert!(proposed.status.success(), "{}", text(&proposed));
    let approved = stale.feedback(&[
        "removal-decide",
        "--item",
        &stale.card,
        "--decision",
        "approve",
        "--proposal",
        "proposal-skill",
        "--target",
        "arm-skill",
        "--actions",
        "experiment",
        "--loss",
        "synthetic-loss",
    ]);
    assert!(approved.status.success(), "{}", text(&approved));
    for _ in 0..2 {
        let resume = stale.resume();
        assert!(resume.status.success(), "{}", text(&resume));
    }
    let session = session_id("subtractive-stale-candidate");
    stale.simulate_arm("candidate", "candidate", "solved", 50, 1.5, 1, 1, &session);
    let resume = stale.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = stale.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    let candidate_row: Value =
        serde_json::from_slice(&fs::read(stale.arm_dir("candidate").join("row.json")).unwrap())
            .unwrap();
    assert_eq!(
        candidate_row["consumption"]["status"], "consumed",
        "the candidate still consumes the removed burden: {candidate_row}"
    );
    let report: Value =
        serde_json::from_slice(&fs::read(stale.run.join("comparison/report.json")).unwrap())
            .unwrap();
    let unit = &report["units"][0];
    assert_eq!(unit["applicability"], "unknown", "{unit}");
    assert_eq!(unit["evidence_complete"], false, "{unit}");
    assert_eq!(unit["positive_effect"], false, "{unit}");
    let evaluation: Value =
        serde_json::from_slice(&fs::read(stale.run.join("comparison/evaluation.json")).unwrap())
            .unwrap();
    assert_ne!(evaluation["decision"], "adopt", "{evaluation}");
    assert!(
        evaluation["reasons"]
            .as_array()
            .is_some_and(|reasons| reasons
                .iter()
                .any(|reason| reason.as_str().unwrap_or("").contains("not established"))),
        "{evaluation}"
    );
}

/// A comparison-level dispatch refused before submission is re-attempted on
/// an explicit resume: the recorded same-arm selection is reused instead of
/// selecting the arm again, and the re-attempt is prepared through the
/// dispatch gate as the next arm attempt.
#[test]
fn refused_comparison_dispatch_resumes_with_the_recorded_selection() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("resume-selection");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

    // The fixture launcher cannot execute a conversation: the first dispatch
    // is refused before submission and no model request is made, after the
    // shared runtime-selection owner recorded this arm's runtime.
    let refused = fixture.resume();
    let output = text(&refused);
    assert!(refused.status.success(), "{output}");
    assert!(output.contains("before submission"), "{output}");
    let cursor = fixture.cursor();
    let first = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["role"] == "baseline")
        .cloned()
        .expect("the refused baseline dispatch is recorded");
    assert_eq!(first["id"], "base-1", "{cursor}");
    assert_eq!(first["state"], "failed", "{cursor}");
    assert!(
        first["reason"]
            .as_str()
            .unwrap_or("")
            .contains("no model request was made"),
        "{cursor}"
    );
    assert_eq!(cursor["selected_variant"], "baseline", "{cursor}");
    let recorded_runtime = cursor["selected_runtime"].clone();
    assert!(recorded_runtime.is_string(), "{cursor}");

    // The explicit resume re-attempts the refused dispatch: the recorded
    // same-arm selection is reused instead of selecting the arm again, and
    // the re-attempt is prepared through the dispatch gate as the next arm
    // attempt (here the fixture launcher refuses it again before submission).
    let resumed = fixture.resume();
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    let cursor = fixture.cursor();
    assert_eq!(cursor["selected_variant"], "baseline", "{cursor}");
    assert_eq!(cursor["selected_runtime"], recorded_runtime, "{cursor}");
    let variant_selections = cursor["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|effect| effect["kind"] == "variant-selected")
        .count();
    assert_eq!(
        variant_selections, 1,
        "the re-attempt reuses the recorded selection: {cursor}"
    );
    let second = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == "base-2")
        .cloned()
        .expect("the re-attempt is dispatched as the next arm attempt");
    assert_eq!(second["state"], "failed", "{cursor}");
    assert!(
        second["reason"]
            .as_str()
            .unwrap_or("")
            .contains("no model request was made"),
        "{cursor}"
    );
    assert!(
        cursor["effects"].as_array().unwrap().iter().any(|effect| {
            effect["kind"] == "dispatch-prepared"
                && effect["detail"]
                    .as_str()
                    .unwrap_or("")
                    .contains("attempt=base-2")
        }),
        "the re-attempt passed the gate into a prepared dispatch: {cursor}"
    );
    assert!(
        fixture
            .run
            .join("assignments")
            .join("base-2.json")
            .is_file(),
        "the re-attempt wrote its exact assignment"
    );
}

/// The supervisor identity is frozen when the comparison window is prepared.
/// Replacing the retained record (what candidate code with run-state reach
/// would attempt) refuses every later advance, retains the refusal on the run
/// and starts no new measured attempt; restoring the exact identity continues
/// the window.
#[test]
fn the_frozen_supervisor_identity_governs_the_comparison_window() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("supervisor-drift");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("before submission"), "{output}");
    let record_path = fixture.run.join("comparison/supervisor.json");
    let record = load_json(&record_path);
    let running = build_identity::hash_file(&manager()).unwrap();
    assert_eq!(
        record["executableSha256"].as_str().unwrap(),
        running.to_ascii_lowercase(),
        "{record}"
    );

    let mut tampered = record.clone();
    tampered["executableSha256"] = json!("0".repeat(64));
    fs::write(&record_path, serde_json::to_vec_pretty(&tampered).unwrap()).unwrap();
    let attempts_before = fixture.cursor()["attempts"].as_array().unwrap().len();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("supervisor executable changed"),
        "the changed frozen supervisor must be refused with its cause: {output}"
    );
    let status = fixture.status_json();
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts_before,
        "a changed supervisor starts no new measured attempt: {status}"
    );
    assert_eq!(
        status["comparison"]["baseline"]["accepted"],
        Value::Null,
        "no arm result enters the comparison under a changed supervisor: {status}"
    );
    let cursor = fixture.cursor();
    assert!(
        cursor["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|effect| effect["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("supervisor executable changed"))),
        "the refusal stays in the run's own recovery history: {cursor}"
    );
    // A repeated advance keeps refusing the changed supervisor instead of
    // being cleared into progress.
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("supervisor executable changed"),
        "the guard keeps refusing while the identity stays changed: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts_before,
        "{output}"
    );

    fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        !output.contains("supervisor executable changed"),
        "the restored frozen identity continues the window: {output}"
    );
}
