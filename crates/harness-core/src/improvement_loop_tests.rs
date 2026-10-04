use super::*;
use crate::board_hypothesis::{AuthorityRequest, RemovalAction};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

fn fixture_root(name: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temp dir");
    let project = root.path().join(format!("proj-{name}"));
    fs::create_dir_all(project.join("crates/one/src")).unwrap();
    fs::create_dir_all(root.path().join("home")).unwrap();
    fs::create_dir_all(root.path().join("openspec/changes/add-synthetic")).unwrap();
    fs::write(project.join("bd.exe"), "placeholder").unwrap();
    root
}

fn experiment() -> crate::improvement_spec::ExperimentContract {
    crate::improvement_spec::ExperimentContract {
        acceptance_artifact: PathBuf::from("specs/synthetic/spec.md"),
        acceptance_heading: "#### Scenario: Synthetic case".to_owned(),
        mechanism: "bounded-output".to_owned(),
        counterexample: "diagnostics vanish".to_owned(),
        applicability: "local tool runs".to_owned(),
        independent_acceptance: "the oracle checker runs".to_owned(),
        meaningful_effect: "fewer repeated loads".to_owned(),
        operating_conditions: "cold context".to_owned(),
        comparison_policy: "matched pairs".to_owned(),
        stopping_rule: "two repeats".to_owned(),
    }
}

fn spec_fixture(root: &Path, name: &str) -> RunSpec {
    let project = root.join(format!("proj-{name}"));
    RunSpec {
        schema: RUN_SCHEMA,
        run: format!("loop-{name}"),
        project: project.clone(),
        codex_home: root.join("home"),
        board: BoardInputs {
            bd: project.join("bd.exe"),
            project: project.clone(),
        },
        specification: crate::improvement_spec::Specification {
            project: project.clone(),
            change: "add-synthetic".to_owned(),
            store: None,
            planning_root: root.join("openspec"),
        },
        hypothesis_item: "bdct-h1".to_owned(),
        experiment: experiment(),
        base_revision: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        writable_scope: vec!["crates/one".to_owned()],
        runner: None,
        local_runner: None,
        qualification: None,
        comparison: None,
        publication_scope: vec![PublicationStage::Experiment],
        oracle: "outcome-oracle:private-request".to_owned(),
        removal: None,
        evidence_root: None,
    }
}

#[test]
fn run_inputs_reject_missing_or_inconsistent_fields() {
    let root = fixture_root("inputs");
    let spec = spec_fixture(root.path(), "inputs");
    spec.validate().expect("complete inputs validate");

    let mut missing_root = spec.clone();
    missing_root.specification.planning_root = root.path().join("absent");
    assert!(missing_root.validate().is_err());

    let mut mismatched = spec.clone();
    mismatched.specification.project = root.path().to_path_buf();
    assert!(mismatched.validate().is_err());

    let mut pair = spec.clone();
    pair.runner = Some(RunnerInputs {
        profile: "ds".to_owned(),
        model: Some("deepseek-flash".to_owned()),
        model_provider: None,
        reasoning_effort: None,
        retry: None,
    });
    assert!(pair.validate().is_err());

    let mut traversal = spec.clone();
    traversal.writable_scope = vec!["../outside".to_owned()];
    assert!(traversal.validate().is_err());
    let mut metadata = spec.clone();
    metadata.writable_scope = vec![".git/hooks".to_owned()];
    assert!(metadata.validate().is_err());

    let mut revision = spec.clone();
    revision.base_revision = "not-a-revision".to_owned();
    assert!(revision.validate().is_err());

    let mut qualification = spec.clone();
    qualification.local_runner = Some(crate::outcome_qualification::LocalRunner {
        endpoint: "http://127.0.0.1:9/v1".to_owned(),
        model: "synthetic".to_owned(),
        identity: Default::default(),
    });
    assert!(
        qualification.validate().is_err(),
        "a local runner without its qualification record path is inconsistent"
    );

    let mut duplicate = spec.clone();
    duplicate.publication_scope = vec![PublicationStage::Experiment, PublicationStage::Experiment];
    assert!(duplicate.validate().is_err());

    let mut zero = spec.clone();
    zero.writable_scope.clear();
    assert!(zero.validate().is_err());
}

#[test]
fn supervisor_gate_refuses_candidate_scopes_over_control_inputs() {
    let root = fixture_root("supervisor");
    let spec = spec_fixture(root.path(), "supervisor");
    let run_dir = root.path().join("run-state");
    let change_root = spec.project.join("openspec/changes/add-synthetic");
    spec.supervisor_gate(&run_dir, &change_root, &spec.oracle)
        .expect("a candidate source scope is admitted");

    let mut covers_change = spec.clone();
    covers_change.writable_scope = vec!["openspec/changes/add-synthetic/specs".to_owned()];
    assert!(
        covers_change
            .supervisor_gate(&run_dir, &change_root, &spec.oracle)
            .is_err(),
        "a candidate cannot rewrite its own planning artifacts"
    );

    let mut covers_run = spec.clone();
    covers_run.writable_scope = vec!["crates/one".to_owned()];
    let nested_run = spec.project.join("crates/one/run-state");
    assert!(
        covers_run
            .supervisor_gate(&nested_run, &change_root, &spec.oracle)
            .is_err(),
        "a candidate cannot rewrite the controller's own state"
    );

    let covers_oracle = spec.clone();
    let oracle_path = spec.project.join("crates/one/oracle/request.json");
    assert!(
        covers_oracle
            .supervisor_gate(&run_dir, &change_root, &oracle_path.display().to_string())
            .is_err(),
        "a candidate cannot reach acceptance inputs"
    );
    assert!(absolute_path_reference("outcome-oracle:request").is_none());
}

#[test]
fn a_second_start_duplicates_ownership_and_is_refused() {
    let root = fixture_root("ownership");
    let spec = spec_fixture(root.path(), "ownership");
    let run_dir = root.path().join("run");
    let change_root = root.path().join("openspec/changes/add-synthetic");
    let digest = spec.digest().unwrap();
    let store = RunStore::create(&run_dir, &spec, &digest, &change_root).unwrap();
    let error = RunStore::create(&run_dir, &spec, &digest, &change_root).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("duplicate run ownership"), "{message}");
    assert!(message.contains("improve resume"), "{message}");
    assert_eq!(store.cursor().unwrap().phase, Phase::Planning);
}

#[test]
fn the_exclusive_mutation_guard_serializes_state_writers() {
    let root = fixture_root("mutation");
    let spec = spec_fixture(root.path(), "mutation");
    let run_dir = root.path().join("run");
    let change_root = root.path().join("openspec/changes/add-synthetic");
    let (store, guard) = RunStore::lock_new(&run_dir).unwrap();
    store
        .create_locked(&spec, &spec.digest().unwrap(), &change_root)
        .unwrap();
    store.claim_ownership(&spec.run).unwrap();

    // A second writer waits bounded and then reports the busy owner instead of
    // reading or changing state.
    let error = RunStore::lock_new_within(&run_dir, Duration::from_millis(150)).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("run-mutation guard"), "{message}");
    assert!(
        message.contains("no state was read or changed"),
        "{message}"
    );

    // Releasing the guard admits the next writer, which then observes the
    // fresh state and refuses to create a duplicate run.
    drop(guard);
    let (store, _guard) = RunStore::lock_new_within(&run_dir, Duration::from_secs(5)).unwrap();
    assert_eq!(store.cursor().unwrap().phase, Phase::Planning);
    let error = store
        .create_locked(&spec, &spec.digest().unwrap(), &change_root)
        .unwrap_err();
    assert!(
        error.to_string().contains("duplicate run ownership"),
        "{error}"
    );
}

#[test]
fn ownership_takeover_is_recorded_under_the_guard() {
    let root = fixture_root("live-owner");
    let spec = spec_fixture(root.path(), "live-owner");
    let run_dir = root.path().join("run");
    let change_root = root.path().join("openspec/changes/add-synthetic");
    let store = RunStore::create(&run_dir, &spec, &spec.digest().unwrap(), &change_root).unwrap();

    let pwsh = resolve_pwsh().expect("owner PowerShell 7");
    let mut sleeper = std::process::Command::new(&pwsh)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 60",
        ])
        .spawn()
        .expect("pwsh 7 sleeper");
    let process = crate::process_service::ServiceProcess::observe(
        sleeper.id(),
        &pwsh,
        0,
        &crate::process_service::current_user().unwrap(),
    )
    .expect("the live sleeper is identifiable by its exact identity");
    let identity = process.identity();
    let record = OwnerRecord {
        schema: OWNER_SCHEMA,
        run: spec.run.clone(),
        pid: identity.pid,
        created: identity.creation_time,
        program: pwsh.clone(),
        claimed_ms: now_ms(),
    };
    write_json_atomic(&store.owner_path(), &record).unwrap();
    let ownership = store.claim_ownership(&spec.run).unwrap();
    assert_eq!(
        ownership.previous.as_ref().map(|previous| previous.pid),
        Some(sleeper.id())
    );
    assert_eq!(ownership.previous_live, Some(true));
    let note = ownership.takeover_note().expect("takeover is recorded");
    assert!(
        note.contains("still running") && note.contains("exclusive run-mutation guard"),
        "{note}"
    );
    assert_eq!(store.owner().unwrap().unwrap().pid, std::process::id());
    let _ = sleeper.kill();
    let _ = sleeper.wait();

    let dead = OwnerRecord {
        schema: OWNER_SCHEMA,
        run: spec.run.clone(),
        pid: 0xFFFF_FFFE,
        created: 7,
        program: PathBuf::from(r"C:\missing\owner.exe"),
        claimed_ms: 0,
    };
    write_json_atomic(&store.owner_path(), &dead).unwrap();
    let ownership = store
        .claim_ownership(&spec.run)
        .expect("stale owner taken over");
    assert_eq!(ownership.owner.pid, std::process::id());
    assert_eq!(ownership.previous_live, Some(false));
    assert!(
        ownership
            .takeover_note()
            .unwrap()
            .contains("no longer running")
    );
}

