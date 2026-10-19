//! Public `improve review` preparation and resumable semantic review for one
//! retained ordinary completion.
//!
//! This adapter binds the immutable completion, its verified Git source and
//! its retained action sidecars to the shared semantic-review packet and
//! ReviewHost lifecycle. Resume reuses the shared visible dispatch/recovery
//! protocol; it does not create a qualification run or a second controller.

use harness_core::{
    build_identity,
    delivery_reflection::Purpose,
    improvement_loop::{
        AttemptRole, Cursor, MAX_RUN_SPEC_BYTES, MUTATION_LOCK_WAIT, Phase,
        QualificationBindingIdentity, ResumeReport, SemanticReviewInputs, SemanticReviewState,
        TaskInputs, WorkflowParticipant, acquire_mutation_lock, now_ms,
    },
    improvement_runtime::{self, FileIdentity},
    task_worktree::{self, AssignmentSubmission, CompletionResult, RetainedCompletion},
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use super::super::{Options, improvement_comparison as accounting, invalid, reconcile_attempts};
use super::{
    ACTION_EVIDENCE_SCHEMA, ActionStreamFacts, RetainedActionStream, ReviewHost, ReviewPacketView,
    ReviewSubject, action_evidence_from, advance_semantic_review_host, ensure_review_packet,
    load_or_install_reviewer_runtime, retained_judgment, same_retained_place, write_bytes_atomic,
};
use crate::executor_cli::{ConversationState, pool_evidence_paths, receipt_view};

const ORDINARY_REVIEW_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct OrdinaryReviewDeclaration {
    schema: u32,
    review: String,
    task: TaskInputs,
    task_prompt: FileIdentity,
    completion: OrdinaryCompletionInputs,
    lead_session: Option<String>,
    action_evidence_root: PathBuf,
    semantic_review: SemanticReviewInputs,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct OrdinaryCompletionInputs {
    codex_home: PathBuf,
    run_generation: String,
    executor_profile: String,
}

/// The exact ordinary completion identity frozen before any review effect.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct OrdinaryCompletionBinding {
    run_generation: String,
    task_name: String,
    assignment_sha256: String,
    base_revision: String,
    submission_revision: String,
    changed_paths: Vec<String>,
    checkout: PathBuf,
    session: String,
    turn: String,
    result: CompletionResult,
    role: String,
    state: String,
    executor_profile: String,
}

/// The durable review cursor, host identity and shared semantic-review state.
/// Every field is derived from frozen inputs; repeated preparation must
/// serialize the same bytes.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct OrdinaryReviewState {
    schema: u32,
    declaration_sha256: String,
    completion: OrdinaryCompletionBinding,
    cursor: Cursor,
    semantic_review: Option<SemanticReviewState>,
}

/// The ordinary-review implementation of the shared ReviewHost. The packet
/// root, reviewer runtime and action sidecars are separate owned locations;
/// no mutable pool or qualification cursor is consulted.
pub(super) struct OrdinaryReviewHost {
    review_root: PathBuf,
    reviewer_root: PathBuf,
    assignments: PathBuf,
    state_path: PathBuf,
    action_evidence_root: PathBuf,
    state: OrdinaryReviewState,
    subject: ReviewSubject,
}

impl OrdinaryReviewHost {
    fn new(
        review_root: PathBuf,
        reviewer_root: PathBuf,
        assignments: PathBuf,
        state_path: PathBuf,
        action_evidence_root: PathBuf,
        state: OrdinaryReviewState,
        subject: ReviewSubject,
    ) -> Self {
        Self {
            review_root,
            reviewer_root,
            assignments,
            state_path,
            action_evidence_root,
            state,
            subject,
        }
    }
}

impl ReviewPacketView for OrdinaryReviewHost {
    fn subject(&self) -> &ReviewSubject {
        &self.subject
    }

    fn review_revision(&self) -> Option<&str> {
        self.state
            .semantic_review
            .as_ref()
            .and_then(|review| review.review_revision.as_deref())
    }
}

impl ReviewHost for OrdinaryReviewHost {
    fn review_root(&self) -> &Path {
        &self.review_root
    }

    fn judgment_path(&self) -> PathBuf {
        self.reviewer_root.join("judgment.json")
    }

    fn assignments_dir(&self) -> PathBuf {
        self.assignments.clone()
    }

    fn dispatch_run_id(&self) -> &str {
        &self.subject.review_id
    }

    fn cursor(&self) -> &Cursor {
        &self.state.cursor
    }

