//! Seeded comparison, workload-artifact, supervisor, calibration and
//! owned-heavy checks split from `improvement_comparison`.
#![cfg(windows)]

#[path = "improvement_comparison_common.rs"]
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

/// A forged candidate result is rejected by the frozen checker, the decision
/// is published, and nothing is integrated or activated.
#[test]
fn a_forged_candidate_result_is_rejected_and_never_integrated() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("forged");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("before submission"),
        "the fixture launcher cannot execute a conversation: {output}"
    );
    let status = fixture.status_json();
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 1, "{cursor}");
    assert_eq!(attempts[0]["role"], "baseline");
    assert_eq!(attempts[0]["state"], "failed");
    assert!(
        status["comparison"]["baseline"]["runtime"].is_string(),
        "the baseline installation receipt is retained: {status}"
    );
    assert!(
        fixture.run.join("comparison/bindings.json").is_file(),
        "the prepared bindings are retained"
    );
    assert!(
        fixture.run.join("comparison/planning.json").is_file(),
        "the workload's own planning receipt is retained"
    );

    // A receipt that no longer matches the accepted dispatch generation is
    // never settled into the comparison.
    let session = session_id("baseline-session");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let receipt_path = fixture.run.join("baseline-receipt.json");
    let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    receipt["originatingLead"]["runGeneration"] = json!("gen-2");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert!(
        fixture.cursor()["attempts"][0]["reason"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("generation"),
        "{status}"
    );
    assert_eq!(
        status["comparison"]["baseline"]["accepted"],
        Value::Null,
        "an unverified generation never enters the comparison: {status}"
    );

    // Restoring the accepted generation lets the retained evidence settle.
    receipt["originatingLead"]["runGeneration"] = json!("gen-1");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "the verified baseline solution is accepted: {status}"
    );
    assert!(
        status["comparison"]["baseline"]["revision"].is_string(),
        "{status}"
    );
    assert!(
        status["comparison"]["baseline"]["oracle"].is_string(),
        "{status}"
    );

    // The candidate conversation returns a forged solution: the frozen
    // checker rejects it and the frozen policy records a reject that cannot
    // become a board decision record because the matched evidence is absent.
    let session = session_id("candidate-session");
    let slot = fixture.simulate_arm("candidate", "candidate", "wrong", 60, 2.0, 1, 1, &session);
    // Uncommitted or untracked output never enters acceptance.
    fs::write(slot.join("scratch-output.txt"), "forged scratch\n").unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("uncommitted or untracked"),
        "uncommitted output is refused instead of entering acceptance: {output}"
    );
    assert_eq!(
        fixture.cursor()["comparison"]["candidate"]["accepted"],
        Value::Null
    );
    fs::remove_file(slot.join("scratch-output.txt")).unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], false,
        "{status}"
    );
    assert!(
        status["comparison"]["decision"]
            .as_str()
            .unwrap()
            .contains("outcome=reject"),
        "{status}"
    );
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    let evaluation: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/evaluation.json")).unwrap())
            .unwrap();
    assert_eq!(evaluation["decision"], "reject", "{evaluation}");
    assert!(
        evaluation["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                .as_str()
                .unwrap()
                .contains("independent acceptance failed")),
        "{evaluation}"
    );
    // The supported non-adoption is published on the real board, parses as a
    // complete non-adoption, and carries the same exact lineage the
    // integration owner re-derives: raw base and candidate revisions plus the
    // declared acceptance reference.
    let comments =
        harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, &fixture.card)
            .unwrap();
    let records = harness_core::benefit_gate::parse_gate_comments(&comments);
    let joined = comments.join("\n");
    assert!(joined.contains("benefit-gate v2"), "{joined}");
    assert!(joined.contains("outcome=reject"), "{joined}");
    let assessment =
        harness_core::benefit_gate::assess(&records, &fixture.card).expect("attributable");
    assert_eq!(
        assessment.verdict,
        harness_core::benefit_gate::Verdict::NonAdoption
    );
    assert!(!harness_core::benefit_gate::default_allowed(
        &records,
        &fixture.card
    ));
    let expected_revisions = format!("{}..{}", checkout.base, checkout.revision);
    assert!(
        assessment
            .latest
            .revisions
            .as_deref()
            .is_some_and(|revisions| revisions == expected_revisions),
        "{joined}"
    );
    assert!(
        !joined.contains("matched="),
        "the non-adoption never fabricates a matched count: {joined}"
    );
    // The newest non-adoption authorizes no integration.
    let bindings: harness_core::improvement_experiment::ExperimentBindings =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/bindings.json")).unwrap())
            .unwrap();
    let reject_evaluation: harness_core::improvement_policy::PolicyEvaluation =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/evaluation.json")).unwrap())
            .unwrap();
    let spec: harness_core::improvement_loop::RunSpec =
        serde_json::from_slice(&fs::read(fixture.run.join("spec.json")).unwrap()).unwrap();
    let experiment = fixture.cursor()["experiment"].as_str().unwrap().to_owned();
    fs::create_dir_all(fixture.root.join("integration-evidence")).unwrap();
    let outcome = harness_core::improvement_activation::integrate(
        &harness_core::improvement_activation::IntegrationRequest {
            spec: &spec,
            bindings: &bindings,
            evaluation: &reject_evaluation,
            experiment,
            removal_required: false,
            frozen_removal: None,
            mainline: fixture.proj.clone(),
            check: harness_core::improvement_activation::CheckSpec {
                program: fixture.proj.join("Cargo.toml"),
                args: Vec::new(),
                timeout: std::time::Duration::from_secs(30),
            },
            evidence: fixture.root.join("integration-evidence"),
            prior: None,
        },
    )
    .unwrap();
    assert!(
        matches!(
            outcome,
            harness_core::improvement_activation::IntegrationOutcome::Blocked(_)
        ),
        "a reject decision must not authorize integration"
    );
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        base,
        "a rejected experiment never changes the accepted mainline"
    );

    // A repeated resume reuses the retained verdict: no replay, no dispatch,
    // no second verdict record.
    let before = fixture.cursor();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert_eq!(fixture.cursor()["attempts"], before["attempts"]);
    assert_eq!(
        fixture.cursor()["comparison"]["decision"],
        before["comparison"]["decision"]
    );

    // Rejection recovery consumes the retained verdict through the real
    // controller path: with continuous supervision the decision boundary
    // restores the accepted runtime through the same shared runtime-selection
    // owner the measured dispatches consumed, verifies the identity actually
    // selected, and retains the restoration, the candidate commit, the
    // measured rows and the decision evidence. No model work and no removal
    // approval are involved in restoring the accepted baseline.
    let baseline_runtime: harness_core::improvement_runtime::ArmRuntime = serde_json::from_slice(
        &fs::read(fixture.arm_dir("baseline").join("runtime.json")).unwrap(),
    )
    .unwrap();
    let cursor_before = fixture.cursor();
    assert_eq!(
        cursor_before["selected_variant"], "candidate",
        "{cursor_before}"
    );
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let consumed = fixture.resume();
    let output = text(&consumed);
    assert!(consumed.status.success(), "{output}");
    let cursor = fixture.cursor();
    assert_eq!(cursor["selected_variant"], "baseline", "{cursor}\n{output}");
    assert_eq!(
        PathBuf::from(cursor["selected_runtime"].as_str().unwrap_or_default())
            .canonicalize()
            .unwrap(),
        baseline_runtime.variant.build.canonicalize().unwrap(),
        "the recorded selection is the accepted baseline runtime: {cursor}"
    );
    assert_eq!(
        cursor["selected_identity"],
        format!(
            "sha256:{}",
            &baseline_runtime.variant.source_sha256
                [..16.min(baseline_runtime.variant.source_sha256.len())]
        ),
        "{cursor}"
    );
    let (active, _) = harness_core::build_selection::selected(&fixture.state).unwrap();
    assert_eq!(
        active.canonicalize().unwrap(),
        baseline_runtime.variant.build.canonicalize().unwrap(),
        "the shared runtime-selection journal resolves to the accepted baseline"
    );
    let restoration: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("restoration.json")).unwrap()).unwrap();
    assert_eq!(restoration["status"], "restored", "{restoration}");
    assert_eq!(restoration["decision"], "reject", "{restoration}");
    assert_eq!(restoration["selected"], "candidate", "{restoration}");
    assert_eq!(
        PathBuf::from(restoration["runtime"].as_str().unwrap_or_default())
            .canonicalize()
            .unwrap(),
        baseline_runtime.variant.build.canonicalize().unwrap(),
        "{restoration}"
    );
    assert_eq!(
        restoration["record_sha256"],
        build_identity::hash_file(&baseline_runtime.variant.build.join("build.json")).unwrap(),
        "{restoration}"
    );
    let status = fixture.status_json();
    assert_eq!(status["phase"], "idle", "{status}\n{output}");
    assert!(
        status["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("accepted runtime"),
        "the idle condition records the verified settlement: {status}"
    );
    // The rejected candidate stays outside the accepted mainline with its
    // commit, its failed acceptance and every measurement/decision artifact
    // retained.
    assert!(
        checkout.path.is_dir(),
        "the candidate checkout is preserved"
    );
    assert_eq!(git_output(&fixture.proj, &["rev-parse", "HEAD"]), base);
    assert!(!fixture.run.join("integration.json").is_file());
    assert!(!fixture.run.join("activation.json").is_file());
    for artifact in [
        "comparison/decision.json",
        "comparison/report.json",
        "comparison/evaluation.json",
        "lineage.json",
    ] {
        assert!(
            fixture.run.join(artifact).is_file(),
            "{artifact} is retained"
        );
    }
    // Restoration is a selection, never a dispatch or a build: exactly one
    // restoration selection is journaled and the attempt history is unchanged.
    assert_eq!(cursor["attempts"], cursor_before["attempts"], "{cursor}");
    let effects_before = cursor_before["effects"].as_array().unwrap().len();
    let variants = cursor["effects"]
        .as_array()
        .unwrap()
        .iter()
        .skip(effects_before)
        .filter(|effect| effect["kind"] == "variant-selected")
        .count();
    assert_eq!(
        variants, 1,
        "exactly one restoration selection is journaled: {cursor}"
    );
    // A later resume leaves the restored runtime untouched: the verification
    // is repeated as state, not re-run as work.
    let settled = fixture.resume();
    assert!(settled.status.success(), "{}", text(&settled));
    let cursor = fixture.cursor();
    assert_eq!(cursor["selected_variant"], "baseline", "{cursor}");
    assert_eq!(
        PathBuf::from(cursor["selected_runtime"].as_str().unwrap_or_default())
            .canonicalize()
            .unwrap(),
        baseline_runtime.variant.build.canonicalize().unwrap(),
        "no later resume switches the restored selection away: {cursor}"
    );
}

