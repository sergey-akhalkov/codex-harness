//! Benefit-gated mainline integration and experimental-baseline activation.
//!
//! This owner is the only place where an evidenced candidate may leave its
//! branch: it re-reads the current Beads decision and any applicable removal
//! authority, runs the declared native combined-tree check through the
//! existing process owner in the candidate's owned worktree, fast-forwards
//! only the exact evaluated revision into the accepted mainline, and activates
//! the re-verified prepared candidate runtime as the run's experimental
//! baseline. Benefit decision, removal authorization, integration/checking,
//! experimental activation and live publication stay distinct: a benefit-gate
//! adoption is not consent, an integration receipt is not publication, and a
//! selection receipt is not a live installation.
//!
//! # Controller call order
//!
//! After the sequential prepared-arm driver independently accepted both real
//! workload attempts and the unchanged supervisor applied the frozen policy:
//!
//! 1. Publish the supported decision with `benefit_gate::publish_decision`
//!    (or confirm the identical retry).
//! 2. [`integrate`] with the explicit [`RunSpec`], the frozen
//!    [`ExperimentBindings`], the retained [`PolicyEvaluation`], the declared
//!    combined-tree check and the removal digest frozen at run start.
//!    `Integrated` means this call moved the mainline to the exact evaluated
//!    revision, `Confirmed` means the exact effect was already observed and
//!    nothing was replayed, and `Blocked` names the missing fact without
//!    changing anything. The newest board decision and removal authority and
//!    the exact clean Git identities are re-read after the declared check and
//!    immediately before the effect, so a withdrawal, a moved mainline or a
//!    changed candidate tree during a long check blocks without mutation. A
//!    removal the run declares in its spec and a reviewable removal proposal
//!    the evaluated hypothesis card records itself resolve through the same
//!    gate, so an omitted or renamed treatment declaration cannot withdraw a
//!    capability without the user's scoped decision. A run whose frozen
//!    candidate state declares a removal treatment is refused the same way
//!    while the evaluated card records no reviewable proposal to decide on.
//! 3. [`activate`] with the returned [`IntegrationReceipt`], the installed
//!    candidate [`ArmRuntime`] and the run's owned native state: it re-verifies
//!    the consumed installation identity, re-reads the same decision and
//!    removal authority, and selects the prepared candidate variant as the
//!    experimental baseline. Live publication stays with the installation
//!    lifecycle and is never performed here.
//!
//! # Boundaries
//!
//! No model call is made, no board record is written, no candidate file is
//! modified and no caller tree is reset or cleaned. The check runs in the
//! candidate's owned worktree before any mainline effect; the integration is a
//! fast-forward of the exact evaluated revision, refused when the mainline
//! moved, the candidate changed or the tree is dirty. A changed base or
//! candidate cannot inherit the recorded benefit evidence: the newest board
//! decision must match the retained evaluation for the exact experiment,
//! evaluated revisions and acceptance. Receipts are returned for the
//! controller's run store; only the declared check output is written, under
//! the caller-supplied evidence directory.

use crate::{
    benefit_gate, board_feedback, board_hypothesis, build_identity, improvement_experiment,
    improvement_loop, improvement_policy, improvement_runtime, process, task_worktree,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// Schema of the retained integration and activation receipts.
pub const RECEIPT_SCHEMA: u32 = 1;

/// How long the process owner waits for a stopped check's descendants before
/// reporting the cleanup as failed.
const CHECK_CLEANUP: Duration = Duration::from_secs(30);

fn invalid(detail: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("improvement activation: {detail}"),
    )
}

fn blocked(pending: bool, reason: impl Into<String>) -> Blocked {
    Blocked {
        pending,
        reason: reason.into(),
        check: None,
    }
}

/// A resolved gate: either the evidence is complete and the effect may be
/// attempted, or the exact missing fact is reported without any effect.
enum Gate<T> {
    Ready(T),
    Blocked(Box<Blocked>),
}

impl<T> Gate<T> {
    fn blocked(blocked: Blocked) -> Self {
        Self::Blocked(Box::new(blocked))
    }
}

/// Why an integration or activation effect was not performed. Nothing was
/// changed, and the reason names the fact or decision the next action needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Blocked {
    /// True when the effect waits for an explicit user/board decision (a
    /// removal approval, its scope, or an extended publication scope), false
    /// when evidence, checks or observed state must change first.
    pub pending: bool,
    pub reason: String,
    /// The retained combined-tree check receipt, when the block is a failed
    /// check on the exact evaluated revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<CheckReceipt>,
}

/// The declared native combined-tree check. The program is an explicit
/// absolute executable path; no PATH lookup or shell resolution happens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub timeout: Duration,
}

/// One retained check output stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckOutput {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

/// The retained outcome of one declared combined-tree check executed on the
/// exact candidate revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckReceipt {
    pub program: PathBuf,
    /// Digest of the checker program at the time the check ran. Reusing this
    /// receipt requires the same program bytes, so a changed checker cannot
    /// authorize activation or skip a repeated check.
    pub program_sha256: String,
    pub args: Vec<String>,
    /// The exact revision whose tree was checked.
    pub revision: String,
    /// The owned allocation the check ran in.
    pub cwd: PathBuf,
    /// 0 for a passed check; 124 timeout, 125 observed memory limit, 130
    /// cancellation, otherwise the actual process exit code.
    pub exit_code: u32,
    /// `exited`, `timeout`, `memory-limit` or `cancelled`.
    pub reason: String,
    pub duration_ms: u64,
    pub stdout: CheckOutput,
    pub stderr: CheckOutput,
}

impl CheckReceipt {
    /// True only when the check ran to completion and exited successfully.
    pub fn passed(&self) -> bool {
        self.reason == "exited" && self.exit_code == 0
    }
}

