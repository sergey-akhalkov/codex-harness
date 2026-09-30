use super::*;
use crate::board_hypothesis::{AuthorityRequest, RemovalAction};
use std::{
    fs,
    path::{Path, PathBuf},
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
        publication_scope: vec![PublicationStage::Experiment],
        oracle: "outcome-oracle:private-request".to_owned(),
        removal: None,
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
fn a_live_owner_blocks_takeover_and_a_dead_owner_allows_it() {
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
    let error = store.claim_ownership(&spec.run).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("owned by live process") || message.contains("cannot be verified"),
        "{message}"
    );
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
    let claimed = store
        .claim_ownership(&spec.run)
        .expect("stale owner taken over");
    assert_eq!(claimed.pid, std::process::id());
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