    fn cursor_mut(&mut self) -> &mut Cursor {
        &mut self.state.cursor
    }

    fn review_state(&self) -> Option<&SemanticReviewState> {
        self.state.semantic_review.as_ref()
    }

    fn review_state_mut(&mut self) -> &mut SemanticReviewState {
        self.state
            .semantic_review
            .get_or_insert_with(SemanticReviewState::default)
    }

    fn set_review_state(&mut self, review: SemanticReviewState) {
        self.state.semantic_review = Some(review);
    }

    fn persist(&mut self) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.state)?;
        write_bytes_atomic(&self.state_path, &bytes)
    }

    fn runner_declared(&self) -> bool {
        // The ordinary declaration has no separate custom runner. Its validated
        // semantic-review runtime and profile are the model declaration, so a
        // prepared reviewer binding is the truthful dispatch input; the shared
        // gate still enforces every other visibility and replay condition.
        self.state
            .semantic_review
            .as_ref()
            .is_some_and(|review| review.binding.is_some())
    }

    fn set_condition(&mut self, condition: Option<String>) {
        self.state.cursor.condition = condition;
    }

    fn reviewer_home(&self) -> io::Result<PathBuf> {
        self.state
            .semantic_review
            .as_ref()
            .and_then(|review| review.home.clone())
            .ok_or_else(|| invalid("the ordinary semantic reviewer home is missing"))
    }

    fn reviewer_binding(&self) -> io::Result<QualificationBindingIdentity> {
        self.state
            .semantic_review
            .as_ref()
            .and_then(|review| review.binding.clone())
            .ok_or_else(|| invalid("the ordinary semantic reviewer binding is missing"))
    }

    fn role_prior(&self, role: AttemptRole) -> Option<WorkflowParticipant> {
        match role {
            AttemptRole::SemanticReview => self
                .state
                .semantic_review
                .as_ref()
                .and_then(|review| review.participant.clone()),
            _ => None,
        }
    }

    fn role_checkout(&self, role: AttemptRole) -> io::Result<PathBuf> {
        match role {
            AttemptRole::SemanticReview => self
                .state
                .semantic_review
                .as_ref()
                .and_then(|review| review.source.clone())
                .ok_or_else(|| invalid("the frozen review packet source is missing")),
            _ => Err(invalid(
                "the ordinary review host has no mutable ordinary dispatch checkout",
            )),
        }
    }

    fn action_evidence(
        &self,
        participants: &[super::assessment::ReviewParticipant],
    ) -> io::Result<(
        Vec<super::assessment::ReviewAction>,
        Vec<super::assessment::ReviewActionCoverage>,
        bool,
    )> {
        let root = self.action_evidence_root.clone();
        let expected_turn = self.state.completion.turn.clone();
        action_evidence_from(participants, |participant| {
            let sidecar = root.join(format!("{}.json", participant.attempt));
            let raw = root.join(format!("{}.jsonl", participant.attempt));
            let metadata = match fs::symlink_metadata(&sidecar) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            };
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.len() > MAX_RUN_SPEC_BYTES
            {
                return Ok(Some((
                    incomplete_action_stream(
                        participant,
                        format!(
                            "the retained action sidecar {} is not an ordinary bounded file; preserve it instead of recapturing actions",
                            sidecar.display()
                        ),
                    ),
                    raw,
                )));
            }
            let bytes = match fs::read(&sidecar) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Ok(Some((
                        incomplete_action_stream(
                            participant,
                            format!(
                                "the retained action sidecar {} could not be read: {error}; preserve it instead of recapturing actions",
                                sidecar.display()
                            ),
                        ),
                        raw,
                    )));
                }
            };
            let record: RetainedActionStream = match serde_json::from_slice(&bytes) {
                Ok(record) => record,
                Err(error) => {
                    return Ok(Some((
                        incomplete_action_stream(
                            participant,
                            format!(
                                "the retained action sidecar {} is not readable: {error}; preserve it instead of recapturing actions",
                                sidecar.display()
                            ),
                        ),
                        raw,
                    )));
                }
            };
            if record.schema != ACTION_EVIDENCE_SCHEMA
                || record.attempt != participant.attempt
                || record.role != participant.role
                || record.session != participant.session
            {
                return Ok(Some((
                    incomplete_action_stream(
                        participant,
                        format!(
                            "the retained action sidecar {} does not bind attempt {} role {} session {}; preserve it instead of recapturing actions",
                            sidecar.display(),
                            participant.attempt,
                            participant.role,
                            participant.session
                        ),
                    ),
                    raw,
                )));
            }
            if record.captured {
                let Some(observed_turn) = record
                    .observed_turn
                    .as_deref()
                    .filter(|turn| !turn.is_empty())
                else {
                    return Ok(Some((
                        incomplete_action_stream(
                            participant,
                            format!(
                                "the retained action sidecar {} records no completed turn identity; expected {expected_turn}",
                                sidecar.display()
                            ),
                        ),
                        raw,
                    )));
                };
                if observed_turn != expected_turn {
                    return Ok(Some((
                        incomplete_action_stream(
                            participant,
                            format!(
                                "the retained action sidecar {} observes turn {observed_turn}, not the completed turn {expected_turn}",
                                sidecar.display()
                            ),
                        ),
                        raw,
                    )));
                }
                // `source` records the original live capture path. The adjacent
                // retained copy is authenticated by its recorded digest and native
                // session/turn evidence; the original may have been released.
            }
            Ok(Some((record, raw)))
        })
    }
}