/// One integration attempt's explicit inputs. Everything the effect binds to
/// is supplied by the unchanged supervisor; nothing is discovered from the
/// candidate's writable scope.
#[derive(Debug)]
pub struct IntegrationRequest<'a> {
    /// The explicit run inputs; its `publication_scope` must permit
    /// integration, and it declares any removal treatment.
    pub spec: &'a improvement_loop::RunSpec,
    /// The frozen experiment binding the measured pair was prepared under.
    pub bindings: &'a improvement_experiment::ExperimentBindings,
    /// The frozen policy evaluation applied by the unchanged supervisor to
    /// the retained paired attempts and independent acceptance.
    pub evaluation: &'a improvement_policy::PolicyEvaluation,
    /// The experiment identity the decision record names.
    pub experiment: String,
    /// True when the run's frozen candidate state declares this treatment a
    /// removal, even though the spec names no removal scope: the effect then
    /// requires a current reviewable removal proposal on the evaluated card
    /// plus its covering user decision.
    pub removal_required: bool,
    /// The removal proposal digest frozen at run start, when the run declares
    /// a removal treatment.
    pub frozen_removal: Option<String>,
    /// The accepted mainline checkout that receives the evaluated revision.
    pub mainline: PathBuf,
    /// The declared native combined-tree check.
    pub check: CheckSpec,
    /// Owned directory that retains the check's actual output.
    pub evidence: PathBuf,
    /// A previously returned receipt, when the controller retained one; an
    /// exactly matching receipt makes a resume a verified no-op instead of a
    /// repeated check. A stale receipt is never inherited: the check is then
    /// re-derived from the observed tree.
    pub prior: Option<IntegrationReceipt>,
}

/// The exact integration of one evaluated candidate revision into the accepted
/// mainline under one published decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntegrationReceipt {
    pub schema: u32,
    pub item: String,
    pub experiment: String,
    /// Digest of the exact decision record this effect was performed under.
    pub decision_sha256: String,
    pub policy_digest: String,
    pub acceptance: String,
    pub base_revision: String,
    pub candidate_revision: String,
    pub mainline: PathBuf,
    /// The observed mainline revision after the effect; always the exact
    /// evaluated candidate revision.
    pub integrated_revision: String,
    /// True when this call moved the mainline; false when the exact state was
    /// already present and nothing was replayed.
    pub applied: bool,
    pub checks: CheckReceipt,
    /// The exact retained workload implementations of the run's own hypothesis
    /// card whose change the integrated candidate carries. Empty when the
    /// candidate carries no retained workload solution (a fresh, supported
    /// implementation) or none could be attributed. This is lineage only: it
    /// neither adopts the workload hypothesis nor authorizes its removal.
    #[serde(default)]
    pub workload_lineage: Vec<WorkloadLineage>,
}

/// One retained workload implementation whose exact change the integrated
/// candidate carries. The record names the owning hypothesis card and the
/// exact verified revision, so the resulting baseline can be traced to the
/// independently accepted workload solution it contains even when rebasing
/// onto a new baseline changed the commit identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadLineage {
    /// The hypothesis card that retains the implementation reference.
    pub item: String,
    /// The arm branch that produced the retained solution.
    pub branch: String,
    /// The frozen workload revision the solution was produced from.
    pub base: String,
    /// The exact retained solution revision, independent of the rebased
    /// candidate identity recorded in the receipt.
    pub revision: String,
}

/// The resolved result of one integration attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum IntegrationOutcome {
    /// This call integrated the candidate revision into the mainline.
    Integrated(IntegrationReceipt),
    /// Observed state already records the exact integration; nothing was
    /// replayed and no second effect was counted.
    Confirmed(IntegrationReceipt),
    /// Nothing was changed; the reason names the missing fact or decision.
    Blocked(Blocked),
}

/// One activation attempt's explicit inputs. Activation selects the prepared
/// candidate runtime inside the run's owned state; live installation
/// publication is a separate lifecycle owner and never happens here.
#[derive(Debug)]
pub struct ActivationRequest<'a> {
    /// The explicit run inputs; its `publication_scope` must permit
    /// integration, and it declares any removal treatment.
    pub spec: &'a improvement_loop::RunSpec,
    /// The frozen experiment binding the evaluated candidate belongs to.
    pub bindings: &'a improvement_experiment::ExperimentBindings,
    /// The frozen policy evaluation applied by the unchanged supervisor.
    pub evaluation: &'a improvement_policy::PolicyEvaluation,
    /// The experiment identity the decision record names.
    pub experiment: String,
    /// True when the run's frozen candidate state declares this treatment a
    /// removal, even though the spec names no removal scope: the effect then
    /// requires a current reviewable removal proposal on the evaluated card
    /// plus its covering user decision.
    pub removal_required: bool,
    /// The removal proposal digest frozen at run start, when the run declares
    /// a removal treatment.
    pub frozen_removal: Option<String>,
    /// The accepted mainline checkout whose observed revision must be the
    /// checked integrated revision.
    pub mainline: PathBuf,
    /// The integration receipt whose check covered the integrated revision.
    pub integration: IntegrationReceipt,
    /// The run's owned native state that holds the prepared variants.
    pub state: PathBuf,
    /// The installed candidate arm runtime retained by the experiment owner.
    pub runtime: &'a improvement_runtime::ArmRuntime,
    /// True while a measured attempt holds its frozen runtime; selection is
    /// then refused and the attempt keeps the runtime it began with.
    pub attempt_active: bool,
}

/// The exact activation of one checked integrated revision as the
/// experimental baseline, with the identity actually consumed and selected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationReceipt {
    pub schema: u32,
    pub item: String,
    pub experiment: String,
    pub decision_sha256: String,
    pub policy_digest: String,
    pub acceptance: String,
    pub base_revision: String,
    pub candidate_revision: String,
    /// The mainline revision observed as the checked integrated revision.
    pub integrated_revision: String,
    pub mainline: PathBuf,
    /// True when this call changed the active runtime pointer.
    pub applied: bool,
    /// The installed arm identity re-verified as actually consumed.
    pub consumption: improvement_runtime::Consumption,
    /// The prepared variant identity reported by the selection owner.
    pub selected: improvement_experiment::ConsumedVariant,
    /// The build source root re-verified as consumed (the candidate checkout).
    pub runtime_source: PathBuf,
}

/// The resolved result of one activation attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum ActivationOutcome {
    /// This call selected the verified candidate runtime.
    Activated(ActivationReceipt),
    /// The exact candidate runtime was already selected; nothing was replayed.
    Confirmed(ActivationReceipt),
    /// Nothing was changed; the reason names the missing fact or decision.
    Blocked(Blocked),
}

/// The frozen identities every effect binds to, resolved from the requested
/// inputs and the current board.
struct Evidence {
    item: String,
    experiment: String,
    decision_sha256: String,
    policy_digest: String,
    acceptance: String,
    base_revision: String,
    candidate_revision: String,
}

