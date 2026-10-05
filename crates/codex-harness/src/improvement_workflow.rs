//! The planning/implementation workflow of one improvement run.
//!
//! This module consumes the accepted owners instead of duplicating them: the
//! grounded intake owner turns one retained investigator result into board
//! outcomes, the installed OpenSpec CLI owns the candidate's planning
//! artifacts, the task-worktree owner allocates and verifies the candidate
//! branch, the visible executor dispatch owner opens every model
//! conversation, and the Beads board owns the card references. The controller
//! contributes only the deterministic bookkeeping between them:
//!
//! 1. a retained terminal investigator result is re-read from the run store
//!    and consumed through [`improvement_intake::intake`] against an evidence
//!    index the controller builds from retained owner evidence - never from
//!    investigator-supplied labels;
//! 2. an admitted or reconsidered card becomes the selected candidate, and a
//!    matching `existing` outcome for this run's declared card is reused;
//! 3. the candidate's own OpenSpec change is qualified inside its owned
//!    worktree; a missing change is scaffolded through the installed CLI and
//!    authored by a bounded planning conversation, and implementation is
//!    dispatched only after the change qualifies and, when the run's
//!    predeclared comparison policy binds an experiment selection, states
//!    that selection under its exact heading with the predeclared method and
//!    claim; a missing or substituted section keeps dependent work
//!    undispatched and the planning conversation authors it;
//! 4. the returned implementation checkout is validated against the exact
//!    committed base, the declared writable scope and the frozen planning
//!    artifacts, transferred onto the candidate branch and retained as
//!    `candidate-ready`.
//! 5. when this run's own hypothesis card retains an independently accepted
//!    `role=workload` solution whose exact change can be materialized onto
//!    the freshly allocated candidate branch, that solution is carried
//!    model-free as this candidate's implementation - the same content
//!    identity the activation lineage owner later verifies.
//!
//! Every conversation works in the dispatcher's own pooled slot checkout, so
//! the controller validates the *returned* checkout and then advances the
//! owned candidate branch to the committed revision. Nothing here merges into
//! the accepted mainline, records a benefit decision or applies a removal.

use super::*;
use harness_core::board_hypothesis::{self, BoundedImplementation};
use harness_core::improvement_intake::{
    self, ClaimKind, EvidenceIndex, EvidenceItem, EvidenceOwner, IntakeOutcome,
};
use harness_core::improvement_loop::{
    CandidateState, IntakeState, OutcomeRecord, RemovalGate, candidate_change_dir,
    candidate_change_name, changed_paths_within_scope, frozen_candidate_removal_digest,
};
use harness_core::improvement_policy::{
    ComparisonPolicy, EffectPath, ExperimentMethod, ExperimentSelection, parse_experiment_selection,
};
use harness_core::improvement_spec::{
    MeasurementReceipt, MeasurementScope, OpenSpec, PlanningReceipt, REMOVAL_PROPOSAL_CLAUSES,
    REMOVAL_PROPOSAL_GUIDE, REMOVAL_PROPOSAL_HEADING, RemovalProposalReceipt, Specification,
};
use harness_core::task_worktree::{self, CandidateCheckout, ReuseBlock, WorktreeReuse};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

/// The run-local qualified receipt of the selected candidate's own OpenSpec
/// change. The run's declared `planning.json` stays the frozen anchor receipt.
const CANDIDATE_PLANNING_FILE: &str = "candidate-planning.json";
/// The run's declared measurement scope: explicit local run data the
/// hypothesis's own OpenSpec change must state before any directed baseline
/// measurement. The operator places this `MeasurementScope` JSON in the run
/// directory beside the frozen spec, like every other declared local input.
const MEASUREMENT_SCOPE_FILE: &str = "measurement-scope.json";
/// The retained directed-measurement receipt written before the measured-pair
/// owner may direct the baseline attempt and revalidated on later passes.
const MEASUREMENT_RECEIPT_FILE: &str = "measurement-receipt.json";
/// Bounds for transferring the declared scope into the bounded planning
/// assignment: each text field, each evidence reference and the declared
/// reference count stay inside one native structured assignment.
const MAX_MEASUREMENT_FIELD_BYTES: usize = 1024;
const MAX_MEASUREMENT_EVIDENCE: usize = 8;
const MAX_MEASUREMENT_REFERENCE_BYTES: usize = 512;
/// Bounds for the deterministic evidence index the controller builds from the
/// declared local evidence root and its own retained attempt evidence.
const MAX_EVIDENCE_ROOT_FILES: usize = 24;
const MAX_RETAINED_EVIDENCE_ITEMS: usize = 32;
const MAX_EVIDENCE_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_LISTED_EVIDENCE: usize = 6;
const MAX_ASSIGNMENT_BYTES: usize = 256 * 1024;
const MAX_ASSIGNMENT_INPUTS: usize = 32;
/// The number of owner-assigned candidate locations one hypothesis may use:
/// the natural location plus the replacements for preserved ineligible or
/// foreign checkouts that are never touched.
const CANDIDATE_ALLOCATION_ATTEMPTS: u32 = 16;
/// The exact Markdown heading under which a hypothesis's own OpenSpec change
/// states the run's predeclared experiment selection, resolved by the
/// controller before any implementation or directed baseline measurement.
const EXPERIMENT_SELECTION_HEADING: &str = "## Experiment selection";
/// The clause labels a selection section states exactly once, each non-empty
/// and on one line.
const EXPERIMENT_SELECTION_CLAUSES: [&str; 8] = [
    "Method:",
    "Claim:",
    "Outcome:",
    "Rationale:",
    "Controls:",
    "Projection:",
    "Baseline:",
    "Stopping:",
];
/// Bound on one selection clause value the controller resolves.
const MAX_SELECTION_CLAUSE_BYTES: usize = 512;
/// Bound on one planning artifact the selection resolver reads.
const MAX_SELECTION_ARTIFACT_BYTES: u64 = 1024 * 1024;
/// Bound on the predeclared comparison policy file the selection gate reads.
const MAX_POLICY_FILE_BYTES: u64 = 256 * 1024;
/// The maximum retained `role=workload` solution records one hypothesis card
/// may contribute to the model-free carry; the activation lineage owner reads
/// the same bounded comment set.
const MAX_RETAINED_SOLUTIONS: usize = 8;

/// Advances the run as far as the recorded state and the dispatch gates
/// allow. Returns human-readable progress notes; a blocked or idle condition
/// is recorded in the cursor by this function.
pub(super) fn advance(run: &mut Run) -> io::Result<Vec<String>> {
    let mut notes = Vec::new();
    if run.cursor.phase == Phase::Stopped {
        return Ok(notes);
    }
    if let Some(candidate) = run.cursor.candidate.clone() {
        if candidate.is_ready() {
            retain_candidate_ready(run, &candidate, &mut notes)?;
            if run.spec.comparison.is_some() {
                advance_comparison(run, &mut notes)?;
            }
            return Ok(notes);
        }
        advance_candidate(run, &candidate, &mut notes)?;
        return Ok(notes);
    }
    advance_selection(run, &mut notes)?;
    if let Some(candidate) = run.cursor.candidate.clone() {
        if !candidate.is_ready() {
            advance_candidate(run, &candidate, &mut notes)?;
        } else {
            retain_candidate_ready(run, &candidate, &mut notes)?;
            if run.spec.comparison.is_some() {
                advance_comparison(run, &mut notes)?;
            }
        }
    }
    Ok(notes)
}

fn retain_candidate_ready(
    run: &mut Run,
    candidate: &CandidateState,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    // A later advance must not turn a recorded decision or confirmed
    // activation back into candidate-ready. Idle, blocked and stopped are
    // conditions of the current evidence, not a request to re-enter planning.
    if matches!(
        run.cursor.phase,
        Phase::CandidateReady
            | Phase::DecisionRecorded
            | Phase::ActivationConfirmed
            | Phase::Idle
            | Phase::Blocked
            | Phase::Stopped
    ) {
        return Ok(());
    }
    run.cursor.phase = Phase::CandidateReady;
    run.cursor.condition = None;
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!(
        "candidate-ready: hypothesis {} revision {} is retained for the measured-pair owner",
        candidate.hypothesis,
        candidate.revision.as_deref().unwrap_or("unknown")
    ));
    Ok(())
}

/// Engages the measured-pair owner only after the hypothesis's own OpenSpec
/// change states the run's declared measurement scope. The directed-measurement
/// receipt is retained before any baseline direction and revalidated against
/// the current change on every later pass, so a missing, incomplete or changed
/// declared scope can never direct `Phase::BaselineAttempt`.
fn advance_comparison(run: &mut Run, notes: &mut Vec<String>) -> io::Result<()> {
    if !ensure_measurement_qualified(run, notes)? {
        return Ok(());
    }
    super::improvement_comparison::advance(run, notes)
}

/// The measurement gate in front of the measured-pair owner. Returns whether
/// the comparison owner may be engaged; every refusal records the exact
/// missing artifact as the run's blocking condition.
fn ensure_measurement_qualified(run: &mut Run, notes: &mut Vec<String>) -> io::Result<bool> {
    // Mirrors the comparison owner's own phase gate: only these phases can
    // direct the baseline attempt, so the gate runs at exactly those points.
    if run.spec.comparison.is_none()
        || !matches!(
            run.cursor.phase,
            Phase::CandidateReady
                | Phase::BaselineAttempt
                | Phase::CandidateAttempt
                | Phase::Acceptance
                | Phase::Blocked
        )
    {
        return Ok(true);
    }
    let Some(candidate) = run.cursor.candidate.clone() else {
        return Ok(true);
    };
    if !candidate.is_ready() {
        return Ok(true);
    }
    let scope_path = run.store.root().join(MEASUREMENT_SCOPE_FILE);
    let scope = match declared_measurement_scope(run) {
        Ok(Some(scope)) => scope,
        Ok(None) => {
            block(
                run,
                notes,
                format!(
                    "no hypothesis measurement scope is declared: {} is missing; no directed baseline measurement is eligible - declare the MeasurementScope JSON there and resume",
                    scope_path.display()
                ),
            )?;
            return Ok(false);
        }
        Err(error) => {
            block(run, notes, error.to_string())?;
            return Ok(false);
        }
    };
    let openspec = OpenSpec::default();
    if let Some(path) = candidate.measurement_receipt.clone() {
        return revalidate_measurement_receipt(run, notes, &candidate, &openspec, &path, &scope);
    }
    let Some(checkout) = candidate.worktree.clone() else {
        block(
            run,
            notes,
            "the candidate allocation is missing, so the hypothesis's own OpenSpec change cannot be re-resolved; no directed baseline measurement is eligible"
                .to_owned(),
        )?;
        return Ok(false);
    };
    let target = candidate_specification(run, &checkout, &candidate);
    match openspec.begin_measurement(&target, &scope) {
        Ok(receipt) => {
            let path = run.store.root().join(MEASUREMENT_RECEIPT_FILE);
            write_json_atomic(&path, &receipt)?;
            let scope_digest = receipt.scope_digest.clone();
            if let Some(retained) = run.cursor.candidate.as_mut() {
                retained.measurement_receipt = Some(path.clone());
            }
            run.cursor.effect(
                EffectKind::MeasurementQualified,
                format!(
                    "candidate={} change={} artifact={} heading={} scope_digest={}",
                    candidate.hypothesis,
                    candidate.change,
                    scope.declaration_artifact.display(),
                    scope.declaration_heading,
                    &scope_digest[..16.min(scope_digest.len())]
                ),
            );
            run.store.save_cursor(&run.cursor)?;
            notes.push(format!(
                "measurement: change {} states the declared measurement scope ({} resolved artifact(s)); the receipt is retained before any baseline direction",
                candidate.change,
                receipt.artifacts.len()
            ));
            Ok(true)
        }
        Err(error) => {
            block(
                run,
                notes,
                format!(
                    "the hypothesis's own OpenSpec change {} does not state the declared measurement scope: {error}; no directed baseline measurement is eligible",
                    candidate.change
                ),
            )?;
            Ok(false)
        }
    }
}

/// Rebinds one retained directed-measurement receipt. The declared scope must
/// still be exactly the retained one and the hypothesis's own change must
/// still resolve to the same identity with its scope section stated; the
/// retained receipt is never rewritten by a revalidation.
fn revalidate_measurement_receipt(
    run: &mut Run,
    notes: &mut Vec<String>,
    candidate: &CandidateState,
    openspec: &OpenSpec,
    path: &Path,
    scope: &MeasurementScope,
) -> io::Result<bool> {
    if !path.is_file() {
        block(
            run,
            notes,
            format!(
                "the retained directed-measurement receipt at {} is missing; no directed baseline measurement is eligible before the hypothesis's own change is re-resolved",
                path.display()
            ),
        )?;
        return Ok(false);
    }
    let receipt: MeasurementReceipt = match read_json(path, MAX_RUN_SPEC_BYTES) {
        Ok(receipt) => receipt,
        Err(error) => {
            block(
                run,
                notes,
                format!(
                    "the retained directed-measurement receipt at {} is unreadable: {error}; no directed baseline measurement is eligible",
                    path.display()
                ),
            )?;
            return Ok(false);
        }
    };
    if &receipt.scope != scope {
        block(
            run,
            notes,
            format!(
                "the declared measurement scope changed after the directed-measurement receipt was retained for change {}; the retained baseline cannot be reused and no directed baseline measurement is eligible without a fresh declaration",
                candidate.change
            ),
        )?;
        return Ok(false);
    }
    match openspec.revalidate_measurement(&receipt) {
        Ok(()) => {
            notes.push(format!(
                "measurement: the retained receipt rebinds to change {} as it currently stands",
                candidate.change
            ));
            Ok(true)
        }
        Err(error) => {
            block(
                run,
                notes,
                format!(
                    "the retained directed-measurement receipt no longer rebinds to the hypothesis's own change {}: {error}; no directed baseline measurement is eligible",
                    candidate.change
                ),
            )?;
            Ok(false)
        }
    }
}

/// Reads the run's declared measurement scope. `Ok(None)` means the operator
/// declared none; an unreadable, incomplete or oversized declaration is an
/// explicit refusal naming the exact field.
fn declared_measurement_scope(run: &Run) -> io::Result<Option<MeasurementScope>> {
    let path = run.store.root().join(MEASUREMENT_SCOPE_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let scope: MeasurementScope = read_json(&path, MAX_RUN_SPEC_BYTES).map_err(|error| {
        invalid(format!(
            "the declared measurement scope at {} is unusable: {error}",
            path.display()
        ))
    })?;
    scope.validate().map_err(|error| {
        invalid(format!(
            "the declared measurement scope at {} is incomplete: {error}",
            path.display()
        ))
    })?;
    for (name, value) in [
        ("observed_problem", &scope.observed_problem),
        ("investigation_scope", &scope.investigation_scope),
        ("measurement_question", &scope.measurement_question),
        ("workload.operation", &scope.workload.operation),
        ("workload.contract", &scope.workload.contract),
        ("limits", &scope.limits),
    ] {
        if value.len() > MAX_MEASUREMENT_FIELD_BYTES {
            return Err(invalid(format!(
                "the declared measurement scope field {name} is {} bytes; the bounded planning transfer accepts at most {MAX_MEASUREMENT_FIELD_BYTES}",
                value.len()
            )));
        }
    }
    if scope.evidence_references.len() > MAX_MEASUREMENT_EVIDENCE {
        return Err(invalid(format!(
            "the declared measurement scope carries {} evidence references; the bounded planning transfer accepts at most {MAX_MEASUREMENT_EVIDENCE}",
            scope.evidence_references.len()
        )));
    }
    for reference in &scope.evidence_references {
        if reference.len() > MAX_MEASUREMENT_REFERENCE_BYTES {
            return Err(invalid(format!(
                "the declared measurement scope evidence reference {} is {} bytes; the bounded planning transfer accepts at most {MAX_MEASUREMENT_REFERENCE_BYTES}",
                reference,
                reference.len()
            )));
        }
    }
    Ok(Some(scope))
}

/// One experiment-selection section resolved from an already qualified
/// change. Resolving it applies nothing, and the frozen artifact digests must
/// still match the receipt, so a selection is never read from a drifted
/// change.
struct ResolvedExperimentSelection {
    artifact: PathBuf,
    section_digest: String,
    selection: ExperimentSelection,
}

/// The experiment selection the run's predeclared comparison policy binds.
/// `Ok(None)` means no readable policy declares one; a missing, unreadable or
/// invalid policy file stays with the comparison owner, which reports it at
/// its own gate, so this early resolver never duplicates that verdict. A
/// readable policy whose selection clause is unusable is an error: dependent
/// work stops with the exact cause instead of continuing without its declared
/// plan.
fn predeclared_experiment_selection(run: &Run) -> io::Result<Option<ExperimentSelection>> {
    let Some(comparison) = &run.spec.comparison else {
        return Ok(None);
    };
    let bytes = match fs::read(&comparison.policy) {
        Ok(bytes) if bytes.len() as u64 <= MAX_POLICY_FILE_BYTES => bytes,
        _ => return Ok(None),
    };
    let policy: ComparisonPolicy = match serde_json::from_slice(&bytes) {
        Ok(policy) => policy,
        Err(_) => return Ok(None),
    };
    if policy.declare().is_err() {
        return Ok(None);
    }
    match parse_experiment_selection(&policy.uncertainty) {
        Ok(selection) => Ok(selection),
        Err(error) => Err(invalid(format!(
            "the predeclared experiment selection is unusable: {error}"
        ))),
    }
}

/// Resolves the experiment-selection section a qualified change must state:
/// every clause present exactly once, non-empty and on one line, with the
/// declared method and claim path naming a bounded experiment unit.
fn resolve_experiment_selection(
    receipt: &PlanningReceipt,
) -> io::Result<ResolvedExperimentSelection> {
    let mut stated: Option<(PathBuf, String)> = None;
    for (path, digest) in &receipt.artifacts {
        if !path.starts_with(&receipt.change_root) || path == &receipt.change_root {
            return Err(invalid("a planning artifact escapes the selected change"));
        }
        let relative = path
            .strip_prefix(&receipt.change_root)
            .map_err(|_| invalid("a planning artifact escapes the selected change"))?;
        let bytes = fs::read(path).map_err(|error| {
            invalid(format!(
                "the planning artifact {} is unreadable: {error}",
                relative.display()
            ))
        })?;
        if bytes.len() as u64 > MAX_SELECTION_ARTIFACT_BYTES {
            return Err(invalid(format!(
                "the planning artifact {} exceeds the bounded planning contract",
                relative.display()
            )));
        }
        if build_identity::hash_bytes(&bytes) != *digest {
            return Err(invalid(format!(
                "the planning artifact {} changed after qualification; re-qualify the change before resolving its experiment selection",
                relative.display()
            )));
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            invalid(format!(
                "the planning artifact {} is not UTF-8 text",
                relative.display()
            ))
        })?;
        if let Some(section) = extract_experiment_selection_section(&text) {
            if let Some((first, _)) = &stated {
                let first = first.strip_prefix(&receipt.change_root).unwrap_or(first);
                return Err(invalid(format!(
                    "the experiment selection is stated in more than one resolved artifact ({} and {}); state it exactly once under '{EXPERIMENT_SELECTION_HEADING}'",
                    first.display(),
                    relative.display()
                )));
            }
            stated = Some((path.clone(), section));
        }
    }
    let Some((artifact, section)) = stated else {
        return Err(invalid(format!(
            "the change states no experiment selection: missing the section '{EXPERIMENT_SELECTION_HEADING}' in its resolved artifacts"
        )));
    };
    let clauses = experiment_selection_clauses(&section)?;
    let clause = |label: &str| clauses.get(label).cloned().unwrap_or_default();
    let method = ExperimentMethod::parse(clause("Method:").trim()).ok_or_else(|| {
        invalid(
            "the experiment selection Method clause is not bounded-replay, real-operation, agent-task, paired-implementations or sequence",
        )
    })?;
    let claim = EffectPath::parse(clause("Claim:").trim()).ok_or_else(|| {
        invalid(
            "the experiment selection Claim clause is not local-operation, agent-choice, task-strategy, repeated-use or size-only",
        )
    })?;
    let selection = ExperimentSelection {
        method,
        claim,
        outcome: clause("Outcome:"),
        rationale: clause("Rationale:"),
        controls: clause("Controls:"),
        projection: clause("Projection:"),
        baseline: clause("Baseline:"),
        stopping: clause("Stopping:"),
    };
    if let Some(problem) = selection.problem() {
        return Err(invalid(format!(
            "the experiment selection section is unusable: {problem}"
        )));
    }
    Ok(ResolvedExperimentSelection {
        artifact,
        section_digest: build_identity::hash_bytes(section.as_bytes()),
        selection,
    })
}