fn incomplete_action_stream(
    participant: &super::assessment::ReviewParticipant,
    note: String,
) -> RetainedActionStream {
    RetainedActionStream {
        schema: ACTION_EVIDENCE_SCHEMA,
        attempt: participant.attempt.clone(),
        role: participant.role.clone(),
        session: participant.session.clone(),
        source: None,
        captured: false,
        source_bytes: 0,
        retained_bytes: 0,
        source_sha256: None,
        truncated: false,
        observed_events: None,
        observed_malformed: None,
        observed_rounds: None,
        observed_turn: None,
        note: Some(note),
    }
}

/// `improve review` accepts one and only one of preparation or resume. Both
/// paths share the same retained declaration, packet and reviewer lifecycle;
/// resume either dispatches the one pending reviewer turn or reports its
/// retained accepted, rejected, pending or unknown outcome without replay.
pub(super) fn run(args: &[OsString]) -> io::Result<i32> {
    let options = Options::parse(
        args,
        &["--review", "--declaration"],
        &["--prepare", "--resume"],
    )?;
    let review = PathBuf::from(options.required("--review")?);
    match (options.has("--prepare"), options.has("--resume")) {
        (true, false) => {
            let declaration = PathBuf::from(options.required("--declaration")?);
            prepare(&review, &declaration)?;
            println!(
                "ordinary review prepared; next: improve review --resume --review {}",
                review.display()
            );
            Ok(0)
        }
        (false, true) => resume(&review, options.get("--declaration").map(PathBuf::from)),
        (false, false) => Err(invalid(
            "improve review requires exactly one of --prepare or --resume",
        )),
        (true, true) => Err(invalid(
            "--prepare and --resume are mutually exclusive for improve review",
        )),
    }
}

/// Capture one completed ordinary executor turn's visible action stream into
/// the ordinary-owned evidence root before the recorded pool slot can be
/// reused. First preparation reuses the shared qualification capture
/// implementation with exact completion and native receipt facts; later
/// preparation never rereads the pool. Existing sidecars are immutable, and
/// the shared review host still reports any recorded gap before dispatch.
fn capture_ordinary_action_stream(
    declaration: &OrdinaryReviewDeclaration,
    completion: &RetainedCompletion,
    previously_prepared: bool,
) -> io::Result<()> {
    let root = &declaration.action_evidence_root;
    let sidecar = root.join(format!("{}.json", completion.run_generation));
    let raw = root.join(format!("{}.jsonl", completion.run_generation));
    if sidecar.exists() || raw.exists() || previously_prepared {
        return Ok(());
    }
    let session = completion
        .session
        .as_deref()
        .filter(|session| !session.trim().is_empty())
        .ok_or_else(|| invalid("the retained ordinary completion has no executor session"))?;
    let turn = completion
        .turn
        .as_deref()
        .filter(|turn| !turn.trim().is_empty())
        .ok_or_else(|| invalid("the retained ordinary completion has no completed turn"))?;
    let (receipt, detail) = pool_evidence_paths(
        &declaration.completion.codex_home,
        &declaration.task.source,
        completion.slot,
    )?;
    let view = receipt_view(&receipt)?;
    if view.state != ConversationState::Completed {
        return Err(invalid(format!(
            "the native dispatch receipt {} for run generation {} is not completed ({:?}); preserve the retained completion instead of reading a reused, lost or live pool slot",
            receipt.display(),
            completion.run_generation,
            view.state
        )));
    }
    let identity = &view.identity;
    if identity.generation.as_deref() != Some(completion.run_generation.as_str())
        || identity.slot != Some(completion.slot)
        || identity.owner.as_deref() != Some(completion.owner.as_str())
        || identity.session.as_deref() != Some(session)
        || identity.turn.as_deref() != Some(turn)
    {
        return Err(invalid(format!(
            "the native dispatch receipt {} does not bind run generation {}, slot {}, owner {}, session {} and turn {}; preserve the retained completion instead of capturing a later pool occupant",
            receipt.display(),
            completion.run_generation,
            completion.slot,
            completion.owner,
            session,
            turn
        )));
    }
    let observation = super::super::improvement_comparison::retained_observation_at(&receipt);
    super::retain_action_stream_at(
        &sidecar,
        &raw,
        ActionStreamFacts {
            attempt: &completion.run_generation,
            role: "executor",
            session,
            source: Some(&detail),
            observation: observation.as_ref(),
        },
    )?;
    Ok(())
}