/// An accepted, measurably faster candidate yields the evidence-bound adopt
/// decision through the frozen policy; the removal authority is re-checked
/// before the candidate's own measured dispatch and nothing is integrated or
/// activated by the verdict.
#[test]
fn an_accepted_faster_candidate_is_adopted_without_activation_or_integration() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("adopt");
    // The baseline arm consumes the skill the candidate treatment removes, so
    // the declared removal is exercised with retained consumption evidence;
    // a declared removal without that evidence cannot adopt.
    let skill = fixture.proj.join(".agents/skills/target-beta");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: target-beta\ndescription: Fixture skill.\n---\n\nfixture skill body\n",
    )
    .unwrap();
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
            "advertise the removable fixture skill",
        ],
    );
    // Re-declare the run spec on the source that carries the removable skill:
    // the frozen base revision and the candidate branch must agree.
    fixture.write_spec(
        &[(
            "removal",
            json!({"proposal": "proposal-alpha", "target": "target-beta"}),
        )],
        None,
    );
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let mut checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fs::remove_dir_all(checkout.path.join(".agents/skills/target-beta")).unwrap();
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
            "remove the fixture skill target-beta",
        ],
    );
    checkout.revision = git_output(&checkout.path, &["rev-parse", "HEAD"]);
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, true);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("adopt-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("removal"),
        "the candidate treatment waits for the informed removal decision: {output}"
    );
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "{status}"
    );
    let candidate_attempts = fixture.cursor()["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|attempt| attempt["role"] == "candidate")
        .count();
    assert_eq!(
        candidate_attempts, 0,
        "no candidate dispatch before the decision"
    );

    // The recorded refusal blocks the dependent measured dispatch; the
    // controller does not repeat the request for the same basis.
    let proposed = fixture.feedback(&[
        "removal-propose",
        "--item",
        &fixture.card,
        "--proposal",
        "proposal-alpha",
        "--target",
        "target-beta",
        "--evidence",
        "evidence-1",
        "--loss",
        "synthetic-loss",
        "--preview",
        "preview-1",
    ]);
    assert!(proposed.status.success(), "{}", text(&proposed));
    let refused = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "refuse",
        "--proposal",
        "proposal-alpha",
        "--target",
        "target-beta",
        "--loss",
        "synthetic-loss",
    ]);
    assert!(refused.status.success(), "{}", text(&refused));
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("removal authority: refused by the user"),
        "the refusal is visible and unbypassable: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|attempt| attempt["role"] == "candidate")
            .count(),
        0,
        "a refusal cannot be bypassed through workload dispatch: {status}"
    );

    // The informed approval unblocks the candidate's measured conversation.
    let approved = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "approve",
        "--proposal",
        "proposal-alpha",
        "--target",
        "target-beta",
        "--actions",
        "experiment",
        "--loss",
        "synthetic-loss",
    ]);
    assert!(approved.status.success(), "{}", text(&approved));
    // The recorded refusal keeps the phase blocked until a resume boundary
    // resolves it against the new approval: this resume clears the recorded
    // block, and the next advances to the approved measured conversation.
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let candidate_attempts = fixture.cursor()["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|attempt| attempt["role"] == "candidate")
        .count();
    assert_eq!(
        candidate_attempts,
        1,
        "the approved treatment dispatches: {}",
        text(&resume)
    );

    // The candidate solves the workload and is measurably faster.
    let session = session_id("adopt-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 50, 1.5, 1, 1, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );
    assert!(
        status["comparison"]["decision"]
            .as_str()
            .unwrap()
            .contains("outcome=adopt"),
        "{status}"
    );
    let comments = fixture.bd_comments(&fixture.card);
    assert!(comments.contains("outcome=adopt"), "{comments}");
    assert!(comments.contains("matched=1"), "{comments}");
    assert!(
        fixture.run.join("comparison/report.json").is_file()
            && fixture.run.join("comparison/evaluation.json").is_file(),
        "the authoritative accounting and evaluation are retained"
    );
    // The verdict is not an integration or an activation; the recorded
    // selection is the candidate arm runtime this pair actually consumed.
    assert_eq!(git_output(&fixture.proj, &["rev-parse", "HEAD"]), base);
    let candidate_runtime: harness_core::improvement_runtime::ArmRuntime = serde_json::from_slice(
        &fs::read(fixture.arm_dir("candidate").join("runtime.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(status["selected_variant"], "candidate", "{status}");
    assert_eq!(
        PathBuf::from(status["selected_runtime"].as_str().unwrap_or_default())
            .canonicalize()
            .unwrap(),
        candidate_runtime.variant.build.canonicalize().unwrap(),
        "the recorded selection is the consumed candidate runtime: {status}"
    );
    assert_eq!(
        status["selected_identity"],
        format!(
            "sha256:{}",
            &candidate_runtime.variant.source_sha256
                [..16.min(candidate_runtime.variant.source_sha256.len())]
        ),
        "{status}"
    );

    // The published adoption is consumable by the existing integration owner:
    // it re-derives the expected record from the exact raw revisions and the
    // binding acceptance, so the comparison-produced decision must match it
    // byte-for-field, and the declared combined-tree check then fast-forwards
    // the exact evaluated revision into the accepted mainline. The frozen
    // candidate state declares this treatment a removal, so the effect owner
    // refuses integration while the approval covers only the isolated
    // experiment.
    let expected_answer = fixture.root.join("answer-expected.txt");
    fs::write(&expected_answer, "integrated\n").unwrap();
    fs::create_dir_all(fixture.root.join("integration-evidence")).unwrap();
    let bindings: harness_core::improvement_experiment::ExperimentBindings =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/bindings.json")).unwrap())
            .unwrap();
    let evaluation: harness_core::improvement_policy::PolicyEvaluation =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/evaluation.json")).unwrap())
            .unwrap();
    let spec: harness_core::improvement_loop::RunSpec =
        serde_json::from_slice(&fs::read(fixture.run.join("spec.json")).unwrap()).unwrap();
    let experiment = fixture.cursor()["experiment"].as_str().unwrap().to_owned();
    let removal_required = fixture.cursor()["candidate"]["removal_required"]
        .as_bool()
        .expect("the frozen candidate state declares whether the treatment is a removal");
    assert!(
        removal_required,
        "the run's candidate is a declared removal treatment: {}",
        fixture.cursor()
    );
    let frozen_removal = fixture.cursor()["removal_frozen"]
        .as_str()
        .map(str::to_owned);
    let check = harness_core::improvement_activation::CheckSpec {
        program: PathBuf::from(env!("CARGO_BIN_EXE_harness-improvement-fixture")),
        args: vec![
            "check".into(),
            checkout.path.as_os_str().to_owned(),
            expected_answer.as_os_str().to_owned(),
        ],
        timeout: std::time::Duration::from_secs(120),
    };
    let removal_blocked = harness_core::improvement_activation::integrate(
        &harness_core::improvement_activation::IntegrationRequest {
            spec: &spec,
            bindings: &bindings,
            evaluation: &evaluation,
            experiment: experiment.clone(),
            removal_required,
            frozen_removal: frozen_removal.clone(),
            mainline: fixture.proj.clone(),
            check: check.clone(),
            evidence: fixture.root.join("integration-evidence"),
            prior: None,
        },
    )
    .unwrap();
    let harness_core::improvement_activation::IntegrationOutcome::Blocked(blocked) =
        &removal_blocked
    else {
        panic!(
            "a declared removal must not integrate before its covering approval: {removal_blocked:?}"
        );
    };
    assert!(blocked.pending, "{}", blocked.reason);
    assert!(blocked.reason.contains("not covered"), "{}", blocked.reason);
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        base,
        "the experiment-only approval never moves the accepted mainline"
    );

    let integration_approval = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "approve",
        "--proposal",
        "proposal-alpha",
        "--target",
        "target-beta",
        "--actions",
        "experiment,integration",
        "--loss",
        "synthetic-loss",
    ]);
    assert!(
        integration_approval.status.success(),
        "{}",
        text(&integration_approval)
    );
    let outcome = harness_core::improvement_activation::integrate(
        &harness_core::improvement_activation::IntegrationRequest {
            spec: &spec,
            bindings: &bindings,
            evaluation: &evaluation,
            experiment,
            removal_required,
            frozen_removal,
            mainline: fixture.proj.clone(),
            check,
            evidence: fixture.root.join("integration-evidence"),
            prior: None,
        },
    )
    .unwrap();
    match &outcome {
        harness_core::improvement_activation::IntegrationOutcome::Integrated(receipt) => {
            assert_eq!(receipt.candidate_revision, checkout.revision);
            assert_eq!(receipt.acceptance, bindings.acceptance);
        }
        harness_core::improvement_activation::IntegrationOutcome::Confirmed(receipt) => {
            assert_eq!(receipt.candidate_revision, checkout.revision);
        }
        harness_core::improvement_activation::IntegrationOutcome::Blocked(blocked) => {
            panic!(
                "the comparison-produced adoption must be consumable by the integration owner: {blocked:?}"
            );
        }
    }
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        checkout.revision,
        "the declared check and fast-forward integrated the exact evaluated revision"
    );

    let before = fixture.cursor();
    let comments_before = fixture.bd_comments(&fixture.card);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert_eq!(fixture.cursor()["attempts"], before["attempts"]);
    assert_eq!(fixture.bd_comments(&fixture.card), comments_before);
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

/// The continuous controller consumes a supported adoption through the real
/// integration and activation owners: the checked revision reaches the
/// mainline, the prepared candidate runtime becomes the experimental
/// baseline, lineage is retained and the settled continuation is not replayed.
#[test]
fn a_continuous_controller_consumes_an_adoption_through_integration_and_activation() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("controller-activate");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    // The measured arms' independent checks run inside the heavy-command
    // budget; this case owns isolated accounts so it does not depend on any
    // ambient shared lease.
    let heavy_account = fixture.root.join("heavy-account");
    let cpu_account = fixture.root.join("cpu-account");
    let accounts = [
        (
            "CODEX_HARNESS_HEAVY_ACCOUNT",
            heavy_account.to_str().unwrap(),
        ),
        ("CODEX_HARNESS_CPU_ACCOUNT", cpu_account.to_str().unwrap()),
    ];
    let run_arg = fixture.run.to_str().unwrap().to_owned();
    let resume = fixture.improve_with_env(&["resume", "--run", &run_arg], &accounts);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("before submission"),
        "the fixture launcher cannot run a conversation: {output}"
    );
    let session = session_id("activate-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let resume = fixture.improve_with_env(&["resume", "--run", &run_arg], &accounts);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "{status}\n{output}"
    );

    let session = session_id("activate-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 50, 1.5, 1, 1, &session);
    let resume = fixture.improve_with_env(&["resume", "--run", &run_arg], &accounts);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}\n{output}");
    assert!(
        status["comparison"]["decision"]
            .as_str()
            .unwrap()
            .contains("outcome=adopt"),
        "{status}"
    );
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        base,
        "the verdict alone integrates nothing"
    );

    // Hand the exact supported decision to the continuous controller: the
    // declared combined-tree check and the prepared candidate runtime flow
    // through the existing integration and activation owners.
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let expected_answer = fixture.root.join("answer-expected.txt");
    fs::write(&expected_answer, "integrated\n").unwrap();
    fs::write(
        fixture.run.join("integration-check.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "program": env!("CARGO_BIN_EXE_harness-improvement-fixture"),
            "args": ["check", checkout.path, expected_answer],
            "timeout_seconds": 120,
        }))
        .unwrap(),
    )
    .unwrap();
    let attempts_before = fixture.cursor()["attempts"].as_array().unwrap().len();
    let resume = fixture.improve_with_env(&["resume", "--run", &run_arg], &accounts);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("integration owner"), "{output}");
    assert!(output.contains("activation owner"), "{output}");
    let status = fixture.status_json();
    assert_eq!(
        status["phase"], "activation-confirmed",
        "{status}\n{output}"
    );
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        checkout.revision,
        "the checked integrated revision is the mainline"
    );
    assert!(fixture.run.join("integration.json").is_file());
    assert!(fixture.run.join("activation.json").is_file());
    let lineage: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("lineage.json")).unwrap()).unwrap();
    assert_eq!(lineage["decision"], "adopt", "{lineage}");
    let marker: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("continuation.json")).unwrap()).unwrap();
    assert_eq!(marker["phase"], "idle", "{marker}");
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts_before,
        "integration and activation dispatch no model work"
    );

    // The settled continuation is idempotent: a repeated resume replays
    // neither the decision nor the activation.
    let again = fixture.improve_with_env(&["resume", "--run", &run_arg], &accounts);
    let output = text(&again);
    assert!(again.status.success(), "{output}");
    assert!(output.contains("already recorded"), "{output}");
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts_before
    );
    assert_eq!(fixture.status_json()["phase"], "activation-confirmed");
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