/// The fields shared by integration and activation.
struct FrozenContext<'a> {
    spec: &'a improvement_loop::RunSpec,
    bindings: &'a improvement_experiment::ExperimentBindings,
    evaluation: &'a improvement_policy::PolicyEvaluation,
    experiment: &'a str,
    removal_required: bool,
    frozen_removal: Option<&'a str>,
}

impl<'a> IntegrationRequest<'a> {
    fn context(&self) -> FrozenContext<'_> {
        FrozenContext {
            spec: self.spec,
            bindings: self.bindings,
            evaluation: self.evaluation,
            experiment: &self.experiment,
            removal_required: self.removal_required,
            frozen_removal: self.frozen_removal.as_deref(),
        }
    }
}

impl<'a> ActivationRequest<'a> {
    fn context(&self) -> FrozenContext<'_> {
        FrozenContext {
            spec: self.spec,
            bindings: self.bindings,
            evaluation: self.evaluation,
            experiment: &self.experiment,
            removal_required: self.removal_required,
            frozen_removal: self.frozen_removal.as_deref(),
        }
    }
}

/// Resolves the current benefit decision and removal authority for one exact
/// experiment, base, candidate and acceptance. The newest attributable
/// decision controls: a changed, conflicting, incomplete or non-adoption
/// record blocks the effect instead of inheriting older evidence, and an
/// experiment-only removal approval never covers integration.
fn resolve_evidence(context: &FrozenContext<'_>) -> io::Result<Gate<Evidence>> {
    let spec = context.spec;
    let bindings = context.bindings;
    let item = &spec.hypothesis_item;
    if bindings.hypothesis != *item {
        return Ok(Gate::blocked(blocked(
            false,
            format!(
                "the experiment binding belongs to hypothesis {} while the run investigates {item}",
                bindings.hypothesis
            ),
        )));
    }
    if !spec.permits(improvement_loop::PublicationStage::Integration) {
        return Ok(Gate::blocked(blocked(
            true,
            "the run's declared publication scope does not permit integration; the scope needs the user's decision before any mainline or baseline effect",
        )));
    }
    if bindings.schema != improvement_experiment::EXPERIMENT_SCHEMA {
        return Err(invalid(format!(
            "unsupported experiment binding schema {}",
            bindings.schema
        )));
    }
    let experiment = board_hypothesis::require_token("experiment", context.experiment, 128)?;
    let base = bindings.candidate.base.clone();
    let candidate = bindings.candidate.revision.clone();
    if bindings.base_revision != base {
        return Err(invalid(
            "the experiment binding records a base revision that is not the candidate checkout base",
        ));
    }
    if base == candidate {
        return Ok(Gate::blocked(blocked(
            false,
            "the evaluated candidate revision equals the accepted base; there is no treatment to integrate",
        )));
    }
    if context.evaluation.policy_digest != bindings.policy_digest {
        return Ok(Gate::blocked(blocked(
            false,
            "the retained evaluation was produced by a different policy than the frozen experiment binding; apply the predeclared policy and publish that decision",
        )));
    }
    if context.evaluation.decision != improvement_policy::PolicyDecision::Adopt {
        return Ok(Gate::blocked(blocked(
            false,
            format!(
                "the frozen policy decided {} for experiment {experiment}: {}",
                context.evaluation.decision.as_str(),
                reason_text(context.evaluation)
            ),
        )));
    }
    let draft = match context.evaluation.decision_draft(
        item,
        &experiment,
        &base,
        &candidate,
        &bindings.acceptance,
    ) {
        Ok(draft) => draft,
        Err(error) => {
            return Ok(Gate::blocked(blocked(
                false,
                format!(
                    "the retained evaluation cannot produce a complete decision record: {error}"
                ),
            )));
        }
    };
    let expected = match draft.record() {
        Ok(text) => text,
        Err(error) => {
            return Ok(Gate::blocked(blocked(
                false,
                format!(
                    "the retained evaluation cannot produce a complete decision record: {error}"
                ),
            )));
        }
    };
    let expected_record = parse_expected_record(&expected);
    let comments = board_feedback::list_comments(&spec.board.bd, &spec.board.project, item)?;
    let records = benefit_gate::parse_gate_comments(&comments);
    let Some(assessment) = benefit_gate::assess(&records, item) else {
        return Ok(Gate::blocked(blocked(
            false,
            format!(
                "no benefit-gate decision is recorded for {item}; publish the supported decision before the effect"
            ),
        )));
    };
    if assessment.verdict != benefit_gate::Verdict::Consistent {
        let reasons: Vec<&str> = assessment
            .limitations
            .iter()
            .map(|limitation| limitation.as_str())
            .collect();
        return Ok(Gate::blocked(blocked(
            false,
            format!(
                "the newest recorded decision for {item} cannot authorize adoption: {}",
                if reasons.is_empty() {
                    "the record is unreadable".to_owned()
                } else {
                    reasons.join("; ")
                }
            ),
        )));
    }
    if assessment.latest != &expected_record {
        return Ok(Gate::blocked(blocked(
            false,
            format!(
                "the newest recorded decision for {item} is not the decision this evaluation publishes for experiment {experiment} at {base}..{candidate}; the newest record controls, so publish the decision for the evaluated revisions again"
            ),
        )));
    }
    let decision_sha256 = build_identity::hash_bytes(expected.as_bytes());
    let declared = spec
        .removal
        .as_ref()
        .map(|removal| (removal.proposal.clone(), removal.target.clone()));
    let recorded =
        evaluated_removal(&comments, item).map(|recorded| (recorded.proposal, recorded.target));
    match declared.or(recorded) {
        Some((proposal, target)) => {
            // The declared run scope binds the effect when it exists.
            // Otherwise the evaluated hypothesis card itself records a
            // reviewable removal proposal, so the treatment withdraws an
            // existing capability (a subtraction, disabling or consolidation
            // admitted by the decision owner) and the same gate applies: an
            // omitted or renamed run declaration cannot bypass the user's
            // decision. A decision that covers only the isolated experiment
            // still leaves integration and baseline activation pending.
            if let Err(block) =
                removal_block(item, &proposal, &target, &comments, context.frozen_removal)
            {
                return Ok(Gate::blocked(block));
            }
        }
        // The run's frozen candidate state declares a removal treatment, but
        // the evaluated card records no reviewable proposal at all: nothing
        // exists for the user to have decided on, so the effect owner refuses
        // instead of treating the candidate as an ordinary change.
        None if context.removal_required => {
            return Ok(Gate::blocked(blocked(
                true,
                format!(
                    "the run declares a removal treatment for {item}, but its evaluated card records no reviewable removal proposal; prepare the proposal and obtain the user's scoped decision before any mainline or baseline effect"
                ),
            )));
        }
        None => {}
    }
    Ok(Gate::Ready(Evidence {
        item: item.clone(),
        experiment,
        decision_sha256,
        policy_digest: bindings.policy_digest.clone(),
        acceptance: bindings.acceptance.clone(),
        base_revision: base,
        candidate_revision: candidate,
    }))
}