fn prepare(review: &Path, declaration_path: &Path) -> io::Result<()> {
    let review_root = ensure_owned_directory(review, "ordinary review root")?;
    let _mutation = acquire_mutation_lock(&review_root, MUTATION_LOCK_WAIT)?;
    let raw = read_bounded_file(declaration_path, "ordinary review declaration")?;
    let declaration: OrdinaryReviewDeclaration = serde_json::from_slice(&raw).map_err(|error| {
        invalid(format!(
            "the ordinary review declaration {} is not valid schema-1 JSON: {error}",
            declaration_path.display()
        ))
    })?;
    validate_declaration(&declaration)?;
    let declaration_sha256 = build_identity::hash_bytes(&raw);
    let declaration_copy = review_root.join("declaration.json");
    retain_declaration(&declaration_copy, &raw)?;

    let packet_root = review_root.join("packet");
    let reviewer_root = review_root.join("reviewer");
    let assignments = review_root.join("assignments");
    let state_path = review_root.join("state.json");
    let completion = task_worktree::load_retained_completion(
        &declaration.completion.codex_home,
        &declaration.task.source,
        &declaration.completion.run_generation,
    )?
    .ok_or_else(|| {
        invalid(format!(
            "no retained completion exists for run generation {} in {}",
            declaration.completion.run_generation,
            declaration.task.source.display()
        ))
    })?;
    let binding = validate_completion(&declaration, &completion)?;
    let subject = review_subject(&declaration, &binding)?;

    let retained_state = load_state(&state_path)?;
    let previously_prepared = retained_state.is_some();
    let state = match retained_state {
        Some(state) => state,
        None => OrdinaryReviewState {
            schema: ORDINARY_REVIEW_SCHEMA,
            declaration_sha256: declaration_sha256.clone(),
            completion: binding.clone(),
            cursor: fresh_cursor(&declaration, &declaration_sha256, &review_root),
            semantic_review: None,
        },
    };
    validate_state(&state, &declaration_sha256, &binding, &declaration.review)?;
    if state.semantic_review.is_none()
        && (packet_root.exists() || reviewer_root.exists() || assignments.exists())
    {
        return Err(invalid(format!(
            "partial ordinary review preparation exists at {} without state.json; preserve the owned review state and recover it instead of starting a second review",
            review_root.display()
        )));
    }

    let mut host = OrdinaryReviewHost::new(
        packet_root,
        reviewer_root,
        assignments,
        state_path,
        declaration.action_evidence_root.clone(),
        state,
        subject,
    );
    capture_ordinary_action_stream(&declaration, &completion, previously_prepared)?;
    prepare_reviewer(&mut host, &declaration)?;
    host.persist()?;

    let packet = ensure_review_packet(&mut host, &declaration.semantic_review)?;
    if let Some(gap) = packet.action_gap {
        let condition = format!(
            "the retained ordinary completion action view is missing or incomplete ({gap}); no reviewer turn was dispatched and no complete packet is claimed; restore the retained native detail stream"
        );
        host.set_condition(Some(condition.clone()));
        host.persist()?;
        return Err(invalid(condition));
    }
    Ok(())
}

