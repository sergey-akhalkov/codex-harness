//! Native owned temporary Beads boards and rollout-reader evidence for the
//! grounded improvement intake: admission once, same-condition reuse,
//! changed-basis reconsideration and nonblocking workload references. All
//! projects, rollouts and cards are synthetic fixtures inside the test's
//! temporary directory.

use harness_core::benefit_gate::{
    DecisionDraft, DecisionOutcome, Publication, QualityOutcome, publish_decision,
};
use harness_core::board_hypothesis::{
    self, Admission, BoundedHypothesis, HypothesisDraft, SearchFilter,
};
use harness_core::improvement_intake::{
    ClaimKind, EvidenceIndex, EvidenceItem, EvidenceOwner, EvidenceRef, IntakeOutcome,
    InvestigatorReport, Proposal, ReadIdentity, RemovalBasis, RemovalClaim, RepeatedReadClaim,
    RetainedRead, SelectionDeferral, Treatment, WorkloadLink, WorkloadRef, intake,
};
use harness_core::improvement_policy::{EffectPath, ExperimentMethod, ExperimentSelection};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn bd_name() -> &'static str {
    if cfg!(windows) { "bd.exe" } else { "bd" }
}

fn bd_executable() -> PathBuf {
    if let Some(value) = std::env::var_os("HARNESS_BD_EXE") {
        return PathBuf::from(value);
    }
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        let candidate = PathBuf::from(home).join("harness/bin").join(bd_name());
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(bd_name());
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
}

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// One owned synthetic project with a seeded Git checkout and an initialized
/// bd board.
fn board_project(root: &Path) -> PathBuf {
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "--initial-branch=main"]);
    git(&project, &["config", "user.email", "fixture@example.test"]);
    git(&project, &["config", "user.name", "Fixture"]);
    fs::write(project.join("README.md"), "synthetic\n").unwrap();
    git(&project, &["add", "."]);
    git(&project, &["commit", "-qm", "seed"]);
    let init = Command::new(bd_executable())
        .args([
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "bdct",
        ])
        .current_dir(&project)
        .output()
        .expect("bd init runs");
    assert!(
        init.status.success(),
        "bd init: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    project
}

/// One documented rollout record set; `corrupt` appends one unparseable line.
fn write_rollout(dir: &Path, name: &str, corrupt: bool) -> PathBuf {
    let counts = |amount: u64| {
        json!({
            "input_tokens": amount,
            "cached_input_tokens": amount / 2,
            "output_tokens": amount / 2,
            "reasoning_output_tokens": amount / 5,
            "total_tokens": amount + amount / 2,
        })
    };
    let record = |turn: &str, response: &str, delta: u64, cumulative: u64| {
        json!({
            "type": "token_usage_record",
            "payload": {
                "turn_id": turn,
                "response_id": response,
                "usage": counts(delta),
                "turn_token_usage": counts(cumulative),
                "thread_token_usage": counts(cumulative),
            }
        })
    };
    let events = [
        json!({"type": "session_meta", "payload": {"id": "thread_fixture", "session_id": "thread_fixture"}}),
        json!({"type": "turn_context", "payload": {"model": "fixture-model", "effort": "high"}}),
        record("turn_one", "resp_one", 10, 10),
        record("turn_two", "resp_two", 20, 30),
    ];
    let mut text = events
        .iter()
        .map(|event| format!("{event}\n"))
        .collect::<String>();
    if corrupt {
        text.push_str("this line is not json\n");
    }
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path
}

fn admit_card(bd: &Path, project: &Path, mechanism: &str, conditions: &str, basis: &str) -> String {
    let bounded = BoundedHypothesis::try_from_draft(HypothesisDraft {
        mechanism: mechanism.to_owned(),
        conditions: conditions.to_owned(),
        observation: basis.to_owned(),
        predicted: "Synthetic predicted effect".to_owned(),
        counterexample: "Synthetic counterexample".to_owned(),
        acceptance: "Synthetic independent acceptance".to_owned(),
        spec: "openspec/changes/add-synthetic".to_owned(),
        basis: basis.to_owned(),
    })
    .unwrap();
    match board_hypothesis::admit_hypothesis(bd, project, &bounded, None).unwrap() {
        Admission::Created { id } => id,
        other => panic!("expected a created workload card, got {other:?}"),
    }
}

fn base_proposal(basis: &str) -> Proposal {
    Proposal {
        mechanism: "bounded-output".to_owned(),
        conditions: "local-tool-runs".to_owned(),
        observation: "rollout:cycle-1#seed".to_owned(),
        predicted: Some("less repeated context loading".to_owned()),
        counterexample: Some("diagnostics vanish on failure".to_owned()),
        acceptance: Some("the independent oracle passes".to_owned()),
        alternatives: Some(
            "no change, reuse of the existing reader, simplification and subtraction each leave the measured burden in place"
                .to_owned(),
        ),
        spec: Some("openspec/changes/add-synthetic".to_owned()),
        basis: basis.to_owned(),
        treatment: Treatment::Addition,
        evidence: vec![EvidenceRef {
            locator: "rollout:cycle-1#seed".to_owned(),
            kind: ClaimKind::Observed,
        }],
        repeated_read: None,
        workload: None,
        next_check: None,
        selection: Some(selection(ExperimentMethod::RealOperation, EffectPath::LocalOperation)),
        deferral: None,
    }
}

/// One complete, valid experiment-selection declaration for the declared
/// method and claim path. Individual counterexamples replace only the field
/// under test.
fn selection(method: ExperimentMethod, claim: EffectPath) -> ExperimentSelection {
    ExperimentSelection {
        method,
        claim,
        outcome: "the declared outcome measured through the real operation".to_owned(),
        rationale: "the unit exercises the claimed mechanism under the declared conditions"
            .to_owned(),
        controls: "frozen inputs and the accepted baseline conditions".to_owned(),
        projection: "one bounded local cycle with the retention cost staying bounded".to_owned(),
        baseline: "the accepted revision, excluding the candidate edit".to_owned(),
        stopping:
            "stop after the declared attempts and escalate only for a named missing observation"
                .to_owned(),
    }
}

fn report(candidate: Proposal) -> InvestigatorReport {
    InvestigatorReport {
        schema: 1,
        candidates: vec![candidate],
        idle_reason: None,
    }
}