/// The latest recorded removal proposal version on the evaluated card, when
/// one exists; the last matching record is the current version.
fn evaluated_removal(
    comments: &[String],
    item: &str,
) -> Option<board_hypothesis::RecordedRemovalProposal> {
    board_hypothesis::parse_removal_proposals(comments)
        .into_iter()
        .rfind(|proposal| proposal.item == item)
}

/// Resolves the current removal authority for the integration stage and maps
/// it to the exact block: a missing, changed or uncovered decision pends for
/// the user's decision, while a refusal and a withdrawal are reported as
/// their own final decisions and are not re-prompted without a new basis.
fn removal_block(
    item: &str,
    proposal: &str,
    target: &str,
    comments: &[String],
    frozen_reviewed: Option<&str>,
) -> Result<(), Blocked> {
    let authority = board_hypothesis::AuthorityRequest {
        proposal: proposal.to_owned(),
        target: target.to_owned(),
        action: board_hypothesis::RemovalAction::Integration,
    };
    match improvement_loop::removal_gate_at(item, &authority, comments, frozen_reviewed) {
        improvement_loop::RemovalGate::Authorized { .. } => Ok(()),
        improvement_loop::RemovalGate::Pending { reason } => Err(Blocked {
            pending: true,
            reason,
            check: None,
        }),
        improvement_loop::RemovalGate::Refused { .. } => Err(blocked(
            false,
            format!(
                "the user refused removal proposal {proposal} target {target}; the latest decision controls and blocks this effect"
            ),
        )),
        improvement_loop::RemovalGate::Withdrawn { .. } => Err(blocked(
            false,
            format!(
                "the user withdrew approval for removal proposal {proposal} target {target}; the latest decision controls and blocks this effect"
            ),
        )),
    }
}

fn reason_text(evaluation: &improvement_policy::PolicyEvaluation) -> String {
    let text = if evaluation.reasons.is_empty() {
        evaluation.decision.as_str().to_owned()
    } else {
        evaluation.reasons.join("; ")
    };
    text.chars().take(512).collect()
}

/// Parses one freshly rendered decision record through the benefit-gate
/// parser, so the board comparison below uses exactly the reader's field
/// semantics.
fn parse_expected_record(expected: &str) -> benefit_gate::GateRecord {
    let expected = expected.to_owned();
    benefit_gate::parse_gate_comments(std::slice::from_ref(&expected))
        .pop()
        .expect("a rendered v2 decision record parses")
}