/// Requests, rounds, outer calls and tool operations travel from the retained
/// receipt and the attempt's own rollout through the accounting row into the
/// authoritative report: a batched outer call keeps its lower call count from
/// being read as less work, and the call and operation counters stay distinct
/// end to end.
#[test]
fn batched_calls_and_operations_stay_distinct_through_the_report() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("batched-counters");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));

    let baseline_session = session_id("batched-baseline");
    fixture.simulate_arm(
        "baseline",
        "baseline",
        "solved",
        1200,
        60.0,
        2,
        6,
        &baseline_session,
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));

    let candidate_session = session_id("batched-candidate");
    fixture.simulate_arm(
        "candidate",
        "candidate",
        "solved",
        50,
        1.0,
        1,
        6,
        &candidate_session,
    );
    // One batched outer call performed all six tool operations the candidate
    // receipt observed; the completed-item evidence is unchanged.
    keep_recorded_calls(&fixture, "candidate", &candidate_session, 1);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");

    let baseline_row = load_json(&fixture.arm_dir("baseline").join("row.json"));
    let native = &baseline_row["native_runs"][0];
    assert_eq!(native["requests"], 1, "{baseline_row}");
    assert_eq!(native["rounds"], 1, "{baseline_row}");
    assert_eq!(native["tool_calls"], 6, "{baseline_row}");
    assert_eq!(native["tool_operations"], 6, "{baseline_row}");
    assert_eq!(native["usage"]["response_count"], 1, "{baseline_row}");
    assert_eq!(
        native["counter_sources"]["requests"], "rollout-usage:deduplicated-responses",
        "{baseline_row}"
    );
    assert_eq!(
        native["counter_sources"]["tool_operations"], "receipt-observation:completed-tool-items",
        "{baseline_row}"
    );

    let candidate_row = load_json(&fixture.arm_dir("candidate").join("row.json"));
    let native = &candidate_row["native_runs"][0];
    assert_eq!(native["requests"], 1, "{candidate_row}");
    assert_eq!(native["rounds"], 1, "{candidate_row}");
    assert_eq!(native["tool_calls"], 1, "{candidate_row}");
    assert_eq!(native["tool_operations"], 6, "{candidate_row}");

    // The outcome report keeps every counter available and the batched shape
    // distinct: the lower outer-call count alone is not less recorded work.
    let report = load_json(&fixture.run.join("comparison/report.json"));
    let events = &report["variation"][0]["within_run_events"];
    assert_eq!(events["requests"], 2, "{events}");
    assert_eq!(events["rounds"], 2, "{events}");
    assert_eq!(events["tool_calls"], 7, "{events}");
    assert_eq!(events["tool_operations"], 12, "{events}");
}

