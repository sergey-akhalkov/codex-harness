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
    RetainedRead, Treatment, WorkloadLink, WorkloadRef, intake,
};
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