#[test]
fn stopped_unknown_attempts_remain_reconciliation_candidates() {
    let mut cursor = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        PathBuf::from(r"C:\work\openspec\changes\x"),
        "bdct-h1",
    );
    cursor
        .push_attempt(attempt(
            "a1",
            AttemptRole::Implementer,
            AttemptState::Started,
        ))
        .unwrap();
    cursor
        .push_attempt(attempt(
            "a2",
            AttemptRole::Baseline,
            AttemptState::Completed,
        ))
        .unwrap();
    assert_eq!(cursor.attempts_requiring_reconciliation().len(), 1);
    cursor.stop("owner pause");
    assert_eq!(cursor.attempt("a1").unwrap().state, AttemptState::Unknown);
    let pending: Vec<&str> = cursor
        .attempts_requiring_reconciliation()
        .iter()
        .map(|attempt| attempt.id.as_str())
        .collect();
    assert_eq!(
        pending,
        ["a1"],
        "a retained unknown attempt is reconciled on the next resume while a terminal arm is not"
    );
}

fn resolve_pwsh() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("pwsh.exe"))
        .find(|candidate| candidate.is_file())
}

fn attempt(id: &str, role: AttemptRole, state: AttemptState) -> Attempt {
    Attempt {
        id: id.to_owned(),
        role,
        binding: None,
        retained: None,
        owner: dispatch_owner("loop-fixture", role, 1),
        title: format!("CEx (ds) - {id}"),
        profile: "ds".to_owned(),
        model: None,
        model_provider: None,
        reasoning_effort: None,
        checkout: None,
        assignment: None,
        receipt: None,
        result: None,
        detail: None,
        state,
        reason: None,
        reuse_refused: None,
        started_ms: 0,
        updated_ms: 0,
    }
}

#[test]
fn stop_and_resume_preserve_each_effect_boundary_without_replay() {
    let phases = [
        Phase::Planning,
        Phase::CandidateReady,
        Phase::BaselineAttempt,
        Phase::CandidateAttempt,
        Phase::Acceptance,
        Phase::DecisionRecorded,
        Phase::ActivationConfirmed,
        Phase::Idle,
    ];
    for phase in phases {
        let mut cursor = Cursor::new(
            "loop-fixture",
            "a".repeat(64),
            PathBuf::from(r"C:\work\openspec\changes\x"),
            "bdct-h1",
        );
        cursor.phase = phase;
        cursor
            .push_attempt(attempt(
                "a1",
                AttemptRole::Investigator,
                AttemptState::Completed,
            ))
            .unwrap();
        cursor
            .push_attempt(attempt(
                "a2",
                AttemptRole::Implementer,
                AttemptState::Started,
            ))
            .unwrap();
        let effects = cursor.effects.len();
        cursor.stop("user stop");
        assert_eq!(cursor.phase, Phase::Stopped);
        assert_eq!(cursor.attempt("a2").unwrap().state, AttemptState::Unknown);
        assert!(cursor.effects.len() > effects);
        // The unknown in-flight attempt is retained and blocks new dispatch.
        let gate = dispatch_gate(
            &cursor,
            AttemptRole::Investigator,
            &DispatchFacts {
                runner_declared: true,
                launcher: Some(PathBuf::from(r"C:\home\harness\bin\codex.exe")),
                ..Default::default()
            },
        );
        assert!(matches!(gate, DispatchGate::Blocked { .. }));
        assert_eq!(cursor.resume_phase(), phase);
        assert_eq!(cursor.attempt("a1").unwrap().state, AttemptState::Completed);
        assert_eq!(cursor.attempt("a2").unwrap().state, AttemptState::Unknown);
        assert_eq!(cursor.attempts.len(), 2, "resume never appends a replay");
    }
}

#[test]
fn unknown_attempts_block_new_dispatch_and_are_never_resubmitted() {
    let mut cursor = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        PathBuf::from(r"C:\work\openspec\changes\x"),
        "bdct-h1",
    );
    cursor
        .push_attempt(attempt(
            "a1",
            AttemptRole::Implementer,
            AttemptState::Started,
        ))
        .unwrap();
    let mut started = attempt("a1", AttemptRole::Implementer, AttemptState::Started);
    started.settle(ObservedOutcome::Unknown, 1);
    cursor.attempts[0] = started;
    let gate = dispatch_gate(
        &cursor,
        AttemptRole::Implementer,
        &DispatchFacts {
            runner_declared: true,
            launcher: Some(PathBuf::from(r"C:\home\harness\bin\codex.exe")),
            ..Default::default()
        },
    );
    match gate {
        DispatchGate::Blocked { reason } => {
            assert!(reason.contains("unknown outcome"), "{reason}");
            assert!(reason.contains("never resubmitted"), "{reason}");
        }
        other => panic!("expected blocked, got {other:?}"),
    }
    let report = ResumeReport {
        unknown: vec!["a1".to_owned()],
        ..Default::default()
    };
    assert!(report.condition().unwrap().contains("unknown outcomes"));
}

#[test]
fn completed_arms_are_reused_only_while_their_inputs_validate() {
    let mut completed = attempt("a1", AttemptRole::Baseline, AttemptState::Completed);
    assert!(settle_completed_reuse(&mut completed, Ok(())));
    assert!(completed.reuse_refused.is_none());
    assert!(!settle_completed_reuse(
        &mut completed,
        Err("planning artifacts changed".to_owned())
    ));
    assert!(
        completed
            .reuse_refused
            .as_deref()
            .unwrap()
            .contains("planning")
    );
    let report = ResumeReport {
        remeasure: vec![("a1".to_owned(), "planning artifacts changed".to_owned())],
        ..Default::default()
    };
    assert!(report.condition().unwrap().contains("remeasurement"));
}

const DETAIL: &str = "consumer list: none known";

fn proposal_text(loss: &str, evidence: &str, preview: &str, detail: &str) -> String {
    format!(
        "removal-proposal v1 item=bdct-h1 proposal=openspec/changes/remove-x target=skill-x evidence={evidence} loss={loss} preview={preview} detail={detail}"
    )
}

fn reviewed_digest(loss: &str, evidence: &str, preview: &str, detail: &str) -> String {
    let text = proposal_text(loss, evidence, preview, detail);
    let record = crate::board_hypothesis::parse_removal_proposals(&[text])
        .into_iter()
        .next()
        .expect("complete proposal");
    crate::board_hypothesis::reviewed_proposal_digest(&record)
}

fn decision_text(decision: &str, actions: &str, loss: &str, detail: &str) -> String {
    format!(
        "removal-decision v1 item=bdct-h1 decision={decision} proposal=openspec/changes/remove-x target=skill-x actions={actions} loss={loss} evidence=evidence-1 preview=preview-1 reviewed={} basis=user-turn-7",
        reviewed_digest(loss, "evidence-1", "preview-1", detail)
    )
}

fn request() -> AuthorityRequest {
    AuthorityRequest {
        proposal: "openspec/changes/remove-x".to_owned(),
        target: "skill-x".to_owned(),
        action: RemovalAction::Experiment,
    }
}

#[test]
fn removal_gate_blocks_missing_refused_withdrawn_and_changed_scope() {
    let proposal = proposal_text("retired-skill", "evidence-1", "preview-1", DETAIL);
    let missing = removal_gate_at("bdct-h1", &request(), std::slice::from_ref(&proposal), None);
    assert!(matches!(missing, RemovalGate::Pending { .. }));

    let approved = vec![
        proposal.clone(),
        decision_text("approve", "experiment", "retired-skill", DETAIL),
    ];
    let frozen = reviewed_digest("retired-skill", "evidence-1", "preview-1", DETAIL);
    let authorized = removal_gate_at("bdct-h1", &request(), &approved, Some(&frozen));
    assert!(matches!(authorized, RemovalGate::Authorized { .. }));
    assert!(authorized_permits(&authorized));

    // A repeated decision for a changed detailed proposal content invalidates
    // the earlier approval: the board owner reports it as uncovered.
    let mut changed = approved.clone();
    changed.push(proposal_text(
        "retired-skill",
        "evidence-1",
        "preview-1",
        "consumer list: one indirect caller found",
    ));
    let pending = removal_gate_at("bdct-h1", &request(), &changed, Some(&frozen));
    match pending {
        RemovalGate::Pending { reason } => assert!(reason.contains("changed"), "{reason}"),
        other => panic!("expected pending, got {other:?}"),
    }

    let refused = vec![
        proposal.clone(),
        decision_text("refuse", "none", "retired-skill", DETAIL),
    ];
    assert!(matches!(
        removal_gate_at("bdct-h1", &request(), &refused, None),
        RemovalGate::Refused { .. }
    ));
    let withdrawn = vec![
        proposal,
        decision_text("withdraw", "none", "retired-skill", DETAIL),
    ];
    assert!(matches!(
        removal_gate_at("bdct-h1", &request(), &withdrawn, None),
        RemovalGate::Withdrawn { .. }
    ));
}

/// One recorded proposal/decision pair for an arbitrary capability target, in
/// the exact board comment shapes the owners write.
fn capability_proposal_text(target: &str, detail: &str) -> String {
    format!(
        "removal-proposal v1 item=bdct-h1 proposal=remove-skill target={target} evidence=evidence-1 loss=retired-capability preview=preview-1 detail={detail}"
    )
}