#[test]
fn supported_proposal_admits_once_reuses_rejection_and_links_reconsideration() {
    let bd = bd_executable();
    let temp = tempfile::tempdir().unwrap();
    let project = board_project(temp.path());
    let seed_path = write_rollout(temp.path(), "seed.jsonl", false);
    let fresh_path = write_rollout(temp.path(), "fresh.jsonl", false);
    let index = EvidenceIndex::new(vec![
        EvidenceItem::read_rollout("rollout:cycle-1#seed", &seed_path).unwrap(),
        EvidenceItem::read_rollout("rollout:cycle-2#fresh", &fresh_path).unwrap(),
    ])
    .unwrap();

    // The workload card exists and is deferred: the candidate relation must
    // stay nonblocking and the deferral must not block the candidate.
    let workload = admit_card(
        &bd,
        &project,
        "workload-mechanism",
        "workload-conditions",
        "rollout:cycle-1#seed",
    );
    board_hypothesis::defer_hypothesis(&bd, &project, &workload, "tomorrow").unwrap();

    let workload_ref = Some(WorkloadRef {
        item: workload.clone(),
        experiment: "exp-cycle-1".to_owned(),
        evidence: None,
    });
    let mut candidate = base_proposal("rollout:cycle-1#seed");
    candidate.workload = workload_ref.clone();

    let first = intake(&bd, &project, &report(candidate.clone()), &index).unwrap();
    let id = match &first.outcomes[0] {
        IntakeOutcome::Admitted {
            id,
            removal_required: false,
            workload:
                Some(WorkloadLink::Recorded {
                    item,
                    experiment,
                    comment: true,
                    related: true,
                }),
            evidence,
        } => {
            assert_eq!(item, &workload);
            assert_eq!(experiment, "exp-cycle-1");
            assert!(
                evidence
                    .iter()
                    .any(|locator| locator == "rollout:cycle-1#seed")
            );
            id.clone()
        }
        other => panic!("expected one admission, got {other:?}"),
    };

    let card = board_hypothesis::load_card(&bd, &project, &id).unwrap();
    assert!(card.labels.iter().any(|label| label == "hypothesis"));
    let admission = board_hypothesis::parse_admission(&card.description).unwrap();
    assert_eq!(admission.mechanism.as_deref(), Some("bounded-output"));
    assert_eq!(admission.basis.as_deref(), Some("rollout:cycle-1#seed"));
    assert!(!matches!(card.status.as_str(), "closed" | "deferred"));
    assert!(
        card.dependencies
            .iter()
            .any(|dependency| dependency.id == workload && dependency.dependency_type == "related"),
        "the workload relationship must use the nonblocking related edge: {:?}",
        card.dependencies
    );
    assert!(
        card.dependencies
            .iter()
            .all(|dependency| dependency.dependency_type != "blocks"),
        "a workload must never become a blocking dependency: {:?}",
        card.dependencies
    );
    let workload_card = board_hypothesis::load_card(&bd, &project, &workload).unwrap();
    assert_eq!(
        workload_card.status, "deferred",
        "the candidate admission must not disturb the workload card"
    );

    // A repeated identical candidate creates no duplicate card and no
    // duplicate trial.
    let second = intake(&bd, &project, &report(candidate.clone()), &index).unwrap();
    match &second.outcomes[0] {
        IntakeOutcome::Existing {
            id: existing,
            workload:
                Some(WorkloadLink::Recorded {
                    comment, related, ..
                }),
            ..
        } => {
            assert_eq!(existing, &id);
            assert!(!*comment, "the identical trial is not recorded twice");
            assert!(!*related, "the related edge already exists");
        }
        other => panic!("expected the existing card, got {other:?}"),
    }
    let matches = board_hypothesis::search_hypotheses(
        &bd,
        &project,
        &SearchFilter {
            mechanism: Some("bounded-output".to_owned()),
            conditions: Some("local-tool-runs".to_owned()),
        },
    )
    .unwrap();
    assert_eq!(matches.len(), 1, "no duplicate hypothesis card");
    assert_eq!(matches[0].trials, 1, "one recorded trial");
    assert_eq!(matches[0].decisions, 0);

    // An evidenced same-condition rejection is reused, not rerun.
    let rejection = DecisionDraft {
        item: id.clone(),
        experiment: "exp-cycle-1".to_owned(),
        outcome: DecisionOutcome::Reject,
        quality: QualityOutcome::Unchanged,
        matched: 2,
        tolerance_percent: 10.0,
        baseline_seconds: 100.0,
        candidate_seconds: 101.0,
        baseline_arm: "direct".to_owned(),
        candidate_arm: "lane".to_owned(),
        accounting: "check+coordination+rework".to_owned(),
        baseline_revision: "base1234567".to_owned(),
        candidate_revision: "cand7654321".to_owned(),
        acceptance: "evidence-9".to_owned(),
        coverage: "time+rounds+tool_ops".to_owned(),
        scope: "task:synthetic.model:local".to_owned(),
        reason: "synthetic-regression".to_owned(),
        detail: None,
    };
    assert!(matches!(
        publish_decision(&bd, &project, &rejection).unwrap(),
        Publication::Recorded { .. }
    ));
    board_hypothesis::close_hypothesis(
        &bd,
        &project,
        &id,
        "reject",
        "synthetic-measured-regression",
        Some("exp-cycle-1"),
    )
    .unwrap();

    let reused = intake(&bd, &project, &report(candidate.clone()), &index).unwrap();
    match &reused.outcomes[0] {
        IntakeOutcome::ReusedRejection {
            id: reused_id,
            experiment,
            reason,
            basis,
        } => {
            assert_eq!(reused_id, &id);
            assert_eq!(experiment.as_deref(), Some("exp-cycle-1"));
            assert_eq!(reason.as_deref(), Some("synthetic-regression"));
            assert_eq!(basis.as_deref(), Some("rollout:cycle-1#seed"));
        }
        other => panic!("expected the recorded rejection to be reused, got {other:?}"),
    }

    // New retained evidence links a visible reconsideration while preserving
    // the earlier conclusion.
    let mut fresh = base_proposal("rollout:cycle-2#fresh");
    fresh.workload = workload_ref;
    fresh.evidence = vec![
        EvidenceRef {
            locator: "rollout:cycle-1#seed".to_owned(),
            kind: ClaimKind::Observed,
        },
        EvidenceRef {
            locator: "rollout:cycle-2#fresh".to_owned(),
            kind: ClaimKind::Observed,
        },
    ];
    let reconsidered = intake(&bd, &project, &report(fresh), &index).unwrap();
    match &reconsidered.outcomes[0] {
        IntakeOutcome::Reconsidered {
            id: reconsidered_id,
            basis,
            prior_outcome,
            workload: Some(WorkloadLink::Recorded { comment, .. }),
            ..
        } => {
            assert_eq!(reconsidered_id, &id);
            assert_eq!(basis, "rollout:cycle-2#fresh");
            assert_eq!(prior_outcome, "reject");
            assert!(!*comment, "the identical trial is not recorded twice");
        }
        other => panic!("expected a linked reconsideration, got {other:?}"),
    }
    let card = board_hypothesis::load_card(&bd, &project, &id).unwrap();
    assert!(
        !matches!(card.status.as_str(), "closed" | "deferred"),
        "a reconsidered card is reactivated: {}",
        card.status
    );
    let matches = board_hypothesis::search_hypotheses(
        &bd,
        &project,
        &SearchFilter {
            mechanism: Some("bounded-output".to_owned()),
            conditions: Some("local-tool-runs".to_owned()),
        },
    )
    .unwrap();
    assert_eq!(
        matches.len(),
        1,
        "reconsideration creates no duplicate card"
    );
    assert_eq!(matches[0].reconsiderations, 1);
    assert_eq!(
        matches[0].decisions, 1,
        "the earlier conclusion is preserved"
    );
    assert_eq!(
        matches[0]
            .latest_decision
            .as_ref()
            .and_then(|decision| decision.outcome.as_deref()),
        Some("reject")
    );

    // An unresolvable workload reference never blocks the candidate.
    let mut unlinked = base_proposal("rollout:cycle-1#seed");
    unlinked.workload = Some(WorkloadRef {
        item: "bdct-missing".to_owned(),
        experiment: "exp-cycle-1".to_owned(),
        evidence: None,
    });
    let outcomes = intake(&bd, &project, &report(unlinked), &index).unwrap();
    match &outcomes.outcomes[0] {
        IntakeOutcome::Existing {
            workload: Some(WorkloadLink::Unlinked { item, .. }),
            ..
        } => assert_eq!(item, "bdct-missing"),
        other => panic!("expected an unlinked workload on the existing card, got {other:?}"),
    }

    // A same-condition inconclusive conclusion on a deferred card is reused
    // instead of producing another identical trial.
    let inconclusive = DecisionDraft {
        outcome: DecisionOutcome::Inconclusive,
        reason: "synthetic-missing-observation".to_owned(),
        ..rejection
    };
    assert!(matches!(
        publish_decision(&bd, &project, &inconclusive).unwrap(),
        Publication::Recorded { .. }
    ));
    board_hypothesis::defer_hypothesis(&bd, &project, &id, "tomorrow").unwrap();
    let reused = intake(&bd, &project, &report(candidate.clone()), &index).unwrap();
    match &reused.outcomes[0] {
        IntakeOutcome::ReusedInconclusive {
            id: reused_id,
            experiment,
            reason,
            basis,
        } => {
            assert_eq!(reused_id, &id);
            assert_eq!(experiment.as_deref(), Some("exp-cycle-1"));
            assert_eq!(reason.as_deref(), Some("synthetic-missing-observation"));
            assert_eq!(basis.as_deref(), Some("rollout:cycle-1#seed"));
        }
        other => panic!("expected the inconclusive result to be reused, got {other:?}"),
    }
}