/// Integrates the exact evaluated candidate revision into the accepted
/// mainline after the declared combined-tree check passes, or confirms that
/// the exact integration is already present. Nothing is changed on any
/// blocked path.
pub fn integrate(request: &IntegrationRequest<'_>) -> io::Result<IntegrationOutcome> {
    let evidence = match resolve_evidence(&request.context())? {
        Gate::Ready(evidence) => evidence,
        Gate::Blocked(blocked) => return Ok(IntegrationOutcome::Blocked(*blocked)),
    };
    if !request.mainline.is_absolute() {
        return Err(invalid("the mainline checkout path must be absolute"));
    }
    if !request.mainline.is_dir() {
        return Err(invalid(format!(
            "the mainline checkout {} is missing",
            request.mainline.display()
        )));
    }
    if !request.evidence.is_absolute() {
        return Err(invalid("the evidence directory must be absolute"));
    }
    let candidate_checkout = &request.bindings.candidate;
    if let Err(error) = task_worktree::verify_candidate_checkout(candidate_checkout) {
        return Ok(IntegrationOutcome::Blocked(blocked(
            false,
            format!(
                "the evaluated candidate checkout no longer records its bound revision ({error}); preserve or re-create the exact evaluated revision before integration"
            ),
        )));
    }
    let candidate_repo = git_common_dir(&candidate_checkout.source)?;
    let mainline_repo = git_common_dir(&request.mainline)?;
    if !same_path(&candidate_repo, &mainline_repo) {
        return Ok(IntegrationOutcome::Blocked(blocked(
            false,
            format!(
                "the mainline checkout {} is not the repository the candidate branch belongs to",
                request.mainline.display()
            ),
        )));
    }
    match resolve_frozen_base(request) {
        Ok(resolved) if resolved == evidence.base_revision => {}
        Ok(resolved) => {
            return Ok(IntegrationOutcome::Blocked(blocked(
                false,
                format!(
                    "the run's frozen base {} resolves to {resolved} instead of the evaluated base {}",
                    request.spec.base_revision, evidence.base_revision
                ),
            )));
        }
        Err(reason) => return Ok(IntegrationOutcome::Blocked(blocked(false, reason))),
    }
    if !git_clean(&request.mainline)? {
        return Ok(IntegrationOutcome::Blocked(blocked(
            false,
            format!(
                "the mainline checkout {} has uncommitted or untracked work; integration preserves it and refuses until the tree is clean",
                request.mainline.display()
            ),
        )));
    }
    if !git_clean(&candidate_checkout.path)? {
        return Ok(IntegrationOutcome::Blocked(blocked(
            false,
            format!(
                "the candidate checkout {} has uncommitted or untracked work; commit or resolve it so the checked revision is the evaluated one",
                candidate_checkout.path.display()
            ),
        )));
    }
    // The baseline's workload lineage is derived read-only from the run's own
    // hypothesis card before any effect, so the receipt can name the exact
    // retained solution the integrated candidate carries.
    let workload_lineage = carried_workload_lineage(request, candidate_checkout)?;
    // The declared combined-tree check is frozen once a retained receipt has
    // exercised it: a later declaration cannot silently run a different
    // (possibly weakened) check to reauthorize the same integrated revision.
    // The checker bytes at the declared path remain re-derivable, and every
    // receipt records the digest actually executed.
    if let Some(prior) = request
        .prior
        .as_ref()
        .filter(|prior| prior.item == evidence.item && prior.experiment == evidence.experiment)
        && (!same_path(&prior.checks.program, &request.check.program)
            || prior.checks.args != declared_args(&request.check))
    {
        return Ok(IntegrationOutcome::Blocked(blocked(
            false,
            format!(
                "the declared combined-tree check changed after the retained receipt exercised a different check ({} vs {}); a changed check declaration cannot reauthorize the same integrated revision, so a fresh decision is required",
                prior.checks.program.display(),
                request.check.program.display()
            ),
        )));
    }
    let head = git_head(&request.mainline)?;
    if head == evidence.candidate_revision {
        let checks = match request
            .prior
            .as_ref()
            .filter(|prior| prior_matches(prior, request, &evidence))
        {
            Some(prior) => prior.checks.clone(),
            None => {
                let checks = run_check(request, &evidence.candidate_revision)?;
                if !checks.passed() {
                    return Ok(IntegrationOutcome::Blocked(Blocked {
                        pending: false,
                        reason: format!(
                            "the declared combined-tree check did not pass on the observed integrated revision {} ({})",
                            evidence.candidate_revision,
                            check_failure(&checks)
                        ),
                        check: Some(checks),
                    }));
                }
                checks
            }
        };
        // A prior receipt or a repeated check is not enough: the current board
        // evidence and the exact observed Git identities must still authorize
        // this effect immediately before the receipt is returned.
        if let Gate::Blocked(blocked) = revalidate(request, &evidence.candidate_revision)? {
            return Ok(IntegrationOutcome::Blocked(*blocked));
        }
        return Ok(IntegrationOutcome::Confirmed(IntegrationReceipt {
            schema: RECEIPT_SCHEMA,
            item: evidence.item,
            experiment: evidence.experiment,
            decision_sha256: evidence.decision_sha256,
            policy_digest: evidence.policy_digest,
            acceptance: evidence.acceptance,
            base_revision: evidence.base_revision,
            candidate_revision: evidence.candidate_revision.clone(),
            mainline: request.mainline.clone(),
            integrated_revision: evidence.candidate_revision,
            applied: false,
            checks,
            workload_lineage,
        }));
    }
    if head != evidence.base_revision {
        return Ok(IntegrationOutcome::Blocked(blocked(
            false,
            format!(
                "the mainline records {head} instead of the evaluated base {} or the evaluated candidate {}; a changed base cannot inherit this benefit evidence",
                evidence.base_revision, evidence.candidate_revision
            ),
        )));
    }
    if !git_ancestor(
        &request.mainline,
        &evidence.base_revision,
        &evidence.candidate_revision,
    )? {
        return Ok(IntegrationOutcome::Blocked(blocked(
            false,
            format!(
                "the evaluated candidate revision {} is not a descendant of the accepted base {}; integration would not be a fast-forward of the exact evaluated revision",
                evidence.candidate_revision, evidence.base_revision
            ),
        )));
    }
    let checks = run_check(request, &evidence.candidate_revision)?;
    if !checks.passed() {
        return Ok(IntegrationOutcome::Blocked(Blocked {
            pending: false,
            reason: format!(
                "the declared combined-tree check failed on {} ({})",
                evidence.candidate_revision,
                check_failure(&checks)
            ),
            check: Some(checks),
        }));
    }
    // The check can take minutes: re-read the board decision and removal
    // authority and re-verify the exact clean Git identities before the
    // mainline effect. Withdrawal or drift during the check blocks here.
    if let Gate::Blocked(blocked) = revalidate(request, &evidence.base_revision)? {
        return Ok(IntegrationOutcome::Blocked(*blocked));
    }
    if let Err(error) = git_merge_fast_forward(&request.mainline, &evidence.candidate_revision) {
        let observed = git_head(&request.mainline).unwrap_or_else(|_| "<unreadable>".to_owned());
        return Ok(IntegrationOutcome::Blocked(blocked(
            false,
            format!(
                "the fast-forward integration was refused ({error}); the mainline records {observed}"
            ),
        )));
    }
    let integrated = git_head(&request.mainline)?;
    if integrated != evidence.candidate_revision {
        return Ok(IntegrationOutcome::Blocked(blocked(
            false,
            format!(
                "the mainline records {integrated} after integration instead of {}; reconcile the observed state before retrying",
                evidence.candidate_revision
            ),
        )));
    }
    Ok(IntegrationOutcome::Integrated(IntegrationReceipt {
        schema: RECEIPT_SCHEMA,
        item: evidence.item,
        experiment: evidence.experiment,
        decision_sha256: evidence.decision_sha256,
        policy_digest: evidence.policy_digest,
        acceptance: evidence.acceptance,
        base_revision: evidence.base_revision,
        candidate_revision: evidence.candidate_revision.clone(),
        mainline: request.mainline.clone(),
        integrated_revision: evidence.candidate_revision,
        applied: true,
        checks,
        workload_lineage,
    }))
}

/// Activates the verified installed candidate runtime as the run's
/// experimental baseline. The exact prepared variant must be the one the
/// evaluated candidate was built from, its source must be the candidate
/// checkout at the checked integrated revision, and the installed home must
/// still consume exactly that prepared identity. Any mismatch leaves the
/// active runtime unchanged.
pub fn activate(request: &ActivationRequest<'_>) -> io::Result<ActivationOutcome> {
    activate_with_trust(request, &[])
}

/// The same activation for an arm whose measured dispatches may have trusted
/// exactly the workspaces authorized by the owning dispatcher. The caller
/// supplies the owned slot paths from its frozen pool declaration; the arm
/// configuration may carry those trusted-project entries and nothing else.
/// `activate` keeps the exact-bytes contract for a never-dispatched arm.
pub fn activate_with_trust(
    request: &ActivationRequest<'_>,
    trusted_workspaces: &[std::path::PathBuf],
) -> io::Result<ActivationOutcome> {
    activate_inner(request, trusted_workspaces)
}