fn capability_decision_text(decision: &str, actions: &str, target: &str, reviewed: &str) -> String {
    format!(
        "removal-decision v1 item=bdct-h1 decision={decision} proposal=remove-skill target={target} actions={actions} loss=retired-capability evidence=evidence-1 preview=preview-1 reviewed={reviewed} basis=user-turn-7"
    )
}

/// The digest a decision owner records for one complete proposal comment.
fn recorded_comment_digest(proposal: &str) -> String {
    let record = crate::board_hypothesis::parse_removal_proposals(&[proposal.to_owned()])
        .into_iter()
        .next()
        .expect("a complete recorded proposal");
    crate::board_hypothesis::reviewed_proposal_digest(&record)
}

#[test]
fn removal_targets_accept_the_board_grammar_and_refuse_path_shapes() {
    let root = fixture_root("target-grammar");
    let mut spec = spec_fixture(root.path(), "target-grammar");
    let removal = |target: &str| RemovalScope {
        proposal: "remove-skill".to_owned(),
        target: target.to_owned(),
    };
    // The board owner records capability targets with the same single-token
    // grammar, including the `owner:name` separator.
    spec.removal = Some(removal("skill:context-heavy"));
    spec.validate().expect("a board-recorded target declares");
    spec.removal = Some(removal("kit@2.0+trial#1;stable,quiet"));
    spec.validate()
        .expect("the board token set stays declarable");
    let sized = "x".repeat(96);
    spec.removal = Some(removal(&sized));
    spec.validate()
        .expect("a target sized for the board stays declarable");

    let oversized = "x".repeat(201);
    for target in [
        ".",
        "..",
        "../escaped",
        "skill/context",
        r"nested\target",
        r"..\escaped",
        "/etc/passwd",
        r"C:\absolute",
        "skill:two words",
        oversized.as_str(),
    ] {
        spec.removal = Some(removal(target));
        assert!(spec.validate().is_err(), "{target} must stay undeclarable");
    }
}

#[test]
fn workload_removal_targets_accept_the_board_grammar_and_refuse_path_shapes() {
    let root = fixture_root("workload-target");
    let mut spec = comparison_spec(root.path(), "workload-target");
    let removal = |target: &str| RemovalScope {
        proposal: "remove-skill".to_owned(),
        target: target.to_owned(),
    };
    spec.comparison.as_mut().unwrap().workload_removal = Some(removal("skill:context-heavy"));
    spec.validate()
        .expect("a board-recorded workload target declares");
    for target in ["..", "nested/target", r"nested\target", r"C:\absolute"] {
        spec.comparison.as_mut().unwrap().workload_removal = Some(removal(target));
        assert!(spec.validate().is_err(), "{target} must stay undeclarable");
    }
}

#[test]
fn board_grammar_removal_target_is_declared_and_gated_end_to_end() {
    let root = fixture_root("capability-gate");
    let mut spec = spec_fixture(root.path(), "capability-gate");
    spec.removal = Some(RemovalScope {
        proposal: "remove-skill".to_owned(),
        target: "skill:context-heavy".to_owned(),
    });
    // The declaration loads through the run's own entry point, so the accepted
    // grammar is exactly what a real run accepts.
    let spec_path = root.path().join("run.json");
    fs::write(&spec_path, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    let spec = RunSpec::load(&spec_path).expect("the declared run loads");

    let proposal = capability_proposal_text("skill:context-heavy", "consumer list: none known");
    let expected_reviewed = recorded_comment_digest(&proposal);
    let comments = vec![
        proposal,
        capability_decision_text(
            "approve",
            "experiment",
            "skill:context-heavy",
            &expected_reviewed,
        ),
    ];
    let frozen = frozen_removal_digest(&spec, &comments).expect("the reviewed version freezes");
    let mut cursor = Cursor::new(
        &spec.run,
        spec.digest().unwrap(),
        spec.project.join("openspec/changes/add-synthetic"),
        &spec.hypothesis_item,
    );
    cursor.removal_frozen = Some(frozen);
    let gate = declared_removal_gate(&spec, &cursor, &comments, RemovalAction::Experiment)
        .expect("the declared removal has a gate");
    match &gate {
        RemovalGate::Authorized { reviewed } => assert_eq!(reviewed, &expected_reviewed),
        other => panic!("expected an authorized removal, got {other:?}"),
    }
    selection_gate(&cursor, "candidate", Some(&gate))
        .expect("the reviewed board target clears selection");
}

#[test]
fn a_fresh_decision_after_a_changed_proposal_clears_the_stale_frozen_digest() {
    let root = fixture_root("fresh-decision");
    let mut spec = spec_fixture(root.path(), "fresh-decision");
    spec.removal = Some(RemovalScope {
        proposal: "remove-skill".to_owned(),
        target: "skill:context-heavy".to_owned(),
    });
    spec.validate().expect("the declared removal validates");
    let stable = capability_proposal_text("skill:context-heavy", "consumer list: none known");
    let stable_reviewed = recorded_comment_digest(&stable);
    let approved = vec![
        stable,
        capability_decision_text(
            "approve",
            "experiment",
            "skill:context-heavy",
            &stable_reviewed,
        ),
    ];
    // The run freezes the version and decision that existed when it started.
    let frozen = frozen_removal_digest(&spec, &approved).expect("the frozen reviewed version");
    let mut cursor = Cursor::new(
        &spec.run,
        spec.digest().unwrap(),
        spec.project.join("openspec/changes/add-synthetic"),
        &spec.hypothesis_item,
    );
    cursor.removal_frozen = Some(frozen);

    // The reviewed proposal changes after the freeze (a newly discovered
    // consumer): the older approval no longer covers it and selection refuses.
    let changed = capability_proposal_text(
        "skill:context-heavy",
        "consumer list: one indirect caller found",
    );
    let changed_reviewed = recorded_comment_digest(&changed);
    let mut changed_comments = approved.clone();
    changed_comments.push(changed);
    let stale = declared_removal_gate(&spec, &cursor, &changed_comments, RemovalAction::Experiment)
        .expect("the declared removal has a gate");
    match &stale {
        RemovalGate::Pending { reason } => {
            assert!(
                reason.contains("changed after the frozen approval"),
                "{reason}"
            );
            assert!(reason.contains("fresh decision"), "{reason}");
        }
        other => panic!("expected pending on the changed proposal, got {other:?}"),
    }
    assert!(selection_gate(&cursor, "candidate", Some(&stale)).is_err());

    // A fresh decision bound to the changed version - the exact scope
    // `removal-check` resolves - clears the stale frozen digest.
    let mut fresh = changed_comments.clone();
    fresh.push(capability_decision_text(
        "approve",
        "experiment",
        "skill:context-heavy",
        &changed_reviewed,
    ));
    let gate = declared_removal_gate(&spec, &cursor, &fresh, RemovalAction::Experiment)
        .expect("the declared removal has a gate");
    match &gate {
        RemovalGate::Authorized { reviewed } => assert_eq!(reviewed, &changed_reviewed),
        other => panic!("expected the fresh approval to authorize, got {other:?}"),
    }
    selection_gate(&cursor, "candidate", Some(&gate)).expect("the fresh decision clears select");

    // A withdrawal recorded for the changed version stops selection as a final
    // decision instead of prompting again for the stale version.
    let mut withdrawn = changed_comments.clone();
    withdrawn.push(capability_decision_text(
        "withdraw",
        "none",
        "skill:context-heavy",
        &changed_reviewed,
    ));
    let gate = declared_removal_gate(&spec, &cursor, &withdrawn, RemovalAction::Experiment)
        .expect("the declared removal has a gate");
    assert!(matches!(gate, RemovalGate::Withdrawn { .. }), "{gate:?}");
    let refusal = selection_gate(&cursor, "candidate", Some(&gate)).unwrap_err();
    assert!(refusal.contains("withdrawn"), "{refusal}");

    let mut refused = changed_comments.clone();
    refused.push(capability_decision_text(
        "refuse",
        "none",
        "skill:context-heavy",
        &changed_reviewed,
    ));
    let gate = declared_removal_gate(&spec, &cursor, &refused, RemovalAction::Experiment)
        .expect("the declared removal has a gate");
    assert!(matches!(gate, RemovalGate::Refused { .. }), "{gate:?}");
    let refusal = selection_gate(&cursor, "candidate", Some(&gate)).unwrap_err();
    assert!(refusal.contains("declined"), "{refusal}");
}

fn authorized_permits(gate: &RemovalGate) -> bool {
    matches!(gate, RemovalGate::Authorized { .. })
}

#[test]
fn dispatch_gate_requires_model_inputs_visibility_and_current_authority() {
    let cursor = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        PathBuf::from(r"C:\work\openspec\changes\x"),
        "bdct-h1",
    );
    let ready = dispatch_gate(
        &cursor,
        AttemptRole::Investigator,
        &DispatchFacts {
            runner_declared: true,
            launcher: Some(PathBuf::from(r"C:\home\harness\bin\codex.exe")),
            ..Default::default()
        },
    );
    // A missing file at the launcher path is missing visibility, not a ready
    // dispatch; the fixture proves the gate itself, so name both outcomes.
    assert!(matches!(ready, DispatchGate::Blocked { .. }));
    match ready {
        DispatchGate::Blocked { reason } => {
            assert!(reason.contains("missing visibility"), "{reason}")
        }
        _ => unreachable!(),
    }

    let ctx = tempfile::tempdir().unwrap();
    let launcher = ctx.path().join("codex.exe");
    fs::write(&launcher, "fixture").unwrap();
    let ready = dispatch_gate(
        &cursor,
        AttemptRole::Investigator,
        &DispatchFacts {
            runner_declared: true,
            launcher: Some(launcher.clone()),
            ..Default::default()
        },
    );
    assert_eq!(ready, DispatchGate::Ready);

    let pending = dispatch_gate(
        &cursor,
        AttemptRole::Investigator,
        &DispatchFacts {
            runner_declared: false,
            launcher: Some(launcher.clone()),
            ..Default::default()
        },
    );
    match pending {
        DispatchGate::Blocked { reason } => {
            assert!(reason.contains("model inputs are pending"), "{reason}")
        }
        _ => unreachable!(),
    }

    let lost = dispatch_gate(
        &cursor,
        AttemptRole::Investigator,
        &DispatchFacts {
            runner_declared: true,
            launcher: Some(launcher.clone()),
            surface_loss: Some("the owned native frontend exited".to_owned()),
            ..Default::default()
        },
    );
    match lost {
        DispatchGate::Blocked { reason } => {
            assert!(reason.contains("missing visibility"), "{reason}");
            assert!(reason.contains("no hidden fallback"), "{reason}");
        }
        _ => unreachable!(),
    }

    let unqualified = dispatch_gate(
        &cursor,
        AttemptRole::Candidate,
        &DispatchFacts {
            runner_declared: true,
            launcher: Some(launcher.clone()),
            qualification_block: Some("missing material identity: seed".to_owned()),
            ..Default::default()
        },
    );
    match unqualified {
        DispatchGate::Blocked { reason } => {
            assert!(reason.contains("local qualification"), "{reason}")
        }
        _ => unreachable!(),
    }

    // A declared configuration that cannot be honored is an unusable profile,
    // not a reason to dispatch under a different model or route.
    let misbound = dispatch_gate(
        &cursor,
        AttemptRole::Investigator,
        &DispatchFacts {
            runner_declared: true,
            launcher: Some(launcher.clone()),
            binding_error: Some(
                "the declared model other-model does not match the installed profile binding deepseek-flash"
                    .to_owned(),
            ),
            ..Default::default()
        },
    );
    match misbound {
        DispatchGate::Blocked { reason } => {
            assert!(
                reason.contains("the dispatch profile is unusable"),
                "{reason}"
            )
        }
        _ => unreachable!(),
    }

    let removal = dispatch_gate(
        &cursor,
        AttemptRole::Candidate,
        &DispatchFacts {
            runner_declared: true,
            launcher: Some(launcher),
            removal: Some(RemovalGate::Pending {
                reason: "no removal decision is recorded".to_owned(),
            }),
            ..Default::default()
        },
    );
    match removal {
        DispatchGate::Blocked { reason } => {
            assert!(reason.contains("removal approval is pending"), "{reason}")
        }
        _ => unreachable!(),
    }

    let mut active = cursor.clone();
    active
        .push_attempt(attempt("a1", AttemptRole::Candidate, AttemptState::Started))
        .unwrap();
    assert!(matches!(
        dispatch_gate(
            &active,
            AttemptRole::Investigator,
            &DispatchFacts::default()
        ),
        DispatchGate::Blocked { .. }
    ));
    assert!(selection_gate(&active, "candidate", None).is_err());
}