#[test]
fn native_reader_evidence_retains_attribution_coverage_and_partial_limits() {
    let bd = bd_executable();
    let temp = tempfile::tempdir().unwrap();
    let project = board_project(temp.path());

    let clean = EvidenceItem::read_rollout(
        "rollout:clean#1",
        &write_rollout(temp.path(), "clean.jsonl", false),
    )
    .unwrap();
    assert_eq!(clean.owner, EvidenceOwner::Rollout);
    assert_eq!(clean.kind, ClaimKind::Observed);
    assert!(clean.coverage.contains("lines="), "{}", clean.coverage);
    assert!(clean.coverage.contains("corrupt=0"), "{}", clean.coverage);
    assert!(clean.errors.is_empty(), "{:?}", clean.errors);
    assert!(!clean.is_partial());
    assert!(
        clean
            .warnings
            .iter()
            .any(|warning| warning == "missing_usage"),
        "missing counters stay explicit instead of becoming zero: {:?}",
        clean.warnings
    );

    let corrupt = EvidenceItem::read_rollout(
        "rollout:corrupt#1",
        &write_rollout(temp.path(), "corrupt.jsonl", true),
    )
    .unwrap();
    assert!(corrupt.is_partial());
    assert!(
        corrupt
            .errors
            .iter()
            .any(|error| error.contains("not valid JSON")),
        "{:?}",
        corrupt.errors
    );

    let index = EvidenceIndex::new(vec![clean.clone(), corrupt.clone()]).unwrap();
    let mut candidate = base_proposal("rollout:clean#1");
    candidate.observation = "rollout:corrupt#1".to_owned();
    candidate.evidence = vec![EvidenceRef {
        locator: "rollout:corrupt#1".to_owned(),
        kind: ClaimKind::Observed,
    }];
    candidate.next_check = Some("retain a complete rollout for the observation".to_owned());
    let outcomes = intake(&bd, &project, &report(candidate), &index).unwrap();
    match &outcomes.outcomes[0] {
        IntakeOutcome::Deferred { reason, next } => {
            assert!(reason.contains("partial"), "{reason}");
            assert_eq!(next, "retain a complete rollout for the observation");
        }
        other => panic!("expected a deferral on partial evidence, got {other:?}"),
    }
    assert!(
        board_hypothesis::list_hypothesis_cards(&bd, &project)
            .unwrap()
            .is_empty(),
        "a deferred candidate performs no board mutation"
    );

    // The read-fact owner captures actual identities through the existing
    // owners: two unchanged reads share one identity, a content change
    // between reads does not.
    let stable_path = temp.path().join("stable.txt");
    fs::write(&stable_path, "stable contents\n").unwrap();
    let stable_first = RetainedRead::capture(&stable_path, "context:task-1").unwrap();
    let stable_second = RetainedRead::capture(&stable_path, "context:task-1").unwrap();
    assert_eq!(stable_first, stable_second);
    let stable_item = EvidenceItem::new(
        "source:stable#reads",
        EvidenceOwner::Source,
        ClaimKind::Observed,
        "retained_reads=2",
        &[],
        &[],
    )
    .unwrap()
    .with_reads(vec![stable_first.clone(), stable_second])
    .unwrap();

    let changed_path = temp.path().join("changed.txt");
    fs::write(&changed_path, "first contents\n").unwrap();
    let changed_first = RetainedRead::capture(&changed_path, "context:task-1").unwrap();
    fs::write(&changed_path, "second, different contents\n").unwrap();
    let changed_second = RetainedRead::capture(&changed_path, "context:task-1").unwrap();
    assert_ne!(changed_first, changed_second);
    let changed_item = EvidenceItem::new(
        "source:changed#reads",
        EvidenceOwner::Source,
        ClaimKind::Observed,
        "retained_reads=2",
        &[],
        &[],
    )
    .unwrap()
    .with_reads(vec![changed_first.clone(), changed_second])
    .unwrap();

    let index = EvidenceIndex::new(vec![
        clean.clone(),
        corrupt.clone(),
        stable_item,
        changed_item,
    ])
    .unwrap();
    let claim = |evidence: &str, first: &RetainedRead, second_content: &str| RepeatedReadClaim {
        operation: "read:observed.txt".to_owned(),
        evidence: evidence.to_owned(),
        first: ReadIdentity {
            file: first.file.clone(),
            content: first.content.clone(),
            context: first.context.clone(),
        },
        second: ReadIdentity {
            file: first.file.clone(),
            content: second_content.to_owned(),
            context: first.context.clone(),
        },
    };
    let with_rollout_observation = |candidate: &mut Proposal| {
        candidate.observation = "rollout:clean#1".to_owned();
        candidate.evidence = vec![EvidenceRef {
            locator: "rollout:clean#1".to_owned(),
            kind: ClaimKind::Observed,
        }];
    };

    // Counterexample: a complete generic rollout item carries no retained read
    // facts, so equal claimed tokens still cannot establish the repetition.
    let mut generic = base_proposal("rollout:clean#1");
    with_rollout_observation(&mut generic);
    generic.repeated_read = Some(RepeatedReadClaim {
        operation: "read:src/lib.rs".to_owned(),
        evidence: "rollout:clean#1".to_owned(),
        first: ReadIdentity {
            file: "file:src/lib.rs".to_owned(),
            content: "sha256.aaaaaaaa".to_owned(),
            context: "context:task-1".to_owned(),
        },
        second: ReadIdentity {
            file: "file:src/lib.rs".to_owned(),
            content: "sha256.aaaaaaaa".to_owned(),
            context: "context:task-1".to_owned(),
        },
    });
    let outcomes = intake(&bd, &project, &report(generic), &index).unwrap();
    match &outcomes.outcomes[0] {
        IntakeOutcome::Deferred { reason, next } => {
            assert!(
                reason.contains("no two retained structured read facts"),
                "{reason}"
            );
            assert!(next.contains("capture both reads"), "{next}");
        }
        other => panic!("expected a deferral without retained read facts, got {other:?}"),
    }

    // Unknown claimed identity still defers to structured identity capture.
    let mut unknown = base_proposal("rollout:clean#1");
    with_rollout_observation(&mut unknown);
    unknown.repeated_read = Some(claim("source:stable#reads", &stable_first, "unknown"));
    let outcomes = intake(&bd, &project, &report(unknown), &index).unwrap();
    assert!(
        matches!(&outcomes.outcomes[0], IntakeOutcome::Deferred { reason, .. } if reason.contains("identity is unknown")),
        "{:?}",
        outcomes.outcomes[0]
    );

    // Retained reads differ in content while the claimed reads are fabricated
    // equal: the avoidable-repeat claim is refused.
    let mut fabricated = base_proposal("rollout:clean#1");
    with_rollout_observation(&mut fabricated);
    fabricated.repeated_read = Some(claim(
        "source:changed#reads",
        &changed_first,
        &changed_first.content,
    ));
    let outcomes = intake(&bd, &project, &report(fabricated), &index).unwrap();
    assert!(
        matches!(&outcomes.outcomes[0], IntakeOutcome::Refused { reasons } if reasons.iter().any(|reason| reason.contains("record no two reads with the same"))),
        "{:?}",
        outcomes.outcomes[0]
    );

    // A claim whose tokens do not match the retained facts is a mismatch, not
    // observed support.
    let mut mismatch = base_proposal("rollout:clean#1");
    with_rollout_observation(&mut mismatch);
    mismatch.repeated_read = Some(claim(
        "source:stable#reads",
        &changed_first,
        &changed_first.content,
    ));
    let outcomes = intake(&bd, &project, &report(mismatch), &index).unwrap();
    assert!(
        matches!(&outcomes.outcomes[0], IntakeOutcome::Refused { reasons } if reasons.iter().any(|reason| reason.contains("do not match the retained read facts"))),
        "{:?}",
        outcomes.outcomes[0]
    );
    assert!(
        board_hypothesis::list_hypothesis_cards(&bd, &project)
            .unwrap()
            .is_empty(),
        "deferred and refused repeated-read candidates perform no board mutation"
    );

    // Two retained reads with one identity and a matching claim continue
    // through the actual owned board intake.
    let mut supported = base_proposal("rollout:clean#1");
    with_rollout_observation(&mut supported);
    supported.repeated_read = Some(claim(
        "source:stable#reads",
        &stable_first,
        &stable_first.content,
    ));
    let outcomes = intake(&bd, &project, &report(supported), &index).unwrap();
    let IntakeOutcome::Admitted { id, .. } = &outcomes.outcomes[0] else {
        panic!(
            "expected the matching retained reads to be admitted, got {:?}",
            outcomes.outcomes[0]
        );
    };
    let card = board_hypothesis::load_card(&bd, &project, id).unwrap();
    assert!(card.labels.iter().any(|label| label == "hypothesis"));
}