fn resume(review: &Path, declaration_path: Option<PathBuf>) -> io::Result<i32> {
    let review_root = ensure_owned_directory(review, "ordinary review root")?;
    let _mutation = acquire_mutation_lock(&review_root, MUTATION_LOCK_WAIT)?;
    let state_path = review_root.join("state.json");
    let state = load_state(&state_path)?.ok_or_else(|| {
        invalid(format!(
            "no prepared ordinary review state exists at {}; run improve review --prepare first",
            state_path.display()
        ))
    })?;

    let declaration_copy = review_root.join("declaration.json");
    let retained = read_bounded_file(&declaration_copy, "retained ordinary review declaration")?;
    if let Some(path) = declaration_path {
        let supplied = read_bounded_file(&path, "ordinary review declaration")?;
        if supplied != retained {
            return Err(invalid(
                "the ordinary review declaration changed after preparation; preserve the frozen review and declaration",
            ));
        }
    }
    let declaration: OrdinaryReviewDeclaration =
        serde_json::from_slice(&retained).map_err(|error| {
            invalid(format!(
                "the retained ordinary review declaration {} is not valid schema-1 JSON: {error}",
                declaration_copy.display()
            ))
        })?;
    validate_declaration(&declaration)?;
    let declaration_sha256 = build_identity::hash_bytes(&retained);

    let completion = task_worktree::load_retained_completion(
        &declaration.completion.codex_home,
        &declaration.task.source,
        &declaration.completion.run_generation,
    )?
    .ok_or_else(|| {
        invalid(format!(
            "the retained ordinary completion for run generation {} is missing; preserve the review and restore the exact completion",
            declaration.completion.run_generation
        ))
    })?;
    let binding = validate_completion(&declaration, &completion)?;
    let subject = review_subject(&declaration, &binding)?;
    validate_state(&state, &declaration_sha256, &binding, &declaration.review)?;

    let reviewer_root = review_root.join("reviewer");
    let runtime_path = reviewer_root.join("runtime.json");
    read_bounded_file(
        &runtime_path,
        "retained ordinary reviewer runtime receipt",
    )
    .map_err(|error| {
        invalid(format!(
            "the ordinary reviewer runtime receipt is missing or changed at {}; run improve review --prepare again before resuming: {error}",
            runtime_path.display()
        ))
    })?;
    if state.semantic_review.is_none() {
        return Err(invalid(
            "the ordinary review has no prepared reviewer state; run improve review --prepare before resuming",
        ));
    }

    let mut host = OrdinaryReviewHost::new(
        review_root.join("packet"),
        reviewer_root,
        review_root.join("assignments"),
        state_path,
        declaration.action_evidence_root.clone(),
        state,
        subject,
    );
    // Resume may load the retained runtime, but it must never install a
    // replacement after preparation. The receipt check above makes the shared
    // loader take its existing-runtime branch; every declaration, completion,
    // criteria and runtime binding is validated before this point.
    prepare_reviewer(&mut host, &declaration)?;
    host.persist()?;

    let mut report = ResumeReport::default();
    reconcile_attempts(&review_root, &mut host.state.cursor, now_ms(), &mut report);
    host.persist()?;

    match advance_semantic_review_host(&mut host, &declaration.semantic_review)? {
        super::SemanticReviewOutcome::Settled => {
            let review = host.state.semantic_review.clone().ok_or_else(|| {
                invalid("the settled ordinary review has no retained semantic review state")
            })?;
            let acceptance = retained_judgment(&host, &review, &declaration.semantic_review)?
                .ok_or_else(|| {
                    invalid("the settled ordinary judgment is not an accepted or rejected result")
                })?;
            let result = match acceptance.kind.as_str() {
                "accept" => "accepted",
                "reject" => "rejected",
                other => {
                    return Err(invalid(format!(
                        "the retained ordinary judgment kind {other:?} is not accepted or rejected"
                    )));
                }
            };
            println!(
                "ordinary review {result}; judgment={} sha256={} findings={} unresolved_issues={}",
                acceptance.judgment.display(),
                acceptance.sha256,
                acceptance.findings,
                acceptance.unresolved_issues
            );
            Ok(0)
        }
        super::SemanticReviewOutcome::Notes(notes) => {
            if !report.settled.is_empty() {
                println!(
                    "ordinary review reconciled settled attempts: {}",
                    report.settled.join(", ")
                );
            }
            for note in notes {
                println!("ordinary review pending: {note}");
            }
            Ok(0)
        }
    }
}

fn fresh_cursor(
    declaration: &OrdinaryReviewDeclaration,
    declaration_sha256: &str,
    review_root: &Path,
) -> Cursor {
    let mut cursor = Cursor::new(
        &declaration.review,
        declaration_sha256.to_owned(),
        review_root.to_path_buf(),
        &declaration.task.name,
    );
    cursor.phase = Phase::Idle;
    cursor.updated_ms = 0;
    cursor
}