#[test]
fn dispatch_owners_stay_bounded_distinct_and_deterministic() {
    let long_run = "improve-loop-run-with-a-deliberately-very-long-identifier-0123456789abcdef";
    let first = dispatch_owner(long_run, AttemptRole::Investigator, 1);
    let second = dispatch_owner(long_run, AttemptRole::Implementer, 1);
    let other = dispatch_owner(
        "another-long-run-identifier-abcdef0123456789-fedcba9876543210",
        AttemptRole::Investigator,
        1,
    );
    assert!(first.len() <= MAX_DISPATCH_OWNER, "{first}");
    assert!(second.len() <= MAX_DISPATCH_OWNER, "{second}");
    assert!(other.len() <= MAX_DISPATCH_OWNER, "{other}");
    assert_ne!(first, second);
    assert_ne!(first, other);
    assert_eq!(
        first,
        dispatch_owner(long_run, AttemptRole::Investigator, 1)
    );
    assert_eq!(
        dispatch_owner("short", AttemptRole::Investigator, 1),
        "short-inv-1"
    );
}

#[test]
fn variant_sets_name_prepared_runtimes_without_consuming_them() {
    let root = fixture_root("variants");
    let path = root.path().join("variants.json");
    let baseline = root.path().join("state-baseline");
    fs::create_dir_all(baseline.join("builds/base")).unwrap();
    let set = VariantSet {
        schema: VARIANTS_SCHEMA,
        baseline: Variant {
            state: baseline.clone(),
            build: baseline.join("builds/base"),
            identity: Some("baseline-sha".to_owned()),
        },
        candidate: Variant {
            state: root.path().join("state-candidate"),
            build: root.path().join("state-candidate/builds/cand"),
            identity: None,
        },
    };
    write_json_atomic(&path, &set).unwrap();
    let loaded = VariantSet::load(&path).unwrap();
    assert_eq!(loaded.baseline.build, set.baseline.build);
    assert!(loaded.named("candidate").is_some());
    assert!(loaded.named("other").is_none());
}

fn binding_fixture() -> DispatchBinding {
    DispatchBinding {
        slot: 1,
        owner: "loop-fixture-implementer-1".to_owned(),
        generation: "gen-accepted".to_owned(),
        receipt: PathBuf::from(r"C:\pool\spawn-1.json"),
        session: None,
        host: None,
    }
}

#[test]
fn dispatch_identity_requires_the_accepted_generation_and_host() {
    let binding = binding_fixture();
    let accepted = ObservedIdentity {
        slot: Some(1),
        owner: Some("loop-fixture-implementer-1".to_owned()),
        generation: Some("gen-accepted".to_owned()),
        session: None,
        host: None,
    };
    assert_eq!(
        verify_dispatch_identity(&binding, &accepted),
        IdentityCheck::Verified
    );

    let replaced = ObservedIdentity {
        generation: Some("gen-newer".to_owned()),
        ..accepted.clone()
    };
    match verify_dispatch_identity(&binding, &replaced) {
        IdentityCheck::Mismatch(reason) => assert!(reason.contains("generation"), "{reason}"),
        other => panic!("expected a generation mismatch, got {other:?}"),
    }
    let unrecorded = ObservedIdentity {
        generation: None,
        ..accepted.clone()
    };
    assert!(matches!(
        verify_dispatch_identity(&binding, &unrecorded),
        IdentityCheck::Missing(_)
    ));
    let other_slot = ObservedIdentity {
        slot: Some(2),
        ..accepted.clone()
    };
    assert!(matches!(
        verify_dispatch_identity(&binding, &other_slot),
        IdentityCheck::Mismatch(_)
    ));

    // Once a session/host has been observed for the verified generation, a
    // different one is refused instead of being read as this attempt's run.
    let mut frozen = binding.clone();
    frozen.session = Some("session-a".to_owned());
    frozen.host = Some(HostBinding {
        pid: 7,
        created: 9,
        program: PathBuf::from(r"C:\fixture\codex.exe"),
    });
    let matched = ObservedIdentity {
        session: Some("session-a".to_owned()),
        host: Some(HostBinding {
            pid: 7,
            created: 9,
            program: PathBuf::from(r"C:\fixture\codex.exe"),
        }),
        ..accepted.clone()
    };
    assert_eq!(
        verify_dispatch_identity(&frozen, &matched),
        IdentityCheck::Verified
    );
    let drifted = ObservedIdentity {
        session: Some("session-b".to_owned()),
        ..matched.clone()
    };
    assert!(matches!(
        verify_dispatch_identity(&frozen, &drifted),
        IdentityCheck::Mismatch(_)
    ));
    let other_host = ObservedIdentity {
        host: Some(HostBinding {
            pid: 8,
            created: 9,
            program: PathBuf::from(r"C:\fixture\codex.exe"),
        }),
        ..matched
    };
    assert!(matches!(
        verify_dispatch_identity(&frozen, &other_host),
        IdentityCheck::Mismatch(_)
    ));
}

#[test]
fn freeze_observed_records_the_first_verified_session_and_host_only() {
    let mut attempt = attempt("a1", AttemptRole::Implementer, AttemptState::Started);
    attempt.binding = Some(binding_fixture());
    let first = ObservedIdentity {
        slot: Some(1),
        owner: Some("loop-fixture-implementer-1".to_owned()),
        generation: Some("gen-accepted".to_owned()),
        session: Some("session-a".to_owned()),
        host: Some(HostBinding {
            pid: 7,
            created: 9,
            program: PathBuf::from(r"C:\fixture\codex.exe"),
        }),
    };
    attempt.freeze_observed(&first);
    let binding = attempt.binding.as_ref().unwrap();
    assert_eq!(binding.session.as_deref(), Some("session-a"));
    assert_eq!(binding.host.as_ref().map(|host| host.pid), Some(7));
    let later = ObservedIdentity {
        session: Some("session-b".to_owned()),
        ..first
    };
    attempt.freeze_observed(&later);
    let binding = attempt.binding.as_ref().unwrap();
    assert_eq!(
        binding.session.as_deref(),
        Some("session-a"),
        "the first verified identity is not overwritten by later observations"
    );
}