#[test]
fn refusal_deferral_and_idle_leave_the_board_untouched() {
    let bd = bd_executable();
    let temp = tempfile::tempdir().unwrap();
    let project = board_project(temp.path());
    let evidence = EvidenceItem::new(
        "outcome:cycle-1#task",
        EvidenceOwner::Outcome,
        ClaimKind::Observed,
        "rounds=2 attempts=1",
        &[],
        &[],
    )
    .unwrap();
    let index = EvidenceIndex::new(vec![evidence]).unwrap();

    let idle = intake(
        &bd,
        &project,
        &InvestigatorReport {
            schema: 1,
            candidates: Vec::new(),
            idle_reason: None,
        },
        &index,
    )
    .unwrap();
    assert!(matches!(&idle.outcomes[0], IntakeOutcome::Idle { .. }));

    let mut missing = base_proposal("outcome:cycle-1#task");
    missing.observation = "outcome:cycle-1#task".to_owned();
    missing.acceptance = None;
    missing.evidence = vec![EvidenceRef {
        locator: "outcome:cycle-1#task".to_owned(),
        kind: ClaimKind::Observed,
    }];
    let outcomes = intake(&bd, &project, &report(missing), &index).unwrap();
    assert!(
        matches!(&outcomes.outcomes[0], IntakeOutcome::Refused { reasons } if reasons.iter().any(|reason| reason.contains("acceptance is required"))),
        "{:?}",
        outcomes.outcomes[0]
    );

    let mut removal = base_proposal("outcome:cycle-1#task");
    removal.observation = "outcome:cycle-1#task".to_owned();
    removal.evidence = vec![EvidenceRef {
        locator: "outcome:cycle-1#task".to_owned(),
        kind: ClaimKind::Observed,
    }];
    removal.treatment = Treatment::Subtraction {
        removal: RemovalClaim {
            target: "dormant-helper".to_owned(),
            basis: RemovalBasis::UsageVolume {
                invocations: 0,
                window: "90d".to_owned(),
            },
        },
    };
    let outcomes = intake(&bd, &project, &report(removal), &index).unwrap();
    match &outcomes.outcomes[0] {
        IntakeOutcome::Deferred { reason, next } => {
            assert!(reason.contains("lead for investigation"), "{reason}");
            assert!(next.contains("removal proposal"), "{next}");
        }
        other => panic!("expected a removal deferral, got {other:?}"),
    }

    assert!(
        board_hypothesis::list_hypothesis_cards(&bd, &project)
            .unwrap()
            .is_empty(),
        "idle, refused and deferred candidates leave the board untouched"
    );

    // A complete coverage/lost-use/restoration basis admits the removal
    // candidate for planning, but no removal is applied here: the outcome
    // only flags that the controller's removal gate must find the user's
    // recorded decision before dependent work.
    let mut removal = base_proposal("outcome:cycle-1#task");
    removal.observation = "outcome:cycle-1#task".to_owned();
    removal.evidence = vec![EvidenceRef {
        locator: "outcome:cycle-1#task".to_owned(),
        kind: ClaimKind::Observed,
    }];
    removal.treatment = Treatment::Subtraction {
        removal: RemovalClaim {
            target: "dormant-helper".to_owned(),
            basis: RemovalBasis::Coverage {
                interval: "180d".to_owned(),
                tasks: "all recorded synthetic tasks".to_owned(),
                gaps: "no machine-readable use from two workstations".to_owned(),
                lost_uses: "manual fallback remains available".to_owned(),
                restoration: "restore from the retained source revision".to_owned(),
                consumption:
                    "each arm records the effective catalogue identity and where the capability was consumed"
                        .to_owned(),
            },
        },
    };
    let outcomes = intake(&bd, &project, &report(removal), &index).unwrap();
    match &outcomes.outcomes[0] {
        IntakeOutcome::Admitted {
            id,
            removal_required: true,
            workload: None,
            ..
        } => {
            let card = board_hypothesis::load_card(&bd, &project, id).unwrap();
            assert!(
                !matches!(card.status.as_str(), "closed" | "deferred"),
                "the removal candidate is a pending card, not an applied removal"
            );
        }
        other => panic!("expected a removal candidate admission, got {other:?}"),
    }
}