/// Extracts the selection section body: the lines after the exact heading
/// until the next heading of the same or higher level. A heading deeper than
/// the section heading stays inside it.
fn extract_experiment_selection_section(text: &str) -> Option<String> {
    let level = EXPERIMENT_SELECTION_HEADING
        .bytes()
        .take_while(|byte| *byte == b'#')
        .count();
    let mut lines = text
        .lines()
        .skip_while(|line| line.trim() != EXPERIMENT_SELECTION_HEADING);
    lines.next()?;
    let body: Vec<&str> = lines
        .take_while(|line| {
            let line = line.trim();
            let next_level = line.bytes().take_while(|byte| *byte == b'#').count();
            next_level == 0 || next_level > level || !line[next_level..].starts_with(' ')
        })
        .collect();
    Some(body.join("\n"))
}

/// Parses the clause values of one experiment-selection section. Every clause
/// must be stated exactly once, non-empty, on one line and within the bounded
/// clause size; a clause line may carry a Markdown bullet.
fn experiment_selection_clauses(section: &str) -> io::Result<BTreeMap<&'static str, String>> {
    let mut clauses: BTreeMap<&'static str, String> = BTreeMap::new();
    for line in section.lines() {
        let line = line.trim();
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line)
            .trim_start();
        for label in EXPERIMENT_SELECTION_CLAUSES {
            let Some(value) = line.strip_prefix(label) else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                return Err(invalid(format!(
                    "the experiment selection clause {label} is empty"
                )));
            }
            if value.len() > MAX_SELECTION_CLAUSE_BYTES {
                return Err(invalid(format!(
                    "the experiment selection clause {label} exceeds {MAX_SELECTION_CLAUSE_BYTES} bytes"
                )));
            }
            if clauses.insert(label, value.to_owned()).is_some() {
                return Err(invalid(format!(
                    "the experiment selection clause {label} is stated more than once"
                )));
            }
            break;
        }
    }
    for label in EXPERIMENT_SELECTION_CLAUSES {
        if !clauses.contains_key(label) {
            return Err(invalid(format!(
                "the experiment selection section is incomplete: missing the clause {label}"
            )));
        }
    }
    Ok(clauses)
}

/// The controller's verdict about the run's predeclared experiment selection.
enum SelectionFreeze {
    /// The run predeclares no selection; the gate does not apply.
    NotPredeclared,
    /// The qualified change states the predeclared selection.
    Frozen(ResolvedExperimentSelection),
    /// The change does not (yet) state it; the detail names the exact gap.
    Pending(String),
}

/// Verifies that the qualified change states the run's predeclared experiment
/// selection before implementation or measurement depends on it: the declared
/// method and claim path must match the predeclared binding exactly, and
/// every clause must be stated once. A missing, mismatched or unreadable
/// section stays pending with the exact detail; nothing is applied.
fn selection_freeze(run: &Run, receipt: &PlanningReceipt) -> io::Result<SelectionFreeze> {
    let predeclared = match predeclared_experiment_selection(run)? {
        Some(selection) => selection,
        None => return Ok(SelectionFreeze::NotPredeclared),
    };
    match resolve_experiment_selection(receipt) {
        Ok(resolved) => {
            if resolved.selection.method != predeclared.method
                || resolved.selection.claim != predeclared.claim
            {
                return Ok(SelectionFreeze::Pending(format!(
                    "the change states method={} claim={} but the run predeclares method={} claim={}; align the section with the predeclared selection before implementation",
                    resolved.selection.method.as_str(),
                    resolved.selection.claim.as_str(),
                    predeclared.method.as_str(),
                    predeclared.claim.as_str()
                )));
            }
            Ok(SelectionFreeze::Frozen(resolved))
        }
        Err(error) => Ok(SelectionFreeze::Pending(error.to_string())),
    }
}

/// The planning gate around the predeclared selection. `Ok(Ok(()))` means the
/// qualified change states it (or none is predeclared) and the planning
/// receipt may be retained; `Ok(Err(detail))` means dependent implementation
/// must wait for the selection section; an error means the run's own
/// predeclared clause is unusable and dependent work stops.
#[allow(clippy::type_complexity)]
fn ensure_experiment_selection(
    run: &Run,
    receipt: &PlanningReceipt,
    notes: &mut Vec<String>,
) -> io::Result<Result<(), String>> {
    let freeze = selection_freeze(run, receipt).map_err(|error| {
        invalid(format!(
            "the run's predeclared experiment selection is unusable: {error}; dependent work stops until the comparison policy is corrected"
        ))
    })?;
    match freeze {
        SelectionFreeze::NotPredeclared => Ok(Ok(())),
        SelectionFreeze::Frozen(resolved) => {
            notes.push(format!(
                "experiment selection: change {} states the predeclared selection method={} claim={} ({}#{}, digest {})",
                receipt.specification.change,
                resolved.selection.method.as_str(),
                resolved.selection.claim.as_str(),
                resolved
                    .artifact
                    .strip_prefix(&receipt.change_root)
                    .unwrap_or(&resolved.artifact)
                    .display(),
                EXPERIMENT_SELECTION_HEADING,
                &resolved.section_digest[..16.min(resolved.section_digest.len())]
            ));
            Ok(Ok(()))
        }
        SelectionFreeze::Pending(detail) => Ok(Err(detail)),
    }
}

// ---------------------------------------------------------------------------
// Selection: consume a retained investigator result through grounded intake.
// ---------------------------------------------------------------------------

fn advance_selection(run: &mut Run, notes: &mut Vec<String>) -> io::Result<()> {
    // A retained completed investigator result is consumed first: its
    // terminal outcome settles even when the surface is currently lost.
    if consume_investigator_result(run, notes)? {
        return Ok(());
    }
    let evidence = build_evidence(run)?;
    let facts = dispatch_facts_for(run, AttemptRole::Investigator)?;
    let gate = dispatch_gate(&run.cursor, AttemptRole::Investigator, &facts);
    if evidence.index.is_empty() {
        return match gate {
            // Nothing to investigate from: the honest state is idle, with the
            // next evidence source named, and no model work is started.
            DispatchGate::Ready => idle(
                run,
                notes,
                "no retained evidence is available: declare an evidence root in the run inputs or retain completed attempt evidence, then resume - no model work is started without grounding".to_owned(),
            ),
            // A blocked gate (missing model inputs, lost visibility, an
            // unresolved attempt) is the actionable condition and stays the
            // recorded blocker.
            DispatchGate::Blocked { reason } => {
                run.cursor.effect(EffectKind::DispatchRefused, &reason);
                run.cursor.block(reason);
                run.store.save_cursor(&run.cursor)?;
                Ok(())
            }
        };
    }
    // An unchanged evidence set after a recorded round does not justify
    // another model round; the recorded idle/deferred condition stands.
    if let Some(intake) = &run.cursor.intake
        && intake.evidence_digest == evidence.digest
    {
        if run.cursor.phase != Phase::Idle && run.cursor.condition.is_none() {
            idle(
                run,
                notes,
                "the retained evidence set is unchanged since the last investigator round; awaiting fresh evidence or a new decision - no model work is started".to_owned(),
            )?;
        }
        return Ok(());
    }
    match gate {
        DispatchGate::Ready => {
            dispatch_investigator(run, &evidence, notes)?;
        }
        DispatchGate::Blocked { reason } => {
            run.cursor.effect(EffectKind::DispatchRefused, &reason);
            run.cursor.block(reason);
            run.store.save_cursor(&run.cursor)?;
        }
    }
    Ok(())
}

/// Consumes one retained completed investigator result that differs from the
/// already-consumed one. The retained terminal message may frame the report
/// in investigator prose; only its single unambiguous final JSON payload is
/// parsed (`improvement_intake::parse_terminal_report`) and the raw message
/// digest stays the authoritative identity. Returns whether a result was
/// handled (consumed or explicitly blocked), so the caller never falls
/// through to a new dispatch while unconsumed evidence exists.
fn consume_investigator_result(run: &mut Run, notes: &mut Vec<String>) -> io::Result<bool> {
    let latest = run
        .cursor
        .attempts
        .iter()
        .rev()
        .find(|attempt| {
            attempt.role == AttemptRole::Investigator && attempt.state == AttemptState::Completed
        })
        .cloned();
    let Some(attempt) = latest else {
        return Ok(false);
    };
    let Some(result) = retained_result(&attempt) else {
        let reason = format!(
            "the completed investigator attempt {} retained no terminal result, so no structured report can be consumed; the attempt is not replayed - dispatch a fresh bounded round or retain the result",
            attempt.id
        );
        refused(run, notes, reason)?;
        return Ok(true);
    };
    let bytes = match fs::read(&result) {
        Ok(bytes) if bytes.len() as u64 <= improvement_intake::MAX_REPORT_BYTES => bytes,
        Ok(bytes) => {
            let reason = format!(
                "the retained investigator result at {} is {} bytes; the intake report bound is {} bytes, so it cannot be consumed",
                result.display(),
                bytes.len(),
                improvement_intake::MAX_REPORT_BYTES
            );
            refused(run, notes, reason)?;
            return Ok(true);
        }
        Err(error) => {
            let reason = format!(
                "the retained investigator result at {} is unreadable: {error}; no intake round runs",
                result.display()
            );
            refused(run, notes, reason)?;
            return Ok(true);
        }
    };
    let result_sha256 = build_identity::hash_bytes(&bytes);
    if run
        .cursor
        .intake
        .as_ref()
        .is_some_and(|intake| intake.result_sha256 == result_sha256)
    {
        return Ok(false);
    }
    let report = match improvement_intake::parse_terminal_report(&bytes) {
        Ok(report) => report,
        Err(error) => {
            let reason = format!(
                "the retained investigator result at {} is not a bounded schema-1 investigator report ({error}); no candidate is admitted from unreadable output and no model round is started",
                result.display()
            );
            refused(run, notes, reason)?;
            return Ok(true);
        }
    };
    let evidence = build_evidence(run)?;
    let outcomes = match improvement_intake::intake(
        &run.spec.board.bd,
        &run.spec.board.project,
        &report,
        &evidence.index,
    ) {
        Ok(outcomes) => outcomes,
        Err(error) => {
            let reason = format!(
                "grounded intake could not be completed: {error}; the board failure is reported instead of being replaced by a local journal, and no model round is started"
            );
            refused(run, notes, reason)?;
            return Ok(true);
        }
    };
    let mut records = Vec::new();
    for outcome in &outcomes.outcomes {
        records.push(outcome_record(outcome)?);
    }
    run.cursor.record_intake(IntakeState {
        result_sha256: result_sha256.clone(),
        evidence_digest: evidence.digest.clone(),
        outcomes: records,
        consumed_ms: now_ms(),
    })?;
    run.cursor.effect(
        EffectKind::IntakeConsumed,
        format!(
            "attempt={} result={} sha256={} outcomes={}",
            attempt.id,
            result.display(),
            &result_sha256[..16.min(result_sha256.len())],
            outcomes.outcomes.len()
        ),
    );
    notes.push(format!(
        "intake: consumed the retained investigator result of attempt {} ({} outcome(s))",
        attempt.id,
        outcomes.outcomes.len()
    ));

    let selected = select_outcome(&run.spec, &outcomes.outcomes);
    match selected {
        Some((card, removal_required)) => {
            select_candidate(run, &card, removal_required, notes)?;
        }
        None => {
            let reason = idle_reason(&outcomes.outcomes);
            run.store.save_cursor(&run.cursor)?;
            idle(run, notes, reason)?;
        }
    }
    Ok(true)
}

fn retained_result(attempt: &Attempt) -> Option<PathBuf> {
    let retained = attempt.retained.as_ref()?;
    retained.result.clone()
}

fn outcome_record(outcome: &IntakeOutcome) -> io::Result<OutcomeRecord> {
    match outcome {
        IntakeOutcome::Admitted {
            id,
            removal_required,
            ..
        } => OutcomeRecord::new(
            "admitted",
            Some(id),
            &format!(
                "a new hypothesis card was created{}{}",
                if *removal_required {
                    "; the candidate applies a declared removal and needs the informed decision"
                } else {
                    ""
                },
                ""
            ),
        ),
        IntakeOutcome::Existing { id, status, .. } => OutcomeRecord::new(
            "existing",
            Some(id),
            &format!("an active card matches the proposal (status {status})"),
        ),
        IntakeOutcome::ReusedRejection {
            id,
            experiment,
            reason,
            ..
        } => OutcomeRecord::new(
            "reused-rejection",
            Some(id),
            &format!(
                "the same-condition rejection is reused (experiment {}, reason {})",
                experiment.as_deref().unwrap_or("none"),
                reason.as_deref().unwrap_or("none")
            ),
        ),
        IntakeOutcome::ReusedInconclusive {
            id,
            experiment,
            reason,
            ..
        } => OutcomeRecord::new(
            "reused-inconclusive",
            Some(id),
            &format!(
                "the same-condition inconclusive result is reused (experiment {}, reason {})",
                experiment.as_deref().unwrap_or("none"),
                reason.as_deref().unwrap_or("none")
            ),
        ),
        IntakeOutcome::Reconsidered {
            id,
            basis,
            prior_outcome,
            ..
        } => OutcomeRecord::new(
            "reconsidered",
            Some(id),
            &format!("a fresh basis {basis} reopened the card (prior outcome {prior_outcome})"),
        ),
        IntakeOutcome::NoChange { reason } => OutcomeRecord::new(
            "no-change",
            None,
            &format!("no change is supported: {reason}"),
        ),
        IntakeOutcome::ReuseSuffices { existing } => OutcomeRecord::new(
            "reuse-suffices",
            None,
            &format!("an existing route {existing} satisfies the evidenced need"),
        ),
        IntakeOutcome::Deferred { reason, next } => {
            OutcomeRecord::new("deferred", None, &format!("{reason}; next check: {next}"))
        }
        IntakeOutcome::Refused { reasons } => {
            OutcomeRecord::new("refused", None, &reasons.join("; "))
        }
        IntakeOutcome::Idle { reason } => OutcomeRecord::new("idle", None, reason),
    }
}

/// The candidate selection rule: admission or reconsideration selects the new
/// hypothesis; a matching `existing` outcome selects this run's declared card
/// (its own change is the run's frozen planning anchor); every other outcome
/// leaves the loop idle without duplicate work.
fn select_outcome(spec: &RunSpec, outcomes: &[IntakeOutcome]) -> Option<(String, bool)> {
    for outcome in outcomes {
        match outcome {
            IntakeOutcome::Admitted {
                id,
                removal_required,
                ..
            } => return Some((id.clone(), *removal_required)),
            IntakeOutcome::Reconsidered { id, .. } => return Some((id.clone(), false)),
            _ => {}
        }
    }
    for outcome in outcomes {
        if let IntakeOutcome::Existing { id, status, .. } = outcome
            && id == &spec.hypothesis_item
            && !matches!(status.as_str(), "closed" | "deferred")
        {
            return Some((id.clone(), spec.removal.is_some()));
        }
    }
    None
}

fn idle_reason(outcomes: &[IntakeOutcome]) -> String {
    if let Some(IntakeOutcome::Idle { reason }) = outcomes.first() {
        return reason.clone();
    }
    let mut parts = Vec::new();
    for outcome in outcomes.iter().take(4) {
        parts.push(match outcome {
            IntakeOutcome::Deferred { reason, next } => {
                format!("deferred: {reason} (next: {next})")
            }
            IntakeOutcome::Refused { reasons } => format!("refused: {}", reasons.join("; ")),
            IntakeOutcome::ReusedRejection { id, .. } => {
                format!("reused prior rejection of {id}")
            }
            IntakeOutcome::ReusedInconclusive { id, .. } => {
                format!("reused prior inconclusive result of {id}")
            }
            IntakeOutcome::NoChange { reason } => format!("no change: {reason}"),
            IntakeOutcome::ReuseSuffices { existing } => {
                format!("reuse of {existing} suffices")
            }
            IntakeOutcome::Existing { id, status, .. } => {
                format!("existing card {id} ({status}) continues outside this run")
            }
            IntakeOutcome::Admitted { id, .. } | IntakeOutcome::Reconsidered { id, .. } => {
                format!("candidate {id} requires its own run authority")
            }
            IntakeOutcome::Idle { reason } => reason.clone(),
        });
    }
    format!(
        "no grounded candidate remains: {}; awaiting fresh evidence, an authorized decision or a new run",
        parts.join("; ")
    )
}

fn idle(run: &mut Run, notes: &mut Vec<String>, reason: String) -> io::Result<()> {
    run.cursor.phase = Phase::Idle;
    run.cursor.condition = Some(reason.clone());
    run.cursor.effect(EffectKind::IdleRecorded, &reason);
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!("idle: {reason}"));
    Ok(())
}