#[test]
fn terminal_evidence_is_retained_bounded_with_digests() {
    let root = fixture_root("retained");
    let spec = spec_fixture(root.path(), "retained");
    let run_dir = root.path().join("run");
    let change_root = root.path().join("openspec/changes/add-synthetic");
    let store = RunStore::create(&run_dir, &spec, &spec.digest().unwrap(), &change_root).unwrap();
    let receipt = root.path().join("spawn-1.json");
    let receipt_bytes = b"{\"observation\":{\"state\":\"completed\"}}\n";
    fs::write(&receipt, receipt_bytes).unwrap();
    let result = root.path().join("message-1.txt");
    fs::write(&result, "final answer\n").unwrap();

    let retained = store
        .retain_evidence("implementer-1", &receipt, Some(&result))
        .unwrap();
    assert_eq!(retained.receipt_sha256, digest_bytes(receipt_bytes));
    assert_eq!(fs::read(&retained.receipt).unwrap(), receipt_bytes);
    let retained_result = retained.result.as_ref().unwrap();
    assert_eq!(
        retained.result_sha256.as_deref(),
        Some(digest_bytes(b"final answer\n").as_str())
    );
    assert_eq!(fs::read(retained_result).unwrap(), b"final answer\n");
    assert!(retained.note.is_none());

    // An oversized result is not silently truncated: the locator stays
    // explicit and the note names why the snapshot is absent.
    let oversized = root.path().join("oversized.txt");
    fs::write(
        &oversized,
        vec![b'x'; (MAX_RETAINED_RESULT_BYTES + 1) as usize],
    )
    .unwrap();
    let retained = store
        .retain_evidence("implementer-2", &receipt, Some(&oversized))
        .unwrap();
    assert!(retained.result.is_none());
    assert!(
        retained.note.as_deref().unwrap().contains("not retained"),
        "{retained:?}"
    );

    // A missing result or receipt is reported, never fabricated.
    let retained = store
        .retain_evidence(
            "implementer-3",
            &receipt,
            Some(&root.path().join("gone.txt")),
        )
        .unwrap();
    assert!(retained.result.is_none());
    assert!(retained.note.as_deref().unwrap().contains("already gone"));
    assert!(
        store
            .retain_evidence("implementer-4", &root.path().join("absent.json"), None)
            .is_err()
    );
}

#[test]
fn planner_role_applies_no_treatment_and_is_not_measured() {
    assert_eq!(AttemptRole::Planner.as_str(), "planner");
    assert_eq!(AttemptRole::Planner.short(), "plan");
    assert!(!AttemptRole::Planner.is_measured());
    assert!(!AttemptRole::Planner.applies_treatment());
    assert!(AttemptRole::Implementer.applies_treatment());
    // A declared removal blocks the treatment-applying conversations and not
    // the bounded planning conversation.
    let root = fixture_root("planner");
    let launcher = root.path().join("codex.exe");
    fs::write(&launcher, "fixture launcher").unwrap();
    let mut cursor = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        root.path().join("openspec/changes/add-synthetic"),
        "bdct-h1",
    );
    let facts = DispatchFacts {
        runner_declared: true,
        launcher: Some(launcher),
        removal: Some(RemovalGate::Pending {
            reason: "no reviewed removal proposal is recorded".to_owned(),
        }),
        ..Default::default()
    };
    assert!(matches!(
        dispatch_gate(&cursor, AttemptRole::Planner, &facts),
        DispatchGate::Ready
    ));
    assert!(matches!(
        dispatch_gate(&cursor, AttemptRole::Implementer, &facts),
        DispatchGate::Blocked { .. }
    ));
    cursor.phase = Phase::CandidateReady;
    assert!(!Phase::CandidateReady.requires_comparison_owner());
    assert!(Phase::BaselineAttempt.requires_comparison_owner());
}

#[test]
fn candidate_change_references_resolve_or_refuse_mismatched_sources() {
    assert_eq!(
        candidate_change_name("openspec/changes/add-bounded-output").unwrap(),
        "add-bounded-output"
    );
    assert_eq!(
        candidate_change_name("add-bounded-output").unwrap(),
        "add-bounded-output"
    );
    // Path-shaped references must live under the run's own planning root.
    assert!(candidate_change_name("other/openspec/changes/add-x").is_err());
    assert!(candidate_change_name("").is_err());
    assert!(candidate_change_name("openspec/changes/..").is_err());
    assert!(candidate_change_name(&format!("openspec/changes/{}", "x".repeat(201))).is_err());
    let error = candidate_change_name("somewhere/else/add-x").unwrap_err();
    assert!(error.contains("mismatched source"), "{error}");
}

#[test]
fn changed_paths_stay_inside_the_scope_or_the_candidate_change() {
    let scope = vec!["crates/one".to_owned()];
    let inside = vec![
        "crates/one/src/lib.rs".to_owned(),
        "openspec/changes/add-x/tasks.md".to_owned(),
    ];
    assert!(changed_paths_within_scope(&inside, &scope, "add-x").is_ok());

    let escaped = vec!["global/orchestration.toml".to_owned()];
    let error = changed_paths_within_scope(&escaped, &scope, "add-x").unwrap_err();
    assert!(
        error.contains("outside the declared writable scope"),
        "{error}"
    );

    // A sibling prefix is not inside the scope, and traversal is refused.
    assert!(
        changed_paths_within_scope(&["crates/one-extra/lib.rs".to_owned()], &scope, "add-x")
            .is_err()
    );
    assert!(
        changed_paths_within_scope(&["crates/../global/x".to_owned()], &scope, "add-x").is_err()
    );
    // Another change's directory is not the candidate's own change.
    assert!(
        changed_paths_within_scope(
            &["openspec/changes/add-y/specs/a.md".to_owned()],
            &scope,
            "add-x"
        )
        .is_err()
    );
}

#[test]
fn evidence_locators_are_stable_and_path_safe() {
    assert_eq!(
        evidence_locator("rollouts/run-1.jsonl").as_deref(),
        Some("file:rollouts/run-1.jsonl")
    );
    assert_eq!(
        evidence_locator(r"rollouts\run-1.jsonl").as_deref(),
        Some("file:rollouts/run-1.jsonl")
    );
    assert!(evidence_locator("../escape.jsonl").is_none());
    assert!(evidence_locator("/rooted.jsonl").is_none());
    assert!(evidence_locator("C:/drive.jsonl").is_none());
    assert!(evidence_locator(r"\\server\share\x.jsonl").is_none());
    assert!(evidence_locator("rollouts/../../escape.jsonl").is_none());
    assert!(evidence_locator("file:rollouts/x.jsonl").is_none());
    assert!(evidence_locator("rollouts/").is_some());
    assert!(evidence_locator("   ").is_none());
}

#[test]
fn cursor_round_trips_intake_and_candidate_state_with_legacy_defaults() {
    let root = fixture_root("state");
    let mut cursor = Cursor::new(
        "loop-state",
        "a".repeat(64),
        root.path().join("openspec/changes/add-synthetic"),
        "bdct-h1",
    );
    cursor
        .record_intake(IntakeState {
            result_sha256: "b".repeat(64),
            evidence_digest: "c".repeat(64),
            outcomes: vec![OutcomeRecord::new("admitted", Some("bdct-h2"), "a new card").unwrap()],
            consumed_ms: 7,
        })
        .unwrap();
    let candidate = CandidateState {
        removal_required: true,
        removal_frozen: Some("digest".to_owned()),
        worktree: None,
        planning_receipt: Some(root.path().join("candidate-planning.json")),
        planner_attempt: Some("planner-1".to_owned()),
        implementer_attempt: None,
        revision: Some("d".repeat(40)),
        result: None,
        ..CandidateState::new("bdct-h2", "add-bounded-output").unwrap()
    };
    cursor.select_candidate(candidate.clone()).unwrap();
    let json = serde_json::to_value(&cursor).unwrap();
    let decoded: Cursor = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(
        decoded.intake.as_ref().map(|intake| intake.outcomes.len()),
        Some(1)
    );
    assert_eq!(decoded.candidate.as_ref(), Some(&candidate));
    assert!(cursor.candidate.as_ref().unwrap().is_ready());

    // A legacy cursor without the new fields still reads: the intake and
    // candidate state default to absent.
    let legacy = serde_json::json!({
        "schema": 1,
        "run": "loop-state",
        "spec_digest": "a".repeat(64),
        "change_root": root.path().join("openspec/changes/add-synthetic"),
        "phase": "planning",
        "previous_phase": null,
        "condition": null,
        "hypothesis_item": "bdct-h1",
        "experiment": "loop-state-experiment-1",
        "removal_frozen": null,
        "attempts": [],
        "effects": [],
        "effects_dropped": 0,
        "selected_variant": null,
        "selected_runtime": null,
        "selected_identity": null,
        "updated_ms": 1,
    });
    let decoded: Cursor = serde_json::from_value(legacy).unwrap();
    assert!(decoded.intake.is_none());
    assert!(decoded.candidate.is_none());
    assert_eq!(decoded.attempts_dropped, 0);
    assert_eq!(decoded.retry, RetryPolicy::default());

    // A run that already validated one candidate does not silently switch
    // hypotheses.
    let mut validated = cursor.clone();
    validated.candidate.as_mut().unwrap().revision = Some("e".repeat(40));
    let switch = validated.select_candidate(CandidateState::new("bdct-h3", "add-y").unwrap());
    assert!(switch.is_err());
    let repeat = validated.select_candidate(candidate);
    assert!(repeat.is_ok());
}