/// A qualified API-observed record and an unconsumed observation template are
/// both handled through the real controller entry point: the record is
/// accepted, and an observation input that is not a consumed client file is
/// refused before any preparation.
#[test]
fn api_observed_qualification_drift_blocks_the_measured_arm() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("api-observed");
    let server = ObservationServer::start("{\"build\":\"b-1\"}\n");
    install_api_observed_qualification(&fixture, &server, None);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("before submission"),
        "the API-observed arm reaches the visible dispatch owner: {output}"
    );
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 1);

    // A retained record whose bound digests no longer agree is refused by the
    // qualification owner before dependent work.
    let mut record: Value =
        serde_json::from_slice(&fs::read(&fixture.qualification).unwrap()).unwrap();
    let digest = record["observation_digest"].as_str().unwrap().to_owned();
    record["observation_digest"] = json!(format!("{digest}0"));
    fs::write(
        &fixture.qualification,
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("internally inconsistent"), "{output}");
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        1,
        "an inconsistent qualification dispatches nothing"
    );
    record["observation_digest"] = json!(digest);
    fs::write(
        &fixture.qualification,
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();

    // A changed observed server fact refuses the arm before its conversation:
    // the retained qualification no longer matches what the endpoint serves.
    server.set("{\"build\":\"b-2\"}\n");
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("differs from the qualified identity"),
        "observed drift blocks the measured arm: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        1,
        "a drifted observation dispatches nothing"
    );
    assert_eq!(
        fixture.cursor()["comparison"]["baseline"]["accepted"],
        Value::Null
    );

    // An observation input that is not one of the consumed client files is
    // refused before any preparation: an unused qualified template cannot
    // stand in for the arm's actual configuration.
    let unconsumed = Fixture::new("api-unconsumed-input");
    let server = ObservationServer::start("{\"build\":\"b-1\"}\n");
    install_api_observed_qualification(&unconsumed, &server, None);
    let mut spec: Value = serde_json::from_slice(&fs::read(&unconsumed.spec).unwrap()).unwrap();
    spec["comparison"]["observation_inputs"] = json!([
        {"name": "profile", "path": unconsumed.home.join("config.toml")},
    ]);
    fs::write(&unconsumed.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    let checkout = unconsumed.prepare_candidate(Some("// candidate implementation\n"));
    unconsumed.prepare_builds(&checkout);
    unconsumed.start_with_ready_candidate(&checkout, false);
    let resume = unconsumed.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("not one of the client files"), "{output}");
    assert_eq!(
        unconsumed.cursor()["attempts"].as_array().unwrap().len(),
        0,
        "no measured attempt is even prepared from an unconsumed template"
    );
    assert!(!unconsumed.run.join("comparison").exists());
}