#[test]
fn removal_coverage_requires_consumption_evidence_before_admission() {
    let bd = bd_executable();
    let temp = tempfile::tempdir().unwrap();
    let project = board_project(temp.path());
    let evidence = EvidenceItem::new(
        "outcome:cycle-1#task",
        EvidenceOwner::Outcome,
        ClaimKind::Observed,
        "rounds=2 attempts=1",
        &[],
        &[],
    )
    .unwrap();
    let index = EvidenceIndex::new(vec![evidence]).unwrap();

    // A coverage basis that names no way to observe actual consumption of
    // the removed burden is refused before removal planning: invocation
    // counts and catalogue/context exposure are distinct, so zero
    // invocations cannot stand in for consumption evidence.
    let mut removal = base_proposal("outcome:cycle-1#task");
    removal.observation = "outcome:cycle-1#task".to_owned();
    removal.evidence = vec![EvidenceRef {
        locator: "outcome:cycle-1#task".to_owned(),
        kind: ClaimKind::Observed,
    }];
    removal.treatment = Treatment::Subtraction {
        removal: RemovalClaim {
            target: "dormant-helper".to_owned(),
            basis: RemovalBasis::Coverage {
                interval: "180d".to_owned(),
                tasks: "all recorded synthetic tasks".to_owned(),
                gaps: "no machine-readable use from two workstations".to_owned(),
                lost_uses: "manual fallback remains available".to_owned(),
                restoration: "restore from the retained source revision".to_owned(),
                consumption: "   ".to_owned(),
            },
        },
    };
    let outcomes = intake(&bd, &project, &report(removal), &index).unwrap();
    assert!(
        matches!(&outcomes.outcomes[0], IntakeOutcome::Refused { reasons } if reasons.iter().any(|reason| reason.contains("consumption evidence is required"))),
        "{:?}",
        outcomes.outcomes[0]
    );
    assert!(
        board_hypothesis::list_hypothesis_cards(&bd, &project)
            .unwrap()
            .is_empty(),
        "a coverage basis without consumption evidence creates no card"
    );
}