#[test]
fn candidate_removal_gate_requires_a_recorded_proposal_and_decision() {
    let missing = candidate_removal_gate("bdct-h2", &[], None);
    match missing {
        RemovalGate::Pending { reason } => {
            assert!(
                reason.contains("no reviewable removal proposal"),
                "{reason}"
            );
        }
        other => panic!("expected pending, got {other:?}"),
    }
    let proposal = proposal_text("retired-skill", "evidence-1", "preview-1", DETAIL);
    let pending = candidate_removal_gate("bdct-h1", std::slice::from_ref(&proposal), None);
    assert!(matches!(pending, RemovalGate::Pending { .. }));
    let approved = vec![
        proposal.clone(),
        decision_text("approve", "experiment", "retired-skill", DETAIL),
    ];
    let authorized = candidate_removal_gate("bdct-h1", &approved, None);
    assert!(matches!(authorized, RemovalGate::Authorized { .. }));
    let frozen = frozen_candidate_removal_digest("bdct-h1", &[proposal]);
    assert!(frozen.is_some());
}

#[test]
fn evidence_root_must_be_absolute_when_declared() {
    let root = fixture_root("evidence-root");
    let mut spec = spec_fixture(root.path(), "evidence-root");
    spec.evidence_root = Some(PathBuf::from("relative/evidence"));
    assert!(spec.validate().is_err());
    spec.evidence_root = Some(root.path().join("evidence"));
    assert!(spec.validate().is_ok());
    spec.evidence_root = None;
    assert!(spec.validate().is_ok());
}

/// One complete declared comparison over the synthetic fixture project: a
/// runner, the explicit local runner, a qualification record, the workload's
/// own planning target and the frozen acceptance request.
fn comparison_spec(root: &Path, name: &str) -> RunSpec {
    let mut spec = spec_fixture(root, name);
    let project = root.join(format!("proj-{name}"));
    let state = root.join("state");
    fs::create_dir_all(state.join("builds")).unwrap();
    for build in ["baseline-build", "candidate-build"] {
        fs::create_dir_all(state.join("builds").join(build)).unwrap();
    }
    let task = root.join("task");
    fs::create_dir_all(&task).unwrap();
    let upstream = root.join("upstream.exe");
    fs::write(&upstream, "client").unwrap();
    let request = root.join("acceptance-request.json");
    let request_bytes = serde_json::to_vec(&serde_json::json!({
        "schema": 1,
        "kind": "real-task",
        "case_root": root.join("task-workspace"),
        "task_contract_sha256": "a".repeat(64),
        "oracle": {
            "program": root.join("checker.exe"),
            "program_sha256": "b".repeat(64),
            "arguments": ["{workspace}"],
            "inputs": {},
        },
        "timeout_seconds": 60,
    }))
    .unwrap();
    fs::write(&request, &request_bytes).unwrap();
    let policy = root.join("policy.json");
    fs::write(&policy, "{}").unwrap();
    let qualification = root.join("qualification.json");
    fs::write(&qualification, "{}").unwrap();
    spec.runner = Some(RunnerInputs {
        profile: "ds".to_owned(),
        model: None,
        model_provider: None,
        reasoning_effort: None,
        retry: None,
    });
    spec.local_runner = Some(crate::outcome_qualification::LocalRunner {
        endpoint: "http://127.0.0.1:45999/v1".to_owned(),
        model: "fixture-glyph-1".to_owned(),
        identity: crate::outcome_qualification::MaterialIdentity::default(),
    });
    spec.qualification = Some(qualification);
    spec.comparison = Some(ComparisonInputs {
        schema: COMPARISON_SCHEMA,
        specification: crate::improvement_spec::Specification {
            project: project.clone(),
            change: "add-workload".to_owned(),
            store: None,
            planning_root: project,
        },
        contract: experiment(),
        workload_card: "bdct-workload".to_owned(),
        workload_removal: None,
        task: TaskInputs {
            source: task,
            revision: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            name: "workload-b".to_owned(),
            writable_scope: vec!["crates/one".to_owned()],
        },
        runtimes: RuntimeInputs {
            state: state.clone(),
            baseline_build: state.join("builds/baseline-build"),
            candidate_build: state.join("builds/candidate-build"),
            baseline_label: "H".to_owned(),
            candidate_label: "H+A".to_owned(),
            upstream,
            client: crate::improvement_runtime::ClientInputs {
                runner: crate::outcome_qualification::LocalRunner {
                    endpoint: "http://127.0.0.1:45999/v1".to_owned(),
                    model: "fixture-glyph-1".to_owned(),
                    identity: crate::outcome_qualification::MaterialIdentity::default(),
                },
                reasoning_effort: Some("low".to_owned()),
                catalogue: None,
                overlay: None,
                executor_profile: None,
            },
        },
        policy,
        acceptance: AcceptanceInputs {
            request,
            request_sha256: format!("{:x}", Sha256::digest(&request_bytes)),
        },
        observation_inputs: Vec::new(),
    });
    spec
}

#[test]
fn comparison_inputs_are_validated_before_preparation() {
    let root = fixture_root("comparison-inputs");
    let spec = comparison_spec(root.path(), "comparison-inputs");
    spec.validate()
        .expect("complete comparison inputs validate");

    // A comparison without the runner profile cannot open a visible
    // conversation, and one without the local runner has no measured route.
    let mut without_runner = spec.clone();
    without_runner.runner = None;
    assert!(without_runner.validate().is_err());
    let mut without_local = spec.clone();
    without_local.local_runner = None;
    assert!(without_local.validate().is_err());

    // The measured client route cannot silently differ from the declared
    // local runner.
    let mut rerouted = spec.clone();
    rerouted
        .comparison
        .as_mut()
        .unwrap()
        .runtimes
        .client
        .runner
        .model = "another-model".to_owned();
    assert!(rerouted.validate().is_err());

    let mutate = |f: &dyn Fn(&mut ComparisonInputs)| {
        let mut changed = spec.clone();
        f(changed.comparison.as_mut().unwrap());
        changed
    };
    assert!(
        mutate(&|c| c.task.writable_scope.clear())
            .validate()
            .is_err(),
        "an empty workload scope has no writable task"
    );
    assert!(
        mutate(&|c| c.task.revision = "not-hex".to_owned())
            .validate()
            .is_err()
    );
    assert!(
        mutate(&|c| c.runtimes.candidate_label = c.runtimes.baseline_label.clone())
            .validate()
            .is_err(),
        "arms must stay distinguishable"
    );
    assert!(
        mutate(&|c| c.acceptance.request_sha256 = "nope".to_owned())
            .validate()
            .is_err()
    );
    assert!(
        mutate(&|c| c.acceptance.request = PathBuf::from("relative.json"))
            .validate()
            .is_err()
    );
    assert!(
        mutate(&|c| c.policy = PathBuf::from("policy.json"))
            .validate()
            .is_err(),
        "the policy is an explicit absolute private input"
    );
    assert!(
        mutate(&|c| c.runtimes.upstream = root.path().join("missing.exe"))
            .validate()
            .is_err()
    );
    assert!(
        mutate(&|c| c.schema = COMPARISON_SCHEMA + 1)
            .validate()
            .is_err()
    );

    // A declared policy file that is not the owner's declaration is refused
    // rather than reinterpreted.
    assert!(spec.comparison.as_ref().unwrap().declared_policy().is_err());
    let policy = crate::improvement_policy::ComparisonPolicy {
        schema: 1,
        objective: crate::improvement_policy::Objective::Time,
        basis: crate::improvement_policy::Basis::Efficiency,
        meaningful_effect_percent: Some(10.0),
        tolerance_percent: 5.0,
        require_acceptance: true,
        task_mix: "one frozen task".to_owned(),
        stopping: crate::improvement_policy::StoppingRule {
            max_attempts_per_arm: 1,
            required_units: 1,
        },
        repeated_selection: crate::improvement_policy::RepeatedSelection::Predeclared,
        trade_off: None,
        uncertainty: "unknown evidence stays inconclusive".to_owned(),
        horizon_tasks: 1.0,
        overhead: crate::improvement_policy::Overhead {
            implementation_seconds: 1.0,
            evaluation_seconds: 1.0,
            maintenance_seconds_per_task: 0.0,
        },
    };
    let policy_path = spec.comparison.as_ref().unwrap().policy.clone();
    fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    let declared = spec
        .comparison
        .as_ref()
        .unwrap()
        .declared_policy()
        .expect("a complete owner declaration is accepted");
    assert!(declared.digest_matches(&declared.digest));
}

#[test]
fn comparison_state_round_trips_and_defaults_without_legacy_state() {
    // A cursor written before the comparison owner knew its state stays
    // readable: the new field defaults to absent.
    let root = fixture_root("comparison-state");
    let spec = comparison_spec(root.path(), "state");
    let mut cursor = Cursor::new(
        &spec.run,
        "digest".to_owned(),
        root.path().join("openspec"),
        &spec.hypothesis_item,
    );
    assert!(cursor.comparison.is_none());
    let mut state = ComparisonState::new("policy-digest".to_owned());
    state.arm_mut(ComparisonArm::Baseline).accepted = Some(true);
    state.arm_mut(ComparisonArm::Candidate).condition =
        Some("wrong generation; not replayed".to_owned());
    cursor.comparison = Some(state);
    let bytes = serde_json::to_vec(&cursor).unwrap();
    let parsed: Cursor = serde_json::from_slice(&bytes).unwrap();
    let comparison = parsed.comparison.expect("comparison state round-trips");
    assert_eq!(comparison.schema, COMPARISON_STATE_SCHEMA);
    assert_eq!(comparison.policy_digest, "policy-digest");
    assert_eq!(comparison.arm(ComparisonArm::Baseline).accepted, Some(true));
    assert!(
        comparison
            .arm(ComparisonArm::Candidate)
            .condition
            .as_deref()
            .unwrap()
            .contains("not replayed")
    );
    assert_eq!(ComparisonArm::Baseline.role(), AttemptRole::Baseline);
    assert_eq!(ComparisonArm::Candidate.role(), AttemptRole::Candidate);
    assert_eq!(ComparisonArm::Candidate.as_str(), "candidate");

    let legacy = serde_json::json!({
        "schema": CURSOR_SCHEMA,
        "run": spec.run,
        "spec_digest": "digest",
        "change_root": root.path().join("openspec"),
        "phase": "candidate-ready",
        "previous_phase": null,
        "condition": null,
        "hypothesis_item": spec.hypothesis_item,
        "experiment": "exp-1",
        "removal_frozen": null,
        "attempts": [],
        "effects": [],
        "selected_variant": null,
        "selected_runtime": null,
        "selected_identity": null,
        "updated_ms": 1,
    });
    let parsed: Cursor = serde_json::from_value(legacy).unwrap();
    assert!(parsed.comparison.is_none());
    assert!(parsed.intake.is_none());
}