fn select_candidate(
    run: &mut Run,
    card: &str,
    removal_required: bool,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let snapshot =
        match board_hypothesis::load_card(&run.spec.board.bd, &run.spec.board.project, card) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return refused(
                    run,
                    notes,
                    format!("the selected hypothesis card {card} is not readable: {error}"),
                );
            }
        };
    if !snapshot.labels.iter().any(|label| label == "hypothesis") {
        return refused(
            run,
            notes,
            format!("the selected card {card} is not a hypothesis card"),
        );
    }
    if matches!(snapshot.status.as_str(), "closed" | "deferred") {
        let reason = format!(
            "the selected hypothesis card {card} is {}; a closed or deferred investigation needs a recorded reconsideration basis before implementation",
            snapshot.status
        );
        return refused(run, notes, reason);
    }
    let Some(admission) = board_hypothesis::parse_admission(&snapshot.description) else {
        let reason = format!(
            "the selected hypothesis card {card} carries no recognized admission record; record its mechanism, conditions, acceptance and spec reference before implementation"
        );
        return refused(run, notes, reason);
    };
    let reference = admission.spec.unwrap_or_default();
    let change = match candidate_change_name(&reference) {
        Ok(change) => change,
        Err(reason) => {
            return refused(run, notes, reason);
        }
    };
    let mut candidate = CandidateState::new(card, &change)?;
    candidate.removal_required = removal_required;
    if removal_required {
        let board_comments = comments(&run.spec)?;
        candidate.removal_frozen = frozen_candidate_removal_digest(card, &board_comments);
    }
    run.cursor.select_candidate(candidate)?;
    run.cursor.phase = Phase::Planning;
    run.cursor.condition = None;
    run.cursor.effect(
        EffectKind::CandidateSelected,
        format!(
            "hypothesis={card} change={change}{}",
            if removal_required {
                " removal-required=true"
            } else {
                ""
            }
        ),
    );
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!(
        "candidate: selected hypothesis {card} for its own OpenSpec change {change}"
    ));
    Ok(())
}

// ---------------------------------------------------------------------------
// Evidence index: retained owner evidence only.
// ---------------------------------------------------------------------------

struct Evidence {
    index: EvidenceIndex,
    digest: String,
    listing: Vec<String>,
    root: Option<PathBuf>,
}

/// Builds the controller-owned evidence index: the declared local evidence
/// root's retained files (read through the rollout/build-identity owners) and
/// the run's own retained attempt receipts and terminal results. Locators are
/// derived from the retained paths, so an investigator can cite only items
/// that actually exist.
fn build_evidence(run: &Run) -> io::Result<Evidence> {
    let mut items: Vec<EvidenceItem> = Vec::new();
    let mut listing: Vec<String> = Vec::new();
    if let Some(root) = &run.spec.evidence_root {
        for (relative, path) in walk_evidence_root(root)? {
            let locator = match harness_core::improvement_loop::evidence_locator(&relative) {
                Some(locator) => locator,
                None => continue,
            };
            let item = if relative.to_ascii_lowercase().ends_with(".jsonl") {
                EvidenceItem::read_rollout(&locator, &path)?
            } else {
                match EvidenceItem::read_source(&locator, &run.spec.run, &path) {
                    Ok(item) => item,
                    Err(error) => EvidenceItem::new(
                        &locator,
                        EvidenceOwner::Source,
                        ClaimKind::Inferred,
                        &format!("the retained file {relative} could not be captured"),
                        &[format!("{error}")],
                        &[],
                    )?,
                }
            };
            listing.push(summarize(&item));
            items.push(item);
            if items.len() >= MAX_EVIDENCE_ROOT_FILES {
                break;
            }
        }
    }
    let mut retained: Vec<EvidenceItem> = Vec::new();
    for attempt in run.cursor.attempts.iter().rev() {
        let Some(retained_evidence) = &attempt.retained else {
            continue;
        };
        let locator = format!("run:{}/receipt", attempt.id);
        retained.push(EvidenceItem::new(
            &locator,
            EvidenceOwner::AuthorizedWork,
            ClaimKind::Observed,
            &format!(
                "retained native receipt of attempt {} role={} state={} sha256={}",
                attempt.id,
                attempt.role.as_str(),
                attempt.state.as_str(),
                &retained_evidence.receipt_sha256[..16.min(retained_evidence.receipt_sha256.len())]
            ),
            &[],
            &[],
        )?);
        if let (Some(_result), Some(sha)) =
            (&retained_evidence.result, &retained_evidence.result_sha256)
        {
            retained.push(EvidenceItem::new(
                &format!("run:{}/result", attempt.id),
                EvidenceOwner::AuthorizedWork,
                ClaimKind::Observed,
                &format!(
                    "retained terminal result of attempt {} role={} sha256={}",
                    attempt.id,
                    attempt.role.as_str(),
                    &sha[..16.min(sha.len())]
                ),
                &[],
                &[],
            )?);
        }
        if retained.len() >= MAX_RETAINED_EVIDENCE_ITEMS {
            break;
        }
    }
    retained.reverse();
    for item in retained {
        listing.push(summarize(&item));
        items.push(item);
    }
    let digest = index_digest(&items);
    let index = EvidenceIndex::new(items)?;
    Ok(Evidence {
        index,
        digest,
        listing,
        root: run.spec.evidence_root.clone(),
    })
}

fn summarize(item: &EvidenceItem) -> String {
    let partial = if item.is_partial() { " partial" } else { "" };
    format!(
        "{} [{} {}{}]",
        item.locator,
        item.owner.as_str(),
        item.kind.as_str(),
        partial
    )
}

fn index_digest(items: &[EvidenceItem]) -> String {
    let mut text = String::new();
    for item in items {
        text.push_str(&format!(
            "{}|{}|{}|{}|{}|{}\n",
            item.locator,
            item.owner.as_str(),
            item.kind.as_str(),
            item.coverage,
            item.errors.join(";"),
            item.warnings.join(";")
        ));
    }
    build_identity::hash_bytes(text.as_bytes())
}

/// Deterministically walks the declared evidence root: sorted relative paths,
/// bounded file count, repository metadata and oversized files skipped.
fn walk_evidence_root(root: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    if !root.is_dir() {
        return Ok(files);
    }
    let mut stack = vec![root.to_path_buf()];
    let mut visited = 0_usize;
    while let Some(directory) = stack.pop() {
        if visited >= MAX_EVIDENCE_ROOT_FILES * 8 {
            break;
        }
        visited += 1;
        let mut entries: Vec<PathBuf> = fs::read_dir(&directory)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        entries.sort();
        for path in entries {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.starts_with('.') || matches!(name.as_str(), "target" | "node_modules") {
                continue;
            }
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if !metadata.is_file()
                || metadata.len() == 0
                || metadata.len() > MAX_EVIDENCE_FILE_BYTES
            {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            files.push((relative, path));
        }
    }
    files.sort();
    files.truncate(MAX_EVIDENCE_ROOT_FILES);
    Ok(files)
}

// ---------------------------------------------------------------------------
// Candidate stages: allocation, planning and implementation.
// ---------------------------------------------------------------------------

fn advance_candidate(
    run: &mut Run,
    candidate: &CandidateState,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let mut candidate = candidate.clone();
    ensure_allocation(run, &mut candidate, notes)?;
    run.cursor.candidate = Some(candidate.clone());
    run.store.save_cursor(&run.cursor)?;
    // A legacy allocation that was not reconciled still names a missing path
    // inside the protected run state. Planning there would scaffold or dispatch
    // against the wrong checkout, so dependent work waits without a model replay.
    if candidate.worktree.as_ref().is_none_or(|checkout| {
        checkout.path.starts_with(run.store.root()) && !checkout.path.is_dir()
    }) {
        return Ok(());
    }
    if candidate.planning_receipt.is_none() {
        ensure_planning(run, &mut candidate, notes)?;
        run.cursor.candidate = Some(candidate.clone());
        run.store.save_cursor(&run.cursor)?;
    }
    if candidate.planning_receipt.is_some() && !candidate.is_ready() {
        ensure_implementation(run, &mut candidate, notes)?;
        run.cursor.candidate = Some(candidate.clone());
        run.store.save_cursor(&run.cursor)?;
        if candidate.is_ready() {
            retain_candidate_ready(run, &candidate, notes)?;
        }
    }
    Ok(())
}

fn block(run: &mut Run, notes: &mut Vec<String>, reason: String) -> io::Result<()> {
    run.cursor.effect(EffectKind::DispatchRefused, &reason);
    run.cursor.block(reason.clone());
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!("blocked: {reason}"));
    Ok(())
}

/// A content outcome the loop cannot turn into work: unsupported evidence,
/// refused output or a mismatched source. It is recorded as idle with the
/// exact reason and never triggers filler model work; a later resume can
/// re-evaluate it after the missing fact is supplied.
fn refused(run: &mut Run, notes: &mut Vec<String>, reason: String) -> io::Result<()> {
    idle(run, notes, reason)
}

/// Binds the candidate branch/worktree to the admitted Beads card and the
/// exact committed base. The worktree lives in the run's own candidate area
/// beside the run root, never inside it: the run root is protected run state,
/// and a candidate nested under it makes every declared source scope overlap
/// that state, which the supervisor gate correctly refuses. An existing
/// recorded allocation is kept for this candidate; a pre-existing path is
/// reused only through the worktree owner's read-only eligibility verdict and
/// never forced. When the owner-assigned location already holds an ineligible
/// or foreign checkout - an active consumer, local or untracked changes,
/// commits beyond the recorded revision, a detached tree or another task's
/// worktree - that checkout is left exactly as it stands and the next owned
/// location in the candidate area is used instead; only when no owned location
/// remains is the unavailable prerequisite reported. A recorded legacy
/// allocation that still nests inside the run state cannot reach its own
/// planner, so an inactive, verified, preserved one is relocated through the
/// worktree owner's own Git operation and anything else is refused without
/// touching it. A move whose board publication or cursor save was interrupted
/// is reconciled on the next resume only when Git registers that same branch,
/// base and revision at the owner-assigned destination; ambiguous, dirty,
/// active or mismatched state is not adopted.
fn ensure_allocation(
    run: &mut Run,
    candidate: &mut CandidateState,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let area = match candidate_checkout_root(run) {
        Ok(root) => root,
        Err(reason) => return refused(run, notes, reason),
    };
    if let Some(checkout) = candidate.worktree.clone() {
        // A recorded allocation is kept exactly as it stands; only the legacy
        // geometry that nests it inside the protected run state is relocated,
        // and only while this candidate still needs its planning conversation.
        if candidate.planning_receipt.is_some() || !checkout.path.starts_with(run.store.root()) {
            return Ok(());
        }
        if !run.cursor.attempts_requiring_reconciliation().is_empty() {
            // An unresolved or unknown attempt keeps its allocation; the
            // recorded planning state owns the exact reconciliation on this
            // resume.
            return Ok(());
        }
        let path = area.join(&candidate.hypothesis);
        return relocate_recorded_allocation(run, candidate, checkout, &path, notes);
    }
    let active = run
        .cursor
        .attempts
        .iter()
        .any(|attempt| attempt.state.is_in_flight());
    let mut preserved: Vec<String> = Vec::new();
    let mut allocated: Option<CandidateCheckout> = None;
    for ordinal in 1..=CANDIDATE_ALLOCATION_ATTEMPTS {
        let path = if ordinal == 1 {
            area.join(&candidate.hypothesis)
        } else {
            area.join(format!("{}-{ordinal}", candidate.hypothesis))
        };
        let branch = if ordinal == 1 {
            format!("improve/{}/{}", run.spec.run, candidate.hypothesis)
        } else {
            format!(
                "improve/{}/{}-{ordinal}",
                run.spec.run, candidate.hypothesis
            )
        };
        if !path.exists() {
            match task_worktree::allocate_candidate_checkout(
                &run.spec.project,
                &path,
                &branch,
                &run.spec.base_revision,
            ) {
                Ok(checkout) => allocated = Some(checkout),
                Err(error) => {
                    let reason = format!(
                        "the candidate branch {branch} could not be allocated from the frozen base {} in {}: {error}",
                        run.spec.base_revision,
                        path.display()
                    );
                    return refused(run, notes, reason);
                }
            }
            break;
        }
        match task_worktree::worktree_reuse(
            &run.spec.project,
            &path,
            &run.spec.project,
            &run.spec.base_revision,
            active,
        )? {
            WorktreeReuse::Eligible { revision } => {
                // Keep the existing allocation's own dedicated branch; a tree
                // without one is not an owned allocation and is never adopted
                // under this run's branch name.
                let existing = match git_text(&path, &["rev-parse", "--abbrev-ref", "HEAD"]) {
                    Ok(branch) if branch != "HEAD" && !branch.trim().is_empty() => branch,
                    Ok(_) => {
                        preserved.push(format!(
                            "{} (detached HEAD: a detached tree is not a dedicated candidate branch)",
                            path.display()
                        ));
                        continue;
                    }
                    Err(error) => {
                        let reason = format!(
                            "the preserved candidate worktree {} branch could not be read: {error}",
                            path.display()
                        );
                        return refused(run, notes, reason);
                    }
                };
                allocated = Some(CandidateCheckout {
                    source: run.spec.project.clone(),
                    path: path.clone(),
                    branch: existing,
                    base: revision.clone(),
                    revision,
                });
                break;
            }
            WorktreeReuse::Blocked { kind, reason } => {
                if active || kind == ReuseBlock::CurrentCheckout {
                    let reason = format!(
                        "the recorded candidate worktree {} cannot be reused ({kind:?}): {reason}",
                        path.display()
                    );
                    return refused(run, notes, reason);
                }
                preserved.push(format!("{} ({kind:?}: {reason})", path.display()));
            }
        }
    }
    let Some(checkout) = allocated else {
        let reason = format!(
            "no owned candidate allocation remains for hypothesis {}: every owner-assigned location is occupied and left untouched ({})",
            candidate.hypothesis,
            preserved.join("; ")
        );
        return refused(run, notes, reason);
    };
    if let Err(error) = verify_candidate_base(run, &checkout) {
        return refused(run, notes, error);
    }
    board_hypothesis::record_implementation(
        &run.spec.board.bd,
        &run.spec.board.project,
        &candidate.hypothesis,
        &BoundedImplementation {
            role: board_hypothesis::HypothesisRole::Candidate,
            branch: checkout.branch.clone(),
            base: checkout.base.clone(),
            revision: checkout.revision.clone(),
            worktree: checkout.path.to_string_lossy().into_owned(),
            runtime: None,
            baseline_runtime: None,
        },
    )
    .map_err(|error| {
        invalid(format!(
            "the candidate allocation could not be recorded on hypothesis card {}: {error}",
            candidate.hypothesis
        ))
    })?;
    let preserved_note = if preserved.is_empty() {
        String::new()
    } else {
        format!(" preserved {}", preserved.join("; "))
    };
    run.cursor.effect(
        EffectKind::CandidateAllocated,
        format!(
            "hypothesis={} branch={} base={} worktree={}{preserved_note}",
            candidate.hypothesis,
            checkout.branch,
            checkout.base,
            checkout.path.display()
        ),
    );
    notes.push(format!(
        "allocation: branch {} at {} in {}",
        checkout.branch,
        checkout.base,
        checkout.path.display()
    ));
    if !preserved.is_empty() {
        notes.push(format!(
            "allocation: left {} preserved checkout(s) untouched: {}",
            preserved.len(),
            preserved.join("; ")
        ));
    }
    candidate.worktree = Some(checkout);
    Ok(())
}

/// The owner-assigned candidate area of one run: a sibling directory named
/// after the run root. Candidate worktrees are Git checkouts of the run's
/// project, so they must stay outside the run root, which holds only the
/// protected run state (spec, cursor, owner record, retained assignments and
/// receipts).
fn candidate_checkout_root(run: &Run) -> Result<PathBuf, String> {
    let root = run.store.root();
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            format!(
                "the run directory {} names no candidate area; start the run in a named directory so its candidate worktrees stay outside the protected run state",
                root.display()
            )
        })?;
    let parent = root
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            format!(
                "the run directory {} has no parent directory for its candidate area",
                root.display()
            )
        })?;
    Ok(parent.join(format!("{name}-candidates")))
}

/// Relocates one recorded legacy allocation out of the protected run state
/// into the owner-assigned candidate area, preserving its branch, revision and
/// commits. The worktree owner's read-only verdict must first prove the
/// allocation is registered to the run's project, clean and exactly at its
/// recorded revision; Git's own `worktree move` then carries it over. A move
/// that already finished, while the cursor and board still name the old path,
/// is reconciled only when that same identity is registered at the destination.
/// Every other state is refused without another move, reset or deletion.
fn relocate_recorded_allocation(
    run: &mut Run,
    candidate: &mut CandidateState,
    checkout: CandidateCheckout,
    destination: &Path,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    if !checkout.path.exists() {
        return reconcile_interrupted_relocation(
            run,
            candidate,
            &checkout,
            destination,
            notes,
            None,
        );
    }
    match task_worktree::worktree_reuse(
        &run.spec.project,
        &checkout.path,
        &run.spec.project,
        &checkout.revision,
        false,
    )? {
        WorktreeReuse::Eligible { .. } => {}
        WorktreeReuse::Blocked { kind, reason } => {
            let reason = format!(
                "the recorded candidate worktree {} lies inside the protected run state and is not relocatable ({kind:?}): {reason}; the allocation and its commits are left untouched",
                checkout.path.display()
            );
            return refused(run, notes, reason);
        }
    }
    if destination.exists() {
        let reason = format!(
            "the recorded candidate worktree {} lies inside the protected run state and its owner-assigned location {} already exists; both are left untouched",
            checkout.path.display(),
            destination.display()
        );
        return refused(run, notes, reason);
    }
    let (Some(from), Some(to)) = (checkout.path.to_str(), destination.to_str()) else {
        let reason = format!(
            "the candidate worktree {} lies inside the protected run state and its path is not Unicode; it is left untouched",
            checkout.path.display()
        );
        return refused(run, notes, reason);
    };
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let output = Command::new("git")
        .args(["worktree", "move", from, to])
        .current_dir(&run.spec.project)
        .output()
        .map_err(|error| invalid(format!("git worktree move: {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return settle_failed_worktree_move(run, candidate, &checkout, destination, &stderr, notes);
    }
    let relocated = CandidateCheckout {
        path: destination.to_path_buf(),
        ..checkout
    };
    if let Err(error) = task_worktree::verify_candidate_checkout(&relocated) {
        // The move itself completed, so the recorded allocation follows the
        // worktree; dependent work still waits for a consistent allocation.
        candidate.worktree = Some(relocated);
        let reason = format!(
            "the candidate worktree was moved out of the protected run state but does not verify: {error}; dependent work is refused until the allocation is consistent"
        );
        return refused(run, notes, reason);
    }
    publish_relocated_allocation(
        run,
        candidate,
        relocated,
        notes,
        RelocationPublication::Moved,
    )
}

/// Why a failed `git worktree move` is not treated as success, an evidenced
/// non-move, or an ambiguous partial effect.
struct FailedMoveObservation {
    old_exists: bool,
    old_registered: bool,
    destination_exists: bool,
    destination_registered: bool,
    destination_matches_identity: bool,
}

enum FailedMoveClassification {
    /// Git reported failure, but the registered destination is the requested move.
    Completed,
    /// The recorded allocation is still registered at the old path.
    Unmoved { reason: String },
    /// The observed paths do not prove the allocation was unchanged.
    Ambiguous { reason: String },
}

/// Classifies a failed Git move from what is actually registered afterwards.
/// A partial effect is never described as untouched: that word is reserved
/// for the pre-move refusal, which has not invoked Git.
fn classify_failed_worktree_move(
    old: &Path,
    stderr: &str,
    observed: &FailedMoveObservation,
) -> FailedMoveClassification {
    if !observed.old_exists
        && !observed.old_registered
        && observed.destination_exists
        && observed.destination_registered
        && observed.destination_matches_identity
    {
        return FailedMoveClassification::Completed;
    }
    if observed.old_exists && observed.old_registered && !observed.destination_registered {
        return FailedMoveClassification::Unmoved {
            reason: format!(
                "the recorded candidate worktree {} could not be relocated out of the protected run state: {stderr}; it is still registered at that path and its commits were not moved",
                old.display()
            ),
        };
    }
    FailedMoveClassification::Ambiguous {
        reason: format!(
            "the recorded candidate worktree {} could not be relocated out of the protected run state: {stderr}; observed old_exists={} old_registered={} destination_exists={} destination_registered={} destination_identity_matches={}; this failure is not claimed to have left the allocation unchanged, because that was not established, and nothing was reset or deleted",
            old.display(),
            observed.old_exists,
            observed.old_registered,
            observed.destination_exists,
            observed.destination_registered,
            observed.destination_matches_identity
        ),
    }
}

struct RegisteredWorktree {
    path: PathBuf,
    head: String,
    branch: Option<String>,
    prunable: bool,
}

fn registered_worktrees(project: &Path) -> Result<Vec<RegisteredWorktree>, String> {
    let output = git_text(project, &["worktree", "list", "--porcelain"])?;
    let mut trees = Vec::new();
    let mut current: Option<RegisteredWorktree> = None;
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(tree) = current.take() {
                trees.push(tree);
            }
            current = Some(RegisteredWorktree {
                path: normalize_git_path(path)?,
                head: String::new(),
                branch: None,
                prunable: false,
            });
            continue;
        }
        let Some(tree) = current.as_mut() else {
            continue;
        };
        if let Some(head) = line.strip_prefix("HEAD ") {
            tree.head = head.trim().to_owned();
        } else if let Some(branch) = line.strip_prefix("branch ") {
            tree.branch = Some(
                branch
                    .trim()
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch.trim())
                    .to_owned(),
            );
        } else if line == "detached" {
            tree.branch = None;
        } else if line.starts_with("prunable") {
            tree.prunable = true;
        }
    }
    if let Some(tree) = current {
        if tree.head.is_empty() {
            return Err(format!(
                "git worktree list omitted HEAD for {}",
                tree.path.display()
            ));
        }
        trees.push(tree);
    }
    Ok(trees)
}