/// The simplification review path through the real board: usage evidence is
/// cited from an existing owner's retained report instead of a new audit;
/// zero invocations never delete anything; incomplete usage coverage defers;
/// and a complete coverage/lost-use/consumption/restoration review admits only
/// a pending candidate behind the user's informed removal decision.
#[test]
fn skill_usage_review_reuses_owners_and_gates_removal() {
    let bd = bd_executable();
    let temp = tempfile::tempdir().unwrap();
    let project = board_project(temp.path());
    // The installed skill-usage owner's retained report: the review cites it
    // by its content identity instead of re-deriving usage.
    let report_path = temp.path().join("skills-usage.txt");
    fs::write(
        &report_path,
        "skill:context-heavy last_invocation=none catalogue=loaded exposure=every-session\nskill:rare-recovery last_invocation=2026-09-07 route=manual-recovery\n",
    )
    .unwrap();
    let index = EvidenceIndex::new(vec![
        EvidenceItem::read_source("file:skills-usage.txt", "context:review-1", &report_path)
            .unwrap(),
        EvidenceItem::new(
            "outcome:cycle-1#task",
            EvidenceOwner::Outcome,
            ClaimKind::Observed,
            "rounds=2 attempts=1",
            &[],
            &[],
        )
        .unwrap(),
    ])
    .unwrap();

    // Zero invocations over a year are a lead for investigation: the exposure
    // a dormant skill still adds is not shown absent, and nothing is deleted.
    let mut usage_volume = base_proposal("file:skills-usage.txt");
    usage_volume.observation = "file:skills-usage.txt".to_owned();
    usage_volume.evidence = vec![EvidenceRef {
        locator: "file:skills-usage.txt".to_owned(),
        kind: ClaimKind::Observed,
    }];
    usage_volume.treatment = Treatment::Subtraction {
        removal: RemovalClaim {
            target: "skill:context-heavy".to_owned(),
            basis: RemovalBasis::UsageVolume {
                invocations: 0,
                window: "365d".to_owned(),
            },
        },
    };
    let outcomes = intake(&bd, &project, &report(usage_volume), &index).unwrap();
    let IntakeOutcome::Deferred { reason, next } = &outcomes.outcomes[0] else {
        panic!("expected deferral, got {:?}", outcomes.outcomes[0]);
    };
    assert!(reason.contains("lead for investigation"), "{reason}");
    assert!(reason.contains("catalogue"), "{reason}");
    assert!(next.contains("consumption"), "{next}");
    assert!(
        board_hypothesis::list_hypothesis_cards(&bd, &project)
            .unwrap()
            .is_empty(),
        "usage volume alone admits nothing and removes nothing"
    );

    // An incomplete usage reader (one session store unreadable) cannot ground
    // removal planning: unresolved supported use stays explicit.
    let partial = EvidenceIndex::new(vec![
        EvidenceItem::new(
            "rollout:fleet#partial",
            EvidenceOwner::Rollout,
            ClaimKind::Observed,
            "lines=120; one workstation's session store was unreadable",
            &[
                "one workstation's usage records were unreadable; their use stays unknown"
                    .to_owned(),
            ],
            &[],
        )
        .unwrap(),
    ])
    .unwrap();
    let mut incomplete = base_proposal("rollout:fleet#partial");
    incomplete.observation = "rollout:fleet#partial".to_owned();
    incomplete.evidence = vec![EvidenceRef {
        locator: "rollout:fleet#partial".to_owned(),
        kind: ClaimKind::Observed,
    }];
    incomplete.treatment = Treatment::Subtraction {
        removal: RemovalClaim {
            target: "skill:context-heavy".to_owned(),
            basis: RemovalBasis::Coverage {
                interval: "365d".to_owned(),
                tasks: "every recorded task on the covered workstations".to_owned(),
                gaps: "two workstations record no usage; their consumers are uncheckable"
                    .to_owned(),
                lost_uses: "a rare manual recovery in a degraded environment".to_owned(),
                restoration: "restore from the retained source revision".to_owned(),
                consumption:
                    "each covered arm records the effective catalogue exposure, not just invocations"
                        .to_owned(),
            },
        },
    };
    let outcomes = intake(&bd, &project, &report(incomplete), &partial).unwrap();
    assert!(
        matches!(&outcomes.outcomes[0], IntakeOutcome::Deferred { reason, .. } if reason.contains("partial")),
        "{:?}",
        outcomes.outcomes[0]
    );
    assert!(
        board_hypothesis::list_hypothesis_cards(&bd, &project)
            .unwrap()
            .is_empty(),
        "partial usage evidence admits no card"
    );

    // A complete coverage review of the same skill states the interval,
    // task/environment coverage, telemetry gaps, the rare recovery use at
    // risk, the per-arm consumption evidence and restoration. It is admitted
    // only as a pending candidate: no removal is applied here, and the user's
    // informed decision remains owned by the removal gate.
    let mut review = base_proposal("file:skills-usage.txt");
    review.observation = "file:skills-usage.txt".to_owned();
    review.evidence = vec![EvidenceRef {
        locator: "file:skills-usage.txt".to_owned(),
        kind: ClaimKind::Observed,
    }];
    review.treatment = Treatment::Simplification {
        removal: RemovalClaim {
            target: "skill:context-heavy".to_owned(),
            basis: RemovalBasis::Coverage {
                interval: "2026-01-01..2026-10-01".to_owned(),
                tasks: "every recorded synthetic task on one reviewed workstation".to_owned(),
                gaps: "two workstations record no usage; their consumers are uncheckable"
                    .to_owned(),
                lost_uses: "a rare manual recovery in a degraded environment".to_owned(),
                restoration: "restore from the retained source revision".to_owned(),
                consumption:
                    "each arm records the effective catalogue exposure and where the capability was consumed"
                        .to_owned(),
            },
        },
    };
    let outcomes = intake(&bd, &project, &report(review), &index).unwrap();
    let IntakeOutcome::Admitted {
        id,
        removal_required: true,
        ..
    } = &outcomes.outcomes[0]
    else {
        panic!(
            "expected a removal candidate admission, got {:?}",
            outcomes.outcomes[0]
        );
    };
    let card = board_hypothesis::load_card(&bd, &project, id).unwrap();
    assert!(
        !matches!(card.status.as_str(), "closed" | "deferred"),
        "the review is a pending candidate, not an applied removal"
    );
    assert!(report_path.is_file(), "nothing was deleted by intake");
}