/// A bearer-authenticated API-observed plan is re-collected with its declared
/// transport input: the bearer file rides along with the observed client
/// inputs from `observation_auth`, the measured arm reaches its visible
/// dispatch, and the synthetic token is never echoed back.
#[test]
fn api_observed_bearer_auth_is_recollected_from_the_declared_transport_input() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("api-bearer-auth");
    let server = ObservationServer::start("{\"build\":\"b-1\"}\n");
    let (overlay, bearer) =
        install_api_observed_qualification(&fixture, &server, Some("route-key"));
    let bearer = bearer.expect("the bearer plan declares its transport input");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        !output.contains("declared bearer auth client input was not supplied"),
        "the declared bearer transport input is supplied from the comparison inputs: {output}"
    );
    assert!(
        output.contains("before submission"),
        "the bearer-authenticated API-observed arm reaches its visible dispatch: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        1,
        "the re-collected observation set admits the measured attempt"
    );
    assert!(
        overlay.is_file(),
        "the observed client input stays explicit"
    );
    assert!(
        bearer.is_file(),
        "the bearer transport input stays an explicit private file"
    );
    assert!(
        !output.contains("synthetic-route-key"),
        "the bearer token is never echoed: {output}"
    );
}

/// A bearer token file declared as an observed client input is still refused:
/// it is transport, not one of the client files the arms consume, so it cannot
/// enter through `observation_inputs` even under a bearer plan.
#[test]
fn api_observed_bearer_auth_inside_observation_inputs_is_still_refused() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("api-bearer-in-observed-inputs");
    let server = ObservationServer::start("{\"build\":\"b-1\"}\n");
    let (overlay, bearer) =
        install_api_observed_qualification(&fixture, &server, Some("route-key"));
    let bearer = bearer.expect("the bearer plan declares its transport input");
    let mut spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    spec["comparison"]["observation_inputs"] = json!([
        {"name": "overlay", "path": overlay},
        {"name": "route-key", "path": bearer},
    ]);
    spec["comparison"]["observation_auth"] = Value::Null;
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("not one of the client files"), "{output}");
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        0,
        "no measured attempt is even prepared from the transport file"
    );
    assert!(!fixture.run.join("comparison").exists());
}

/// A bearer-authenticated plan without the declared transport input still
/// blocks: the missing bearer file is reported with the collector's own exact
/// reason instead of being dropped or silently succeeded.
#[test]
fn api_observed_bearer_auth_without_the_transport_input_still_blocks() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("api-bearer-absent");
    let server = ObservationServer::start("{\"build\":\"b-1\"}\n");
    install_api_observed_qualification(&fixture, &server, Some("route-key"));
    let mut spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    spec["comparison"]["observation_auth"] = Value::Null;
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("the pre-attempt API observation set is unavailable"),
        "{output}"
    );
    assert!(
        output.contains("declared bearer auth client input was not supplied"),
        "the missing transport input blocks with the collector's exact reason: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        0,
        "the bearer plan dispatches nothing without its declared transport input"
    );
}

/// The selected API-observed policy reaches the planning/implementation
/// dispatch through the real controller: a qualified API record lets the
/// bounded investigator conversation start, while a blocked full-material
/// record keeps its refusal and starts nothing.
#[test]
fn api_observed_qualification_reaches_planning_and_implementation() {
    // Qualified API-observed identity: the investigator dispatch is attempted.
    let fixture = Fixture::new("api-planning");
    let server = ObservationServer::start("{\"build\":\"b-1\"}\n");
    let (overlay, _) = install_api_observed_qualification(&fixture, &server, None);
    let evidence = fixture.root.join("evidence");
    fs::create_dir_all(&evidence).unwrap();
    fs::write(evidence.join("observation.txt"), "retained observation\n").unwrap();
    let mut spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    spec["evidence_root"] = json!(evidence);
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    let launcher = fixture.home.join("harness/bin/codex.exe");
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    fs::copy(programs().launcher.as_path(), &launcher).unwrap();
    let start = fixture.start();
    let output = text(&start);
    assert!(start.status.success(), "{output}");
    assert!(
        !output.contains("qualification"),
        "the qualified API record must not block planning work: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        1,
        "the bounded investigator conversation is dispatched: {output}"
    );
    assert_eq!(fixture.cursor()["attempts"][0]["role"], "investigator");
    assert!(
        overlay.is_file(),
        "the observed client overlay stays the explicit private input"
    );

    // A blocked full-material record keeps its refusal for the measured pair:
    // no comparison preparation, no dispatch, the owner's reasons retained.
    let fixture = Fixture::new("legacy-blocked");
    fs::write(
        &fixture.qualification,
        serde_json::to_vec_pretty(&json!({
            "status": "blocked",
            "policy": {
                "repeats": 2,
                "required_outputs": ["solution.txt"],
                "ignored_metadata": [],
            },
            "missing_identity": ["weights"],
            "unfinished_attempts": [],
            "unverified_attempts": [],
            "tool_exchange_missing": [],
            "runner_mismatch": [],
            "missing_outputs": ["solution.txt"],
            "divergent_outputs": [],
            "observed_repeats": 1,
            "required_repeats": 2,
        }))
        .unwrap(),
    )
    .unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("qualification") && output.contains("missing required outputs"),
        "a blocked full-material record keeps its refusal: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        0,
        "an unqualified record starts no model work: {output}"
    );
    assert!(!fixture.run.join("comparison").exists());
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