fn activate_inner(
    request: &ActivationRequest<'_>,
    trusted_workspaces: &[std::path::PathBuf],
) -> io::Result<ActivationOutcome> {
    let evidence = match resolve_evidence(&request.context())? {
        Gate::Ready(evidence) => evidence,
        Gate::Blocked(blocked) => return Ok(ActivationOutcome::Blocked(*blocked)),
    };
    let integration = &request.integration;
    if integration.schema != RECEIPT_SCHEMA {
        return Err(invalid(format!(
            "unsupported integration receipt schema {}",
            integration.schema
        )));
    }
    if integration.item != evidence.item
        || integration.experiment != evidence.experiment
        || integration.decision_sha256 != evidence.decision_sha256
        || integration.policy_digest != evidence.policy_digest
        || integration.acceptance != evidence.acceptance
        || integration.base_revision != evidence.base_revision
        || integration.candidate_revision != evidence.candidate_revision
        || integration.integrated_revision != evidence.candidate_revision
        || integration.checks.revision != evidence.candidate_revision
    {
        return Ok(ActivationOutcome::Blocked(blocked(
            false,
            "the retained integration receipt does not belong to the current decision and evaluated revisions, or its check covered another revision; no stale or foreign receipt authorizes activation",
        )));
    }
    if !integration.checks.passed() {
        return Ok(ActivationOutcome::Blocked(blocked(
            false,
            format!(
                "the retained integration receipt records a failed combined-tree check ({})",
                check_failure(&integration.checks)
            ),
        )));
    }
    if let Err(reason) = verify_check_evidence(&integration.checks) {
        return Ok(ActivationOutcome::Blocked(blocked(
            false,
            format!(
                "the retained combined-tree check evidence cannot authorize activation: {reason}"
            ),
        )));
    }
    if !same_path(&integration.mainline, &request.mainline) {
        return Ok(ActivationOutcome::Blocked(blocked(
            false,
            "the retained integration receipt belongs to a different mainline checkout",
        )));
    }
    let head = git_head(&request.mainline)?;
    if head != evidence.candidate_revision {
        return Ok(ActivationOutcome::Blocked(blocked(
            false,
            format!(
                "the mainline records {head} instead of the checked integrated revision {}; complete the integration before activation",
                evidence.candidate_revision
            ),
        )));
    }
    if let Err(error) = task_worktree::verify_candidate_checkout(&request.bindings.candidate) {
        return Ok(ActivationOutcome::Blocked(blocked(
            false,
            format!(
                "the evaluated candidate checkout no longer records the checked integrated revision ({error})"
            ),
        )));
    }
    let candidate_arm = request
        .bindings
        .arm(improvement_experiment::Arm::Candidate)?;
    if request.runtime.arm != improvement_experiment::Arm::Candidate
        || request.runtime.variant != candidate_arm.runtime
    {
        return Ok(ActivationOutcome::Blocked(blocked(
            false,
            "the installed candidate runtime is not the prepared candidate variant of this experiment",
        )));
    }
    if !same_path(&request.runtime.source, &request.bindings.candidate.path) {
        return Ok(ActivationOutcome::Blocked(blocked(
            false,
            format!(
                "the installed runtime was built from {} while the evaluated candidate checkout is {}; the consumed identity does not match the checked integrated revision",
                request.runtime.source.display(),
                request.bindings.candidate.path.display()
            ),
        )));
    }
    let consumption = match improvement_runtime::verify_consumption_with_trust(
        request.runtime,
        trusted_workspaces,
    ) {
        Ok(consumption) => consumption,
        Err(error) => {
            return Ok(ActivationOutcome::Blocked(blocked(
                false,
                format!(
                    "the installed candidate runtime no longer verifies as actually consumed: {error}"
                ),
            )));
        }
    };
    // Every deterministic precondition is resolved before the selection owner
    // writes the active pointer: the consumed installation must already be the
    // prepared candidate variant, and `select_variant` re-verifies the
    // selected artifact against that same variant before any change.
    if !same_path(&consumption.build, &request.runtime.variant.build)
        || consumption.record_sha256 != request.runtime.variant.record_sha256
    {
        return Ok(ActivationOutcome::Blocked(blocked(
            false,
            "the consumed installed identity is not the prepared candidate variant of this experiment",
        )));
    }
    let selected = match improvement_experiment::select_variant(
        &request.state,
        &request.runtime.variant,
        request.attempt_active,
    ) {
        Ok(selected) => selected,
        Err(error) => {
            return Ok(ActivationOutcome::Blocked(blocked(
                false,
                format!("the candidate runtime was not selected: {error}"),
            )));
        }
    };
    let applied = selected.changed;
    let receipt = ActivationReceipt {
        schema: RECEIPT_SCHEMA,
        item: evidence.item,
        experiment: evidence.experiment,
        decision_sha256: evidence.decision_sha256,
        policy_digest: evidence.policy_digest,
        acceptance: evidence.acceptance,
        base_revision: evidence.base_revision,
        candidate_revision: evidence.candidate_revision.clone(),
        integrated_revision: evidence.candidate_revision,
        mainline: request.mainline.clone(),
        applied,
        consumption,
        selected,
        runtime_source: request.runtime.source.clone(),
    };
    Ok(if applied {
        ActivationOutcome::Activated(receipt)
    } else {
        ActivationOutcome::Confirmed(receipt)
    })
}

/// Runs the declared check in the candidate's owned worktree, retaining its
/// actual output under the caller's evidence directory. The check runs before
/// any mainline effect, so a failed check cannot leave an integrated tree.
/// Output is retained exactly as written; bounding its size is separate
/// retention work (parent task 5.4), not part of this gate.
fn run_check(request: &IntegrationRequest<'_>, revision: &str) -> io::Result<CheckReceipt> {
    let spec = &request.check;
    if !spec.program.is_absolute() {
        return Err(invalid(
            "the declared check program must be an explicit absolute executable path",
        ));
    }
    if spec.timeout.is_zero() {
        return Err(invalid("the declared check timeout must be positive"));
    }
    build_identity::ordinary(&spec.program).map_err(|error| {
        invalid(format!(
            "the declared check program {} is missing or not an ordinary file: {error}",
            spec.program.display()
        ))
    })?;
    let program_sha256 = build_identity::hash_file(&spec.program)?;
    fs::create_dir_all(&request.evidence)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    let stdout_path = request.evidence.join(format!("check-{stamp}.stdout.log"));
    let stderr_path = request.evidence.join(format!("check-{stamp}.stderr.log"));
    let cwd = request.bindings.candidate.path.clone();
    let mut command = process::CommandSpec::new(&spec.program);
    command.args = spec.args.clone();
    command.current_dir = Some(cwd.clone());
    command.stdout = Some(fs::File::create(&stdout_path)?);
    command.stderr = Some(fs::File::create(&stderr_path)?);
    let job = process::Job::new(process::Limits::default())?;
    let started = Instant::now();
    let child = job.spawn(&command)?;
    let outcome = job.wait(
        &child,
        process::Deadline::after(spec.timeout)?,
        &process::Cancellation::default(),
        CHECK_CLEANUP,
    )?;
    let duration_ms = started.elapsed().as_millis() as u64;
    Ok(CheckReceipt {
        program: spec.program.clone(),
        program_sha256,
        args: declared_args(spec),
        revision: revision.to_owned(),
        cwd,
        exit_code: outcome.exit_code,
        reason: stop_reason(outcome.reason).to_owned(),
        duration_ms,
        stdout: check_output(&stdout_path)?,
        stderr: check_output(&stderr_path)?,
    })
}