fn review_subject(
    declaration: &OrdinaryReviewDeclaration,
    binding: &OrdinaryCompletionBinding,
) -> io::Result<ReviewSubject> {
    let participant = super::assessment::ReviewParticipant {
        attempt: binding.run_generation.clone(),
        role: binding.role.clone(),
        session: binding.session.clone(),
        profile: binding.executor_profile.clone(),
        state: binding.state.clone(),
    };
    Ok(ReviewSubject {
        review_id: declaration.review.clone(),
        task_name: declaration.task.name.clone(),
        task_prompt_path: declaration.task_prompt.path.clone(),
        task_prompt_sha256: declaration.task_prompt.sha256.clone(),
        writable_scope: declaration.task.writable_scope.clone(),
        report: binding.result.locator.clone(),
        report_sha256: binding.result.sha256.clone(),
        submission_revision: binding.submission_revision.clone(),
        base_revision: binding.base_revision.clone(),
        // The packet is materialized from immutable Git objects in the
        // declared source repository, not from whatever HEAD a reused
        // checkout happens to contain now.
        checkout: declaration.task.source.clone(),
        changed_paths: binding.changed_paths.clone(),
        participants: vec![participant],
        lead_session: declaration.lead_session.clone(),
        executor_session: Some(binding.session.clone()),
    })
}

fn prepare_reviewer(
    host: &mut OrdinaryReviewHost,
    declaration: &OrdinaryReviewDeclaration,
) -> io::Result<()> {
    let runtime_path = host.reviewer_root.join("runtime.json");
    let mut review = host.state.semantic_review.take().unwrap_or_default();
    if let Some(recorded) = &review.runtime
        && recorded != &runtime_path
    {
        return Err(invalid(
            "the retained ordinary reviewer runtime receipt changed; preserve the review instead of repointing it",
        ));
    }
    let mut protected = vec![
        declaration.task.source.clone(),
        declaration.task_prompt.path.clone(),
        declaration.semantic_review.criteria.path.clone(),
    ];
    if let Some(config) = &declaration.semantic_review.runtime.native_config {
        protected.push(config.path.clone());
    }
    let reviewer = load_or_install_reviewer_runtime(
        &host.reviewer_root,
        &declaration.semantic_review,
        protected,
    )?;
    if let Some(recorded) = &review.home
        && !same_retained_place(recorded, &reviewer.home)?
    {
        return Err(invalid(
            "the retained ordinary reviewer home changed; preserve the installed home instead of selecting a fallback",
        ));
    }
    let binding = QualificationBindingIdentity::of(&harness_core::orchestration_config::binding(
        &reviewer.home,
        &declaration.semantic_review.profile,
    )?);
    if binding
        .model
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
        || binding
            .reasoning_effort
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err(invalid(
            "the ordinary semantic reviewer profile must resolve an explicit model and reasoning effort",
        ));
    }
    if review.binding.as_ref().is_some_and(|old| old != &binding) {
        return Err(invalid(
            "the ordinary semantic reviewer model/provider/effort changed after preparation; preserve the review instead of selecting a fallback",
        ));
    }
    let trusted: Vec<PathBuf> = host
        .cursor()
        .attempts
        .iter()
        .filter(|attempt| attempt.role == AttemptRole::SemanticReview)
        .filter_map(|attempt| attempt.checkout.clone())
        .collect();
    improvement_runtime::verify_consumption_with_trust(&reviewer, &trusted)?;
    review.runtime = Some(runtime_path);
    review.home = Some(reviewer.home);
    review.binding = Some(binding);
    host.state.semantic_review = Some(review);
    Ok(())
}

