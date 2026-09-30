//! Group 10 evaluation contract: consumed-package identity, frozen workflow
//! acceptance, context-candidate selection and integration decisions.
//!
//! Everything here is model-free. Model-backed comparisons stay behind their
//! explicit gates; an unexecuted comparison is reported pending and is never
//! recorded as passed.
#![cfg(windows)]

#[path = "fixtures/skill_consumption.rs"]
mod skill_consumption;

use serde_json::{Value, json};
use skill_consumption::*;
use skill_evolution::{comparison, decision, package, plan};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn write_skill(root: &Path, name: &str) -> package::Identity {
    fs::create_dir_all(root.join("references")).unwrap();
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(
        root.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Consumed-package fixture.\n---\nBody\n"),
    )
    .unwrap();
    fs::write(root.join("references/check.md"), "reference body\n").unwrap();
    fs::write(root.join("scripts/helper.ps1"), "Write-Output helper\n").unwrap();
    package::load(root).unwrap()
}

fn input_path(root: &Path, relative: &str) -> String {
    format!(r#"Get-Content "{}""#, root.join(relative).to_string_lossy())
}

#[test]
fn classification_attributes_frozen_body_references_helpers_and_detects_fallback() {
    let root = tempfile::tempdir().unwrap();
    let frozen_root = root.path().join("frozen");
    let identity = write_skill(&frozen_root, "compared-skill");
    let live_root = root.path().join("live").join(".agents").join("skills");
    fs::create_dir_all(live_root.join("compared-skill")).unwrap();
    let frozen = Frozen::new(
        identity.clone(),
        vec![frozen_root.clone()],
        vec![live_root.clone()],
    );
    let inputs = vec![
        input_path(&frozen_root, "SKILL.md"),
        input_path(&frozen_root, "references/check.md"),
        input_path(&frozen_root, "scripts/helper.ps1"),
        input_path(&live_root, "compared-skill/SKILL.md"),
        "Get-Content unrelated.txt".to_owned(),
    ];
    let consumption = frozen.classify(&inputs);
    assert_eq!(consumption.body.len(), 1, "{consumption:?}");
    assert!(consumption.body[0].ends_with("SKILL.md"));
    assert_eq!(consumption.references.len(), 1, "{consumption:?}");
    assert!(consumption.references[0].ends_with("check.md"));
    assert_eq!(consumption.helpers.len(), 1, "{consumption:?}");
    assert!(consumption.helpers[0].ends_with("helper.ps1"));
    assert_eq!(consumption.live.len(), 1, "{consumption:?}");
    assert!(
        consumption.live[0]
            .to_lowercase()
            .contains("compared-skill/skill.md"),
        "{consumption:?}"
    );
    assert!(consumption.foreign.is_empty(), "{consumption:?}");
    // The oracle's name-level signal agrees that a comparison-relevant
    // reference exists, without distinguishing the resolved revision.
    assert!(frozen.name_referenced(&inputs));

    let unrelated = frozen.classify(&[input_path(root.path(), "unrelated.txt")]);
    assert_eq!(unrelated, Consumption::default());
    assert!(!frozen.name_referenced(&["cargo test".to_owned()]));
}

#[test]
fn unattributable_revision_is_foreign_not_attributed_consumption() {
    let root = tempfile::tempdir().unwrap();
    let frozen_root = root.path().join("frozen");
    let identity = write_skill(&frozen_root, "compared-skill");
    let frozen = Frozen::new(identity, vec![frozen_root], Vec::new());
    let foreign = root
        .path()
        .join("elsewhere")
        .join("compared-skill")
        .join("SKILL.md");
    let consumption = frozen.classify(&[input_path(
        &root.path().join("elsewhere"),
        "compared-skill/SKILL.md",
    )]);
    assert!(consumption.body.is_empty(), "{consumption:?}");
    assert_eq!(consumption.foreign.len(), 1, "{consumption:?}");
    assert!(
        consumption.foreign[0]
            .to_lowercase()
            .contains("compared-skill/skill.md"),
        "{consumption:?}"
    );
    assert!(frozen.name_referenced(&[foreign.to_string_lossy().into_owned()]));
}

#[test]
fn evidence_parsing_mirrors_oracle_success_semantics() {
    let root = tempfile::tempdir().unwrap();
    let rows = [
        json!({"type": "item.completed", "item": {
            "type": "command_execution", "id": "c1", "status": "completed",
            "command": "Get-Content SKILL.md", "exit_code": 0, "aggregated_output": "ok"}}),
        json!({"type": "item.completed", "item": {
            "type": "command_execution", "id": "c2", "status": "failed",
            "command": "Get-Content SKILL.md", "exit_code": 0, "aggregated_output": "no"}}),
        json!({"type": "item.completed", "item": {
            "type": "mcp_tool_call", "id": "m1", "server": "s", "tool": "read_file",
            "status": "completed", "arguments": {"path": "SKILL.md"}, "result": {"content": []}}}),
        json!({"type": "item.completed", "item": {
            "type": "mcp_tool_call", "id": "m2", "server": "s", "tool": "read_file",
            "status": "completed", "arguments": {"path": "SKILL.md"},
            "result": {"content": [], "isError": true}}}),
        json!({"type": "item.completed", "item": {"type": "agent_message", "text": "SKILL.md"}}),
    ];
    let text = rows
        .iter()
        .map(|row| row.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(root.path().join("events.jsonl"), text).unwrap();
    let inputs = evidence_inputs(root.path()).unwrap();
    assert_eq!(inputs.len(), 2, "{inputs:?}");
    assert!(inputs[0].contains("Get-Content SKILL.md"));
    assert!(inputs[1].contains("SKILL.md"));

    fs::write(root.path().join("events.jsonl"), "{not json\n").unwrap();
    assert!(evidence_inputs(root.path()).is_err());
    fs::remove_file(root.path().join("events.jsonl")).unwrap();
    assert!(evidence_inputs(root.path()).is_err());
}

#[test]
fn arm_verdicts_expose_missing_treatment_contamination_and_absence() {
    let attributed = Consumption {
        body: vec!["SKILL.md".to_owned()],
        ..Consumption::default()
    };
    let contaminated = Consumption {
        live: vec!["live/compared-skill/skill.md".to_owned()],
        ..Consumption::default()
    };
    let absent = Consumption::default();
    assert_eq!(
        arm_verdict(Expectation::TreatmentExpected, &attributed, &[], &[]),
        ArmVerdict::Attributed
    );
    assert_eq!(
        arm_verdict(Expectation::TreatmentExpected, &absent, &[], &[]),
        ArmVerdict::MissingTreatment
    );
    assert_eq!(
        arm_verdict(Expectation::TreatmentExpected, &contaminated, &[], &[]),
        ArmVerdict::Contaminated
    );
    assert_eq!(
        arm_verdict(
            Expectation::TreatmentExpected,
            &attributed,
            &["changed revision".to_owned()],
            &[]
        ),
        ArmVerdict::Drifted
    );
    assert_eq!(
        arm_verdict(
            Expectation::TreatmentExpected,
            &attributed,
            &[],
            &["missing log".to_owned()]
        ),
        ArmVerdict::Incomplete
    );
    assert_eq!(
        arm_verdict(Expectation::ActivationForbidden, &absent, &[], &[]),
        ArmVerdict::Absent
    );
    assert_eq!(
        arm_verdict(Expectation::ActivationForbidden, &attributed, &[], &[]),
        ArmVerdict::Activated
    );
    assert_eq!(
        arm_verdict(Expectation::ActivationForbidden, &contaminated, &[], &[]),
        ArmVerdict::Contaminated
    );
    // A negative or absence arm is allowed to avoid activation without being
    // reported as a missing-treatment intended case.
    assert_ne!(
        arm_verdict(Expectation::ActivationForbidden, &absent, &[], &[]),
        ArmVerdict::MissingTreatment
    );
}

#[test]
fn replica_drift_detects_a_changed_frozen_library_after_the_run() {
    let root = tempfile::tempdir().unwrap();
    let frozen_root = root.path().join("frozen");
    let identity = write_skill(&frozen_root, "compared-skill");
    let frozen = Frozen::new(identity, vec![frozen_root.clone()], Vec::new());
    assert!(frozen.replica_drift().is_empty());
    fs::write(frozen_root.join("references/check.md"), "rewritten\n").unwrap();
    let drift = frozen.replica_drift();
    assert_eq!(drift.len(), 1, "{drift:?}");
    let consumption = frozen.classify(&[input_path(&frozen_root, "SKILL.md")]);
    assert_eq!(consumption.body.len(), 1);
    assert_eq!(
        arm_verdict(Expectation::TreatmentExpected, &consumption, &drift, &[]),
        ArmVerdict::Drifted
    );
}

struct Batch {
    kind: comparison::Kind,
    pairs: Vec<CasePair>,
}

impl Batch {
    fn facts(&self) -> BatchFacts {
        let declared = declared_workflows_for(self.kind);
        BatchFacts {
            kind: self.kind,
            model: "grok-4.6".to_owned(),
            effort: "xhigh".to_owned(),
            timeout_seconds: 600,
            declared,
            pairs: self
                .pairs
                .iter()
                .map(|pair| CasePair {
                    case_id: pair.case_id.clone(),
                    role: pair.role,
                    baseline: pair.baseline.clone(),
                    candidate: pair.candidate.clone(),
                })
                .collect(),
        }
    }
}

fn declared_workflows_for(kind: comparison::Kind) -> Vec<(String, Role)> {
    let batch = match kind {
        comparison::Kind::UpdateOld => "shortening",
        _ => "add-absence",
    };
    declared_workflows()
        .into_iter()
        .filter(|(name, _, _)| *name == batch)
        .map(|(_, case_id, role)| (case_id.to_owned(), role))
        .collect()
}

fn arm(passed: bool, verdict: ArmVerdict) -> ArmSummary {
    ArmSummary {
        run_status: "completed".to_owned(),
        oracle_passed: Some(passed),
        oracle_readable: true,
        oracle_agreement: true,
        verdict: Some(verdict),
        live_unchanged: true,
        model: "grok-4.6".to_owned(),
        effort: "xhigh".to_owned(),
        elapsed_seconds: Some(20),
        evidence: EvidenceVector {
            accepted_task_cost: Some("complete accepted-task cost".to_owned()),
            required_discovery: true,
            errors_detail_recovery: true,
            cache_basis: Some("within-arm cached_input_tokens".to_owned()),
            uncertainty: Some("one pair; provider matched".to_owned()),
        },
    }
}

fn add_absence_accepting_batch() -> Batch {
    let pair = |case_id: &str, role: Role, baseline: bool, candidate: bool, verdict: ArmVerdict| {
        CasePair {
            case_id: case_id.to_owned(),
            role,
            baseline: arm(baseline, ArmVerdict::Absent),
            candidate: arm(candidate, verdict),
        }
    };
    Batch {
        kind: comparison::Kind::AddAbsence,
        pairs: vec![
            pair(
                plan::ACCEPT_INTENDED,
                Role::Intended,
                false,
                true,
                ArmVerdict::Attributed,
            ),
            pair(
                plan::ACCEPT_NEGATIVE,
                Role::SimilarUnsuitable,
                true,
                true,
                ArmVerdict::Absent,
            ),
            pair(
                plan::ACCEPT_NEGATIVE,
                Role::BoundaryFailure,
                true,
                true,
                ArmVerdict::Attributed,
            ),
            pair(
                plan::ACCEPT_HELD_OUT,
                Role::IndependentHeldOut,
                false,
                true,
                ArmVerdict::Attributed,
            ),
        ],
    }
}

#[test]
fn frozen_acceptance_accepts_only_the_fully_attributed_batch() {
    let batch = add_absence_accepting_batch();
    let evidence = frozen_batch_evidence(&batch.facts());
    assert_eq!(decision::decide(&evidence), decision::Verdict::Accept);

    // A live-library read on an intended arm is retained but excluded from any
    // causal claim: contamination cannot authorize the candidate.
    let mut contaminated = add_absence_accepting_batch();
    contaminated.pairs[0].candidate.verdict = Some(ArmVerdict::Contaminated);
    let evidence = frozen_batch_evidence(&contaminated.facts());
    assert!(!evidence.evidence_complete);
    assert!(!evidence.benefit_established);
    assert_eq!(decision::decide(&evidence), decision::Verdict::Inconclusive);

    // Reference drift on an independent held-out arm is likewise incomparable.
    let mut drifted = add_absence_accepting_batch();
    drifted.pairs[3].candidate.verdict = Some(ArmVerdict::Drifted);
    let evidence = frozen_batch_evidence(&drifted.facts());
    assert!(!evidence.evidence_complete);
    assert_eq!(decision::decide(&evidence), decision::Verdict::Inconclusive);

    // Missing treatment on the intended arm is explicit, not a silent pass.
    let mut missing = add_absence_accepting_batch();
    missing.pairs[0].candidate.verdict = Some(ArmVerdict::MissingTreatment);
    let evidence = frozen_batch_evidence(&missing.facts());
    assert!(!evidence.evidence_complete);
    assert_eq!(decision::decide(&evidence), decision::Verdict::Inconclusive);

    // A skipped declared workflow cannot authorize anything.
    let mut skipped = add_absence_accepting_batch();
    skipped
        .pairs
        .retain(|pair| pair.role != Role::IndependentHeldOut);
    let evidence = frozen_batch_evidence(&skipped.facts());
    assert!(evidence.skipped_required_check);
    assert_eq!(decision::decide(&evidence), decision::Verdict::Reject);

    // A harmed similar-unsuitable workflow is a protected regression.
    let mut regression = add_absence_accepting_batch();
    regression.pairs[1].candidate.oracle_passed = Some(false);
    let evidence = frozen_batch_evidence(&regression.facts());
    assert!(evidence.protected_regression);
    assert_eq!(decision::decide(&evidence), decision::Verdict::Reject);

    // Missing accounting (no cache basis) is incomplete evidence.
    let mut unaccounted = add_absence_accepting_batch();
    unaccounted.pairs[0].candidate.evidence.cache_basis = None;
    let evidence = frozen_batch_evidence(&unaccounted.facts());
    assert!(!evidence.evidence_complete);
    assert_eq!(decision::decide(&evidence), decision::Verdict::Inconclusive);

    // A provider mismatch is inconclusive, not an accept.
    let mut unmatched = add_absence_accepting_batch();
    unmatched.pairs[2].candidate.effort = "low".to_owned();
    let evidence = frozen_batch_evidence(&unmatched.facts());
    assert!(!evidence.provider_matched);
    assert_eq!(decision::decide(&evidence), decision::Verdict::Inconclusive);

    // The update-old shape requires both enabled arms to consume their own
    // frozen revision; contamination on either arm is excluded.
    let mut update_old = Batch {
        kind: comparison::Kind::UpdateOld,
        pairs: vec![
            CasePair {
                case_id: plan::INTENDED_CASE.to_owned(),
                role: Role::Intended,
                baseline: arm(false, ArmVerdict::Attributed),
                candidate: arm(true, ArmVerdict::Attributed),
            },
            CasePair {
                case_id: plan::NEGATIVE_CASE.to_owned(),
                role: Role::SimilarUnsuitable,
                baseline: arm(true, ArmVerdict::Absent),
                candidate: arm(true, ArmVerdict::Absent),
            },
            CasePair {
                case_id: plan::BOUNDARY_CASE.to_owned(),
                role: Role::BoundaryFailure,
                baseline: arm(true, ArmVerdict::Attributed),
                candidate: arm(true, ArmVerdict::Attributed),
            },
            CasePair {
                case_id: plan::HELD_OUT_CASE.to_owned(),
                role: Role::IndependentHeldOut,
                baseline: arm(false, ArmVerdict::Attributed),
                candidate: arm(true, ArmVerdict::Attributed),
            },
        ],
    };
    let evidence = frozen_batch_evidence(&update_old.facts());
    assert_eq!(decision::decide(&evidence), decision::Verdict::Accept);
    update_old.pairs[3].baseline.verdict = Some(ArmVerdict::Contaminated);
    let evidence = frozen_batch_evidence(&update_old.facts());
    assert!(!evidence.evidence_complete);
    assert_eq!(decision::decide(&evidence), decision::Verdict::Inconclusive);
}

#[test]
fn declared_workflows_cover_every_role_with_independent_held_out_cases() {
    let declared = declared_workflows();
    for role in [
        Role::Intended,
        Role::SimilarUnsuitable,
        Role::BoundaryFailure,
        Role::IndependentHeldOut,
        Role::ProtectedOverlapping,
    ] {
        assert!(
            declared
                .iter()
                .any(|(_, _, declared_role)| *declared_role == role),
            "missing role {role:?}"
        );
    }
    let shortcut = plan::shortening_batch();
    assert_ne!(shortcut.intended, shortcut.held_out);
    let absent = plan::independent_acceptance_batch();
    assert_ne!(absent.intended, absent.held_out);
    assert!(
        declared
            .iter()
            .filter(|(batch, _, role)| *batch == "shortening" && *role == Role::IndependentHeldOut)
            .all(|(_, case_id, _)| *case_id != plan::INTENDED_CASE)
    );
    assert!(
        declared
            .iter()
            .filter(|(batch, _, role)| *batch == "add-absence" && *role == Role::IndependentHeldOut)
            .all(|(_, case_id, _)| *case_id != plan::ACCEPT_INTENDED)
    );
}

#[test]
fn context_candidates_freeze_treatment_acceptance_and_installed_support() {
    let candidates = context_candidates();
    assert_eq!(candidates.len(), 4);
    for (id, area) in [
        (
            "instruction-mechanics-layout",
            "instruction mechanics/layout",
        ),
        ("stable-prefix-rendering", "stable-prefix rendering"),
        ("native-deferred-discovery", "native deferred discovery"),
        (
            "deterministic-aggregation",
            "deterministic Code Mode/native aggregation",
        ),
    ] {
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.id == id && candidate.area == area),
            "missing candidate {id}"
        );
    }
    for candidate in &candidates {
        assert!(!candidate.treatment.is_empty(), "{}", candidate.id);
        assert!(
            !candidate.recurring_measurement.is_empty(),
            "{}",
            candidate.id
        );
        assert!(
            !support_text(&candidate.support).is_empty(),
            "{}",
            candidate.id
        );
        assert!(
            !candidate.acceptance.uncertainty.is_empty(),
            "{}",
            candidate.id
        );
        for metric in ACCEPTANCE_METRICS {
            assert!(
                candidate.acceptance.metrics.contains(&metric),
                "{} misses {metric}",
                candidate.id
            );
        }
        for role in [
            Role::Intended,
            Role::SimilarUnsuitable,
            Role::BoundaryFailure,
            Role::IndependentHeldOut,
            Role::ProtectedOverlapping,
        ] {
            assert!(
                candidate.acceptance.roles.contains(&role),
                "{} misses {role:?}",
                candidate.id
            );
        }
        assert!(!candidate.observability.is_empty(), "{}", candidate.id);
        assert!(!candidate.owner_boundary.is_empty(), "{}", candidate.id);
        assert!(!candidate.prior_default.is_empty(), "{}", candidate.id);
        assert!(!candidate.reconsideration.is_empty(), "{}", candidate.id);
    }
    let deferred = candidates
        .iter()
        .find(|candidate| candidate.id == "native-deferred-discovery")
        .unwrap();
    match &deferred.support {
        Support::Unsupported { reason } => {
            assert!(reason.contains("tool_search=removed"), "{reason}");
            assert!(reason.contains("0.157.1"), "{reason}");
            assert!(
                deferred
                    .owner_boundary
                    .contains("no replacement discovery runtime"),
                "{}",
                deferred.owner_boundary
            );
        }
        Support::Qualified { .. } => panic!("deferred discovery is not supported on 0.157.1"),
    }
    for candidate in candidates
        .iter()
        .filter(|c| c.id != "native-deferred-discovery")
    {
        assert!(
            matches!(candidate.support, Support::Qualified { .. }),
            "{}",
            candidate.id
        );
    }
}