fn check_output(path: &Path) -> io::Result<CheckOutput> {
    Ok(CheckOutput {
        path: path.to_path_buf(),
        sha256: build_identity::hash_file(path)?,
        bytes: fs::metadata(path)?.len(),
    })
}

/// Re-resolves the current benefit decision and removal authority and
/// re-verifies the exact clean Git identities immediately before an effect.
/// A long combined-tree check must not let a withdrawal, a moved mainline, a
/// changed candidate revision or dirty work slip into the integration.
fn revalidate(request: &IntegrationRequest<'_>, expected_head: &str) -> io::Result<Gate<()>> {
    match resolve_evidence(&request.context())? {
        Gate::Ready(_) => {}
        Gate::Blocked(blocked) => return Ok(Gate::Blocked(blocked)),
    }
    let candidate = &request.bindings.candidate;
    if let Err(error) = task_worktree::verify_candidate_checkout(candidate) {
        return Ok(Gate::blocked(blocked(
            false,
            format!(
                "the candidate revision changed while the combined-tree check ran ({error}); the effect is refused"
            ),
        )));
    }
    let candidate_repo = git_common_dir(&candidate.source)?;
    let mainline_repo = git_common_dir(&request.mainline)?;
    if !same_path(&candidate_repo, &mainline_repo) {
        return Ok(Gate::blocked(blocked(
            false,
            format!(
                "the mainline checkout {} is no longer the repository the candidate branch belongs to",
                request.mainline.display()
            ),
        )));
    }
    if !git_clean(&request.mainline)? {
        return Ok(Gate::blocked(blocked(
            false,
            format!(
                "the mainline checkout {} gained uncommitted or untracked work while the combined-tree check ran; the effect is refused",
                request.mainline.display()
            ),
        )));
    }
    if !git_clean(&candidate.path)? {
        return Ok(Gate::blocked(blocked(
            false,
            format!(
                "the candidate checkout {} gained uncommitted or untracked work while the combined-tree check ran; the effect is refused",
                candidate.path.display()
            ),
        )));
    }
    let head = git_head(&request.mainline)?;
    if head != expected_head {
        return Ok(Gate::blocked(blocked(
            false,
            format!(
                "the mainline moved to {head} while the combined-tree check ran; the effect on {expected_head} is refused"
            ),
        )));
    }
    Ok(Gate::Ready(()))
}

fn check_failure(check: &CheckReceipt) -> String {
    format!(
        "{} with exit code {} after {} ms",
        check.reason, check.exit_code, check.duration_ms
    )
}

fn stop_reason(reason: process::StopReason) -> &'static str {
    match reason {
        process::StopReason::Exited => "exited",
        process::StopReason::Timeout => "timeout",
        process::StopReason::Cancelled => "cancelled",
        process::StopReason::MemoryLimit => "memory-limit",
    }
}

/// Verifies that one retained check receipt still names available, unchanged
/// evidence: an ordinary checker program with the recorded bytes and both
/// retained output streams present with the recorded digest and length. A
/// missing or modified artifact reports the exact broken reference instead of
/// authorizing activation or skipping a repeated check.
fn verify_check_evidence(check: &CheckReceipt) -> Result<(), String> {
    verify_retained(
        "the declared checker",
        &check.program,
        &check.program_sha256,
    )?;
    for (name, output) in [
        ("the retained check stdout", &check.stdout),
        ("the retained check stderr", &check.stderr),
    ] {
        verify_retained(name, &output.path, &output.sha256)?;
        let bytes = fs::metadata(&output.path)
            .map_err(|error| format!("{name} {} is unreadable ({error})", output.path.display()))?
            .len();
        if bytes != output.bytes {
            return Err(format!(
                "{name} {} changed length since the check",
                output.path.display()
            ));
        }
    }
    Ok(())
}

fn verify_retained(name: &str, path: &Path, sha256: &str) -> Result<(), String> {
    build_identity::ordinary(path)
        .map_err(|error| format!("{name} {} is unavailable ({error})", path.display()))?;
    let actual = build_identity::hash_file(path)
        .map_err(|error| format!("{name} {} is unreadable ({error})", path.display()))?;
    if actual != sha256 {
        return Err(format!("{name} {} changed since the check", path.display()));
    }
    Ok(())
}

fn declared_program_digest(check: &CheckSpec) -> Option<String> {
    build_identity::ordinary(&check.program).ok()?;
    build_identity::hash_file(&check.program).ok()
}

/// True only when a retained receipt describes exactly this decision,
/// evaluation, revisions, mainline and declared check, with a passed check on
/// the integrated revision.
fn prior_matches(
    prior: &IntegrationReceipt,
    request: &IntegrationRequest<'_>,
    evidence: &Evidence,
) -> bool {
    prior.schema == RECEIPT_SCHEMA
        && prior.item == evidence.item
        && prior.experiment == evidence.experiment
        && prior.decision_sha256 == evidence.decision_sha256
        && prior.policy_digest == evidence.policy_digest
        && prior.acceptance == evidence.acceptance
        && prior.base_revision == evidence.base_revision
        && prior.candidate_revision == evidence.candidate_revision
        && prior.integrated_revision == evidence.candidate_revision
        && prior.checks.revision == evidence.candidate_revision
        && prior.checks.passed()
        && prior.checks.program == request.check.program
        && prior.checks.args == declared_args(&request.check)
        && declared_program_digest(&request.check)
            .is_some_and(|digest| digest == prior.checks.program_sha256)
        && verify_check_evidence(&prior.checks).is_ok()
        && same_path(&prior.mainline, &request.mainline)
}

