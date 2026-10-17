//! Seeded comparison, workload-artifact, supervisor and api-observation
//! checks split from `improvement_comparison`.
#![cfg(windows)]

#[path = "fixtures/improvement_comparison_common.rs"]
mod improvement_comparison_common;
use improvement_comparison_common::*;

use harness_core::build_identity;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

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

/// The declared conjunction is one combined verdict through the built
/// runner: the same observed unit set must show quality improvement, a
/// useful total-time reduction and a useful recorded-token reduction with no
/// critical regression, and a member below its declared margin does not
/// adopt. Only control failure followed by candidate success can show the
/// quality member, so a recorded control failure must not stop the candidate
/// measurement.
#[test]
fn a_declared_conjunction_consumes_the_combined_verdict() {
    let _serial = INSTALL.lock().unwrap();

    // Quality improvement with useful total-time and recorded-token
    // reductions: adopted.
    let fixture = Fixture::new("conjunction-adopt");
    let mut policy = load_json(&fixture.policy);
    policy["objective"] = json!("quality");
    policy["conjunction"] = json!({
        "requireQualityImprovement": true,
        "minimumTimeReductionPercent": 10.0,
        "minimumTokenReductionPercent": 25.0,
    });
    fs::write(&fixture.policy, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("conjunction-adopt-baseline");
    fixture.simulate_arm("baseline", "baseline", "wrong", 1200, 60.0, 2, 3, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], false,
        "the recorded control failure proceeds to the candidate measurement: {status}"
    );
    let session = session_id("conjunction-adopt-candidate");
    fixture.simulate_arm_with_tokens(
        "candidate",
        "candidate",
        "solved",
        50,
        1.5,
        1,
        1,
        &session,
        60,
    );
    let resume = fixture.resume();
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
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    assert_eq!(evaluation["decision"], "adopt", "{evaluation}");
    assert_eq!(evaluation["matched"], 1, "{evaluation}");
    assert!(
        evaluation["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| {
                reason
                    .as_str()
                    .is_some_and(|reason| reason.contains("conjunction holds"))
            }),
        "the runner consumes the single combined verdict: {evaluation}"
    );
    let comments = fixture.bd_comments(&fixture.card);
    assert!(comments.contains("outcome=adopt"), "{comments}");
    assert!(comments.contains("matched=1"), "{comments}");

    // The recorded-token member below its declared useful margin: the same
    // combined verdict does not adopt, and no partial credit is given.
    let fixture = Fixture::new("conjunction-member-fails");
    let mut policy = load_json(&fixture.policy);
    policy["objective"] = json!("quality");
    policy["conjunction"] = json!({
        "requireQualityImprovement": true,
        "minimumTimeReductionPercent": 10.0,
        "minimumTokenReductionPercent": 25.0,
    });
    fs::write(&fixture.policy, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("conjunction-member-baseline");
    fixture.simulate_arm("baseline", "baseline", "wrong", 1200, 60.0, 2, 3, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("conjunction-member-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 50, 1.5, 1, 1, &session);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}\n{output}");
    assert!(
        status["comparison"]["decision"]
            .as_str()
            .unwrap()
            .contains("outcome=reject"),
        "{status}"
    );
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    assert_eq!(evaluation["decision"], "reject", "{evaluation}");
    assert!(
        evaluation["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| {
                reason.as_str().is_some_and(|reason| {
                    reason.contains("recorded native token cost improved by 0.0%")
                })
            }),
        "the unmet member is the recorded-token margin: {evaluation}"
    );
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

/// A declared multi-role workflow must not silently run the executor-only
/// comparison while its runner is still being integrated.
#[test]
fn an_unconnected_workflow_refuses_before_comparison_preparation_or_dispatch() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("workflow-not-connected");
    let mut document = load_json(&fixture.spec);
    document["comparison"]["workflow"] = json!({
        "schema": 1,
        "lead_profile": "fixture-lead",
        "executor_profile": "ds",
        "max_corrections": 1
    });
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("workflow runner is not connected"),
        "the unsupported workflow must retain its cause: {output}"
    );
    assert!(
        !fixture.run.join("comparison").exists(),
        "the refusal must precede comparison preparation"
    );
    let status = fixture.status_json();
    let cursor = fixture.cursor();
    assert!(
        cursor["effects"].as_array().unwrap().iter().any(|effect| {
            effect["kind"] == "dispatch-refused"
                && effect["detail"]
                    .as_str()
                    .is_some_and(|detail| detail.contains("workflow runner is not connected"))
        }),
        "the refusal must survive status refresh: {cursor}"
    );
    assert_eq!(status["attempts"].as_array().unwrap().len(), 0, "{output}");

    let retry = fixture.resume();
    assert!(retry.status.success(), "{}", text(&retry));
    assert_eq!(
        fixture.status_json()["attempts"].as_array().unwrap().len(),
        0
    );
    assert!(!fixture.run.join("comparison").exists());
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