fn registered_at<'a>(
    trees: &'a [RegisteredWorktree],
    path: &Path,
) -> Option<&'a RegisteredWorktree> {
    trees
        .iter()
        .find(|tree| same_allocation_path(&tree.path, path))
}

fn normalize_git_path(path: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    let text = if let Some(quoted) = path.strip_prefix('"') {
        let Some(quoted) = quoted.strip_suffix('"') else {
            return Err(format!("git worktree path {path} is not a closed quote"));
        };
        unescape_git_path(quoted)?
    } else {
        path.to_owned()
    };
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    if cfg!(windows) {
        Ok(PathBuf::from(text.replace('/', "\\")))
    } else {
        Ok(PathBuf::from(text))
    }
}

fn unescape_git_path(text: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => {
                return Err(format!(
                    "git worktree path has an unsupported escape \\{other}"
                ));
            }
            None => return Err("git worktree path has a trailing escape".to_owned()),
        }
    }
    Ok(out)
}

fn same_allocation_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) if left == right => true,
        _ => allocation_path_key(left) == allocation_path_key(right),
    }
}

fn allocation_path_key(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let normalized = text.replace('/', "\\");
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

struct RecordedAllocation {
    branch: String,
    base: String,
    revision: String,
    worktree: String,
}

fn recorded_candidate_allocations(
    comments: &[String],
    item: &str,
) -> Result<Vec<RecordedAllocation>, String> {
    let marker = format!(
        "{} item={item} role=candidate ",
        board_hypothesis::IMPLEMENTATION_PREFIX
    );
    let mut records = Vec::new();
    for comment in comments {
        if !comment.starts_with(&marker) {
            continue;
        }
        records.push(parse_recorded_allocation(&comment[marker.len()..])?);
    }
    Ok(records)
}

fn parse_recorded_allocation(rest: &str) -> Result<RecordedAllocation, String> {
    Ok(RecordedAllocation {
        branch: allocation_field(rest, "branch=")?,
        base: allocation_field(rest, "base=")?,
        revision: allocation_field(rest, "revision=")?,
        worktree: allocation_worktree(rest)?,
    })
}

fn allocation_field(text: &str, key: &str) -> Result<String, String> {
    let Some(start) = text.find(key) else {
        return Err(format!(
            "the board implementation record has no {key} field"
        ));
    };
    text[start + key.len()..]
        .split_whitespace()
        .next()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("the board implementation record has an empty {key} field"))
}

fn allocation_worktree(text: &str) -> Result<String, String> {
    let Some(start) = text.find("worktree=") else {
        return Err("the board implementation record has no worktree field".to_owned());
    };
    let value = &text[start + "worktree=".len()..];
    let end = value.find(" runtime=").unwrap_or(value.len());
    let worktree = value[..end].trim();
    if worktree.is_empty() {
        return Err("the board implementation record has an empty worktree field".to_owned());
    }
    Ok(worktree.to_owned())
}

fn board_records_same_allocation(
    run: &Run,
    candidate: &CandidateState,
    checkout: &CandidateCheckout,
    destination: &Path,
) -> Result<(), String> {
    let comments = harness_core::board_feedback::list_comments(
        &run.spec.board.bd,
        &run.spec.board.project,
        &candidate.hypothesis,
    )
    .map_err(|error| {
        format!(
            "the hypothesis card {} comments could not be read: {error}",
            candidate.hypothesis
        )
    })?;
    for record in recorded_candidate_allocations(&comments, &candidate.hypothesis)? {
        // ensure_allocation publishes the checkout base. ensure_planning then
        // commits the scaffold and advances the cursor revision without
        // republishing. That older record is history of this allocation, not a
        // current competing claim, and it must not veto recovery. A different
        // branch, base, or revision still does.
        let same_allocation_revision =
            record.revision == checkout.revision || record.revision == checkout.base;
        if record.branch != checkout.branch
            || record.base != checkout.base
            || !same_allocation_revision
        {
            return Err(format!(
                "the board records branch {} base {} revision {} but the cursor records branch {} base {} revision {}; the destination was not adopted",
                record.branch,
                record.base,
                record.revision,
                checkout.branch,
                checkout.base,
                checkout.revision
            ));
        }
        let recorded = PathBuf::from(&record.worktree);
        if !same_allocation_path(&recorded, &checkout.path)
            && !same_allocation_path(&recorded, destination)
        {
            return Err(format!(
                "the board records worktree {} which is neither the cursor path {} nor the owner-assigned destination {}; the destination was not adopted",
                record.worktree,
                checkout.path.display(),
                destination.display()
            ));
        }
    }
    Ok(())
}

enum RelocationPublication {
    Moved,
    Reconciled { retained_error: String },
}

fn publish_relocated_allocation(
    run: &mut Run,
    candidate: &mut CandidateState,
    relocated: CandidateCheckout,
    notes: &mut Vec<String>,
    publication: RelocationPublication,
) -> io::Result<()> {
    if let Err(error) = board_hypothesis::record_implementation(
        &run.spec.board.bd,
        &run.spec.board.project,
        &candidate.hypothesis,
        &BoundedImplementation {
            role: board_hypothesis::HypothesisRole::Candidate,
            branch: relocated.branch.clone(),
            base: relocated.base.clone(),
            revision: relocated.revision.clone(),
            worktree: relocated.path.to_string_lossy().into_owned(),
            runtime: None,
            baseline_runtime: None,
        },
    ) {
        let reason = format!(
            "the relocated candidate allocation could not be recorded on hypothesis card {}: {error}",
            candidate.hypothesis
        );
        let prior = match &publication {
            RelocationPublication::Reconciled { retained_error } if retained_error != "none" => {
                Some(retained_error.clone())
            }
            _ => run.cursor.condition.clone(),
        };
        retain_publication_error(run, &reason, prior.as_deref())?;
        return Err(invalid(reason));
    }
    match publication {
        RelocationPublication::Moved => {
            run.cursor.effect(
                EffectKind::CandidateAllocated,
                format!(
                    "hypothesis={} branch={} base={} worktree={} relocated from the protected run state",
                    candidate.hypothesis,
                    relocated.branch,
                    relocated.base,
                    relocated.path.display()
                ),
            );
            notes.push(format!(
                "allocation: relocated branch {} at {} to {}",
                relocated.branch,
                relocated.revision,
                relocated.path.display()
            ));
        }
        RelocationPublication::Reconciled { retained_error } => {
            run.cursor.effect(
                EffectKind::CandidateAllocated,
                format!(
                    "hypothesis={} branch={} base={} revision={} worktree={} reconciled interrupted relocation; original error retained: {retained_error}",
                    candidate.hypothesis,
                    relocated.branch,
                    relocated.base,
                    relocated.revision,
                    relocated.path.display()
                ),
            );
            notes.push(format!(
                "allocation: reconciled interrupted relocation of branch {} at {} to {}; original error retained: {retained_error}",
                relocated.branch,
                relocated.revision,
                relocated.path.display()
            ));
        }
    }
    candidate.worktree = Some(relocated);
    Ok(())
}

/// Persists a publication failure without rewriting the recorded worktree
/// path, so the next resume can reconcile the completed Git move and the
/// error remains inspectable after this command returns.
fn retain_publication_error(run: &mut Run, reason: &str, prior: Option<&str>) -> io::Result<()> {
    let detail = match prior.map(str::trim).filter(|text| !text.is_empty()) {
        Some(existing) if !reason.contains(existing) => {
            format!("{reason}; original error retained: {existing}")
        }
        _ => reason.to_owned(),
    };
    run.cursor.phase = Phase::Idle;
    run.cursor.condition = Some(detail.clone());
    run.cursor.effect(EffectKind::IdleRecorded, &detail);
    run.store.save_cursor(&run.cursor)?;
    Ok(())
}

fn relocation_refusal(
    run: &mut Run,
    notes: &mut Vec<String>,
    command_error: Option<&str>,
    reason: String,
) -> io::Result<()> {
    let reason = match command_error.map(str::trim).filter(|text| !text.is_empty()) {
        Some(error) if !reason.contains(error) => {
            format!("{reason}; git worktree move had reported: {error}")
        }
        _ => reason,
    };
    refused(run, notes, reason)
}

/// Reconciles a Git move that finished while the cursor and board still name
/// the old path. Recovery publishes the recorded branch, base and revision at
/// the owner-assigned destination only when Git's registration proves that
/// identity. It never moves, resets, deletes or replays a model attempt.
fn reconcile_interrupted_relocation(
    run: &mut Run,
    candidate: &mut CandidateState,
    checkout: &CandidateCheckout,
    destination: &Path,
    notes: &mut Vec<String>,
    command_error: Option<&str>,
) -> io::Result<()> {
    let trees = match registered_worktrees(&run.spec.project) {
        Ok(trees) => trees,
        Err(error) => {
            return relocation_refusal(
                run,
                notes,
                command_error,
                format!(
                    "the recorded candidate worktree {} is absent and its Git registration could not be read: {error}; the destination was not adopted and this resume did not move, reset or delete a worktree",
                    checkout.path.display()
                ),
            );
        }
    };
    if registered_at(&trees, &checkout.path).is_some() {
        return relocation_refusal(
            run,
            notes,
            command_error,
            format!(
                "the recorded candidate worktree {} is absent on disk but Git still registers it; the owner-assigned destination was not adopted and this resume did not move, reset or delete a worktree",
                checkout.path.display()
            ),
        );
    }
    if !destination.exists() {
        return relocation_refusal(
            run,
            notes,
            command_error,
            format!(
                "the recorded candidate worktree {} is absent and the owner-assigned destination {} does not exist; the move is not recovered and this resume did not move, reset or delete a worktree",
                checkout.path.display(),
                destination.display()
            ),
        );
    }
    let Some(registered) = registered_at(&trees, destination) else {
        return relocation_refusal(
            run,
            notes,
            command_error,
            format!(
                "the recorded candidate worktree {} is absent and {} is not a registered worktree of {}; it is not adopted and was not modified",
                checkout.path.display(),
                destination.display(),
                run.spec.project.display()
            ),
        );
    };
    if registered.prunable
        || registered.head != checkout.revision
        || registered.branch.as_deref() != Some(checkout.branch.as_str())
    {
        return relocation_refusal(
            run,
            notes,
            command_error,
            format!(
                "the registered destination {} is at revision {} on branch {}, not the recorded revision {} on branch {}; it is not adopted and was not reset or checked out",
                destination.display(),
                registered.head,
                registered.branch.as_deref().unwrap_or("detached HEAD"),
                checkout.revision,
                checkout.branch
            ),
        );
    }
    match task_worktree::worktree_reuse(
        &run.spec.project,
        destination,
        &run.spec.project,
        &checkout.revision,
        false,
    )? {
        WorktreeReuse::Eligible { revision } if revision == checkout.revision => {}
        WorktreeReuse::Eligible { revision } => {
            return relocation_refusal(
                run,
                notes,
                command_error,
                format!(
                    "the registered destination {} resolved revision {revision} instead of the recorded revision {}; it is not adopted and was not modified",
                    destination.display(),
                    checkout.revision
                ),
            );
        }
        WorktreeReuse::Blocked { kind, reason } => {
            return relocation_refusal(
                run,
                notes,
                command_error,
                format!(
                    "the registered destination {} is not the exact preserved allocation ({kind:?}): {reason}; it is not adopted and was not modified",
                    destination.display()
                ),
            );
        }
    }
    if let Err(reason) = board_records_same_allocation(run, candidate, checkout, destination) {
        return relocation_refusal(
            run,
            notes,
            command_error,
            format!("the interrupted relocation is not adopted: {reason}"),
        );
    }
    let relocated = CandidateCheckout {
        path: destination.to_path_buf(),
        ..checkout.clone()
    };
    if let Err(error) = verify_candidate_base(run, &relocated) {
        return relocation_refusal(
            run,
            notes,
            command_error,
            format!(
                "the interrupted relocation is not adopted: {error}; the destination was not modified"
            ),
        );
    }
    let retained = match (
        command_error.map(str::trim).filter(|text| !text.is_empty()),
        run.cursor
            .condition
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty()),
    ) {
        (Some(command), Some(existing)) => format!("{command}; {existing}"),
        (Some(command), None) => command.to_owned(),
        (None, Some(existing)) => existing.to_owned(),
        (None, None) => "none".to_owned(),
    };
    publish_relocated_allocation(
        run,
        candidate,
        relocated,
        notes,
        RelocationPublication::Reconciled {
            retained_error: retained,
        },
    )
}

fn observe_failed_move(
    project: &Path,
    checkout: &CandidateCheckout,
    destination: &Path,
) -> Result<FailedMoveObservation, String> {
    let trees = registered_worktrees(project)?;
    let old = registered_at(&trees, &checkout.path);
    let dest = registered_at(&trees, destination);
    let destination_matches_identity = dest.is_some_and(|tree| {
        !tree.prunable
            && tree.head == checkout.revision
            && tree.branch.as_deref() == Some(checkout.branch.as_str())
    });
    Ok(FailedMoveObservation {
        old_exists: checkout.path.exists(),
        old_registered: old.is_some(),
        destination_exists: destination.exists(),
        destination_registered: dest.is_some(),
        destination_matches_identity,
    })
}

fn settle_failed_worktree_move(
    run: &mut Run,
    candidate: &mut CandidateState,
    checkout: &CandidateCheckout,
    destination: &Path,
    stderr: &str,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let observed = match observe_failed_move(&run.spec.project, checkout, destination) {
        Ok(observed) => observed,
        Err(error) => {
            return relocation_refusal(
                run,
                notes,
                Some(stderr),
                format!(
                    "the recorded candidate worktree {} could not be relocated out of the protected run state: {stderr}; the resulting registration could not be read: {error}; this failure is not claimed to have left the allocation unchanged, because that was not established, and nothing was reset or deleted",
                    checkout.path.display()
                ),
            );
        }
    };
    match classify_failed_worktree_move(&checkout.path, stderr, &observed) {
        FailedMoveClassification::Completed => reconcile_interrupted_relocation(
            run,
            candidate,
            checkout,
            destination,
            notes,
            Some(stderr),
        ),
        FailedMoveClassification::Unmoved { reason }
        | FailedMoveClassification::Ambiguous { reason } => refused(run, notes, reason),
    }
}

/// The candidate allocation must descend from the run's exact committed base;
/// a changed or unreadable base blocks all dependent effects.
fn verify_candidate_base(run: &Run, checkout: &CandidateCheckout) -> Result<(), String> {
    let expected = git_text(
        &run.spec.project,
        &[
            "rev-parse",
            &format!("{}^{{commit}}", run.spec.base_revision),
        ],
    )?;
    if expected != checkout.base {
        return Err(format!(
            "the candidate worktree is based on {} instead of the run's frozen base {expected}; implementation is refused",
            checkout.base
        ));
    }
    task_worktree::verify_candidate_checkout(checkout)
        .map_err(|error| format!("the candidate worktree binding does not verify: {error}"))
}