fn validate_declaration(declaration: &OrdinaryReviewDeclaration) -> io::Result<()> {
    if declaration.schema != ORDINARY_REVIEW_SCHEMA {
        return Err(invalid(format!(
            "ordinary review declares schema {}; this controller reads schema {ORDINARY_REVIEW_SCHEMA}",
            declaration.schema
        )));
    }
    if declaration.review.trim().is_empty() {
        return Err(invalid("the ordinary review identity must not be empty"));
    }
    declaration.task.validate()?;
    declaration.semantic_review.validate()?;
    if !declaration.task_prompt.path.is_absolute()
        || !declaration.task_prompt.path.is_file()
        || fs::metadata(&declaration.task_prompt.path)?.len() > MAX_RUN_SPEC_BYTES
        || build_identity::hash_file(&declaration.task_prompt.path)?
            != declaration.task_prompt.sha256
    {
        return Err(invalid(
            "the ordinary task prompt is missing, oversized or changed; preserve the exact task bytes",
        ));
    }
    if !declaration.action_evidence_root.is_absolute() {
        return Err(invalid(
            "the ordinary action-evidence root must be an absolute directory",
        ));
    }
    match fs::symlink_metadata(&declaration.action_evidence_root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(invalid(
                "the ordinary action-evidence root is not an ordinary directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if !declaration.completion.codex_home.is_absolute()
        || declaration.completion.run_generation.trim().is_empty()
        || declaration.completion.executor_profile.trim().is_empty()
    {
        return Err(invalid(
            "the ordinary completion home, run generation and executor profile must be explicit",
        ));
    }
    // Validate relative source paths and reserved packet names before any
    // reviewer state or runtime is created.
    super::review_source_paths(&declaration.semantic_review)?;
    Ok(())
}

fn validate_completion(
    declaration: &OrdinaryReviewDeclaration,
    completion: &RetainedCompletion,
) -> io::Result<OrdinaryCompletionBinding> {
    if !completion.eligibility.eligible {
        return Err(invalid(format!(
            "the retained ordinary completion is not eligible: {}",
            completion.eligibility.reasons.join("; ")
        )));
    }
    if completion.role.as_deref() != Some("executor")
        || completion.state != "completed"
        || completion.exit_code != Some(0)
    {
        return Err(invalid(
            "the retained ordinary completion must be a completed executor run with exit code 0",
        ));
    }
    if completion.task.as_deref() != Some(declaration.task.name.as_str()) {
        return Err(invalid(format!(
            "the retained completion is bound to task {:?}, not {}",
            completion.task, declaration.task.name
        )));
    }
    let assignment = completion.assignment.as_ref().ok_or_else(|| {
        invalid("the retained ordinary completion has no structured assignment binding")
    })?;
    if assignment.issue != declaration.task.name {
        return Err(invalid(format!(
            "the retained assignment issue {} does not match task {}",
            assignment.issue, declaration.task.name
        )));
    }
    if assignment.effective_purpose()? != Some(Purpose::Ordinary) {
        return Err(invalid(
            "the retained assignment is not an ordinary board assignment",
        ));
    }
    let submitted = completion.submitted_assignment.as_ref().ok_or_else(|| {
        invalid("the retained ordinary completion has no submitted assignment identity")
    })?;
    if submitted.kind != AssignmentSubmission::STRUCTURED
        || submitted.assignment_sha256.as_deref() != Some(assignment.assignment_sha256.as_str())
    {
        return Err(invalid(
            "the retained ordinary completion did not submit the bound structured assignment and hash",
        ));
    }
    let session = completion
        .session
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid("the retained ordinary completion has no executor session"))?;
    let turn = completion
        .turn
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid("the retained ordinary completion has no completed turn"))?;
    let result = completion
        .result
        .clone()
        .ok_or_else(|| invalid("the retained ordinary completion has no result artifact"))?;
    if !result.locator.is_absolute()
        || !result.locator.is_file()
        || fs::metadata(&result.locator)?.len() != result.bytes
        || build_identity::hash_file(&result.locator)? != result.sha256
    {
        return Err(invalid(
            "the retained ordinary completion result is missing or changed",
        ));
    }
    let source_revision = completion
        .source
        .revision
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid("the retained ordinary completion has no source revision"))?;
    if completion.source.dirty != Some(false) {
        return Err(invalid(
            "the retained ordinary completion source was not observed clean",
        ));
    }
    if !same_retained_place(
        Path::new(&assignment.source.repo_root),
        &declaration.task.source,
    )? {
        return Err(invalid(
            "the retained assignment source repository does not match the declared task source",
        ));
    }
    let base_revision = resolve_commit(&declaration.task.source, &declaration.task.revision)?;
    let assignment_base = resolve_commit(&declaration.task.source, &assignment.source.commit)?;
    if base_revision != assignment_base {
        return Err(invalid(
            "the retained assignment base differs from the declared task revision",
        ));
    }
    let submission_revision = resolve_commit(&declaration.task.source, &source_revision)?;
    accounting::git(
        &declaration.task.source,
        &[
            "merge-base",
            "--is-ancestor",
            &base_revision,
            &submission_revision,
        ],
    )
    .map_err(|error| {
        invalid(format!(
            "the retained completion is not descended from the declared task base: {error}"
        ))
    })?;
    let diff_range = format!("{base_revision}..{submission_revision}");
    let diff = accounting::git(
        &declaration.task.source,
        &["diff", "--name-only", "--no-renames", "-z", &diff_range],
    )
    .map_err(|error| {
        invalid(format!(
            "the ordinary source diff could not be read: {error}"
        ))
    })?;
    let changed_paths: Vec<String> = diff
        .split('\0')
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    validate_changed_scope(declaration, &changed_paths)?;
    for source in super::review_source_paths(&declaration.semantic_review)? {
        let object = format!("{submission_revision}:{}", source.declared);
        let kind = accounting::git(&declaration.task.source, &["cat-file", "-t", &object])
            .map_err(|error| {
                invalid(format!(
                    "the declared review source {} is not a committed object at {}: {error}",
                    source.declared, submission_revision
                ))
            })?;
        if kind.trim() != "blob" {
            return Err(invalid(format!(
                "the declared review source {} is not a file at {}",
                source.declared, submission_revision
            )));
        }
    }
    Ok(OrdinaryCompletionBinding {
        run_generation: declaration.completion.run_generation.clone(),
        task_name: declaration.task.name.clone(),
        assignment_sha256: assignment.assignment_sha256.clone(),
        base_revision,
        submission_revision,
        changed_paths,
        checkout: declaration.task.source.clone(),
        session,
        turn,
        result,
        role: "executor".to_owned(),
        state: "completed".to_owned(),
        executor_profile: declaration.completion.executor_profile.clone(),
    })
}