/// The gate facts of a run whose visible surface is actually available.
fn ready_facts(root: &Path) -> DispatchFacts {
    let launcher = root.join("codex.exe");
    fs::write(&launcher, "fixture").unwrap();
    DispatchFacts {
        runner_declared: true,
        launcher: Some(launcher),
        ..Default::default()
    }
}

#[test]
fn retry_policy_grows_bounded_and_is_validated_with_the_run_inputs() {
    let policy = RetryPolicy::default();
    assert_eq!(policy.delay_ms(0), 0);
    assert_eq!(
        policy.delay_ms(1),
        0,
        "a single refusal keeps the documented immediate explicit-retry path"
    );
    assert_eq!(policy.delay_ms(2), DEFAULT_RETRY_INITIAL_MS);
    assert_eq!(policy.delay_ms(3), 2 * DEFAULT_RETRY_INITIAL_MS);
    assert_eq!(policy.delay_ms(4), 4 * DEFAULT_RETRY_INITIAL_MS);
    assert_eq!(policy.delay_ms(64), DEFAULT_RETRY_MAX_MS);
    let constant = RetryPolicy {
        initial_ms: 500,
        factor_percent: 100,
        max_ms: 5_000,
    };
    assert_eq!(constant.delay_ms(2), 500);
    assert_eq!(constant.delay_ms(9), 500);
    assert!(
        RetryPolicy {
            initial_ms: 0,
            ..policy
        }
        .validate("test policy")
        .is_err()
    );
    assert!(
        RetryPolicy {
            factor_percent: 99,
            ..policy
        }
        .validate("test policy")
        .is_err()
    );
    assert!(
        RetryPolicy {
            initial_ms: 1_000,
            factor_percent: 200,
            max_ms: 999,
        }
        .validate("test policy")
        .is_err()
    );
    assert!(
        RetryPolicy {
            initial_ms: 1_000,
            factor_percent: 200,
            max_ms: MAX_RETRY_MS + 1,
        }
        .validate("test policy")
        .is_err()
    );

    // The declared policy travels with the runner profile and is validated
    // with the run inputs before any dispatch.
    let root = fixture_root("retry-inputs");
    let mut spec = spec_fixture(root.path(), "retry-inputs");
    spec.runner = Some(RunnerInputs {
        profile: "ds".to_owned(),
        model: None,
        model_provider: None,
        reasoning_effort: None,
        retry: Some(RetryPolicy {
            initial_ms: 0,
            ..RetryPolicy::default()
        }),
    });
    assert!(spec.validate().is_err());
    spec.runner.as_mut().unwrap().retry = Some(RetryPolicy {
        initial_ms: 2_000,
        factor_percent: 150,
        max_ms: 30_000,
    });
    spec.validate().expect("a bounded retry policy validates");
    assert_eq!(spec.retry_policy().initial_ms, 2_000);
    spec.runner.as_mut().unwrap().retry = None;
    assert_eq!(spec.retry_policy(), RetryPolicy::default());
}

#[test]
fn dispatch_gate_applies_configured_bounded_backoff_to_refused_dispatches() {
    let ctx = tempfile::tempdir().unwrap();
    let facts = ready_facts(ctx.path());
    let mut cursor = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        PathBuf::from(r"C:\work\openspec\changes\x"),
        "bdct-h1",
    );
    cursor.retry = RetryPolicy {
        initial_ms: 1_000,
        factor_percent: 200,
        max_ms: 4_000,
    };
    let now = now_ms();
    let mut refused = attempt("inv-1", AttemptRole::Investigator, AttemptState::Failed);
    refused.reason = Some(
        "dispatch refused before submission: the local endpoint was unreachable; no fallback was attempted and no model request was made"
            .to_owned(),
    );
    refused.updated_ms = now;
    cursor.push_attempt(refused).unwrap();

    // A single refusal keeps the documented immediate explicit-retry path:
    // the operator's resume can prepare the fresh attempt at once.
    assert_eq!(
        dispatch_gate(&cursor, AttemptRole::Investigator, &facts),
        DispatchGate::Ready
    );
    // A refusal of one role does not throttle an independent role; measured
    // attempts are serialized separately by the active-attempt rule.
    assert_eq!(
        dispatch_gate(&cursor, AttemptRole::Implementer, &facts),
        DispatchGate::Ready
    );

    // A second consecutive refusal is a persistent endpoint failure: the
    // configured bounded window applies and the reason stays visible.
    let mut persistent = cursor.clone();
    let mut second = attempt("inv-2", AttemptRole::Investigator, AttemptState::Failed);
    second.reason = Some(
        "dispatch refused before submission: the local endpoint was unreachable; no fallback was attempted and no model request was made"
            .to_owned(),
    );
    second.updated_ms = now.saturating_sub(500);
    persistent.push_attempt(second).unwrap();
    match dispatch_gate(&persistent, AttemptRole::Investigator, &facts) {
        DispatchGate::Blocked { reason } => {
            assert!(reason.contains("endpoint backoff"), "{reason}");
            assert!(reason.contains("attempt inv-2"), "{reason}");
            assert!(reason.contains("2 time(s)"), "{reason}");
            assert!(reason.contains("1000 ms"), "{reason}");
            assert!(
                reason.contains("no other provider or route is substituted"),
                "{reason}"
            );
            assert!(reason.contains("completed work is preserved"), "{reason}");
        }
        other => panic!("expected the bounded backoff, got {other:?}"),
    }

    // Once the configured window has elapsed the same recorded state is ready
    // again; the retry itself stays a fresh attempt, never a replay.
    persistent.attempts.last_mut().unwrap().updated_ms = now.saturating_sub(10_000);
    assert_eq!(
        dispatch_gate(&persistent, AttemptRole::Investigator, &facts),
        DispatchGate::Ready
    );

    // Each consecutive refusal grows the delay up to the configured cap.
    let mut doubling = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        PathBuf::from(r"C:\work\openspec\changes\x"),
        "bdct-h1",
    );
    doubling.retry = RetryPolicy {
        initial_ms: 1_000,
        factor_percent: 200,
        max_ms: 4_000,
    };
    for (id, at) in [
        ("inv-1", now.saturating_sub(9_000)),
        ("inv-2", now.saturating_sub(9_000)),
        ("inv-3", now.saturating_sub(1_500)),
    ] {
        let mut refused = attempt(id, AttemptRole::Investigator, AttemptState::Failed);
        refused.reason =
            Some("dispatch refused before submission: no model request was made".to_owned());
        refused.updated_ms = at;
        doubling.push_attempt(refused).unwrap();
    }
    let window = doubling
        .dispatch_backoff(AttemptRole::Investigator, now)
        .expect("the third consecutive refusal is still inside its bounded window");
    assert_eq!(window.failures, 3);
    assert_eq!(window.delay_ms, 2_000);
    assert_eq!(window.ready_at_ms, now.saturating_sub(1_500) + 2_000);
    for id in ["inv-4", "inv-5", "inv-6"] {
        let mut refused = attempt(id, AttemptRole::Investigator, AttemptState::Failed);
        refused.reason =
            Some("dispatch refused before submission: no model request was made".to_owned());
        refused.updated_ms = now;
        doubling.push_attempt(refused).unwrap();
    }
    assert_eq!(
        doubling
            .dispatch_backoff(AttemptRole::Investigator, now)
            .unwrap()
            .delay_ms,
        4_000,
        "the configured cap bounds the growth"
    );

    // A settled model attempt ends the retry chain: a model effect is never
    // replayed, so it imposes no backoff.
    doubling
        .push_attempt(attempt(
            "inv-7",
            AttemptRole::Investigator,
            AttemptState::Completed,
        ))
        .unwrap();
    assert!(
        doubling
            .dispatch_backoff(AttemptRole::Investigator, now)
            .is_none()
    );
    assert_eq!(
        dispatch_gate(&doubling, AttemptRole::Investigator, &facts),
        DispatchGate::Ready
    );
}

#[test]
fn researcher_and_measured_dispatch_never_overlap_on_shared_inference() {
    let ctx = tempfile::tempdir().unwrap();
    let facts = ready_facts(ctx.path());
    let mut measured = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        PathBuf::from(r"C:\work\openspec\changes\x"),
        "bdct-h1",
    );
    measured
        .push_attempt(attempt(
            "base-1",
            AttemptRole::Baseline,
            AttemptState::Started,
        ))
        .unwrap();
    match dispatch_gate(&measured, AttemptRole::Investigator, &facts) {
        DispatchGate::Blocked { reason } => {
            assert!(reason.contains("base-1"), "{reason}");
            assert!(reason.contains("baseline"), "{reason}");
            assert!(reason.contains("until it finishes"), "{reason}");
        }
        other => panic!("expected the measured attempt to serialize model work, got {other:?}"),
    }
    // The frozen runtime keeps its slot: no variant is selected while the
    // measured attempt is active.
    assert!(selection_gate(&measured, "candidate", None).is_err());

    // The reverse direction: researcher work in flight blocks both measured
    // arms, so loop-generated researcher inference cannot overlap an arm.
    let mut researcher = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        PathBuf::from(r"C:\work\openspec\changes\x"),
        "bdct-h1",
    );
    researcher
        .push_attempt(attempt(
            "inv-1",
            AttemptRole::Investigator,
            AttemptState::Started,
        ))
        .unwrap();
    for role in [
        AttemptRole::Baseline,
        AttemptRole::Candidate,
        AttemptRole::Implementer,
        AttemptRole::Planner,
    ] {
        assert!(
            matches!(
                dispatch_gate(&researcher, role, &facts),
                DispatchGate::Blocked { .. }
            ),
            "{role:?} must not overlap the active researcher conversation"
        );
    }
}