/// Both arms independently implement and pass the workload, producing
/// differing valid B solutions: each exact revision is retained under B's own
/// card, and the verdict alone integrates nothing.
#[test]
fn both_accepted_arms_retain_their_differing_valid_workload_solutions() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("workload-artifacts");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(
        text(&resume).contains("before submission"),
        "the fixture launcher cannot run a conversation: {}",
        text(&resume)
    );

    // The baseline arm implements B and passes the frozen oracle.
    let session = session_id("workload-artifacts-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "{status}"
    );
    let baseline_revision = status["comparison"]["baseline"]["revision"]
        .as_str()
        .expect("the accepted baseline revision is retained")
        .to_owned();
    let comments = fixture.bd_comments(&fixture.workload_card);
    assert!(
        comments.contains(&format!(
            "hypothesis-implementation v1 item={} role=workload branch=workload-baseline base={} revision={baseline_revision}",
            fixture.workload_card, fixture.workload_revision
        )),
        "the accepted baseline solution is retained under B's own card: {comments}"
    );

    // The candidate arm implements B differently and also passes: two
    // differing valid B solutions must both stay selectable.
    let session = session_id("workload-artifacts-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 50, 1.5, 1, 1, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    let candidate_revision = status["comparison"]["candidate"]["revision"]
        .as_str()
        .expect("the accepted candidate revision is retained")
        .to_owned();
    assert_ne!(
        baseline_revision, candidate_revision,
        "the arms produced differing valid B solutions"
    );
    let comments = fixture.bd_comments(&fixture.workload_card);
    assert!(
        comments.contains(&format!(
            "hypothesis-implementation v1 item={} role=workload branch=workload-candidate base={} revision={candidate_revision}",
            fixture.workload_card, fixture.workload_revision
        )),
        "the accepted candidate solution is retained under B's own card: {comments}"
    );
    assert!(
        comments.contains(&baseline_revision),
        "the other valid B solution stays retained: {comments}"
    );
    // B's correctness is not B's benefit: the verdict alone integrates nothing.
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        base,
        "the comparison never advances the mainline"
    );
}

/// An arm whose frozen independent acceptance failed leaves no usable B
/// artifact: only the accepted solution is retained, and the comparison still
/// publishes its supported non-adoption and settles without inventing a next
/// candidate patch.
#[test]
fn a_workload_arm_without_independent_acceptance_leaves_no_candidate_patch() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("workload-no-patch");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));

    let session = session_id("workload-no-patch-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    let baseline_revision = status["comparison"]["baseline"]["revision"]
        .as_str()
        .expect("the accepted baseline revision is retained")
        .to_owned();

    // The candidate arm returns a solution the frozen checker rejects.
    let session = session_id("workload-no-patch-candidate");
    fixture.simulate_arm("candidate", "candidate", "wrong", 30, 1.0, 1, 1, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], false,
        "{status}"
    );
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    let failed_revision = status["comparison"]["candidate"]["revision"]
        .as_str()
        .expect("the failed arm's attempted revision stays visible")
        .to_owned();
    let comments = fixture.bd_comments(&fixture.workload_card);
    assert!(
        comments.contains(&baseline_revision),
        "the independently accepted solution stays retained: {comments}"
    );
    assert!(
        !comments.contains(&failed_revision),
        "a failed arm's solution is never presented as a selectable B artifact: {comments}"
    );
    let evaluation: Value = load_json(&fixture.run.join("comparison/evaluation.json"));
    assert_eq!(evaluation["decision"], "reject", "{evaluation}");
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        base,
        "a failed workload arm never advances the mainline"
    );

    // No candidate patch exists, but the run is not blocked on one: its
    // continuation records the retained lineage and goes idle without model
    // work invented to produce a next candidate.
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_eq!(report["phase"], "idle", "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("leaves the baseline unchanged"),
        "{report}"
    );
    assert!(
        fixture.run.join("lineage.json").is_file(),
        "the retained lineage stays available for the next grounded hypothesis"
    );
    let attempts = fixture.cursor()["attempts"].as_array().unwrap().len();
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts,
        "no model work is started to invent a next candidate"
    );
}