fn validate_changed_scope(
    declaration: &OrdinaryReviewDeclaration,
    changed_paths: &[String],
) -> io::Result<()> {
    if declaration.task.read_only {
        if !changed_paths.is_empty() {
            return Err(invalid(
                "a read-only ordinary task has committed changes at its submitted revision",
            ));
        }
        return Ok(());
    }
    for path in changed_paths {
        let in_scope = declaration.task.writable_scope.iter().any(|scope| {
            let scope = scope.trim_matches('/');
            path == scope
                || path
                    .strip_prefix(scope)
                    .is_some_and(|rest| rest.starts_with('/'))
        });
        if !in_scope {
            return Err(invalid(format!(
                "the ordinary submission changed {path}, which is outside the declared writable scope"
            )));
        }
    }
    Ok(())
}

fn resolve_commit(repository: &Path, reference: &str) -> io::Result<String> {
    let text = accounting::git(
        repository,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )
    .map_err(|error| {
        invalid(format!(
            "Git revision {reference} could not be resolved: {error}"
        ))
    })?;
    let commit = text.trim();
    if commit.is_empty() {
        return Err(invalid(format!(
            "Git revision {reference} resolved to an empty commit"
        )));
    }
    Ok(commit.to_owned())
}

fn load_state(path: &Path) -> io::Result<Option<OrdinaryReviewState>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_RUN_SPEC_BYTES
    {
        return Err(invalid(format!(
            "the ordinary review state {} is not an ordinary bounded file",
            path.display()
        )));
    }
    let bytes = fs::read(path)?;
    let state: OrdinaryReviewState = serde_json::from_slice(&bytes).map_err(|error| {
        invalid(format!(
            "the ordinary review state {} is not valid schema-1 JSON: {error}",
            path.display()
        ))
    })?;
    Ok(Some(state))
}

fn validate_state(
    state: &OrdinaryReviewState,
    declaration_sha256: &str,
    completion: &OrdinaryCompletionBinding,
    review: &str,
) -> io::Result<()> {
    if state.schema != ORDINARY_REVIEW_SCHEMA
        || state.declaration_sha256 != declaration_sha256
        || state.completion != *completion
        || state.cursor.run != review
    {
        return Err(invalid(
            "the retained ordinary review state does not bind this declaration and completion; preserve it instead of repointing the review",
        ));
    }
    Ok(())
}

fn read_bounded_file(path: &Path, label: &str) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_RUN_SPEC_BYTES
    {
        return Err(invalid(format!(
            "the {label} {} is not an ordinary bounded file",
            path.display()
        )));
    }
    fs::read(path)
}

fn ensure_owned_directory(path: &Path, label: &str) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid(format!("the {label} must be an absolute path")));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(invalid(format!(
                "the {label} {} is not an ordinary directory",
                path.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::create_dir_all(path)?;
    Ok(path.to_path_buf())
}

fn retain_declaration(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(invalid(format!(
                    "the retained ordinary declaration {} is not an ordinary file",
                    path.display()
                )));
            }
            if fs::read(path)? != bytes {
                return Err(invalid(format!(
                    "the ordinary review declaration changed after preparation at {}; preserve the frozen declaration and start a new review identity",
                    path.display()
                )));
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => write_bytes_atomic(path, bytes),
        Err(error) => Err(error),
    }
}