/// Ensures the candidate's own OpenSpec change is complete and qualified
/// inside its worktree. A missing change is scaffolded through the installed
/// CLI; an incomplete one is authored by a bounded planning conversation, and
/// implementation is dispatched only after re-qualification succeeds.
fn ensure_planning(
    run: &mut Run,
    candidate: &mut CandidateState,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let Some(checkout) = candidate.worktree.clone() else {
        return Ok(());
    };
    let target = candidate_specification(run, &checkout, candidate);
    let openspec = OpenSpec::default();
    // The bounded planning conversation returning its committed change is
    // consumed first: an already-completed conversation settles without a new
    // model round.
    if let Some(id) = candidate.planner_attempt.clone() {
        let Some(attempt) = run.cursor.attempt(&id).cloned() else {
            let reason = format!(
                "the recorded planning attempt {id} is missing from the cursor; reconcile the run state before dependent work"
            );
            return refused(run, notes, reason);
        };
        if attempt.state == AttemptState::Completed {
            return consume_planner_result(run, candidate, notes);
        }
        if attempt.state == AttemptState::Failed && attempt.binding.is_none() {
            // A refusal before submission made no model request; a fresh
            // attempt is the documented recovery.
            candidate.planner_attempt = None;
        } else {
            let reason = format!(
                "the planning attempt {id} is {} ({}); reconcile it through the owning dispatcher before dependent work and never resubmit it",
                attempt.state.as_str(),
                attempt.reason.as_deref().unwrap_or("no recorded reason")
            );
            return block(run, notes, reason);
        }
    }
    // A change complete at the current candidate revision needs no
    // conversation; only a change that does not qualify triggers planning. A
    // removal candidate additionally needs its reviewable removal proposal
    // stated in the change: without it planning stays incomplete, the bounded
    // planning conversation authors it first, and no removal effect is applied
    // during that preparation.
    match openspec.qualify(&target, &run.spec.experiment) {
        Ok(receipt) => match prepare_removal_proposal(run, candidate, &receipt) {
            Ok(prepared) => {
                match ensure_experiment_selection(run, &receipt, notes)? {
                    Ok(()) => {
                        return store_candidate_receipt(
                            run,
                            candidate,
                            &receipt,
                            prepared.as_ref(),
                            notes,
                        );
                    }
                    Err(detail) => notes.push(format!(
                        "experiment selection: the complete change {} does not yet state the predeclared experiment selection ({detail}); the planning conversation authors it under '{EXPERIMENT_SELECTION_HEADING}' before any implementation or directed baseline measurement",
                        receipt.specification.change
                    )),
                }
            }
            Err(error) => {
                notes.push(format!(
                    "removal proposal: the complete change does not yet state a recordable reviewable removal proposal ({error}); the planning conversation authors it before any decision request and nothing is applied"
                ));
            }
        },
        Err(error) => {
            let text = error.to_string();
            if text.contains("different planning root") || text.contains("different change") {
                let reason = format!(
                    "the candidate change {} does not resolve under the run's planning environment: {error}; the mismatched source blocks dependent effects",
                    candidate.change
                );
                return refused(run, notes, reason);
            }
        }
    }
    // The change is missing or incomplete: prepare its model-free scaffold in
    // the candidate worktree and commit it, so the planning conversation's
    // pooled checkout can see the change at the current candidate revision.
    let change_dir = checkout.path.join(candidate_change_dir(&candidate.change));
    if !change_dir.exists() {
        if target.store.is_some() {
            let reason = format!(
                "the candidate change {} is not present and the run plans through a registered OpenSpec store; store preparation is a separate owner and this controller refuses to create a change outside its checkout",
                candidate.change
            );
            return refused(run, notes, reason);
        }
        match openspec.scaffold(&target) {
            Ok(_) => {}
            Err(error) if error.to_string().contains("already exists") => {}
            Err(error) => {
                let reason = format!(
                    "the candidate change {} could not be scaffolded through the installed OpenSpec CLI: {error}",
                    candidate.change
                );
                return refused(run, notes, reason);
            }
        }
        if let Err(reason) = commit_worktree_paths(
            &checkout,
            &[candidate_change_dir(&candidate.change)],
            &format!("scaffold OpenSpec change {}", candidate.change),
        ) {
            return refused(run, notes, reason);
        }
        let revision = match git_text(&checkout.path, &["rev-parse", "HEAD"]) {
            Ok(revision) => revision,
            Err(error) => {
                return refused(
                    run,
                    notes,
                    format!("the scaffold commit is unreadable: {error}"),
                );
            }
        };
        let mut scaffolded = checkout.clone();
        scaffolded.revision = revision;
        candidate.worktree = Some(scaffolded);
        run.cursor.effect(
            EffectKind::PlanningQualified,
            format!(
                "scaffolded OpenSpec change {} in {} and committed it at {} through the installed CLI (model-free preparation)",
                candidate.change,
                change_dir.display(),
                candidate.worktree.as_ref().map(|c| c.revision.clone()).unwrap_or_default()
            ),
        );
        notes.push(format!(
            "planning: scaffolded change {} at {}",
            candidate.change,
            change_dir.display()
        ));
    }
    // The candidate cannot reach its own supervisor, planning artifacts,
    // acceptance inputs or run state through its writable scope.
    let gated = RunSpec {
        project: checkout.path.clone(),
        ..run.spec.clone()
    };
    if let Err(error) = gated.supervisor_gate(run.store.root(), &change_dir, &run.spec.oracle) {
        return refused(run, notes, error.to_string());
    }
    let facts = dispatch_facts_for(run, AttemptRole::Planner)?;
    match dispatch_gate(&run.cursor, AttemptRole::Planner, &facts) {
        DispatchGate::Ready => dispatch_planner(run, candidate, notes),
        DispatchGate::Blocked { reason } => block(run, notes, reason),
    }
}

fn candidate_specification(
    run: &Run,
    checkout: &CandidateCheckout,
    candidate: &CandidateState,
) -> Specification {
    Specification {
        project: checkout.path.clone(),
        change: candidate.change.clone(),
        store: run.spec.specification.store.clone(),
        planning_root: checkout.path.clone(),
    }
}

/// One resolved reviewable removal proposal: the change-side receipt and the
/// bounded record the existing hypothesis owner accepts. Resolving and
/// recording it applies nothing.
struct PreparedRemovalProposal {
    /// The reviewed proposal reference recorded on the card.
    proposal: String,
    receipt: RemovalProposalReceipt,
    bounded: board_hypothesis::BoundedRemovalProposal,
}

/// Resolves the reviewable removal proposal a removal candidate's own
/// OpenSpec change must state before planning completes: the target and source
/// references, the unapplied preview, evidence and its gaps, measured versus
/// predicted benefit, lost scenarios, consumer/configuration/installation
/// impact, alternatives, retained checks and restoration. `Ok(None)` is an
/// ordinary candidate; an error names the exact missing or unrecordable
/// content, and the caller keeps planning incomplete instead of requesting a
/// decision or applying anything.
fn prepare_removal_proposal(
    run: &Run,
    candidate: &CandidateState,
    planning: &PlanningReceipt,
) -> io::Result<Option<PreparedRemovalProposal>> {
    if !candidate.removal_required {
        return Ok(None);
    }
    let openspec = OpenSpec::default();
    let proposal = openspec.removal_proposal(planning)?;
    let reference = match &run.spec.removal {
        Some(declared) if candidate.hypothesis == run.spec.hypothesis_item => {
            if proposal.target != declared.target {
                return Err(invalid(format!(
                    "the change's removal proposal target {} does not match the run's declared removal target {}; align the reviewed proposal before any removal effect",
                    proposal.target, declared.target
                )));
            }
            declared.proposal.clone()
        }
        _ => format!("openspec/changes/{}", candidate.change),
    };
    let section = proposal
        .artifact
        .strip_prefix(&planning.change_root)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| proposal.artifact.display().to_string());
    let detail = format!(
        "section={section}#{} digest={} clauses={}",
        proposal.heading,
        proposal.section_digest,
        REMOVAL_PROPOSAL_CLAUSES.len()
    );
    let bounded = board_hypothesis::BoundedRemovalProposal::try_from_draft(
        board_hypothesis::RemovalProposalDraft {
            proposal: reference.clone(),
            target: proposal.target.clone(),
            evidence: proposal.evidence.clone(),
            loss: proposal.loss.clone(),
            preview: Some(proposal.preview.clone()),
            detail: Some(detail),
        },
    )
    .map_err(|error| {
        invalid(format!(
            "the change's removal proposal is not recordable as a bounded reviewed record: {error}"
        ))
    })?;
    Ok(Some(PreparedRemovalProposal {
        proposal: reference,
        receipt: proposal,
        bounded,
    }))
}

/// Records the resolved reviewable removal proposal on the hypothesis card
/// through the existing board owner, mirroring the clause values the change
/// states, and freezes the reviewed proposal digest for this candidate. A
/// later changed proposal version invalidates the frozen review, so it needs a
/// fresh decision; recording itself applies no removal.
fn record_removal_proposal(
    run: &mut Run,
    candidate: &mut CandidateState,
    prepared: &PreparedRemovalProposal,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let record = board_hypothesis::record_removal_proposal(
        &run.spec.board.bd,
        &run.spec.board.project,
        &candidate.hypothesis,
        &prepared.bounded,
    )
    .map_err(|error| {
        invalid(format!(
            "the reviewable removal proposal could not be recorded on hypothesis card {}: {error}",
            candidate.hypothesis
        ))
    })?;
    let comments = harness_core::board_feedback::list_comments(
        &run.spec.board.bd,
        &run.spec.board.project,
        &candidate.hypothesis,
    )?;
    candidate.removal_frozen = frozen_candidate_removal_digest(&candidate.hypothesis, &comments);
    run.cursor.effect(
        EffectKind::RemovalChecked,
        format!(
            "reviewable-removal-proposal proposal={} target={} evidence={} preview={} section={}#{} digest={} reviewed={} record={} applied=nothing",
            prepared.proposal,
            prepared.bounded.target,
            prepared.bounded.evidence,
            prepared.bounded.preview.as_deref().unwrap_or("none"),
            prepared.receipt.artifact.display(),
            prepared.receipt.heading,
            prepared.receipt.section_digest,
            candidate.removal_frozen.as_deref().unwrap_or("unknown"),
            if record.recorded {
                "written"
            } else {
                "already-recorded"
            }
        ),
    );
    notes.push(format!(
        "removal proposal: recorded {} for target {} on hypothesis card {} ({}; nothing applied)",
        prepared.proposal,
        prepared.bounded.target,
        candidate.hypothesis,
        if record.recorded {
            "written"
        } else {
            "already-recorded"
        }
    ));
    Ok(())
}

fn store_candidate_receipt(
    run: &mut Run,
    candidate: &mut CandidateState,
    receipt: &PlanningReceipt,
    removal: Option<&PreparedRemovalProposal>,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    // A removal candidate's reviewable proposal is recorded before the
    // planning receipt is retained: the proposal has to be available before
    // the informed decision is requested, and a failed recording leaves
    // planning incomplete so the next advance retries it instead of starting
    // dependent work.
    if let Some(prepared) = removal {
        record_removal_proposal(run, candidate, prepared, notes)?;
    }
    write_json_atomic(&run.store.root().join(CANDIDATE_PLANNING_FILE), receipt)?;
    candidate.planning_receipt = Some(run.store.root().join(CANDIDATE_PLANNING_FILE));
    run.cursor.effect(
        EffectKind::PlanningQualified,
        format!(
            "candidate={} change={} artifacts={} state={}",
            candidate.hypothesis,
            receipt.specification.change,
            receipt.artifacts.len(),
            receipt.implementation_state
        ),
    );
    notes.push(format!(
        "planning: change {} qualified ({} artifact(s), state {})",
        receipt.specification.change,
        receipt.artifacts.len(),
        receipt.implementation_state
    ));
    Ok(())
}

fn consume_planner_result(
    run: &mut Run,
    candidate: &mut CandidateState,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let id = candidate.planner_attempt.clone().unwrap_or_default();
    let Some(attempt) = run.cursor.attempt(&id).cloned() else {
        let reason = format!("the recorded planning attempt {id} is missing from the cursor");
        return refused(run, notes, reason);
    };
    if attempt.retained.is_none() {
        let reason = format!(
            "the planning attempt {} completed without retained terminal evidence; unverified output never authorizes implementation",
            attempt.id
        );
        return refused(run, notes, reason);
    }
    // Only the change directory may differ: the planning conversation authors
    // one change and touches nothing else.
    let Some(checkout) = candidate.worktree.clone() else {
        let reason =
            "the candidate allocation is missing; planning validation is refused".to_owned();
        return refused(run, notes, reason);
    };
    // The returned commit is transferred once; a resume after a partial
    // transfer re-qualifies the already-advanced branch instead of replaying.
    let returned_head = match attempt
        .checkout
        .as_ref()
        .map(|path| git_text(path, &["rev-parse", "HEAD"]))
        .transpose()
    {
        Ok(Some(head)) => head,
        Ok(None) => {
            let reason = format!(
                "the planning attempt {} records no returned checkout, so its change cannot be attributed",
                attempt.id
            );
            return refused(run, notes, reason);
        }
        Err(error) => {
            let reason = format!("the planning checkout could not be read: {error}");
            return refused(run, notes, reason);
        }
    };
    if returned_head != checkout.revision
        && let Err(reason) = transfer_returned(run, candidate, &attempt, &[])
    {
        return refused(run, notes, reason);
    }
    let Some(checkout) = candidate.worktree.clone() else {
        let reason = "the candidate allocation is missing after the planning transfer".to_owned();
        return refused(run, notes, reason);
    };
    let target = candidate_specification(run, &checkout, candidate);
    let openspec = OpenSpec::default();
    match openspec.qualify(&target, &run.spec.experiment) {
        Ok(receipt) => match prepare_removal_proposal(run, candidate, &receipt) {
            Ok(prepared) => match ensure_experiment_selection(run, &receipt, notes)? {
                Ok(()) => {
                    store_candidate_receipt(run, candidate, &receipt, prepared.as_ref(), notes)
                }
                Err(detail) => refused(
                    run,
                    notes,
                    format!(
                        "the planning conversation finished but the candidate change {} does not state the predeclared experiment selection ({detail}); implementation stays undispatched until the section '{EXPERIMENT_SELECTION_HEADING}' states the declared method and claim",
                        candidate.change
                    ),
                ),
            },
            Err(error) => {
                let reason = format!(
                    "the planning conversation finished but the candidate change {} does not state a recordable reviewable removal proposal: {error}; implementation stays undispatched until it is complete and no removal is applied",
                    candidate.change
                );
                refused(run, notes, reason)
            }
        },
        Err(error) => {
            let reason = format!(
                "the planning conversation finished but the candidate change {} does not qualify: {error}; implementation stays undispatched until the missing artifacts are complete",
                candidate.change
            );
            refused(run, notes, reason)
        }
    }
}

fn dispatch_planner(
    run: &mut Run,
    candidate: &mut CandidateState,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let host = match dispatch_host(run, candidate) {
        Ok(host) => host,
        Err(reason) => return refused(run, notes, reason),
    };
    let change_dir = host.checkout.join(candidate_change_dir(&candidate.change));
    let inputs = relative_files(&host.checkout, &change_dir)?;
    // A declared measurement scope the planner cannot read stays omitted from
    // the brief; the baseline gate refuses that run with the exact cause
    // instead of letting an unstated scope direct a measurement.
    let declared = match declared_measurement_scope(run) {
        Ok(scope) => scope,
        Err(error) => {
            notes.push(format!(
                "measurement: the declared scope is not usable yet ({error}); the planning brief omits it and no directed baseline measurement is eligible until it is declared"
            ));
            None
        }
    };
    // A predeclared experiment selection the planner cannot read stays
    // omitted from the brief for the same reason: the selection gate keeps
    // dependent work undispatched with the exact cause instead.
    let predeclared = match predeclared_experiment_selection(run) {
        Ok(selection) => selection,
        Err(error) => {
            notes.push(format!(
                "experiment selection: the predeclared selection is unusable ({error}); the planning brief omits it and dependent work stops until the comparison policy is corrected"
            ));
            None
        }
    };
    let assignment = planner_assignment(
        run,
        candidate,
        inputs,
        declared.as_ref(),
        predeclared.as_ref(),
    );
    let attempt_id = next_attempt_id(&run.cursor, AttemptRole::Planner);
    candidate.planner_attempt = Some(attempt_id.clone());
    dispatch_bound_assignment(
        run,
        host,
        assignment,
        AttemptRole::Planner,
        attempt_id,
        notes,
    )
}

/// The bounded planning assignment document. Construction is pure, so the
/// native structured contract can be checked without a controller run.
fn planner_assignment(
    run: &Run,
    candidate: &CandidateState,
    inputs: Vec<String>,
    declared: Option<&MeasurementScope>,
    selection: Option<&ExperimentSelection>,
) -> serde_json::Value {
    let acceptance_artifact = run.spec.experiment.acceptance_artifact.clone();
    let acceptance_heading = run.spec.experiment.acceptance_heading.clone();
    let card_read = format!(
        "read the admitted hypothesis card before authoring: `{} show {} --json`",
        run.spec.board.bd.display(),
        candidate.hypothesis
    );
    let card_project = format!(
        "the board project for the card read is {}",
        run.spec.board.project.display()
    );
    let removal_objective = if candidate.removal_required {
        format!(
            " The change also states the reviewable removal proposal under '{REMOVAL_PROPOSAL_HEADING}' with every required clause; preparing it applies nothing."
        )
    } else {
        String::new()
    };
    let objective = format!(
        "Author the complete OpenSpec change {} for the selected hypothesis card {} using the installed OpenSpec CLI in this checkout, and keep the artifacts consistent with the card's mechanism, conditions, predicted effect, counterexample and acceptance. The card read and the run's predeclared acceptance requirements are recorded in the invariants. Run `openspec validate {} --strict --no-interactive` until it passes, then commit the change and leave the tree clean; do not edit product source.{removal_objective}",
        candidate.change, candidate.hypothesis, candidate.change
    );
    let outputs = vec![
        format!("{}/proposal.md", candidate_change_dir(&candidate.change)),
        format!("{}/design.md", candidate_change_dir(&candidate.change)),
        format!("{}/tasks.md", candidate_change_dir(&candidate.change)),
        format!(
            "{}/{}",
            candidate_change_dir(&candidate.change),
            acceptance_artifact.display()
        ),
    ];
    let mut invariants = vec![
        "only the candidate's own OpenSpec change directory is written; product source stays untouched".to_owned(),
        "the installed OpenSpec workflow definitions and schemas are never edited".to_owned(),
        "the authored change is committed in this checkout and the tree is left without uncommitted or untracked files".to_owned(),
        format!("the implementation conversation that follows must find a strictly valid change for {}", candidate.change),
        card_read,
        card_project,
        format!("the run's predeclared acceptance section is titled '{acceptance_heading}'"),
        format!("the predeclared acceptance section must appear at {} under the change", acceptance_artifact.display()),
    ];
    let mut acceptance = vec![
        format!(
            "`openspec validate {} --strict --no-interactive` passes inside this checkout",
            candidate.change
        ),
        format!(
            "the change contains proposal, requirements, design and tasks plus the predeclared acceptance section '{}'",
            acceptance_heading
        ),
        "one committed revision contains exactly the authored change and the working tree is clean"
            .to_owned(),
    ];
    if let Some(scope) = declared {
        invariants.push(format!(
            "the initial change states the declared measurement scope under the exact heading '{}' in {}",
            scope.declaration_heading,
            scope.declaration_artifact.display()
        ));
        bounded_items(
            "declared observed problem: ",
            &scope.observed_problem,
            &mut invariants,
        );
        bounded_items(
            "declared investigation scope: ",
            &scope.investigation_scope,
            &mut invariants,
        );
        bounded_items(
            "declared measurement question: ",
            &scope.measurement_question,
            &mut invariants,
        );
        bounded_items("declared limits: ", &scope.limits, &mut invariants);
        invariants.push(
            "the targeted measurement runs one existing operation whose contract is linked from this same change; do not create a second hypothesis, card or OpenSpec change for the workload".to_owned(),
        );
        bounded_items(
            "declared workload operation: ",
            &scope.workload.operation,
            &mut invariants,
        );
        bounded_items(
            "declared workload contract link: ",
            &scope.workload.contract,
            &mut invariants,
        );
        for reference in &scope.evidence_references {
            bounded_items("declared evidence reference: ", reference, &mut invariants);
        }
        acceptance.push(format!(
            "the change states the declared measurement scope section '{}' in {}; the controller re-resolves it through the installed OpenSpec CLI before any directed baseline measurement",
            scope.declaration_heading,
            scope.declaration_artifact.display()
        ));
    }
    if let Some(selection) = selection {
        invariants.push(format!(
            "the change states the run's predeclared experiment selection under the exact heading '{EXPERIMENT_SELECTION_HEADING}' in one of its resolved artifacts, with each required clause stated exactly once, non-empty and on one line: {}",
            EXPERIMENT_SELECTION_CLAUSES.join(" ")
        ));
        invariants.push(format!(
            "the predeclared selection is method={} claim={}; the section's Method and Claim clauses must state exactly those bounded tokens, and the section must declare the outcome, rationale, controls, projection, baseline and stopping/escalation/deferral values below",
            selection.method.as_str(),
            selection.claim.as_str()
        ));
        bounded_items(
            "declared required outcome: ",
            &selection.outcome,
            &mut invariants,
        );
        bounded_items(
            "declared applicability rationale: ",
            &selection.rationale,
            &mut invariants,
        );
        bounded_items("declared controls: ", &selection.controls, &mut invariants);
        bounded_items(
            "declared projected use and cost: ",
            &selection.projection,
            &mut invariants,
        );
        bounded_items(
            "declared admissible baseline basis: ",
            &selection.baseline,
            &mut invariants,
        );
        bounded_items(
            "declared stopping, escalation and deferral rules: ",
            &selection.stopping,
            &mut invariants,
        );
        acceptance.push(
            "the change states the predeclared experiment selection; the controller resolves it before any implementation or directed baseline measurement, and a missing, changed or mismatched section blocks dependent work"
                .to_string(),
        );
    }
    if candidate.removal_required {
        invariants.push(format!(
            "the change states the reviewable removal proposal under the exact heading '{REMOVAL_PROPOSAL_HEADING}' in one of its resolved artifacts, with each required clause stated exactly once, non-empty and on one line: {}",
            REMOVAL_PROPOSAL_CLAUSES.join(" ")
        ));
        bounded_items(
            "removal proposal clause meaning: ",
            REMOVAL_PROPOSAL_GUIDE,
            &mut invariants,
        );
        if let Some(declared) = &run.spec.removal
            && candidate.hypothesis == run.spec.hypothesis_item
        {
            invariants.push(format!(
                "the run declares the removal proposal reference {} and target {}; the section's Target clause must state exactly that target",
                declared.proposal, declared.target
            ));
        }
        invariants.push(
            "the removal proposal is prepared, not applied: this conversation authors the change only, and no removal effect happens before the recorded user decision"
                .to_owned(),
        );
        acceptance.push(format!(
            "the change states every removal proposal clause under '{REMOVAL_PROPOSAL_HEADING}'; the controller resolves them through the installed OpenSpec CLI and records the reviewable proposal on the hypothesis card before any removal effect"
        ));
    }
    json!({
        "schema": 1,
        "objective": objective,
        "inputs": inputs,
        "outputs": outputs,
        "invariants": invariants,
        "acceptance": acceptance,
        "consumer": "the improvement controller (codex-harness improve)",
        "escalate": [],
    })
}