#[test]
fn bounded_attempt_retention_releases_only_unprotected_records() {
    let mut cursor = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        PathBuf::from(r"C:\work\openspec\changes\x"),
        "bdct-h1",
    );
    for index in 0..MAX_ATTEMPTS {
        let mut settled = attempt(
            &format!("inv-{index}"),
            AttemptRole::Investigator,
            AttemptState::Completed,
        );
        settled.updated_ms = index as u64;
        cursor.push_attempt(settled).unwrap();
    }
    cursor
        .push_attempt(attempt(
            "inv-next",
            AttemptRole::Investigator,
            AttemptState::Completed,
        ))
        .unwrap();
    assert_eq!(
        cursor.attempts.len(),
        MAX_ATTEMPTS,
        "retention stays bounded"
    );
    assert_eq!(cursor.attempts_dropped, 1);
    assert!(
        cursor.attempt("inv-0").is_none(),
        "the oldest settled record is released"
    );
    assert!(cursor.attempt("inv-next").is_some());
    assert_eq!(cursor.attempts.last().unwrap().id, "inv-next");
    assert!(
        cursor
            .effects
            .iter()
            .any(|effect| effect.kind == EffectKind::AttemptsReleased)
    );

    // An unresolved outcome and the exact attempts the candidate/comparison
    // state references stay protected; only the unreferenced settled record
    // is released.
    let mut protected = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        PathBuf::from(r"C:\work\openspec\changes\x"),
        "bdct-h1",
    );
    protected.comparison = Some(ComparisonState::new("d".repeat(64)));
    protected.comparison.as_mut().unwrap().candidate.attempt = Some("cand-1".to_owned());
    protected
        .push_attempt(attempt(
            "cand-1",
            AttemptRole::Candidate,
            AttemptState::Completed,
        ))
        .unwrap();
    for index in 0..(MAX_ATTEMPTS - 2) {
        protected
            .push_attempt(attempt(
                &format!("inv-{index}"),
                AttemptRole::Investigator,
                AttemptState::Completed,
            ))
            .unwrap();
    }
    let mut unresolved = attempt(
        "inv-unresolved",
        AttemptRole::Investigator,
        AttemptState::Unknown,
    );
    unresolved.updated_ms = 1;
    protected.push_attempt(unresolved).unwrap();
    assert_eq!(protected.attempts.len(), MAX_ATTEMPTS);
    protected
        .push_attempt(attempt(
            "inv-extra",
            AttemptRole::Investigator,
            AttemptState::Completed,
        ))
        .unwrap();
    assert!(
        protected.attempt("cand-1").is_some(),
        "the referenced candidate attempt is protected"
    );
    assert!(
        protected.attempt("inv-unresolved").is_some(),
        "an unresolved outcome stays for recovery"
    );
    assert_eq!(protected.attempts_dropped, 1);
    assert!(
        protected.attempt("inv-0").is_none(),
        "the oldest unreferenced record is released first"
    );

    // When every record is protected, growth is refused explicitly instead of
    // silently dropping pending recovery; the bound is a retention bound and
    // never a lifetime cap for settled work.
    let mut saturated = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        PathBuf::from(r"C:\work\openspec\changes\x"),
        "bdct-h1",
    );
    for index in 0..MAX_ATTEMPTS {
        saturated
            .push_attempt(attempt(
                &format!("act-{index}"),
                AttemptRole::Baseline,
                AttemptState::Unknown,
            ))
            .unwrap();
    }
    let error = saturated
        .push_attempt(attempt(
            "act-next",
            AttemptRole::Baseline,
            AttemptState::Unknown,
        ))
        .unwrap_err();
    assert!(
        error.to_string().contains("every record is protected"),
        "{error}"
    );
}

#[test]
fn retained_evidence_reports_missing_changed_and_unretained_artifacts() {
    let root = fixture_root("unsupported");
    let spec = spec_fixture(root.path(), "unsupported");
    let run_dir = root.path().join("run");
    let change_root = root.path().join("openspec/changes/add-synthetic");
    let store = RunStore::create(&run_dir, &spec, &spec.digest().unwrap(), &change_root).unwrap();
    let receipt = root.path().join("spawn-9.json");
    fs::write(&receipt, b"{\"observation\":{\"state\":\"completed\"}}\n").unwrap();
    let result = root.path().join("message-9.txt");
    fs::write(&result, "final answer\n").unwrap();

    let retained = store
        .retain_evidence("impl-9", &receipt, Some(&result))
        .unwrap();
    assert!(retained.verify().is_ok());
    assert_eq!(retained.unsupported_reason(), None);
    let mut record = attempt("impl-9", AttemptRole::Implementer, AttemptState::Completed);
    record.receipt = Some(receipt.clone());
    record.retained = Some(retained.clone());
    assert_eq!(record.unsupported_evidence(), None);

    // A released or expired snapshot is named explicitly instead of being
    // reconstructed from a summary.
    fs::remove_file(&retained.receipt).unwrap();
    match retained.verify().unwrap_err() {
        RetainedEvidenceFault::Missing { artifact, .. } => assert_eq!(artifact, "receipt"),
        other => panic!("expected a missing artifact, got {other:?}"),
    }
    let unsupported = record.unsupported_evidence().unwrap();
    assert!(
        unsupported.contains("impl-9")
            && unsupported.contains("no longer available")
            && unsupported.contains("unsupported"),
        "{unsupported}"
    );

    // A snapshot that changed after retention is refused by its digest.
    let retained = store
        .retain_evidence("impl-9", &receipt, Some(&result))
        .unwrap();
    fs::write(&retained.receipt, b"tampered\n").unwrap();
    match retained.verify().unwrap_err() {
        RetainedEvidenceFault::Changed { artifact, .. } => assert_eq!(artifact, "receipt"),
        other => panic!("expected a changed artifact, got {other:?}"),
    }

    // A result that was not retained within its bound keeps its explicit
    // cause; the receipt snapshot itself stays available.
    let oversized = root.path().join("oversized-9.txt");
    fs::write(
        &oversized,
        vec![b'x'; (MAX_RETAINED_RESULT_BYTES + 1) as usize],
    )
    .unwrap();
    let partial = store
        .retain_evidence("impl-10", &receipt, Some(&oversized))
        .unwrap();
    assert!(partial.verify().is_ok());
    let reason = partial.unsupported_reason().unwrap();
    assert!(reason.contains("not retained"), "{reason}");

    // A completed attempt whose evidence never entered the store, and a
    // refusal that made no model request, stay distinguishable.
    let mut never = attempt("impl-11", AttemptRole::Implementer, AttemptState::Completed);
    never.receipt = Some(receipt);
    assert!(
        never
            .unsupported_evidence()
            .unwrap()
            .contains("not retained")
    );
    assert_eq!(
        attempt("impl-12", AttemptRole::Implementer, AttemptState::Failed).unsupported_evidence(),
        None
    );

    // The cursor surfaces exactly the unsupported attempts.
    let mut cursor = Cursor::new(
        "loop-fixture",
        "a".repeat(64),
        PathBuf::from(r"C:\work\openspec\changes\x"),
        "bdct-h1",
    );
    cursor.push_attempt(record).unwrap();
    let reasons = cursor.unsupported_evidence();
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].contains("impl-9"), "{reasons:?}");
}

#[test]
fn configured_retry_is_frozen_in_private_state_without_route_details() {
    let root = fixture_root("publication");
    let mut spec = spec_fixture(root.path(), "publication");
    spec.runner = Some(RunnerInputs {
        profile: "ds".to_owned(),
        model: None,
        model_provider: None,
        reasoning_effort: None,
        retry: Some(RetryPolicy {
            initial_ms: 1_500,
            factor_percent: 300,
            max_ms: 9_000,
        }),
    });
    spec.local_runner = Some(crate::outcome_qualification::LocalRunner {
        endpoint: "http://127.0.0.1:45999/v1".to_owned(),
        model: "synthetic-local".to_owned(),
        identity: Default::default(),
    });
    spec.qualification = Some(root.path().join("qualification.json"));
    spec.validate().unwrap();
    let run_dir = root.path().join("run");
    let change_root = root.path().join("openspec/changes/add-synthetic");
    let store = RunStore::create(&run_dir, &spec, &spec.digest().unwrap(), &change_root).unwrap();
    let cursor = store.cursor().unwrap();
    assert_eq!(
        cursor.retry,
        RetryPolicy {
            initial_ms: 1_500,
            factor_percent: 300,
            max_ms: 9_000,
        },
        "the configured schedule is frozen with the run"
    );
    // The machine-owned recovery state carries no local endpoint, route or
    // model detail; those stay in the private run inputs.
    let text = serde_json::to_string(&cursor).unwrap();
    assert!(!text.contains("127.0.0.1"), "{text}");
    assert!(!text.contains("http"), "{text}");
    assert!(!text.contains("synthetic-local"), "{text}");
}