fn support_text(support: &Support) -> &str {
    match support {
        Support::Qualified { evidence } => evidence,
        Support::Unsupported { reason } => reason,
    }
}

#[test]
fn unselected_or_unexecuted_comparisons_stay_pending_and_are_never_adopted() {
    let selections = declared_comparisons();
    assert!(!selections.is_empty());
    for selection in selections {
        assert!(!selection.id.is_empty());
        assert!(!selection.gate.is_empty());
        assert!(!selection.batch.is_empty());
        assert_eq!(selection_state(None), "pending");
        assert_eq!(selection_state(Some("0")), "pending");
        assert_eq!(selection_state(Some("1")), "selected");
    }
    assert!(
        selections
            .iter()
            .any(|selection| selection.kind == comparison::Kind::AddAbsence)
    );
    assert!(
        selections
            .iter()
            .any(|selection| selection.kind == comparison::Kind::UpdateOld)
    );
    let (decision, rationale) = integration_decision(None);
    assert_eq!(decision, Decision::Inconclusive);
    assert!(rationale.contains("pending"), "{rationale}");
    for candidate in context_candidates() {
        let (decision, rationale) = integration_decision(None);
        assert_eq!(decision, Decision::Inconclusive, "{}", candidate.id);
        assert_ne!(decision, Decision::Adopt, "{}", candidate.id);
        assert!(!rationale.is_empty());
        assert!(!candidate.reconsideration.is_empty());
    }
}