/// The declared effect path fixes the smallest sufficient unit, and a
/// directly selected stronger or equal method is admitted without any
/// mandatory sequence of cheaper trials.
#[test]
fn experiment_selection_selects_the_smallest_sufficient_real_unit() {
    let bd = bd_executable();
    let temp = tempfile::tempdir().unwrap();
    let rollout = write_rollout(temp.path(), "seed.jsonl", false);
    let index = EvidenceIndex::new(vec![
        EvidenceItem::read_rollout("rollout:cycle-1#seed", &rollout).unwrap(),
    ])
    .unwrap();

    for (name, method, claim) in [
        (
            "local-operation",
            ExperimentMethod::RealOperation,
            EffectPath::LocalOperation,
        ),
        (
            "agent-choice",
            ExperimentMethod::AgentTask,
            EffectPath::AgentChoice,
        ),
        (
            "task-strategy",
            ExperimentMethod::PairedImplementations,
            EffectPath::TaskStrategy,
        ),
        (
            "repeated-use",
            ExperimentMethod::Sequence,
            EffectPath::RepeatedUse,
        ),
    ] {
        let project = board_project(&temp.path().join(name));
        let mut candidate = base_proposal("rollout:cycle-1#seed");
        candidate.selection = Some(selection(method, claim));
        let outcomes = intake(&bd, &project, &report(candidate), &index).unwrap();
        assert!(
            matches!(&outcomes.outcomes[0], IntakeOutcome::Admitted { .. }),
            "{name}: {:?}",
            outcomes.outcomes[0]
        );
    }

    // The smallest sufficient unit per path; a local build/output treatment
    // selects a short real operation, agent-choice effects require an agent,
    // broad strategy selects full paired completion when necessary, and a
    // repeated-use claim keeps the sequence and state.
    assert_eq!(
        EffectPath::LocalOperation.smallest_sufficient(),
        ExperimentMethod::RealOperation
    );
    assert_eq!(
        EffectPath::AgentChoice.smallest_sufficient(),
        ExperimentMethod::AgentTask
    );
    assert_eq!(
        EffectPath::TaskStrategy.smallest_sufficient(),
        ExperimentMethod::PairedImplementations
    );
    assert_eq!(
        EffectPath::RepeatedUse.smallest_sufficient(),
        ExperimentMethod::Sequence
    );
}

/// Insufficient selections are refused with the exact cause and never touch
/// the board: the paths below cannot exist, so any board access would fail
/// the intake call instead of producing the expected refusal.
#[test]
fn insufficient_experiment_selections_are_refused_without_board_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let rollout = write_rollout(temp.path(), "seed.jsonl", false);
    let index = EvidenceIndex::new(vec![
        EvidenceItem::read_rollout("rollout:cycle-1#seed", &rollout).unwrap(),
    ])
    .unwrap();
    let unowned = (Path::new("no-such-bd"), Path::new("no-such-project"));

    let refused = |selection: Option<ExperimentSelection>,
                   deferral: Option<SelectionDeferral>|
     -> IntakeOutcome {
        let mut candidate = base_proposal("rollout:cycle-1#seed");
        candidate.selection = selection;
        candidate.deferral = deferral;
        let outcomes = intake(unowned.0, unowned.1, &report(candidate), &index).unwrap();
        outcomes.outcomes[0].clone()
    };
    let reasons = |outcome: IntakeOutcome| -> Vec<String> {
        match outcome {
            IntakeOutcome::Refused { reasons } => reasons,
            other => panic!("expected a refusal, got {other:?}"),
        }
    };

    // The declaration itself is required before admission.
    let missing = reasons(refused(None, None));
    assert!(
        missing
            .iter()
            .any(|reason| reason.contains("experiment-selection declaration is required")),
        "{missing:?}"
    );

    // Agent-choice effects require a real agent: a fixed operation bypasses
    // the choices and a retained replay cannot stand in for the unexercised
    // agent.
    let fixed = reasons(refused(
        Some(selection(
            ExperimentMethod::RealOperation,
            EffectPath::AgentChoice,
        )),
        None,
    ));
    assert!(
        fixed
            .iter()
            .any(|reason| reason.contains("bypasses the agent's choices")),
        "{fixed:?}"
    );
    let replay = reasons(refused(
        Some(selection(
            ExperimentMethod::BoundedReplay,
            EffectPath::AgentChoice,
        )),
        None,
    ));
    assert!(
        replay
            .iter()
            .any(|reason| reason.contains("cannot stand in for an unexercised agent")),
        "{replay:?}"
    );

    // A broad strategy claim needs complete paired implementations.
    let short = reasons(refused(
        Some(selection(
            ExperimentMethod::AgentTask,
            EffectPath::TaskStrategy,
        )),
        None,
    ));
    assert!(
        short
            .iter()
            .any(|reason| reason.contains("complete paired task implementations")),
        "{short:?}"
    );

    // A repeated-use claim preserves the sequence and state.
    let single = reasons(refused(
        Some(selection(
            ExperimentMethod::RealOperation,
            EffectPath::RepeatedUse,
        )),
        None,
    ));
    assert!(
        single
            .iter()
            .any(|reason| reason.contains("sequence and state transitions")),
        "{single:?}"
    );
    let misplaced = reasons(refused(
        Some(selection(
            ExperimentMethod::Sequence,
            EffectPath::LocalOperation,
        )),
        None,
    ));
    assert!(
        misplaced
            .iter()
            .any(|reason| reason.contains("only for a repeated-use or recovery claim")),
        "{misplaced:?}"
    );

    // Fewer lines, files, skills or exposed names never establish benefit.
    let size_only = reasons(refused(
        Some(selection(
            ExperimentMethod::RealOperation,
            EffectPath::SizeOnly,
        )),
        None,
    ));
    assert!(
        size_only.iter().any(|reason| reason
            .contains("fewer lines, files, skills or exposed names never establish benefit")),
        "{size_only:?}"
    );
}

/// When the sufficient experiment is not worth its cost, the candidate is
/// deferred with the missing fact and the reconsideration condition instead
/// of running, adopting without support or repeating an inconclusive trial.
#[test]
fn a_costly_low_value_measurement_is_deferred_with_its_reconsideration_condition() {
    let temp = tempfile::tempdir().unwrap();
    let rollout = write_rollout(temp.path(), "seed.jsonl", false);
    let index = EvidenceIndex::new(vec![
        EvidenceItem::read_rollout("rollout:cycle-1#seed", &rollout).unwrap(),
    ])
    .unwrap();
    let mut candidate = base_proposal("rollout:cycle-1#seed");
    candidate.deferral = Some(SelectionDeferral {
        missing_fact: "no owned accepted workload can exercise the sequence within the budget"
            .to_owned(),
        reconsideration: "reconsider when an owned accepted workload with that sequence exists"
            .to_owned(),
    });
    let outcomes = intake(
        Path::new("no-such-bd"),
        Path::new("no-such-project"),
        &report(candidate),
        &index,
    )
    .unwrap();
    let IntakeOutcome::Deferred { reason, next } = &outcomes.outcomes[0] else {
        panic!("expected a deferral, got {:?}", outcomes.outcomes[0]);
    };
    assert!(
        reason.contains("deferred as not worth its cost"),
        "{reason}"
    );
    assert!(
        reason.contains("no owned accepted workload can exercise the sequence"),
        "{reason}"
    );
    assert_eq!(
        next, "reconsider when an owned accepted workload with that sequence exists",
        "{next}"
    );
}