/// Renders one declared scope or guide text into bounded invariant items. Long
/// prose is split at character boundaries so every item stays inside the
/// native structured assignment item limit and no declared text is shortened.
fn bounded_items(prefix: &str, text: &str, items: &mut Vec<String>) {
    let limit = crate::executor_assignment::MAX_ITEM_BYTES;
    let mut rest = text;
    let mut head = prefix;
    while !rest.is_empty() {
        let room = limit.saturating_sub(head.len()).max(1);
        let mut end = rest.len().min(room);
        while end > 0 && !rest.is_char_boundary(end) {
            end -= 1;
        }
        if end == 0 {
            break;
        }
        let (chunk, tail) = rest.split_at(end);
        items.push(format!("{head}{chunk}"));
        rest = tail;
        head = "(continued) ";
    }
}

/// The declared writable scope as bounded invariant items. Every entry stays
/// visible in the rendered brief and no item can exceed the native structured
/// assignment item limit; entries are never shortened, dropped or reordered.
fn scope_invariants(scope: &[String]) -> Vec<String> {
    const PREFIX: &str = "every edit stays inside the declared writable scope: ";
    let limit = crate::executor_assignment::MAX_ITEM_BYTES.saturating_sub(64);
    let mut items = Vec::new();
    let mut current = String::from(PREFIX);
    for entry in scope {
        let addition = if current.len() == PREFIX.len() {
            entry.clone()
        } else {
            format!(", {entry}")
        };
        if current.len() > PREFIX.len() && current.len() + addition.len() > limit {
            items.push(current);
            current = format!("{PREFIX}{entry}");
        } else {
            current.push_str(&addition);
        }
    }
    if current.len() > PREFIX.len() {
        items.push(current);
    }
    items
}

fn dispatch_implementer(
    run: &mut Run,
    candidate: &mut CandidateState,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let host = match dispatch_host(run, candidate) {
        Ok(host) => host,
        Err(reason) => return refused(run, notes, reason),
    };
    let change_dir = host.checkout.join(candidate_change_dir(&candidate.change));
    let inputs = relative_files(&host.checkout, &change_dir)?;
    let assignment = implementer_assignment(run, candidate, inputs);
    let attempt_id = next_attempt_id(&run.cursor, AttemptRole::Implementer);
    candidate.implementer_attempt = Some(attempt_id.clone());
    dispatch_bound_assignment(
        run,
        host,
        assignment,
        AttemptRole::Implementer,
        attempt_id,
        notes,
    )
}

/// The bounded implementation assignment document. Construction is pure, so
/// the native structured contract can be checked without a controller run.
fn implementer_assignment(
    run: &Run,
    candidate: &CandidateState,
    inputs: Vec<String>,
) -> serde_json::Value {
    // The declared scope stays in bounded invariant items instead of the
    // objective: a long multi-file scope cannot overflow the native objective
    // limit, and no path is shortened, dropped or replaced by a basename.
    let mut invariants = scope_invariants(&run.spec.writable_scope);
    invariants.extend([
        format!(
            "the change artifacts under {} are not modified; the frozen planning digests must still verify",
            candidate_change_dir(&candidate.change)
        ),
        "all work is committed on this checkout and the tree is left clean; the controller advances the owned candidate branch to the returned revision".to_owned(),
        "the returned checks are independently re-verified by the controller and the parent acceptance owner; a success sentence alone is not evidence".to_owned(),
        format!(
            "read the admitted hypothesis card before implementing: `{} show {} --json`",
            run.spec.board.bd.display(),
            candidate.hypothesis
        ),
        format!(
            "the board project for the card read is {}",
            run.spec.board.project.display()
        ),
    ]);
    let objective = format!(
        "Implement the complete work items of the candidate's OpenSpec change {}. Stay inside the declared writable scope and keep the change artifacts read-only; both are recorded in the invariants, as is the card read. Commit all work on this checkout's current HEAD and leave no uncommitted or untracked files. Report in your final message the commit revision, the changed paths and the exact check commands you ran with their observed results.",
        candidate.change
    );
    json!({
        "schema": 1,
        "objective": objective,
        "inputs": inputs,
        "outputs": [],
        "invariants": invariants,
        "acceptance": [
            run.spec.experiment.independent_acceptance.clone(),
            format!("the committed candidate re-qualifies through `openspec validate {} --strict --no-interactive`", candidate.change),
        ],
        "consumer": "the improvement controller (codex-harness improve)",
        "escalate": [],
    })
}

/// One conversation's bound checkout: the candidate worktree for the first
/// dispatch, and the implementer's own returned checkout base afterwards.
struct DispatchHost {
    checkout: PathBuf,
    base: String,
}

fn dispatch_host(_run: &Run, candidate: &CandidateState) -> Result<DispatchHost, String> {
    let Some(checkout) = &candidate.worktree else {
        return Err(
            "the candidate allocation is missing; dependent dispatch is refused".to_owned(),
        );
    };
    if !checkout.path.is_dir() {
        return Err(format!(
            "the candidate worktree {} is missing; dependent dispatch is refused",
            checkout.path.display()
        ));
    }
    Ok(DispatchHost {
        checkout: checkout.path.clone(),
        base: checkout.revision.clone(),
    })
}

/// Writes the bounded assignment and dispatches one visible conversation,
/// recording the accepted native identity exactly like the investigator path.
fn dispatch_bound_assignment(
    run: &mut Run,
    host: DispatchHost,
    assignment: serde_json::Value,
    role: AttemptRole,
    attempt_id: String,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let surface = surface(&run.spec);
    // The caller's model-free gate refuses an unusable profile before this
    // point; the dispatch boundary itself repeats the check so a declared
    // configuration that cannot be honored never reaches the visible owner,
    // even if the gate is bypassed or a profile changes in between.
    if let Some(error) = &surface.binding_error {
        let reason = format!(
            "the dispatch profile is unusable: {error}; no dispatch was attempted and no fallback route was used"
        );
        return block(run, notes, reason);
    }
    let Some(binding) = surface.binding.clone() else {
        let reason =
            "the dispatch profile binding is unavailable; no dispatch was attempted".to_owned();
        return block(run, notes, reason);
    };
    let owner = dispatch_owner(&run.spec.run, role, attempt_ordinal(&attempt_id));
    let title = executor_title(&binding.profile, &owner);
    let assignment_path = run
        .store
        .assignments_dir()
        .join(format!("{attempt_id}.json"));
    let bytes = serde_json::to_vec_pretty(&assignment)?;
    if bytes.len() > MAX_ASSIGNMENT_BYTES {
        let reason = format!(
            "the {attempt_id} assignment exceeds the bounded assignment size; the dispatch is refused before any model request"
        );
        return refused(run, notes, reason);
    }
    write_json_atomic(&assignment_path, &assignment)?;
    let attempt = Attempt {
        id: attempt_id.clone(),
        role,
        binding: None,
        retained: None,
        owner: owner.clone(),
        title: title.clone(),
        profile: binding.profile.clone(),
        model: binding.model.clone(),
        model_provider: binding.model_provider.clone(),
        reasoning_effort: binding.reasoning_effort.clone(),
        checkout: None,
        assignment: Some(assignment_path.clone()),
        receipt: None,
        result: None,
        detail: None,
        state: AttemptState::Requested,
        reason: None,
        reuse_refused: None,
        started_ms: now_ms(),
        updated_ms: now_ms(),
    };
    run.cursor.push_attempt(attempt)?;
    run.cursor.effect(
        EffectKind::DispatchPrepared,
        format!(
            "attempt={attempt_id} role={} owner={owner} title=\"{title}\" profile={} model={} provider={} effort={} assignment={}",
            role.as_str(),
            binding.profile,
            binding.model.as_deref().unwrap_or("unknown"),
            binding.model_provider.as_deref().unwrap_or("unknown"),
            binding.reasoning_effort.as_deref().unwrap_or("default"),
            assignment_path.display()
        ),
    );
    run.store.save_cursor(&run.cursor)?;
    match dispatch_visible_conversation(&VisibleConversation {
        codex_home: run.spec.codex_home.clone(),
        source: host.checkout.clone(),
        owner: owner.clone(),
        profile: binding.profile.clone(),
        base: Some(host.base.clone()),
        assignment: assignment_path,
    }) {
        Ok(accepted) => {
            record_accepted(&mut run.cursor, &attempt_id, &accepted);
            run.store.save_cursor(&run.cursor)?;
            notes.push(format!(
                "dispatched: the bounded {} conversation was accepted through the visible owner",
                role.as_str()
            ));
            Ok(())
        }
        Err(error) => {
            let reason = format!(
                "dispatch refused before submission: {error}; no fallback was attempted and no model request was made"
            );
            if let Some(attempt) = run
                .cursor
                .attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
            {
                attempt.state = AttemptState::Failed;
                attempt.reason = Some(reason.clone());
                attempt.updated_ms = now_ms();
            }
            block(run, notes, reason)
        }
    }
}

/// The deterministic identity of the next bounded conversation of one role.
fn next_attempt_id(cursor: &Cursor, role: AttemptRole) -> String {
    let ordinal = cursor
        .attempts
        .iter()
        .filter(|attempt| attempt.role == role)
        .count() as u32
        + 1;
    format!("{}-{ordinal}", role.as_str())
}