/// A comparison interrupted after the baseline arm reuses that completed arm
/// only while its recorded conditions still hold. Changed planning inputs
/// keep the retained arm out of the comparison and record why remeasurement is
/// required; restored inputs reuse the same arm without replaying any model
/// attempt, and the retained patch and branch survive the refusal untouched.
#[test]
fn a_completed_arm_is_reused_only_while_its_planning_inputs_remain_valid() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("arm-reuse");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

    // The interrupted baseline conversation is seeded through the same durable
    // seam the controller recovery uses: the dispatch was refused before
    // submission, its terminal receipt is retained and the committed solution
    // stays in the arm's pooled checkout.
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("arm-reuse-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 20, 5.0, 2, 3, &session);
    let solution_slot = fixture.arm_dir("baseline").join("checkout-wt1");
    let solution_revision = git_output(&solution_slot, &["rev-parse", "HEAD"]);
    let dispatched = fixture.cursor()["attempts"].as_array().unwrap().len();

    // The user stops the loop after the baseline arm: the run suspends new
    // work, preserves the attempt and its committed solution, and marks the
    // in-flight measured effect explicitly instead of replaying it.
    let stop = fixture.stop();
    let output = text(&stop);
    assert!(stop.status.success(), "{output}");
    let stopped = fixture.cursor();
    assert_eq!(stopped["phase"], "stopped", "{stopped}\n{output}");
    assert_eq!(stopped["attempts"].as_array().unwrap().len(), dispatched);
    let state = stopped["attempts"][0]["state"].as_str().unwrap_or_default();
    assert!(
        ["completed", "unknown", "interrupted", "stopped", "failed"].contains(&state),
        "the in-flight effect is explicitly resolved or retained as {state}: {stopped}"
    );

    // Changed planning inputs: the completed arm is not reusable, so nothing
    // is consumed, no further model work is dispatched and no decision is
    // published.
    let change_spec = fixture
        .proj
        .join("openspec/changes/add-synthetic/specs/synthetic/spec.md");
    let original = fs::read(&change_spec).unwrap();
    let changed = format!(
        "{}### Requirement: Additional synthetic behavior\n\nThe system SHALL do the additional synthetic thing.\n\n#### Scenario: Additional synthetic case\n\n- **WHEN** the probe runs again\n- **THEN** it reports success\n",
        String::from_utf8(original.clone()).unwrap()
    );
    fs::write(&change_spec, changed).unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("changed conditions require remeasurement"),
        "the resume names the required remeasurement: {output}"
    );
    assert!(
        output.contains("cannot be reused"),
        "the comparison records the exact refusal: {output}"
    );
    let cursor = fixture.cursor();
    let baseline_attempt = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["role"] == "baseline")
        .unwrap();
    assert_eq!(baseline_attempt["state"], "completed", "{cursor}");
    assert!(
        baseline_attempt["reuse_refused"]
            .as_str()
            .is_some_and(|reason| reason.contains("planning inputs changed")),
        "the refused planning inputs are recorded on the attempt: {cursor}"
    );
    assert_eq!(
        dispatched,
        cursor["attempts"].as_array().unwrap().len(),
        "no attempt is replayed or added while the arm is not reusable: {cursor}"
    );
    assert_eq!(
        cursor["comparison"]["baseline"]["accepted"],
        Value::Null,
        "the refused arm does not enter the comparison: {cursor}"
    );
    assert_eq!(
        cursor["comparison"]["baseline"]["revision"],
        Value::Null,
        "{cursor}"
    );
    // The useful patch and branch survive the refused reuse exactly.
    assert_eq!(
        fs::read_to_string(solution_slot.join("solution.txt")).unwrap(),
        "solved\n",
        "the completed arm's committed solution is preserved"
    );
    assert_eq!(
        git_output(&solution_slot, &["rev-parse", "HEAD"]),
        solution_revision
    );

    // Restored planning inputs make the same completed arm reusable: it is
    // consumed without any replay and the candidate dispatch proceeds.
    fs::write(&change_spec, &original).unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "the restored inputs reuse the retained arm: {status}\n{output}"
    );
    assert_eq!(
        status["comparison"]["baseline"]["revision"].as_str(),
        Some(solution_revision.as_str()),
        "{status}"
    );
    let cursor = fixture.cursor();
    let candidate = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["role"] == "candidate")
        .expect("the candidate dispatch is attempted once the baseline is reusable");
    assert_eq!(candidate["state"], "failed", "{cursor}");
    assert!(
        candidate["reason"]
            .as_str()
            .unwrap_or("")
            .contains("no model request was made"),
        "{cursor}"
    );

    // A consumed arm is not reused either while its inputs are invalid: the
    // pair's remaining work and its decision stay stopped, while the consumed
    // result itself stays retained.
    let changed = format!(
        "{}### Requirement: Another synthetic behavior\n\nThe system SHALL do another synthetic thing.\n\n#### Scenario: Another synthetic case\n\n- **WHEN** the probe runs once more\n- **THEN** it reports success\n",
        String::from_utf8(original.clone()).unwrap()
    );
    fs::write(&change_spec, changed).unwrap();
    let attempts_before = fixture.cursor()["attempts"].as_array().unwrap().len();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("cannot be reused"),
        "the consumed arm's changed inputs still stop the pair: {output}"
    );
    let status = fixture.status_json();
    assert_eq!(status["phase"], "blocked", "{status}\n{output}");
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts_before,
        "the remaining arm is not dispatched from a non-reusable pair"
    );
    assert_eq!(
        status["comparison"]["candidate"]["accepted"],
        Value::Null,
        "{status}"
    );
    assert!(status["comparison"]["decision"].is_null(), "{status}");

    // The same applies to the candidate arm: once its interrupted conversation
    // is retained, changed planning inputs keep it out of the comparison until
    // the inputs are restored, and then the pair settles exactly once.
    fs::write(&change_spec, &original).unwrap();
    let session = session_id("arm-reuse-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 5, 1.0, 1, 1, &session);
    let attempts_before = fixture.cursor()["attempts"].as_array().unwrap().len();
    let changed = format!(
        "{}### Requirement: Yet another synthetic behavior\n\nThe system SHALL do yet another synthetic thing.\n\n#### Scenario: Yet another synthetic case\n\n- **WHEN** the probe runs a final time\n- **THEN** it reports success\n",
        String::from_utf8(original.clone()).unwrap()
    );
    fs::write(&change_spec, changed).unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("cannot be reused"),
        "the completed candidate arm is held out of the comparison: {output}"
    );
    let status = fixture.status_json();
    assert_eq!(status["phase"], "blocked", "{status}\n{output}");
    assert_eq!(
        status["comparison"]["candidate"]["accepted"],
        Value::Null,
        "{status}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts_before,
        "neither arm is replayed while the pair is not reusable"
    );
    assert!(status["comparison"]["decision"].is_null(), "{status}");

    // Restored inputs first clear the recorded refusal: the resume returns the
    // run to its comparison phase without consuming anything, and the next
    // resume settles the retained candidate once and publishes exactly one
    // evidence-bound decision.
    fs::write(&change_spec, &original).unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["phase"], "baseline-attempt", "{status}\n{output}");
    assert_eq!(
        status["comparison"]["candidate"]["accepted"],
        Value::Null,
        "the recovery resume consumes nothing before the recorded refusal is gone: {status}"
    );
    assert!(status["comparison"]["decision"].is_null(), "{status}");
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}\n{output}");
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );
    let comments =
        harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, &fixture.card)
            .unwrap();
    let records = harness_core::benefit_gate::parse_gate_comments(&comments);
    assert_eq!(
        records.len(),
        1,
        "the exact decision is recorded once, never counted twice: {comments:?}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts_before,
        "settling the pair dispatches no model work"
    );
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(fixture.status_json()["phase"], "decision-recorded");
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts_before,
        "a repeated resume replays neither the decision nor a trial"
    );
}