/// A report of the quality a real investigator produces passes one grounded
/// intake round: the five-route alternatives comparison and the no-change
/// reason exceed the former 256-byte statement bound, and the selection
/// clauses exceed the former 192-byte field bound, while every field stays
/// one bounded single line.
#[test]
fn rich_investigator_analysis_within_the_raised_bounds_is_admitted() {
    let bd = bd_executable();
    let temp = tempfile::tempdir().unwrap();
    let project = board_project(temp.path());
    let seed_path = write_rollout(temp.path(), "seed.jsonl", false);
    let index = EvidenceIndex::new(vec![
        EvidenceItem::read_rollout("rollout:cycle-1#seed", &seed_path).unwrap(),
    ])
    .unwrap();

    // A single-line field of exactly `total` bytes carrying rich prose.
    let filled = |prefix: &str, total: usize| {
        let mut value = prefix.to_owned();
        while value.len() < total {
            value.push_str(" detail");
        }
        value.truncate(total);
        value
    };

    let alternatives = filled(
        "all five routes were compared: no change leaves the recorded repeated-read burden in place",
        1200,
    );
    assert!(
        (257..=2048).contains(&alternatives.len()),
        "the fixture must exceed the former statement bound: {}",
        alternatives.len()
    );

    let mut selection = selection(ExperimentMethod::RealOperation, EffectPath::LocalOperation);
    selection.outcome = filled(
        "the declared outcome measured through the real operation",
        256,
    );
    selection.rationale = filled(
        "the unit exercises the claimed mechanism under the declared conditions",
        512,
    );
    selection.controls = filled(
        "frozen inputs and the accepted baseline conditions are retained for both arms",
        320,
    );
    selection.projection = filled(
        "one bounded local cycle with the retention cost staying bounded",
        224,
    );
    selection.baseline = filled("the accepted revision excluding the candidate edit", 200);
    selection.stopping = filled(
        "stop after the declared attempts and escalate only for a named missing observation",
        448,
    );
    for field in [
        &selection.outcome,
        &selection.rationale,
        &selection.controls,
        &selection.projection,
        &selection.baseline,
        &selection.stopping,
    ] {
        assert!(
            (193..=512).contains(&field.len()),
            "the fixture must exceed the former field bound within the raised one: {}",
            field.len()
        );
    }

    let mut addition = base_proposal("rollout:cycle-1#seed");
    addition.alternatives = Some(alternatives);
    addition.selection = Some(selection);

    let reason = filled(
        "no change: the retained outcome records show the burden repeating on every accepted task",
        1100,
    );
    assert!(
        (257..=2048).contains(&reason.len()),
        "the fixture must exceed the former statement bound: {}",
        reason.len()
    );
    let mut no_change = base_proposal("rollout:cycle-1#seed");
    no_change.treatment = Treatment::NoChange {
        reason: reason.clone(),
    };

    let outcomes = intake(
        &bd,
        &project,
        &InvestigatorReport {
            schema: 1,
            candidates: vec![addition, no_change],
            idle_reason: None,
        },
        &index,
    )
    .unwrap();

    assert_eq!(outcomes.outcomes.len(), 2);
    match &outcomes.outcomes[0] {
        IntakeOutcome::Admitted { id, .. } => {
            let card = board_hypothesis::load_card(&bd, &project, id).unwrap();
            assert!(card.labels.iter().any(|label| label == "hypothesis"));
        }
        other => panic!("expected the rich addition to be admitted, got {other:?}"),
    }
    match &outcomes.outcomes[1] {
        IntakeOutcome::NoChange { reason: conclusion } => assert_eq!(conclusion, &reason),
        other => panic!("expected the rich no-change conclusion, got {other:?}"),
    }
}

/// The raised bounds admit honest analysis; genuinely oversized or multiline
/// values still refuse before any board access.
#[test]
fn oversized_or_multiline_values_still_refuse_under_the_raised_bounds() {
    let temp = tempfile::tempdir().unwrap();
    let rollout = write_rollout(temp.path(), "seed.jsonl", false);
    let index = EvidenceIndex::new(vec![
        EvidenceItem::read_rollout("rollout:cycle-1#seed", &rollout).unwrap(),
    ])
    .unwrap();
    let unowned = (Path::new("no-such-bd"), Path::new("no-such-project"));
    let refused = |candidate: Proposal| -> Vec<String> {
        let outcomes = intake(unowned.0, unowned.1, &report(candidate), &index).unwrap();
        match outcomes.outcomes.into_iter().next().unwrap() {
            IntakeOutcome::Refused { reasons } => reasons,
            other => panic!("expected a refusal, got {other:?}"),
        }
    };

    // A single-line alternatives statement beyond the raised bound.
    let mut alternatives = base_proposal("rollout:cycle-1#seed");
    alternatives.alternatives = Some("a".repeat(2049));
    let reasons = refused(alternatives);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("alternatives consideration exceeds 2048 bytes")),
        "{reasons:?}"
    );

    // A multiline alternatives statement refuses inside the byte bound too.
    let mut multiline = base_proposal("rollout:cycle-1#seed");
    multiline.alternatives = Some("first line\nsecond line".to_owned());
    let reasons = refused(multiline);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("alternatives consideration must be a single line")),
        "{reasons:?}"
    );

    // A no-change reason beyond the raised bound.
    let mut no_change = base_proposal("rollout:cycle-1#seed");
    no_change.treatment = Treatment::NoChange {
        reason: "r".repeat(2049),
    };
    let reasons = refused(no_change);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("no-change reason exceeds 2048 bytes")),
        "{reasons:?}"
    );

    // A selection field beyond its raised bound, and a multiline one.
    let mut over = base_proposal("rollout:cycle-1#seed");
    if let Some(selection) = over.selection.as_mut() {
        selection.rationale = "r".repeat(513);
    }
    let reasons = refused(over);
    assert!(
        reasons
            .iter()
            .any(|reason| reason
                .contains("must be one bounded line of at most 512 bytes without ';'")),
        "{reasons:?}"
    );
    let mut multiline = base_proposal("rollout:cycle-1#seed");
    if let Some(selection) = multiline.selection.as_mut() {
        selection.controls = "one\ntwo".to_owned();
    }
    let reasons = refused(multiline);
    assert!(
        reasons
            .iter()
            .any(|reason| reason
                .contains("must be one bounded line of at most 512 bytes without ';'")),
        "{reasons:?}"
    );
}