fn attempt_ordinal(attempt_id: &str) -> u32 {
    attempt_id
        .rsplit('-')
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

/// Dispatches the bounded investigator conversation. Its only output is the
/// schema-1 investigator report as its final message; the controller consumes
/// that report through grounded intake, so the conversation itself admits no
/// hypothesis.
fn dispatch_investigator(
    run: &mut Run,
    evidence: &Evidence,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let host = DispatchHost {
        checkout: run.spec.project.clone(),
        base: run.spec.base_revision.clone(),
    };
    let assignment = investigator_assignment(run, evidence);
    let attempt_id = next_attempt_id(&run.cursor, AttemptRole::Investigator);
    dispatch_bound_assignment(
        run,
        host,
        assignment,
        AttemptRole::Investigator,
        attempt_id,
        notes,
    )
}

/// The bounded investigator assignment document. Construction is pure, so the
/// native structured contract can be checked without a controller run.
fn investigator_assignment(run: &Run, evidence: &Evidence) -> serde_json::Value {
    let mut invariants = vec![
        "the final message is exactly one JSON object: {\"schema\":1,\"candidates\":[<candidate>],\"idle_reason\":\"why no candidate is grounded or null\"}; at most 3 candidates".to_owned(),
        "each candidate carries mechanism (<=96-char token), conditions (<=96-char token applicability), observation (retained locator), predicted (<=256 chars), counterexample (<=256 chars), acceptance (<=256 chars), spec (its own OpenSpec change reference), basis (retained locator), evidence (at least one observed retained locator), treatment and optional next_check".to_owned(),
        "a candidate that selects an experiment (addition, simplification or subtraction) also declares its \"selection\": the claim path (local-operation, agent-choice, task-strategy or repeated-use), the required outcome, the method (bounded-replay, real-operation, agent-task, paired-implementations or sequence), the applicability rationale, controls, projected use/cost, admissible baseline basis and the stopping/escalation/deferral rules".to_owned(),
        "each selection field is one bounded single-line value of at most 512 bytes without ';'".to_owned(),
        "grounded intake refuses an ungrounded or prediction-only citation; a predicted statement is never retained evidence".to_owned(),
        "treatment is one of \"addition\", \"no-change\", \"reuse\", \"simplification\" or \"subtraction\"; consider no change, reuse of the smallest sufficient existing route, simplification and subtraction before additional machinery, and an addition is refused without a bounded single-line \"alternatives\" statement (at most 2048 bytes) saying why each smaller route does not satisfy the evidenced need".to_owned(),
        "a no-change candidate carries its bounded single-line reason (at most 2048 bytes); a reuse candidate names the retained locator of the existing attributable route and concludes reuse-suffices without a new card or implementation".to_owned(),
        "a simplification or subtraction candidate carries its removal target and basis; low or absent invocations are a lead for investigation, never a finding of uselessness - catalogue, instruction and initialization exposure can exist with zero invocations - and usage volume alone is not admitted".to_owned(),
        "a coverage basis states the observation interval, task/environment coverage, telemetry gaps including uncheckable consumer access, the rare, explicit and indirect uses at risk, how each arm's actual consumption (not just invocations) is evidenced, and the restoration route".to_owned(),
        "every candidate states applicability conditions and a counterexample; no candidate may auto-delete a capability, invent demand, commission a new audit service or recurring inventory, or treat fewer lines, files, skills or exposed names as the benefit by itself".to_owned(),
        "usage and outcome evidence cites existing retained owners - the installed `codex-harness skills usage` report for skills, rollout/outcome records otherwise; a candidate changing owned skill scope follows the owned skill-evolution procedure and other domains use their lifecycle owners; this loop publishes nothing itself".to_owned(),
        "the spec field names the candidate's own OpenSpec change under the run's openspec/changes planning root; an existing linked change is valid as it stands, and the controller qualifies it - preparing and authoring a missing change - before any implementation".to_owned(),
        format!(
            "candidate spec references resolve under the run's planning root {} (a spec is a change name or an openspec/changes/<name> reference; a path-shaped reference outside that root is refused before any candidate work)",
            run.spec.specification.planning_root.display()
        ),
        "this conversation edits no file and dispatches no other model work".to_owned(),
    ];
    bounded_items(
        "selection rules: ",
        "the declared method must exercise the declared claim path - a fixed command or retained replay never stands in for an unexercised agent; a broad strategy claim needs complete paired implementations; a repeated-use claim needs the sequence and state; fewer lines, files, skills or exposed names never establish benefit. When the sufficient experiment is not worth its cost, declare the deferral object {missingFact, reconsideration} instead of running or adopting it.",
        &mut invariants,
    );
    // Retained evidence locators are not checkout-relative paths, so the
    // structured inputs field cannot carry them. Each listed locator stays a
    // separately bounded invariant item: the whole inspection input remains
    // visible in the brief and the objective cannot overflow on long roots.
    for locator in evidence.listing.iter().take(MAX_LISTED_EVIDENCE) {
        invariants.push(format!("retained evidence to inspect: {locator}"));
    }
    if let Some(root) = &evidence.root {
        invariants.push(format!(
            "locators shaped file:<relative path> name retained files under {}",
            root.display()
        ));
    }
    let objective = format!(
        "Bounded improvement investigation for run {}. Inspect this checkout's source and every retained evidence locator recorded in the invariants. Consider no change, reuse, simplification and subtraction before additional machinery. Your final message is ONLY the JSON investigator report described in the other invariants; the controller consumes it through grounded intake. Do not edit source and start no other model or paid calls.",
        run.spec.run
    );
    json!({
        "schema": 1,
        "objective": objective,
        "inputs": [],
        "outputs": [],
        "invariants": invariants,
        "acceptance": [
            "the report parses as a bounded schema-1 investigator report and every candidate cites at least one retained observed locator",
        ],
        "consumer": "the improvement controller's grounded intake (codex-harness improve)",
        "escalate": [],
    })
}

fn relative_files(root: &Path, directory: &Path) -> io::Result<Vec<String>> {
    let mut files = Vec::new();
    if directory.is_dir() {
        let mut stack = vec![directory.to_path_buf()];
        while let Some(next) = stack.pop() {
            let mut entries: Vec<PathBuf> = fs::read_dir(&next)?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if let Ok(relative) = path.strip_prefix(root) {
                    files.push(relative.to_string_lossy().replace('\\', "/"));
                }
                if files.len() >= MAX_ASSIGNMENT_INPUTS {
                    break;
                }
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

// ---------------------------------------------------------------------------
// Implementation: dispatch, validate the returned evidence, retain ready.
// ---------------------------------------------------------------------------

fn ensure_implementation(
    run: &mut Run,
    candidate: &mut CandidateState,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    if let Some(id) = candidate.implementer_attempt.clone() {
        let Some(attempt) = run.cursor.attempt(&id).cloned() else {
            let reason = format!(
                "the recorded implementation attempt {id} is missing from the cursor; reconcile the run state before dependent work"
            );
            return refused(run, notes, reason);
        };
        if attempt.state == AttemptState::Failed && attempt.binding.is_none() {
            candidate.implementer_attempt = None;
        } else if attempt.state == AttemptState::Completed {
            return validate_implementation(run, candidate, &attempt, notes);
        } else {
            let reason = format!(
                "the implementation attempt {id} is {} ({}); reconcile it through the owning dispatcher before dependent work and never resubmit it",
                attempt.state.as_str(),
                attempt.reason.as_deref().unwrap_or("no recorded reason")
            );
            return block(run, notes, reason);
        }
    }
    // An independently accepted retained workload solution is materialized
    // model-free before any dispatch: its exact change, checked against the
    // resulting baseline, IS this candidate's implementation.
    match carry_retained_solution(run, candidate, notes)? {
        CarryOutcome::Carried | CarryOutcome::Refused => return Ok(()),
        CarryOutcome::Absent => {}
    }
    let facts = dispatch_facts_for(run, AttemptRole::Implementer)?;
    match dispatch_gate(&run.cursor, AttemptRole::Implementer, &facts) {
        DispatchGate::Ready => dispatch_implementer(run, candidate, notes),
        DispatchGate::Blocked { reason } => block(run, notes, reason),
    }
}

// ---------------------------------------------------------------------------
// Model-free carry of an independently accepted retained workload solution.
//
// When this run's own hypothesis card retains an exact `role=workload`
// implementation - written by the comparison owner only after the arm's
// frozen independent acceptance passed - and that change can be materialized
// onto the freshly allocated candidate branch without changing its content
// identity, the retained solution IS this run's candidate implementation.
// The carry reads the same bounded `hypothesis-implementation v1` records and
// applies the same content rule (`git diff --raw`, status plus resulting blob
// per path) the activation lineage owner verifies, so the resulting baseline
// can later be traced to the exact independently accepted revision. Nothing
// is ever materialized from an unattributable record, and a record that
// cannot reproduce its exact change never becomes a candidate.
// ---------------------------------------------------------------------------

/// One retained `role=workload` implementation record read back from this
/// run's own hypothesis card. Candidate-role records - this run's own
/// allocation or any other implementation - are never eligible, because only
/// the workload record is written after the frozen independent acceptance
/// passed.
struct RetainedWorkloadSolution {
    branch: String,
    base: String,
    revision: String,
}

/// Whether the implementation stage resolved through the retained-solution
/// carry, found nothing attributable (the ordinary grounded implementation
/// path), or refused without fabricating a candidate.
enum CarryOutcome {
    Carried,
    Absent,
    Refused,
}

/// Materializes one exact retained workload solution as this candidate's
/// implementation, or records the exact refusal. A refused carry neither
/// fabricates a candidate nor dispatches a substitute implementation: the
/// recorded condition names the fact the next action needs.
fn carry_retained_solution(
    run: &mut Run,
    candidate: &mut CandidateState,
    notes: &mut Vec<String>,
) -> io::Result<CarryOutcome> {
    let Some(checkout) = candidate.worktree.clone() else {
        return Ok(CarryOutcome::Absent);
    };
    let solutions = retained_workload_solutions(run)?;
    if solutions.is_empty() {
        return Ok(CarryOutcome::Absent);
    }
    // A removal treatment keeps waiting for the user's exact decision before
    // any dependent effect: the carry applies the same authority gate an
    // implementer dispatch would, even though it opens no conversation.
    let facts = dispatch_facts_for(run, AttemptRole::Implementer)?;
    match &facts.removal {
        Some(RemovalGate::Pending { reason }) => {
            block(run, notes, format!("removal approval is pending: {reason}"))?;
            return Ok(CarryOutcome::Refused);
        }
        Some(RemovalGate::Refused { .. }) => {
            block(
                run,
                notes,
                "the user declined this removal; the dependent removal effect stays blocked"
                    .to_owned(),
            )?;
            return Ok(CarryOutcome::Refused);
        }
        Some(RemovalGate::Withdrawn { .. }) => {
            block(
                run,
                notes,
                "the removal approval was withdrawn; the dependent removal effect stays blocked until a new decision"
                    .to_owned(),
            )?;
            return Ok(CarryOutcome::Refused);
        }
        _ => {}
    }
    // Deterministic order: a record produced from the candidate's own base
    // first (its identity is preserved exactly), then the recorded order.
    let mut ordered: Vec<&RetainedWorkloadSolution> = solutions.iter().collect();
    ordered.sort_by_key(|solution| solution.base != checkout.base);
    let mut failures: Vec<String> = Vec::new();
    for solution in ordered {
        // Both commits must be objects of this repository: like the
        // activation lineage owner, a record whose solution is not available
        // here is attributed to nothing.
        if !git_ok(
            &checkout.path,
            &["cat-file", "-e", &format!("{}^{{commit}}", solution.base)],
        )
        .unwrap_or(false)
            || !git_ok(
                &checkout.path,
                &[
                    "cat-file",
                    "-e",
                    &format!("{}^{{commit}}", solution.revision),
                ],
            )
            .unwrap_or(false)
        {
            continue;
        }
        match materialize_retained_solution(run, candidate, &checkout, solution) {
            Ok(head) => {
                run.cursor.effect(
                    EffectKind::ImplementationValidated,
                    format!(
                        "carried retained workload solution branch={} base={} revision={} as {head} (model-free materialization, no model attempt)",
                        solution.branch, solution.base, solution.revision
                    ),
                );
                notes.push(format!(
                    "candidate-ready: carried the exact retained workload solution {} revision {} as {} on the fresh candidate allocation",
                    solution.branch, solution.revision, head
                ));
                return Ok(CarryOutcome::Carried);
            }
            Err(reason) => failures.push(format!(
                "{} revision {}: {reason}",
                solution.branch, solution.revision
            )),
        }
    }
    if failures.is_empty() {
        return Ok(CarryOutcome::Absent);
    }
    let reason = format!(
        "the retained workload solution recorded on card {} cannot be materialized exactly onto the resulting baseline: {}; no candidate is fabricated and no substitute implementation is dispatched - reconcile the named fact and resume",
        run.spec.hypothesis_item,
        failures.join("; ")
    );
    refused(run, notes, reason)?;
    Ok(CarryOutcome::Refused)
}

/// The bounded retained `role=workload` implementation records of the run's
/// own hypothesis card, in their recorded order.
fn retained_workload_solutions(run: &Run) -> io::Result<Vec<RetainedWorkloadSolution>> {
    let comments = harness_core::board_feedback::list_comments(
        &run.spec.board.bd,
        &run.spec.board.project,
        &run.spec.hypothesis_item,
    )?;
    let mut solutions = Vec::new();
    for comment in &comments {
        let Some(solution) = parse_retained_workload_solution(comment, &run.spec.hypothesis_item)
        else {
            continue;
        };
        solutions.push(solution);
        if solutions.len() >= MAX_RETAINED_SOLUTIONS {
            break;
        }
    }
    Ok(solutions)
}

/// One `hypothesis-implementation v1` comment of this run's own card that
/// names a `role=workload` solution. Every other comment - including this
/// run's own `role=candidate` allocation - yields `None`.
fn parse_retained_workload_solution(comment: &str, item: &str) -> Option<RetainedWorkloadSolution> {
    let rest = comment
        .trim_start()
        .strip_prefix(board_hypothesis::IMPLEMENTATION_PREFIX)?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    if implementation_field(&fields, "item") != Some(item)
        || implementation_field(&fields, "role") != Some("workload")
    {
        return None;
    }
    Some(RetainedWorkloadSolution {
        branch: implementation_field(&fields, "branch")?.to_owned(),
        base: implementation_field(&fields, "base")?.to_owned(),
        revision: implementation_field(&fields, "revision")?.to_owned(),
    })
}

fn implementation_field<'a>(fields: &[&'a str], key: &str) -> Option<&'a str> {
    fields.iter().find_map(|entry| {
        let (name, value) = entry.split_once('=')?;
        (name == key && !value.is_empty()).then_some(value)
    })
}

/// Materializes one retained solution's exact committed change onto the
/// freshly allocated candidate branch and verifies every gate the ordinary
/// implementation path applies: the change reproduces the retained content
/// identity exactly, stays inside the declared writable scope or the
/// candidate's own change, and leaves the frozen planning artifacts
/// unchanged. The record's own worktree, branch and runtime are never reused
/// or adopted. Returns the committed candidate revision.
fn materialize_retained_solution(
    run: &Run,
    candidate: &mut CandidateState,
    checkout: &CandidateCheckout,
    solution: &RetainedWorkloadSolution,
) -> Result<String, String> {
    let retained = change_signature(&checkout.path, &solution.base, &solution.revision)?;
    if retained.is_empty() {
        return Err("the retained record names no committed change".to_owned());
    }
    let status = git_text(
        &checkout.path,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?;
    if !status.trim().is_empty() {
        return Err(
            "the candidate worktree is not clean; a retained solution is never materialized over local or untracked work"
                .to_owned(),
        );
    }
    let head = git_text(&checkout.path, &["rev-parse", "HEAD"])?;
    let current = change_signature(&checkout.path, &checkout.base, &head)?;
    let carried = if head != checkout.base && current == retained {
        // A resume after a partial pass: the exact change is already the
        // candidate head, so the same revision is adopted instead of a
        // second commit being replayed.
        head.clone()
    } else if head == checkout.revision {
        let patch = git_bytes(
            &checkout.path,
            &["diff", "--binary", &solution.base, &solution.revision],
        )?;
        git_apply_patch(&checkout.path, &patch)?;
        let message = format!(
            "carry retained workload solution {} ({})",
            solution.branch,
            retained_revision_short(&solution.revision)
        );
        git_text(
            &checkout.path,
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--no-verify",
                "-m",
                &message,
            ],
        )?;
        let committed = git_text(&checkout.path, &["rev-parse", "HEAD"])?;
        if change_signature(&checkout.path, &checkout.base, &committed)? != retained {
            restore_carried_revision(&checkout.path, &head);
            return Err(
                "the materialized change does not reproduce the retained change identity exactly"
                    .to_owned(),
            );
        }
        committed
    } else {
        return Err(format!(
            "the candidate worktree is at {head} instead of its recorded revision {}; an unrecorded revision is never overwritten",
            checkout.revision
        ));
    };
    let verify = |carried: &str| -> Result<(), String> {
        let changed = changed_paths(&checkout.path, &format!("{}..{carried}", checkout.base))?;
        changed_paths_within_scope(&changed, &run.spec.writable_scope, &candidate.change)?;
        let receipt: PlanningReceipt = read_json(
            &run.store.root().join(CANDIDATE_PLANNING_FILE),
            MAX_RUN_SPEC_BYTES,
        )
        .map_err(|error| {
            format!("the qualified candidate planning receipt is unavailable: {error}")
        })?;
        let target = Specification {
            project: checkout.path.clone(),
            change: candidate.change.clone(),
            store: run.spec.specification.store.clone(),
            planning_root: checkout.path.clone(),
        };
        let openspec = OpenSpec::default();
        let current = openspec
            .qualify(&target, &run.spec.experiment)
            .map_err(|error| {
                format!(
                    "the materialized revision broke the candidate's planning contract: {error}"
                )
            })?;
        if artifact_digests(&current)? != artifact_digests(&receipt)?
            || current.contract_digest != receipt.contract_digest
        {
            return Err(
                "the materialized revision changed the candidate's frozen planning artifacts"
                    .to_owned(),
            );
        }
        Ok(())
    };
    if let Err(reason) = verify(&carried) {
        if carried != head {
            restore_carried_revision(&checkout.path, &head);
        }
        return Err(reason);
    }
    board_hypothesis::record_implementation(
        &run.spec.board.bd,
        &run.spec.board.project,
        &candidate.hypothesis,
        &BoundedImplementation {
            role: board_hypothesis::HypothesisRole::Candidate,
            branch: checkout.branch.clone(),
            base: checkout.base.clone(),
            revision: carried.clone(),
            worktree: checkout.path.to_string_lossy().into_owned(),
            runtime: None,
            baseline_runtime: None,
        },
    )
    .map_err(|error| {
        format!(
            "the carried candidate revision could not be recorded on hypothesis card {}: {error}",
            candidate.hypothesis
        )
    })?;
    let mut advanced = checkout.clone();
    advanced.revision = carried.clone();
    candidate.worktree = Some(advanced);
    candidate.revision = Some(carried.clone());
    candidate.result = None;
    Ok(carried)
}

/// Restores the candidate worktree to the revision the carry started from.
/// Only this operation's own commit is undone, and only inside the owned
/// clean candidate allocation that was verified immediately before it.
fn restore_carried_revision(worktree: &Path, revision: &str) {
    let _ = git_text(worktree, &["reset", "--hard", revision]);
}

fn retained_revision_short(revision: &str) -> &str {
    &revision[..12.min(revision.len())]
}

/// The content identity of one committed change, exactly as the activation
/// lineage owner computes it: the status and the resulting blob identity of
/// every changed path, with rename detection disabled. Two commits that apply
/// the same change to different bases share this signature, so a retained
/// solution rebased onto the resulting baseline is recognized as the same
/// exact solution while a different resolution or an extra edit is not.
fn change_signature(repo: &Path, base: &str, revision: &str) -> Result<Vec<String>, String> {
    let raw = git_text(
        repo,
        &[
            "diff",
            "--raw",
            "--no-abbrev",
            "--no-renames",
            base,
            revision,
        ],
    )?;
    let mut signature: Vec<String> = Vec::new();
    for line in raw.lines() {
        // `:<oldmode> <newmode> <oldsha> <newsha> <status>\t<path>`
        let Some(rest) = line.strip_prefix(':') else {
            continue;
        };
        let Some((meta, path)) = rest.split_once('\t') else {
            continue;
        };
        let fields: Vec<&str> = meta.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }
        signature.push(format!("{} {} {}", fields[4], fields[3], path));
    }
    signature.sort();
    Ok(signature)
}