#[test]
fn model_free_evaluation_flow_preserves_live_and_external_openspec_packages() {
    let workspace = workspace_root().canonicalize().unwrap();
    let skills = workspace.join(".agents").join("skills");
    let before = snapshot_external_packages(&skills);
    assert!(!before.is_empty(), "no live skill packages found");
    assert!(
        before.contains_key("project-verification"),
        "compared live package missing"
    );
    assert!(
        before.keys().any(|name| name.starts_with("openspec-")),
        "external OpenSpec packages missing"
    );

    let host = tempfile::Builder::new()
        .prefix("skill-context-eval-")
        .tempdir()
        .unwrap();
    let prepared = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["outcome-prepare", "--case", plan::NEGATIVE_CASE])
        .current_dir(host.path())
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(prepared["model_calls"], 0);

    let case = host.path().join("case");
    let control = host.path().join("control");
    let library = host.path().join("library");
    fs::create_dir(&case).unwrap();
    fs::create_dir(&control).unwrap();
    write_skill(&library, plan::OWNED_SKILL);
    fs::write(control.join("oracle.json"), "{\"case\":\"negative\"}").unwrap();
    fs::write(control.join("baseline.json"), "{\"arm\":\"baseline\"}").unwrap();
    let request = host.path().join("request.json");
    fs::write(
        &request,
        serde_json::to_vec(&json!({
            "source_root": workspace,
            "case_root": case,
            "control_root": control,
            "library_root": library,
            "session_marker": skills.join(plan::OWNED_SKILL).join("SKILL.md")
        }))
        .unwrap(),
    )
    .unwrap();
    let isolated = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "isolate", "--request"])
        .arg(&request)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        isolated.status.success(),
        "{}",
        String::from_utf8_lossy(&isolated.stderr)
    );
    let isolated: Value = serde_json::from_slice(&isolated.stdout).unwrap();
    assert_eq!(isolated["model_calls"], 0);
    assert_eq!(isolated["isolation"]["control_files_write"], "denied");

    let after = snapshot_external_packages(&skills);
    assert_eq!(before, after);
}

fn snapshot_external_packages(skills: &Path) -> BTreeMap<String, String> {
    let mut revisions = BTreeMap::new();
    for entry in fs::read_dir(skills).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name == plan::OWNED_SKILL || name.starts_with("openspec-")) {
            continue;
        }
        if !entry.path().join("SKILL.md").is_file() {
            continue;
        }
        revisions.insert(name, package::load(&entry.path()).unwrap().revision);
    }
    revisions
}