fn declared_args(check: &CheckSpec) -> Vec<String> {
    check
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// Resolves the run's frozen base revision in the candidate repository.
fn resolve_frozen_base(request: &IntegrationRequest<'_>) -> Result<String, String> {
    let reference = format!("{}^{{commit}}", request.spec.base_revision);
    git(
        &request.bindings.candidate.path,
        &["rev-parse", "--verify", &reference],
    )
    .map(|text| text.trim().to_owned())
    .map_err(|error| {
        format!(
            "the run's frozen base revision {} is not resolvable in the candidate repository ({error})",
            request.spec.base_revision
        )
    })
}

fn git(cwd: &Path, args: &[&str]) -> io::Result<String> {
    let out = Command::new("git").args(args).current_dir(cwd).output()?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    Err(io::Error::other(format!(
        "git {} in {}: {}",
        args.first().copied().unwrap_or("git"),
        cwd.display(),
        String::from_utf8_lossy(&out.stderr).trim()
    )))
}

fn git_head(cwd: &Path) -> io::Result<String> {
    Ok(git(cwd, &["rev-parse", "HEAD"])?.trim().to_owned())
}

fn git_clean(cwd: &Path) -> io::Result<bool> {
    Ok(git(cwd, &["status", "--porcelain"])?.trim().is_empty())
}

fn git_common_dir(cwd: &Path) -> io::Result<PathBuf> {
    let text = git(cwd, &["rev-parse", "--git-common-dir"])?
        .trim()
        .to_owned();
    let raw = PathBuf::from(&text);
    let absolute = if raw.is_absolute() {
        raw
    } else {
        cwd.join(raw)
    };
    Ok(canonical(&absolute))
}

fn git_merge_fast_forward(cwd: &Path, revision: &str) -> io::Result<()> {
    git(cwd, &["merge", "--ff-only", revision]).map(|_| ())
}

fn git_ancestor(cwd: &Path, ancestor: &str, descendant: &str) -> io::Result<bool> {
    let out = Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(cwd)
        .output()?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(io::Error::other(format!(
            "git merge-base --is-ancestor in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))),
    }
}

/// The exact retained workload implementations of the run's own hypothesis
/// card whose change the evaluated candidate carries. Only the card this run
/// independently investigates is read, so a retained workload solution is
/// eligible here only when the run's own hypothesis owns it; a revision that
/// is not an object of the candidate's repository, or whose change differs,
/// is never attributed. An empty result means the candidate carries no
/// retained workload solution, which is a supported state for a fresh
/// implementation.
fn carried_workload_lineage(
    request: &IntegrationRequest<'_>,
    candidate: &task_worktree::CandidateCheckout,
) -> io::Result<Vec<WorkloadLineage>> {
    let signature = change_signature(&candidate.source, &candidate.base, &candidate.revision)?;
    if signature.is_empty() {
        return Ok(Vec::new());
    }
    let comments = board_feedback::list_comments(
        &request.spec.board.bd,
        &request.spec.board.project,
        &request.spec.hypothesis_item,
    )?;
    let mut lineage = Vec::new();
    for comment in &comments {
        let Some(record) = retained_workload_solution(comment, &request.spec.hypothesis_item)
        else {
            continue;
        };
        if !git_object_present(&candidate.source, &record.base)
            || !git_object_present(&candidate.source, &record.revision)
        {
            continue;
        }
        let Ok(retained) = change_signature(&candidate.source, &record.base, &record.revision)
        else {
            continue;
        };
        if !retained.is_empty() && retained == signature {
            lineage.push(record);
        }
    }
    Ok(lineage)
}

/// One retained `role=workload` implementation record read back from the
/// card's bounded comments. Every other comment - including the candidate
/// card's own `role=candidate` allocations - yields `None`.
fn retained_workload_solution(comment: &str, item: &str) -> Option<WorkloadLineage> {
    let rest = comment
        .trim_start()
        .strip_prefix(board_hypothesis::IMPLEMENTATION_PREFIX)?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    if field(&fields, "item") != Some(item) || field(&fields, "role") != Some("workload") {
        return None;
    }
    Some(WorkloadLineage {
        item: item.to_owned(),
        branch: field(&fields, "branch")?.to_owned(),
        base: field(&fields, "base")?.to_owned(),
        revision: field(&fields, "revision")?.to_owned(),
    })
}

fn field<'a>(fields: &[&'a str], key: &str) -> Option<&'a str> {
    fields.iter().find_map(|entry| {
        let (name, value) = entry.split_once('=')?;
        (name == key && !value.is_empty()).then_some(value)
    })
}

/// The content identity of one committed change: the status and the resulting
/// blob identity of every changed path. Two commits that apply the same change
/// to different bases share this signature, so a retained solution rebased
/// onto a new baseline is recognized as the same exact solution while a
/// different resolution or an extra edit is not. `git diff --raw` shows the
/// content-addressed result per path, and rename detection is disabled so one
/// logical change always yields the same entries.
fn change_signature(repo: &Path, base: &str, revision: &str) -> io::Result<Vec<String>> {
    let raw = git(
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

fn git_object_present(repo: &Path, revision: &str) -> bool {
    Command::new("git")
        .args(["cat-file", "-e", &format!("{revision}^{{commit}}")])
        .current_dir(repo)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Canonical path without the verbatim prefix `Path::canonicalize` adds on
/// Windows; a missing path keeps its absolute form.
fn canonical(path: &Path) -> PathBuf {
    match fs::canonicalize(path) {
        Ok(resolved) => native_path(&resolved.to_string_lossy()),
        Err(_) => path.to_path_buf(),
    }
}

fn native_path(text: &str) -> PathBuf {
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text).to_owned())
}

/// True when both paths name the same location, including canonical aliases;
/// components compare case-insensitively on Windows, where the filesystem is.
fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) if a == b => true,
        _ => {
            cfg!(windows)
                && a.to_string_lossy()
                    .eq_ignore_ascii_case(&b.to_string_lossy())
        }
    }
}