/// One read-only Git command whose exact stdout bytes are needed (a binary
/// patch), never a display string.
fn git_bytes(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

/// Applies one binary patch to the clean candidate worktree and its index.
/// `git apply` is atomic: a patch that does not apply cleanly changes nothing.
fn git_apply_patch(cwd: &Path, patch: &[u8]) -> Result<(), String> {
    let mut child = Command::new("git")
        .args(["apply", "--index", "-"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("git apply: {error}"))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "git apply stdin is unavailable".to_owned())?;
        stdin
            .write_all(patch)
            .map_err(|error| format!("git apply stdin: {error}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("git apply: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "the exact retained change does not apply cleanly onto the resulting baseline: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// Validates the returned committed implementation against the exact base, the
/// declared writable scope, the frozen planning artifacts and the retained
/// terminal evidence before anything reaches `candidate-ready`.
fn validate_implementation(
    run: &mut Run,
    candidate: &mut CandidateState,
    attempt: &Attempt,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    if attempt.retained.is_none() {
        let reason = format!(
            "the implementation attempt {} completed without retained terminal evidence; unverified output never reaches candidate-ready",
            attempt.id
        );
        return refused(run, notes, reason);
    }
    let Some(returned) = attempt.checkout.clone() else {
        let reason = format!(
            "the implementation attempt {} records no returned checkout, so the committed work cannot be attributed",
            attempt.id
        );
        return refused(run, notes, reason);
    };
    let Some(_checkout) = candidate.worktree.clone() else {
        let reason =
            "the candidate allocation is missing; dependent validation is refused".to_owned();
        return refused(run, notes, reason);
    };
    // Structural facts first: the returned work must be an owned, registered,
    // clean checkout whose committed revision descends from the candidate
    // branch and stays inside the declared writable scope.
    if let Err(reason) = verify_returned(candidate, attempt, &run.spec.writable_scope) {
        return refused(run, notes, reason);
    }
    // Acceptance inputs stay frozen: the committed change must still qualify
    // and its artifact digests must equal the receipt qualified before
    // implementation, checked before the branch advances.
    let receipt = match read_json::<PlanningReceipt>(
        &run.store.root().join(CANDIDATE_PLANNING_FILE),
        MAX_RUN_SPEC_BYTES,
    ) {
        Ok(receipt) => receipt,
        Err(error) => {
            let reason = format!(
                "the qualified candidate planning receipt is unavailable: {error}; candidate-ready is refused"
            );
            return refused(run, notes, reason);
        }
    };
    let returned_target = Specification {
        project: returned.clone(),
        change: candidate.change.clone(),
        store: run.spec.specification.store.clone(),
        planning_root: returned.clone(),
    };
    let openspec = OpenSpec::default();
    let current = match openspec.qualify(&returned_target, &run.spec.experiment) {
        Ok(receipt) => receipt,
        Err(error) => {
            let reason = format!(
                "the committed implementation broke the candidate's planning contract: {error}; candidate-ready is refused"
            );
            return refused(run, notes, reason);
        }
    };
    let frozen = match artifact_digests(&receipt) {
        Ok(frozen) => frozen,
        Err(reason) => return refused(run, notes, reason),
    };
    let committed = match artifact_digests(&current) {
        Ok(committed) => committed,
        Err(reason) => return refused(run, notes, reason),
    };
    if committed != frozen || current.contract_digest != receipt.contract_digest {
        let reason = "the committed implementation changed the candidate's planning artifacts; acceptance inputs stay frozen and the result is refused".to_owned();
        return refused(run, notes, reason);
    }
    // The returned commit is transferred once; a resume after a partial
    // transfer retains the already-advanced revision instead of replaying.
    let current_head = candidate
        .worktree
        .as_ref()
        .map(|checkout| checkout.revision.clone())
        .unwrap_or_default();
    let returned_head = match git_text(&returned, &["rev-parse", "HEAD"]) {
        Ok(head) => head,
        Err(error) => {
            let reason = format!("the returned checkout could not be read: {error}");
            return refused(run, notes, reason);
        }
    };
    let changed = if returned_head == current_head {
        // Already transferred by an earlier command; re-report the owned
        // candidate's scope from its allocation base.
        let base = candidate
            .worktree
            .as_ref()
            .map(|checkout| checkout.base.clone())
            .unwrap_or_default();
        match changed_paths(&returned, &format!("{base}..{returned_head}")) {
            Ok(changed) => changed,
            Err(reason) => return refused(run, notes, reason),
        }
    } else {
        match transfer_returned(run, candidate, attempt, &run.spec.writable_scope) {
            Ok(changed) => changed,
            Err(reason) => return refused(run, notes, reason),
        }
    };
    let head = candidate
        .worktree
        .as_ref()
        .map(|checkout| checkout.revision.clone())
        .unwrap_or_default();
    let returned_result = retained_result(attempt).map(|path| path.to_string_lossy().into_owned());
    board_hypothesis::record_implementation(
        &run.spec.board.bd,
        &run.spec.board.project,
        &candidate.hypothesis,
        &BoundedImplementation {
            role: board_hypothesis::HypothesisRole::Candidate,
            branch: candidate
                .worktree
                .as_ref()
                .map(|checkout| checkout.branch.clone())
                .unwrap_or_default(),
            base: candidate
                .worktree
                .as_ref()
                .map(|checkout| checkout.base.clone())
                .unwrap_or_default(),
            revision: head.clone(),
            worktree: candidate
                .worktree
                .as_ref()
                .map(|checkout| checkout.path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            runtime: None,
            baseline_runtime: None,
        },
    )
    .map_err(|error| {
        invalid(format!(
            "the validated candidate revision could not be recorded on hypothesis card {}: {error}",
            candidate.hypothesis
        ))
    })?;
    candidate.revision = Some(head.clone());
    candidate.result = returned_result.map(PathBuf::from);
    run.cursor.effect(
        EffectKind::ImplementationValidated,
        format!(
            "attempt={} hypothesis={} revision={} changed={} paths retained-result={} (reported checks are retained, not re-executed by this controller)",
            attempt.id,
            candidate.hypothesis,
            head,
            changed.len(),
            candidate
                .result
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".to_owned())
        ),
    );
    notes.push(format!(
        "candidate-ready: validated revision {} ({} changed path(s) inside the declared scope)",
        head,
        changed.len()
    ));
    Ok(())
}

/// Validates one completed conversation's returned checkout and advances the
/// owned candidate branch to its committed revision. The returned work must be
/// an owned, registered, clean worktree of this repository whose revision
/// descends from the current candidate revision; `allowed` is the writable
/// scope it may change, and the candidate's own change directory is always
/// allowed. Returns the changed paths.
fn transfer_returned(
    _run: &Run,
    candidate: &mut CandidateState,
    attempt: &Attempt,
    allowed: &[String],
) -> Result<Vec<String>, String> {
    let (head, changed) = verify_returned(candidate, attempt, allowed)?;
    let Some(checkout) = candidate.worktree.clone() else {
        return Err("the candidate allocation is missing".to_owned());
    };
    git_text(&checkout.path, &["merge", "--ff-only", &head]).map_err(|error| {
        format!(
            "the validated revision {head} could not be advanced onto the candidate branch {}: {error}",
            checkout.branch
        )
    })?;
    let advanced = CandidateCheckout {
        revision: head,
        ..checkout
    };
    task_worktree::verify_candidate_checkout(&advanced).map_err(|error| {
        format!("the candidate branch did not reach the validated revision: {error}")
    })?;
    candidate.worktree = Some(advanced);
    Ok(changed)
}

/// The read-only structural validation of one returned checkout: it is an
/// owned, registered, clean worktree of this repository, its committed
/// revision descends from the current candidate revision and every changed
/// path stays inside `allowed` plus the candidate's own change directory.
/// Returns the committed revision and the changed paths.
fn verify_returned(
    candidate: &CandidateState,
    attempt: &Attempt,
    allowed: &[String],
) -> Result<(String, Vec<String>), String> {
    let Some(checkout) = candidate.worktree.clone() else {
        return Err("the candidate allocation is missing".to_owned());
    };
    let Some(returned) = attempt.checkout.clone() else {
        return Err(format!(
            "the attempt {} records no returned checkout, so its committed work cannot be attributed",
            attempt.id
        ));
    };
    let head = git_text(&returned, &["rev-parse", "HEAD"])?;
    if head == checkout.revision {
        return Err(format!(
            "the attempt {} returned no committed revision beyond the candidate revision {}; an empty result never advances the candidate",
            attempt.id, checkout.revision
        ));
    }
    match task_worktree::worktree_reuse(&checkout.path, &returned, &checkout.path, &head, false)
        .map_err(|error| format!("the returned checkout could not be inspected: {error}"))?
    {
        WorktreeReuse::Eligible { .. } => {}
        WorktreeReuse::Blocked { kind, reason } => {
            return Err(format!(
                "the returned checkout is not eligible ({kind:?}): {reason}"
            ));
        }
    }
    if !git_ok(
        &checkout.path,
        &["merge-base", "--is-ancestor", &checkout.revision, &head],
    )
    .map_err(|error| format!("the returned ancestry could not be checked: {error}"))?
    {
        return Err(format!(
            "the returned revision {head} is not a descendant of the current candidate revision {}; a wrong-base result is refused",
            checkout.revision
        ));
    }
    let changed = changed_paths(&checkout.path, &format!("{}..{}", checkout.revision, head))?;
    changed_paths_within_scope(&changed, allowed, &candidate.change)?;
    Ok((head, changed))
}

/// Commits the named relative paths in the owned candidate worktree (used for
/// the model-free OpenSpec scaffold). Unrelated working-tree state is never
/// staged.
fn commit_worktree_paths(
    checkout: &CandidateCheckout,
    paths: &[String],
    message: &str,
) -> Result<(), String> {
    let mut add: Vec<&str> = vec!["add", "--"];
    add.extend(paths.iter().map(String::as_str));
    git_text(&checkout.path, &add)?;
    let mut commit: Vec<&str> = vec![
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--no-verify",
        "--only",
        "-m",
        message,
        "--",
    ];
    commit.extend(paths.iter().map(String::as_str));
    git_text(&checkout.path, &commit)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Small read-only Git helpers over the owner-allocated worktrees.
// ---------------------------------------------------------------------------

/// The planning artifacts of one receipt keyed by change-relative path, so
/// two checkouts of the same change compare by content and not by checkout
/// location.
fn artifact_digests(
    receipt: &PlanningReceipt,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut map = std::collections::BTreeMap::new();
    for (path, digest) in &receipt.artifacts {
        let relative = path.strip_prefix(&receipt.change_root).map_err(|_| {
            format!(
                "the qualified planning artifact {} escapes its change root; the receipt is refused",
                path.display()
            )
        })?;
        map.insert(
            relative.to_string_lossy().replace('\\', "/"),
            digest.clone(),
        );
    }
    Ok(map)
}

/// The changed paths of one committed range, normalized to forward slashes.
fn changed_paths(cwd: &Path, range: &str) -> Result<Vec<String>, String> {
    let text = git_text(
        cwd,
        &["diff", "--name-only", "--diff-filter=ACDMRTUXB", range],
    )?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.replace('\\', "/"))
        .collect())
}

fn git_text(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if output.stdout.len() > 1024 * 1024 {
        return Err(format!(
            "git {}: output exceeds the read bound",
            args.join(" ")
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_ok(cwd: &Path, args: &[&str]) -> io::Result<bool> {
    let status = Command::new("git").args(args).current_dir(cwd).status()?;
    Ok(status.success())
}

#[cfg(test)]
mod assignment_tests {
    use super::*;

    const LONG_CHANGE: &str = "add-evidence-grounded-terminal-report-intake";

    fn long_scope() -> Vec<String> {
        [
            "crates/one/src/improvement_intake_adapter.rs",
            "crates/one/src/improvement_workflow_brief.rs",
            "crates/one/src/executor_assignment_contract.rs",
            "crates/one/src/terminal_framing_recovery.rs",
            "crates/one/src/retained_evidence_index.rs",
            "crates/one/src/dispatch_scope_validation.rs",
            "crates/one/src/no_replay_recovery.rs",
        ]
        .iter()
        .map(|entry| (*entry).to_owned())
        .collect()
    }

    /// One controller fixture with ordinary long owned roots: a long change
    /// name, seven explicit writable file paths, absolute board paths and
    /// several retained evidence locators.
    fn fixture(root: &Path) -> (Run, CandidateState, Evidence) {
        let checkout = root.join("owned-checkout-with-an-ordinary-long-name");
        let change_dir = checkout.join(candidate_change_dir(LONG_CHANGE));
        fs::create_dir_all(&change_dir).unwrap();
        fs::write(change_dir.join("proposal.md"), "## Why\n\nSynthetic.\n").unwrap();
        let codex_home = root.join("codex-home-with-an-ordinary-long-name");
        let bd = codex_home.join("harness/bin/bd.exe");
        fs::create_dir_all(bd.parent().unwrap()).unwrap();
        fs::write(&bd, "fixture board executable").unwrap();
        let document = json!({
            "schema": 1,
            "run": "workflow-fixture",
            "project": checkout,
            "codex_home": codex_home,
            "board": {"bd": bd, "project": checkout},
            "specification": {
                "project": checkout,
                "change": LONG_CHANGE,
                "store": serde_json::Value::Null,
                "planning_root": checkout,
            },
            "hypothesis_item": "bdcw-card",
            "experiment": {
                "acceptance_artifact": "specs/synthetic/spec.md",
                "acceptance_heading": "#### Scenario: Synthetic case",
                "mechanism": "bounded-output",
                "counterexample": "diagnostics vanish on failure",
                "applicability": "local tool runs",
                "independent_acceptance": "the oracle checker executes",
                "meaningful_effect": "fewer repeated loads",
                "operating_conditions": "cold context",
                "comparison_policy": "matched pairs",
                "stopping_rule": "two repeats",
            },
            "base_revision": "0123456789abcdef0123456789abcdef01234567",
            "writable_scope": long_scope(),
            "runner": serde_json::Value::Null,
            "local_runner": serde_json::Value::Null,
            "qualification": serde_json::Value::Null,
            "evidence_root": serde_json::Value::Null,
            "publication_scope": ["experiment"],
            "oracle": "outcome-oracle:private-request",
            "removal": serde_json::Value::Null,
        });
        let spec: RunSpec = serde_json::from_value(document).unwrap();
        fs::write(
            root.join(SPEC_FILE),
            serde_json::to_vec_pretty(&spec).unwrap(),
        )
        .unwrap();
        let store = RunStore::open(root).unwrap();
        let cursor = Cursor::new(
            &spec.run,
            "spec-digest".to_owned(),
            checkout.clone(),
            &spec.hypothesis_item,
        );
        let run = Run {
            store,
            spec,
            cursor,
            guard: None,
        };
        let candidate = CandidateState::new("bdcw-card", LONG_CHANGE).unwrap();
        let evidence = Evidence {
            index: EvidenceIndex::default(),
            digest: "evidence-digest".to_owned(),
            listing: vec![
                "file:first-retained-locator-with-an-ordinary-length.txt".to_owned(),
                "file:second-retained-locator-with-an-ordinary-length.txt".to_owned(),
                "file:third-retained-locator-with-an-ordinary-length.txt".to_owned(),
            ],
            root: Some(root.join("retained-evidence-root-with-an-ordinary-long-name")),
        };
        (run, candidate, evidence)
    }

    /// Validates one generated document through the native structured
    /// assignment owner and returns the exact rendered brief.
    fn native_brief(document: &serde_json::Value, checkout: &Path, name: &str) -> String {
        let path = checkout
            .parent()
            .unwrap()
            .join(format!("{name}-assignment.json"));
        fs::write(&path, serde_json::to_vec_pretty(document).unwrap()).unwrap();
        let assignment = crate::executor_assignment::Assignment::load(&path)
            .unwrap_or_else(|error| panic!("{name} assignment is refused: {error}"));
        crate::executor_assignment::brief(
            &assignment,
            &crate::executor_assignment::AssignmentContext {
                checkout,
                base: "0123456789abcdef0123456789abcdef01234567",
                owner: "unit-assignment-check",
                source: checkout,
            },
        )
        .unwrap_or_else(|error| panic!("{name} brief is refused: {error}"))
    }

    #[test]
    fn generated_assignments_pass_the_native_structured_contract() {
        let temp = tempfile::tempdir().unwrap();
        let (run, candidate, evidence) = fixture(temp.path());
        let checkout = run.spec.project.clone();

        // Investigator: the objective stays bounded and every retained
        // evidence locator remains visible in the invariant items.
        let brief = native_brief(
            &investigator_assignment(&run, &evidence),
            &checkout,
            "investigator",
        );
        for locator in &evidence.listing {
            assert!(brief.contains(locator), "{brief}");
        }
        assert!(
            brief.contains("retained-evidence-root-with-an-ordinary-long-name"),
            "{brief}"
        );
        assert!(
            brief.contains(&run.spec.specification.planning_root.display().to_string()),
            "the exact specification root stays visible: {brief}"
        );

        // Planner: the complete planning reference stays visible.
        let inputs = vec![format!("{}/proposal.md", candidate_change_dir(LONG_CHANGE))];
        let brief = native_brief(
            &planner_assignment(&run, &candidate, inputs.clone(), None, None),
            &checkout,
            "planner",
        );
        assert!(brief.contains(LONG_CHANGE), "{brief}");
        assert!(brief.contains("#### Scenario: Synthetic case"), "{brief}");
        assert!(brief.contains("specs/synthetic/spec.md"), "{brief}");
        assert!(brief.contains(" show bdcw-card --json"), "{brief}");

        // Implementer: the reproducing multi-file scope stays visible and the
        // brief satisfies the native objective, item and size bounds.
        let brief = native_brief(
            &implementer_assignment(&run, &candidate, inputs),
            &checkout,
            "implementer",
        );
        assert!(brief.contains(LONG_CHANGE), "{brief}");
        for entry in &run.spec.writable_scope {
            assert!(brief.contains(entry), "{entry}: {brief}");
        }
        assert!(brief.contains(" show bdcw-card --json"), "{brief}");
        assert!(brief.contains("the oracle checker executes"), "{brief}");
    }

    #[test]
    fn a_declared_measurement_scope_reaches_the_planner_within_the_native_bounds() {
        let temp = tempfile::tempdir().unwrap();
        let (run, candidate, _evidence) = fixture(temp.path());
        fs::write(
            run.store.root().join(MEASUREMENT_SCOPE_FILE),
            serde_json::to_vec_pretty(&serde_json::json!({
                "observed_problem": "identical repeated reads waste accepted-task time",
                "investigation_scope": "the reader's repeated reads at one frozen revision",
                "measurement_question": "how much accepted-task time do they cost?",
                "workload": {
                    "operation": "cargo build -p example-reader",
                    "contract": "openspec/changes/add-synthetic/proposal.md#Measurement",
                },
                "evidence_references": ["retained outcome record: repeated reads"],
                "limits": "one local machine and one frozen source revision",
                "declaration_artifact": "proposal.md",
                "declaration_heading": "## Measurement",
            }))
            .unwrap(),
        )
        .unwrap();
        let declared = declared_measurement_scope(&run)
            .unwrap()
            .expect("the declared scope reads");
        let inputs = vec![format!("{}/proposal.md", candidate_change_dir(LONG_CHANGE))];
        let brief = native_brief(
            &planner_assignment(&run, &candidate, inputs, Some(&declared), None),
            &run.spec.project,
            "planner-measurement",
        );
        for needle in [
            "## Measurement",
            "declared measurement question: how much accepted-task time do they cost?",
            "declared workload operation: cargo build -p example-reader",
            "do not create a second hypothesis",
            "retained outcome record: repeated reads",
        ] {
            assert!(brief.contains(needle), "{needle}: {brief}");
        }
        // An oversized declaration is refused by name instead of being
        // silently shortened in the bounded brief.
        let mut oversized = serde_json::to_value(&declared).unwrap();
        oversized["limits"] = serde_json::json!("x".repeat(MAX_MEASUREMENT_FIELD_BYTES + 1));
        fs::write(
            run.store.root().join(MEASUREMENT_SCOPE_FILE),
            serde_json::to_vec(&oversized).unwrap(),
        )
        .unwrap();
        let error = declared_measurement_scope(&run).unwrap_err();
        assert!(error.to_string().contains("limits"), "{error}");
        assert!(
            error
                .to_string()
                .contains(&MAX_MEASUREMENT_FIELD_BYTES.to_string()),
            "{error}"
        );
    }

    #[test]
    fn a_removal_candidate_receives_the_reviewable_proposal_requirements() {
        let temp = tempfile::tempdir().unwrap();
        let (run, mut candidate, _evidence) = fixture(temp.path());
        let inputs = vec![format!("{}/proposal.md", candidate_change_dir(LONG_CHANGE))];
        let ordinary = native_brief(
            &planner_assignment(&run, &candidate, inputs.clone(), None, None),
            &run.spec.project,
            "planner-ordinary",
        );
        assert!(
            !ordinary.contains(REMOVAL_PROPOSAL_HEADING),
            "an ordinary candidate carries no removal requirements: {ordinary}"
        );
        candidate.removal_required = true;
        let brief = native_brief(
            &planner_assignment(&run, &candidate, inputs, None, None),
            &run.spec.project,
            "planner-removal",
        );
        assert!(brief.contains(REMOVAL_PROPOSAL_HEADING), "{brief}");
        for label in REMOVAL_PROPOSAL_CLAUSES {
            assert!(brief.contains(label), "{label}: {brief}");
        }
        for needle in [
            "never a prediction presented as a measurement",
            "prepared, not applied",
            "no removal effect happens before the recorded user decision",
            "records the reviewable proposal on the hypothesis card before any removal effect",
        ] {
            assert!(brief.contains(needle), "{needle}: {brief}");
        }
    }

    #[test]
    fn a_partial_git_move_failure_is_not_described_as_untouched_without_evidence() {
        let old = Path::new("run/candidates/card");
        let ambiguous = classify_failed_worktree_move(
            old,
            "fatal: boom",
            &FailedMoveObservation {
                old_exists: false,
                old_registered: false,
                destination_exists: true,
                destination_registered: false,
                destination_matches_identity: false,
            },
        );
        let FailedMoveClassification::Ambiguous { reason } = ambiguous else {
            panic!("a partial destination was not classified as ambiguous");
        };
        assert!(!reason.contains("untouched"), "{reason}");
        assert!(reason.contains("fatal: boom"), "{reason}");
        assert!(
            reason.contains("not claimed to have left the allocation unchanged"),
            "{reason}"
        );

        let unmoved = classify_failed_worktree_move(
            old,
            "fatal: destination missing",
            &FailedMoveObservation {
                old_exists: true,
                old_registered: true,
                destination_exists: false,
                destination_registered: false,
                destination_matches_identity: false,
            },
        );
        let FailedMoveClassification::Unmoved { reason } = unmoved else {
            panic!("a still-registered allocation was not classified as unmoved");
        };
        assert!(reason.contains("still registered"), "{reason}");
        assert!(reason.contains("commits were not moved"), "{reason}");
        assert!(!reason.contains("untouched"), "{reason}");

        let completed = classify_failed_worktree_move(
            old,
            "fatal: reported after the move",
            &FailedMoveObservation {
                old_exists: false,
                old_registered: false,
                destination_exists: true,
                destination_registered: true,
                destination_matches_identity: true,
            },
        );
        assert!(matches!(completed, FailedMoveClassification::Completed));
    }
}
