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
//!    dispatched only after the change qualifies;
//! 4. the returned implementation checkout is validated against the exact
//!    committed base, the declared writable scope and the frozen planning
//!    artifacts, transferred onto the candidate branch and retained as
//!    `candidate-ready`.
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
    CandidateState, IntakeState, OutcomeRecord, candidate_change_dir, candidate_change_name,
    changed_paths_within_scope, frozen_candidate_removal_digest,
};
use harness_core::improvement_spec::{OpenSpec, PlanningReceipt, Specification};
use harness_core::task_worktree::{self, CandidateCheckout, WorktreeReuse};
use std::fs;
use std::process::Command;

/// The run-local qualified receipt of the selected candidate's own OpenSpec
/// change. The run's declared `planning.json` stays the frozen anchor receipt.
const CANDIDATE_PLANNING_FILE: &str = "candidate-planning.json";
/// Bounds for the deterministic evidence index the controller builds from the
/// declared local evidence root and its own retained attempt evidence.
const MAX_EVIDENCE_ROOT_FILES: usize = 24;
const MAX_RETAINED_EVIDENCE_ITEMS: usize = 32;
const MAX_EVIDENCE_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_LISTED_EVIDENCE: usize = 6;
const MAX_ASSIGNMENT_BYTES: usize = 256 * 1024;
const MAX_ASSIGNMENT_INPUTS: usize = 32;

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
                super::improvement_comparison::advance(run, &mut notes)?;
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
                super::improvement_comparison::advance(run, &mut notes)?;
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
    if run.cursor.phase == Phase::CandidateReady {
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
/// never forced. A recorded legacy allocation that still nests inside the run
/// state cannot reach its own planner, so an inactive, verified, preserved one
/// is relocated through the worktree owner's own Git operation and anything
/// else is refused without touching it. A move whose board publication or
/// cursor save was interrupted is reconciled on the next resume only when Git
/// registers that same branch, base and revision at the owner-assigned
/// destination; ambiguous, dirty, active or mismatched state is not adopted.
fn ensure_allocation(
    run: &mut Run,
    candidate: &mut CandidateState,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let path = match candidate_checkout_root(run) {
        Ok(root) => root.join(&candidate.hypothesis),
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
        return relocate_recorded_allocation(run, candidate, checkout, &path, notes);
    }
    let branch = format!("improve/{}/{}", run.spec.run, candidate.hypothesis);
    let checkout = if path.exists() {
        let active = run
            .cursor
            .attempts
            .iter()
            .any(|attempt| attempt.state.is_in_flight());
        match task_worktree::worktree_reuse(
            &run.spec.project,
            &path,
            &run.spec.project,
            &run.spec.base_revision,
            active,
        )? {
            WorktreeReuse::Eligible { revision } => {
                // Keep the existing allocation's own dedicated branch; a
                // detached or foreign checkout is refused instead of being
                // adopted under this run's branch name.
                let existing = match git_text(&path, &["rev-parse", "--abbrev-ref", "HEAD"]) {
                    Ok(branch) if branch != "HEAD" && !branch.trim().is_empty() => branch,
                    Ok(_) => {
                        return refused(
                            run,
                            notes,
                            format!(
                                "the preserved candidate worktree {} is on a detached HEAD; it is not a dedicated candidate branch and is left untouched",
                                path.display()
                            ),
                        );
                    }
                    Err(error) => {
                        return refused(
                            run,
                            notes,
                            format!(
                                "the preserved candidate worktree {} branch could not be read: {error}",
                                path.display()
                            ),
                        );
                    }
                };
                CandidateCheckout {
                    source: run.spec.project.clone(),
                    path: path.clone(),
                    branch: existing,
                    base: revision.clone(),
                    revision,
                }
            }
            WorktreeReuse::Blocked { kind, reason } => {
                let reason = format!(
                    "the recorded candidate worktree {} cannot be reused ({kind:?}): {reason}",
                    path.display()
                );
                return refused(run, notes, reason);
            }
        }
    } else {
        match task_worktree::allocate_candidate_checkout(
            &run.spec.project,
            &path,
            &branch,
            &run.spec.base_revision,
        ) {
            Ok(checkout) => checkout,
            Err(error) => {
                let reason = format!(
                    "the candidate branch {branch} could not be allocated from the frozen base {}: {error}",
                    run.spec.base_revision
                );
                return refused(run, notes, reason);
            }
        }
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
    run.cursor.effect(
        EffectKind::CandidateAllocated,
        format!(
            "hypothesis={} branch={} base={} worktree={}",
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
    // conversation; only a change that does not qualify triggers planning.
    match openspec.qualify(&target, &run.spec.experiment) {
        Ok(receipt) => {
            return store_candidate_receipt(run, candidate, &receipt, notes);
        }
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

fn store_candidate_receipt(
    run: &mut Run,
    candidate: &mut CandidateState,
    receipt: &PlanningReceipt,
    notes: &mut Vec<String>,
) -> io::Result<()> {
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
        Ok(receipt) => store_candidate_receipt(run, candidate, &receipt, notes),
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
    let assignment = planner_assignment(run, candidate, inputs);
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
    let objective = format!(
        "Author the complete OpenSpec change {} for the selected hypothesis card {} using the installed OpenSpec CLI in this checkout, and keep the artifacts consistent with the card's mechanism, conditions, predicted effect, counterexample and acceptance. The card read and the run's predeclared acceptance requirements are recorded in the invariants. Run `openspec validate {} --strict --no-interactive` until it passes, then commit the change and leave the tree clean; do not edit product source.",
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
    json!({
        "schema": 1,
        "objective": objective,
        "inputs": inputs,
        "outputs": outputs,
        "invariants": [
            "only the candidate's own OpenSpec change directory is written; product source stays untouched",
            "the installed OpenSpec workflow definitions and schemas are never edited",
            "the authored change is committed in this checkout and the tree is left without uncommitted or untracked files",
            format!("the implementation conversation that follows must find a strictly valid change for {}", candidate.change),
            card_read,
            card_project,
            format!("the run's predeclared acceptance section is titled '{acceptance_heading}'"),
            format!("the predeclared acceptance section must appear at {} under the change", acceptance_artifact.display()),
        ],
        "acceptance": [
            format!("`openspec validate {} --strict --no-interactive` passes inside this checkout", candidate.change),
            format!("the change contains proposal, requirements, design and tasks plus the predeclared acceptance section '{}'", acceptance_heading),
            "one committed revision contains exactly the authored change and the working tree is clean",
        ],
        "consumer": "the improvement controller (codex-harness improve)",
        "escalate": [],
    })
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
            "attempt={attempt_id} role={} owner={owner} title=\"{title}\" assignment={}",
            role.as_str(),
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
        "the final message is exactly one JSON object: {\"schema\":1,\"candidates\":[{\"mechanism\":\"<=96-char token\",\"conditions\":\"<=96-char token\",\"observation\":\"<retained locator>\",\"predicted\":\"<=256 chars\",\"counterexample\":\"<=256 chars\",\"acceptance\":\"<=256 chars\",\"spec\":\"<own OpenSpec change reference>\",\"basis\":\"<retained locator>\",\"treatment\":\"addition\",\"evidence\":[{\"locator\":\"<retained locator>\",\"kind\":\"observed\"}],\"next_check\":\"optional\"}],\"idle_reason\":\"why no candidate is grounded or null\"}; at most 3 candidates".to_owned(),
        "every candidate cites at least one observed retained locator; intake refuses an ungrounded or prediction-only citation".to_owned(),
        "the spec field names the candidate's own OpenSpec change under the run's openspec/changes planning root; an existing linked change is valid as it stands, and the controller qualifies it - preparing and authoring a missing change - before any implementation".to_owned(),
        "this conversation edits no file and dispatches no other model work".to_owned(),
    ];
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
        "Bounded improvement investigation for run {}. Inspect this checkout's source and every retained evidence locator recorded in the invariants. Your final message is ONLY the JSON investigator report described in the other invariants; the controller consumes it through grounded intake. Do not edit source and start no other model or paid calls.",
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
    let facts = dispatch_facts_for(run, AttemptRole::Implementer)?;
    match dispatch_gate(&run.cursor, AttemptRole::Implementer, &facts) {
        DispatchGate::Ready => dispatch_implementer(run, candidate, notes),
        DispatchGate::Blocked { reason } => block(run, notes, reason),
    }
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

        // Planner: the complete planning reference stays visible.
        let inputs = vec![format!("{}/proposal.md", candidate_change_dir(LONG_CHANGE))];
        let brief = native_brief(
            &planner_assignment(&run, &candidate, inputs.clone()),
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