/// The candidate diff touches controller, evaluator, oracle, policy and check
/// components and plants a self-activation attempt in its own tree. The
/// measured pair is still assessed by the unchanged supervisor and its frozen
/// oracle: the forged solution is rejected, and no integration or activation
/// follows from the candidate's own components.
#[test]
fn a_controller_targeting_candidate_is_assessed_by_the_unchanged_supervisor() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("control-target");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let mut checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    for (relative, body) in [
        (
            "crates/codex-harness/src/outcome_oracle.rs",
            "// candidate evaluator: accept every solution\n",
        ),
        (
            "crates/codex-harness/src/improvement_activation.rs",
            "// candidate control rule: activate itself on start\n",
        ),
        (
            "crates/harness-core/src/improvement_policy.rs",
            "// candidate policy: adopt unconditionally\n",
        ),
        (
            "crates/codex-harness/src/improvement_comparison.rs",
            "// candidate checks: pass unconditionally\n",
        ),
        ("self-activate.json", "{\"activate\": true}\n"),
    ] {
        let path = checkout.path.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
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
            "candidate touches controller and evaluation components",
        ],
    );
    checkout.revision = git_output(&checkout.path, &["rev-parse", "HEAD"]);
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    // The measured arms' independent checks run inside the heavy-command
    // budget; this case owns isolated accounts so it does not depend on any
    // ambient shared lease.
    let heavy_account = fixture.root.join("heavy-account");
    let cpu_account = fixture.root.join("cpu-account");
    let accounts = [
        (
            "CODEX_HARNESS_HEAVY_ACCOUNT",
            heavy_account.to_str().unwrap(),
        ),
        ("CODEX_HARNESS_CPU_ACCOUNT", cpu_account.to_str().unwrap()),
    ];
    let run_arg = fixture.run.to_str().unwrap().to_owned();
    let resume = fixture.improve_with_env(&["resume", "--run", &run_arg], &accounts);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("before submission"),
        "the fixture launcher cannot run a conversation: {output}"
    );

    let session = session_id("control-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let resume = fixture.improve_with_env(&["resume", "--run", &run_arg], &accounts);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "{status}\n{output}"
    );

    let session = session_id("control-candidate");
    fixture.simulate_arm("candidate", "candidate", "wrong", 50, 1.5, 1, 1, &session);
    let resume = fixture.improve_with_env(&["resume", "--run", &run_arg], &accounts);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], false,
        "the frozen oracle rejects the forged solution even though the candidate tree rewrites its evaluator: {status}"
    );
    assert!(
        status["comparison"]["decision"]
            .as_str()
            .unwrap()
            .contains("outcome=reject"),
        "{status}"
    );
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        base,
        "the candidate's own control components never integrate or activate themselves"
    );
    assert!(!fixture.run.join("integration.json").exists(), "{status}");
    assert!(!fixture.run.join("activation.json").exists(), "{status}");

    // The unchanged supervisor and oracle identities governed both arms: the
    // retained supervisor record names the running controller binary, and
    // every oracle record names the frozen request's oracle program and
    // contract.
    let supervisor = load_json(&fixture.run.join("comparison/supervisor.json"));
    let running = build_identity::hash_file(&manager()).unwrap();
    assert_eq!(
        supervisor["executableSha256"].as_str().unwrap(),
        running.to_ascii_lowercase(),
        "{supervisor}"
    );
    assert_eq!(
        supervisor["acceptanceRequestSha256"].as_str().unwrap(),
        fixture.request_sha256,
        "{supervisor}"
    );
    let request = load_json(&fixture.request);
    for arm in ["baseline", "candidate"] {
        let record = load_json(&fixture.arm_dir(arm).join("oracle.json"));
        assert_eq!(record["executed"], true, "{arm}: {record}");
        assert_eq!(record["model_calls"], 0, "{arm}: {record}");
        assert_eq!(
            record["oracle_program_sha256"].as_str().unwrap(),
            request["oracle"]["program_sha256"].as_str().unwrap(),
            "{arm}: {record}"
        );
        assert_eq!(
            record["task_contract_sha256"].as_str().unwrap(),
            request["task_contract_sha256"].as_str().unwrap(),
            "{arm}: {record}"
        );
    }
    // The frozen policy, not the candidate's policy source, produced the
    // recorded evaluation.
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    let bindings = load_json(&fixture.run.join("comparison/bindings.json"));
    assert_eq!(
        evaluation["policyDigest"].as_str().unwrap(),
        bindings["policyDigest"].as_str().unwrap(),
        "{evaluation}"
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

/// A policy file the candidate branch could rewrite cannot govern the
/// comparison: the declared policy must stay outside the candidate's writable
/// scope, and the refusal happens before any preparation or dispatch.
#[test]
fn a_policy_file_inside_the_candidate_scope_cannot_govern_the_comparison() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("policy-in-scope");
    let policy_in_scope = fixture.proj.join("crates/one/comparison-policy.json");
    fs::copy(&fixture.policy, &policy_in_scope).unwrap();
    let mut document = load_json(&fixture.spec);
    document["comparison"]["policy"] = json!(policy_in_scope);
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("writable scope"),
        "the in-scope policy must be refused with its cause: {output}"
    );
    assert!(
        !fixture.run.join("comparison").exists(),
        "the refusal happens before any comparison preparation"
    );
    assert_eq!(
        fixture.status_json()["attempts"].as_array().unwrap().len(),
        0,
        "{output}"
    );
}

/// Weakening the frozen acceptance is refused at every use: once the request
/// bytes change, the next arm consumption blocks, the attempt is retained as
/// never accepted and no decision is published.
#[test]
fn a_weakened_acceptance_request_is_refused_with_the_attempt_retained() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("acceptance-drift");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let heavy_account = fixture.root.join("heavy-account");
    let cpu_account = fixture.root.join("cpu-account");
    let accounts = [
        (
            "CODEX_HARNESS_HEAVY_ACCOUNT",
            heavy_account.to_str().unwrap(),
        ),
        ("CODEX_HARNESS_CPU_ACCOUNT", cpu_account.to_str().unwrap()),
    ];
    let run_arg = fixture.run.to_str().unwrap().to_owned();
    let resume = fixture.improve_with_env(&["resume", "--run", &run_arg], &accounts);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("before submission"), "{output}");
    let session = session_id("acceptance-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let resume = fixture.improve_with_env(&["resume", "--run", &run_arg], &accounts);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "{status}\n{output}"
    );

    // The frozen acceptance request is altered so its supervisor digest no
    // longer matches.
    let mut request = load_json(&fixture.request);
    request["timeout_seconds"] = json!(30);
    fs::write(
        &fixture.request,
        serde_json::to_vec_pretty(&request).unwrap(),
    )
    .unwrap();

    let session = session_id("acceptance-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 50, 1.5, 1, 1, &session);
    let resume = fixture.improve_with_env(&["resume", "--run", &run_arg], &accounts);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("changed since the run declared it"),
        "the weakened acceptance must be refused with its cause: {output}"
    );
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["candidate"]["accepted"],
        Value::Null,
        "no candidate result enters the comparison from a weakened acceptance: {status}"
    );
    assert!(
        !fixture.run.join("comparison/evaluation.json").exists(),
        "no decision is derived from a weakened acceptance: {status}"
    );
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        base,
        "{status}"
    );
    let cursor = fixture.cursor();
    assert!(
        cursor["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|effect| effect["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("changed since the run declared it"))),
        "the refusal stays in the run's own recovery history: {cursor}"
    );
}

/// A policy rewritten after the comparison declared its digest cannot govern
/// the retained evidence: the changed declaration blocks every dependent
/// advance instead of being silently re-frozen.
#[test]
fn a_policy_rewrite_after_its_declaration_is_refused() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("policy-rewrite");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("before submission"), "{output}");

    let mut policy = load_json(&fixture.policy);
    policy["tolerancePercent"] = json!(9.0);
    fs::write(&fixture.policy, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("policy changed after its declaration"),
        "a rewritten decision policy must be refused with its cause: {output}"
    );
    let cursor = fixture.cursor();
    assert!(
        cursor["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|effect| effect["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("policy changed after its declaration"))),
        "the refusal stays in the run's own recovery history: {cursor}"
    );
    // The changed declaration keeps being refused instead of being re-frozen.
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("policy changed after its declaration"),
        "{output}"
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
