//! Operational preparation and binding for one real A-on-B comparison.
//!
//! An experiment binds exact committed inputs, two independent frozen copies
//! of the same task snapshot, two fresh executor homes and the two prepared
//! immutable runtimes selected through the existing journaled build-selection
//! owner. These receipts bind operational inputs, effects and evidence; they
//! are not hypothesis state, decisions, benefit evidence or removal authority
//! (Beads owns those). Independent acceptance stays with the real-task oracle
//! referenced by [`ExperimentBindings::oracle`]; this module never executes a
//! model or substitutes a candidate-produced result.
//!
//! # Controller interface
//!
//! In controller call order:
//!
//! 1. `task_worktree::worktree_reuse` before adopting an existing allocation,
//!    otherwise `task_worktree::allocate_candidate_checkout` for the dedicated
//!    hypothesis branch and owned worktree.
//! 2. `task_worktree::frozen_copy` once per arm for the frozen task snapshot
//!    (the same source revision for both arms); `verify_frozen` re-checks it
//!    from repository content without touching the arm's working tree.
//! 3. [`prepare_home`] once per arm for a fresh owned executor home.
//! 4. `crate::native_build::prepare` for each arm's runtime, then
//!    [`prepare_variant`] to bind the exact published build identity.
//! 5. [`ExperimentBindings::validate`] before the first measured attempt; it
//!    also refuses overlapping or cross-aliased experiment allocations, so
//!    every home, workload, runtime and candidate checkout is separately owned.
//! 6. [`ExperimentBindings::verify_pre_attempt`] with the upcoming arm before
//!    that arm begins: the named arm's workload must be a pristine frozen copy
//!    (no prior solution edits, extra references or objects, and no shared
//!    object database), while a completed arm keeps its solution — still
//!    identity-checked — and `task_worktree::verify_frozen` stays the
//!    post-attempt snapshot check. The gate never cleans useful work.
//! 7. [`select_variant`] between attempts (`attempt_active = true` while a
//!    measured attempt holds its frozen runtime) and report the returned
//!    [`ConsumedVariant`] identity as the actually consumed runtime.
//!
//! # Retention and corroboration
//!
//! A completed real task is retained as identity plus a pristine, replayable
//! copy of its frozen pre-solution inputs ([`retain_completed_task`]). The
//! retention never stores or returns the task's solution, patch or
//! conversation, so selecting the task for corroboration cannot hand the
//! earlier answer to a fresh executor. When the declared adoption scope
//! requires corroboration, [`select_corroboration`] picks applicable,
//! independent, replayable retained tasks by identity only, and
//! [`RetainedTask::prepare_replay`] materializes a fresh pre-solution copy per
//! attempt. A task that does not exercise the mechanism is excluded as
//! non-evidence: too few applicable units leave the broader claim
//! inconclusive instead of turning inapplicable workloads into a rejection,
//! and absent retained evidence is never replaced by a summary or an invented
//! saving.

use crate::build_identity;
use crate::build_selection;
use crate::task_worktree::{self, CandidateCheckout, FrozenCopy};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

pub const EXPERIMENT_SCHEMA: u32 = 1;

fn invalid(detail: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("improvement experiment: {detail}"),
    )
}

fn bounded(name: &str, value: &str, limit: usize) -> io::Result<()> {
    if value.trim().is_empty() || value.len() > limit || value.contains(['\n', '\r']) {
        return Err(invalid(format!(
            "{name} is missing or exceeds {limit} bytes"
        )));
    }
    Ok(())
}

/// One comparison arm. The same task snapshot, fresh homes and separate
/// runtimes are what make the treatment difference attributable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Arm {
    Baseline,
    Candidate,
}

impl Arm {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Candidate => "candidate",
        }
    }
}

/// The exact identity of one prepared immutable runtime variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedVariant {
    pub arm: Arm,
    /// Operator label, for example `H` or `H+A`.
    pub label: String,
    pub build: PathBuf,
    /// Digest of the published `build.json` at preparation time.
    pub record_sha256: String,
    /// Recorded compiled-source identity of the build.
    pub source_sha256: String,
}

/// The runtime identity a `select` operation actually consumed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConsumedVariant {
    pub arm: Arm,
    pub label: String,
    pub build: PathBuf,
    pub record_sha256: String,
    /// Digest of the selected manager binary, when this consumer's binary set
    /// records it.
    pub manager_sha256: Option<String>,
    /// True when selection changed the active pointer; a repeated selection
    /// of the same prepared variant reports `false` with no rebuild.
    pub changed: bool,
}

/// Bind one published immutable build as a prepared runtime variant. The build
/// must already exist in the owned native state; preparation never builds,
/// fetches or mutates it.
pub fn prepare_variant(
    state: &Path,
    arm: Arm,
    label: &str,
    build: &Path,
) -> io::Result<PreparedVariant> {
    let label = label.trim();
    if label.is_empty() || label.len() > 64 || label.contains(['\n', '\r']) {
        return Err(invalid("a prepared variant needs a bounded label"));
    }
    crate::native_build::verify_owned_state(state)?;
    let state = state.canonicalize()?;
    build_identity::ordinary(build)?;
    let build = build.canonicalize()?;
    if build.parent() != Some(state.join("builds").as_path()) {
        return Err(invalid(
            "a prepared variant must be an immutable build published in the owned state",
        ));
    }
    let record = build_identity::verify_record_integrity(&build)?;
    let check = build_identity::integrity(&build);
    if !check.runtime_allowed {
        return Err(invalid(format!(
            "prepared variant is not runtime-allowed: {}",
            check.action
        )));
    }
    Ok(PreparedVariant {
        arm,
        label: label.to_owned(),
        record_sha256: build_identity::hash_file(&build.join("build.json"))?,
        source_sha256: record.source.sha256,
        build,
    })
}

/// Select a prepared variant through the existing journaled build-selection
/// owner and report the identity actually consumed.
///
/// - While a measured attempt is active, selection is refused: the attempt
///   keeps the runtime it began with.
/// - A prepared variant whose recorded bytes changed, or whose artifacts no
///   longer verify, is refused as stale instead of being rebuilt silently.
/// - Selecting an already selected variant reports the same identity with
///   `changed = false`: no source revert, no rebuild and no model call.
pub fn select_variant(
    state: &Path,
    variant: &PreparedVariant,
    attempt_active: bool,
) -> io::Result<ConsumedVariant> {
    if attempt_active {
        return Err(invalid(format!(
            "a measured attempt is active; the {} runtime is frozen until it finishes or is cancelled",
            variant.label
        )));
    }
    let record_sha256 =
        build_identity::hash_file(&variant.build.join("build.json")).map_err(|error| {
            invalid(format!(
                "prepared {} runtime is not readable ({error}); prepare it again",
                variant.label
            ))
        })?;
    if record_sha256 != variant.record_sha256 {
        return Err(invalid(format!(
            "prepared {} runtime changed since preparation; prepare it again before selecting",
            variant.label
        )));
    }
    let check = build_identity::integrity(&variant.build);
    if !check.runtime_allowed {
        return Err(invalid(format!(
            "prepared {} runtime is stale or altered ({}); prepare it again",
            variant.label, check.action
        )));
    }
    let selection = build_selection::activate(state, &variant.build)?;
    let (selected, artifacts) = build_selection::selected(state)?;
    if selected != variant.build.canonicalize()? {
        return Err(invalid(
            "the selected runtime is not the prepared variant; refusing to report an unverified identity",
        ));
    }
    Ok(ConsumedVariant {
        arm: variant.arm,
        label: variant.label.clone(),
        build: selected,
        record_sha256,
        manager_sha256: artifacts.digest("codex-harness.exe").map(str::to_owned),
        changed: selection.changed,
    })
}

/// Create a fresh owned executor home. An existing directory is refused: a
/// home is never merged with or reused from another session's state.
pub fn prepare_home(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    if absolute.exists() {
        return Err(invalid(format!(
            "{} already exists; a fresh executor home is never merged into existing state",
            absolute.display()
        )));
    }
    if let Some(parent) = absolute.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(&absolute)?;
    Ok(fs::canonicalize(&absolute).unwrap_or(absolute))
}

/// One arm's operational allocation: fresh home, frozen task copy and
/// prepared runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArmBinding {
    pub arm: Arm,
    pub home: PathBuf,
    pub workload: FrozenCopy,
    pub runtime: PreparedVariant,
}

/// The compact operational binding of one prepared comparison. Hypothesis
/// identity, decisions and removal authority stay on the Beads card; this
/// receipt records only what was prepared and how it was isolated.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExperimentBindings {
    pub schema: u32,
    /// Beads hypothesis card reference.
    pub hypothesis: String,
    pub case_id: String,
    /// The accepted base revision the candidate branch was created from.
    pub base_revision: String,
    pub candidate: CandidateCheckout,
    /// Identity reference of the fixed independent acceptance oracle.
    pub oracle: String,
    /// Reference to the task's independent acceptance evidence.
    pub acceptance: String,
    /// Digest of the predeclared comparison policy.
    pub policy_digest: String,
    pub arms: Vec<ArmBinding>,
}

/// Canonical allocation path without the verbatim prefix `Path::canonicalize`
/// adds on Windows; a missing path keeps its absolute form so the refusal
/// still names it.
fn allocation_path(path: &Path) -> PathBuf {
    let resolved = fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy().into_owned();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned())
}

/// True when one allocation is the other or is nested inside it, including
/// canonical aliases. Components compare case-insensitively on Windows, where
/// the filesystem is.
fn allocations_overlap(left: &Path, right: &Path) -> bool {
    let components = |path: &Path| -> Vec<String> {
        path.components()
            .map(|component| {
                let text = component.as_os_str().to_string_lossy().into_owned();
                if cfg!(windows) {
                    text.to_ascii_lowercase()
                } else {
                    text
                }
            })
            .collect()
    };
    let left = components(&allocation_path(left));
    let right = components(&allocation_path(right));
    let (short, long) = if left.len() <= right.len() {
        (left, right)
    } else {
        (right, left)
    };
    !short.is_empty() && long[..short.len()] == short[..]
}

impl ExperimentBindings {
    pub fn arm(&self, arm: Arm) -> io::Result<&ArmBinding> {
        self.arms
            .iter()
            .find(|binding| binding.arm == arm)
            .ok_or_else(|| invalid(format!("the {} arm is not bound", arm.as_str())))
    }

    /// Validate the prepared comparison before the first measured attempt.
    /// Everything here is read-only: the candidate checkout, both frozen
    /// copies, both homes and both prepared runtimes are re-verified and any
    /// drift is refused.
    pub fn validate(&self) -> io::Result<()> {
        if self.schema != EXPERIMENT_SCHEMA {
            return Err(invalid("unsupported experiment binding schema"));
        }
        bounded("hypothesis", &self.hypothesis, 128)?;
        bounded("case_id", &self.case_id, 128)?;
        bounded("acceptance", &self.acceptance, 512)?;
        bounded("oracle", &self.oracle, 512)?;
        bounded("policy_digest", &self.policy_digest, 128)?;
        bounded("base_revision", &self.base_revision, 128)?;
        if self.arms.len() != 2 {
            return Err(invalid("exactly one arm binding per arm is required"));
        }
        let mut seen = BTreeSet::new();
        for binding in &self.arms {
            if !seen.insert(binding.arm) {
                return Err(invalid(format!(
                    "the {} arm is bound more than once",
                    binding.arm.as_str()
                )));
            }
        }
        for arm in [Arm::Baseline, Arm::Candidate] {
            self.arm(arm)?;
        }
        let baseline = self.arm(Arm::Baseline)?;
        let candidate_arm = self.arm(Arm::Candidate)?;
        if baseline.runtime.label == candidate_arm.runtime.label {
            return Err(invalid(
                "baseline and candidate runtime labels must be distinct",
            ));
        }

        task_worktree::verify_candidate_checkout(&self.candidate)?;
        if self.candidate.base != self.base_revision {
            return Err(invalid(
                "the candidate checkout is not based on the declared accepted revision",
            ));
        }

        for binding in &self.arms {
            if !binding.home.is_dir() {
                return Err(invalid(format!(
                    "the {} arm home {} is missing",
                    binding.arm.as_str(),
                    binding.home.display()
                )));
            }
            if !binding.workload.path.is_dir() {
                return Err(invalid(format!(
                    "the {} arm workload copy is missing",
                    binding.arm.as_str()
                )));
            }
            task_worktree::verify_frozen(&binding.workload)?;
            if binding.runtime.arm != binding.arm {
                return Err(invalid("a prepared runtime is bound to the wrong arm"));
            }
            let record = build_identity::verify_record_integrity(&binding.runtime.build)?;
            if build_identity::hash_file(&binding.runtime.build.join("build.json"))?
                != binding.runtime.record_sha256
            {
                return Err(invalid(format!(
                    "the prepared {} runtime changed since preparation",
                    binding.runtime.label
                )));
            }
            if record.source.sha256 != binding.runtime.source_sha256 {
                return Err(invalid(format!(
                    "the prepared {} runtime records a different source identity",
                    binding.runtime.label
                )));
            }
        }
        if baseline.workload.source_revision != candidate_arm.workload.source_revision
            || baseline.workload.tree_sha256 != candidate_arm.workload.tree_sha256
        {
            return Err(invalid(
                "both arms must freeze the same committed task snapshot",
            ));
        }
        // Every experiment allocation must be separately owned: equal paths,
        // nested paths, cross-aliased arms and canonical aliases are refused,
        // while ordinary disjoint sibling layouts stay valid.
        let allocations: [(&str, &Path); 7] = [
            ("baseline home", &baseline.home),
            ("candidate home", &candidate_arm.home),
            ("baseline workload", &baseline.workload.path),
            ("candidate workload", &candidate_arm.workload.path),
            ("baseline runtime", &baseline.runtime.build),
            ("candidate runtime", &candidate_arm.runtime.build),
            ("candidate checkout", &self.candidate.path),
        ];
        for (index, (left_label, left)) in allocations.iter().enumerate() {
            for (right_label, right) in allocations.iter().skip(index + 1) {
                if allocations_overlap(left, right) {
                    return Err(invalid(format!(
                        "the {left_label} and {right_label} allocations overlap; each experiment allocation must be separately owned"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Pre-attempt gate for one upcoming arm: call immediately before that
    /// arm begins. It performs the structural [`Self::validate`] checks (which
    /// keep both arms' identities and a completed arm's output verified
    /// through the snapshot check) and additionally requires only the named
    /// arm's workload copy to be the pristine frozen snapshot — no prior
    /// solution edits or untracked artifacts, no extra or moved references,
    /// no unreachable sibling objects and no alternate or shared object
    /// database. A completed arm keeps its committed solution; checking the
    /// other arm never touches it. The gate is read-only: contamination is
    /// reported and preserved, never cleaned, so post-attempt snapshot checks
    /// (`task_worktree::verify_frozen`) and the executor's useful patches
    /// remain usable.
    pub fn verify_pre_attempt(&self, arm: Arm) -> io::Result<()> {
        self.validate()?;
        let binding = self.arm(arm)?;
        task_worktree::verify_frozen_pristine(&binding.workload).map_err(|error| {
            invalid(format!(
                "the {} arm workload is not a pristine pre-attempt snapshot: {error}",
                arm.as_str()
            ))
        })?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Completed real-task retention and corroboration selection.
//
// A completed real task is retained as identity plus a pristine, replayable
// copy of its frozen pre-solution inputs. The retention never stores or
// returns the task's solution, patch or conversation. When the declared
// adoption scope requires corroboration, applicable, independent, replayable
// retained tasks are selected by identity only; a task that does not exercise
// the mechanism is excluded as non-evidence, so too few applicable units leave
// the broader claim inconclusive rather than turning inapplicable workloads
// into a rejection, and absent retained evidence is never replaced by a
// summary or an invented saving.
// ---------------------------------------------------------------------------

/// One completed real task declared for retention, before its replayable copy
/// exists. Every field is a bounded reference; the operational identity is
/// derived from the frozen snapshot that actually ran, not from a summary.
/// The matching durable record on the owning Beads card is written by
/// `crate::board_hypothesis::record_retention` with the same identity fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRetention {
    /// The existing Beads card that owns the retained task; retention never
    /// creates a card or a second task store.
    pub owner: String,
    /// The workload identity within the experiment that completed it.
    pub case_id: String,
    /// The experiment that completed this real task.
    pub experiment: String,
    /// The mechanism this task actually exercises.
    pub mechanism: String,
    /// The declared applicability conditions of that mechanism.
    pub conditions: String,
    /// Identity of the fixed independent acceptance oracle.
    pub oracle: String,
    /// Reference to the retained independent acceptance evidence.
    pub acceptance: String,
}

/// A replayable completed real task retained under its Beads owner. The record
/// carries the frozen input identity and the retained pristine pre-solution
/// copy. It has no field for a solution, patch or conversation, so selection
/// for corroboration returns identity references and the replay is
/// reconstructed from pre-solution inputs alone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetainedTask {
    pub schema: u32,
    pub owner: String,
    pub case_id: String,
    pub experiment: String,
    pub mechanism: String,
    pub conditions: String,
    pub oracle: String,
    pub acceptance: String,
    /// The retained pristine pre-solution copy: the replayable task inputs.
    pub replay: FrozenCopy,
}

impl RetainedTask {
    /// Validate the recorded references. The replay copy's own verification is
    /// separate: a retained copy that is missing or no longer pristine makes
    /// the unit unusable without invalidating the record itself.
    pub fn validate(&self) -> io::Result<()> {
        if self.schema != EXPERIMENT_SCHEMA {
            return Err(invalid("unsupported retained task schema"));
        }
        bounded("owner", &self.owner, 128)?;
        bounded("case_id", &self.case_id, 128)?;
        bounded("experiment", &self.experiment, 128)?;
        bounded("mechanism", &self.mechanism, 96)?;
        bounded("conditions", &self.conditions, 96)?;
        bounded("oracle", &self.oracle, 512)?;
        bounded("acceptance", &self.acceptance, 512)?;
        Ok(())
    }

    /// Materialize a fresh pre-solution copy of the retained task inputs for
    /// one corroboration attempt. The retained copy must still verify as the
    /// pristine frozen snapshot, and the fresh copy must reproduce its exact
    /// identity; the completed task's solution is neither copied nor
    /// reachable from the result.
    pub fn prepare_replay(&self, destination: &Path) -> io::Result<FrozenCopy> {
        self.validate()?;
        task_worktree::verify_frozen_pristine(&self.replay)?;
        let copy =
            task_worktree::frozen_copy(&self.replay.path, &self.replay.revision, destination)?;
        if copy.tree_sha256 != self.replay.tree_sha256 || copy.revision != self.replay.revision {
            let _ = fs::remove_dir_all(&copy.path);
            return Err(invalid(
                "the replayed copy is not the retained frozen snapshot",
            ));
        }
        Ok(copy)
    }
}

/// Retain one completed real task: verify the frozen snapshot that ran, create
/// and verify the pristine replayable copy, and refuse any missing evidence
/// reference. `completed` may already hold the attempt's committed work (use
/// `task_worktree::verify_frozen`); the retained copy is materialized from the
/// frozen source revision, so the retained inputs stay pre-solution.
pub fn retain_completed_task(
    completed: &FrozenCopy,
    destination: &Path,
    retention: &TaskRetention,
) -> io::Result<RetainedTask> {
    bounded("owner", &retention.owner, 128)?;
    bounded("case_id", &retention.case_id, 128)?;
    bounded("experiment", &retention.experiment, 128)?;
    bounded("mechanism", &retention.mechanism, 96)?;
    bounded("conditions", &retention.conditions, 96)?;
    bounded("oracle", &retention.oracle, 512)?;
    bounded("acceptance", &retention.acceptance, 512)?;
    task_worktree::verify_frozen(completed)?;
    let copy =
        task_worktree::frozen_copy(&completed.source, &completed.source_revision, destination)?;
    let verified = (|| {
        if copy.tree_sha256 != completed.tree_sha256 || copy.revision != completed.revision {
            return Err(invalid(
                "the retained copy is not the frozen snapshot the completed task ran",
            ));
        }
        task_worktree::verify_frozen_pristine(&copy)
    })();
    if let Err(error) = verified {
        let _ = fs::remove_dir_all(&copy.path);
        return Err(error);
    }
    Ok(RetainedTask {
        schema: EXPERIMENT_SCHEMA,
        owner: retention.owner.clone(),
        case_id: retention.case_id.clone(),
        experiment: retention.experiment.clone(),
        mechanism: retention.mechanism.clone(),
        conditions: retention.conditions.clone(),
        oracle: retention.oracle.clone(),
        acceptance: retention.acceptance.clone(),
        replay: copy,
    })
}

/// The corroboration the declared adoption scope still requires, fixed before
/// any comparative result. `required_units` counts the additional independent
/// units; identities already part of the declared plan are excluded here
/// rather than after seeing an outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorroborationRequirement {
    /// The mechanism the additional units must exercise.
    pub mechanism: String,
    /// The applicability conditions the units must declare.
    pub conditions: String,
    /// Additional independent units the declared scope requires.
    pub required_units: u32,
    /// Unit identities already in the declared plan - a case id or an owner
    /// card - excluded before any result exists.
    pub excluded: Vec<String>,
}

/// One selected corroboration unit: identity and replay references only. No
/// solution, patch, conversation or acceptance content is part of the
/// selection, so a fresh executor reimplements the task without the earlier
/// answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorroborationUnit {
    pub owner: String,
    pub case_id: String,
    pub experiment: String,
    pub mechanism: String,
    pub conditions: String,
    /// Frozen root commit identity of the replayed snapshot.
    pub revision: String,
    /// Content digest over the frozen tree entries.
    pub tree_sha256: String,
}

/// Why a retained task is not part of the corroboration selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExclusionReason {
    /// The task does not exercise the declared mechanism and conditions; it is
    /// not evidence about the mechanism in either direction.
    NotApplicable,
    /// The unit is already part of the declared plan or is not independent of
    /// an earlier unit (same task or same frozen snapshot).
    AlreadyUsed,
    /// The retained copy no longer verifies as the pristine pre-solution
    /// snapshot, so the task cannot be replayed with retained inputs.
    NotReplayable {
        /// The verification failure; the contaminated state is preserved.
        detail: String,
    },
}

/// One candidate left out of the selection, by identity only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExcludedUnit {
    pub owner: String,
    pub case_id: String,
    pub reason: ExclusionReason,
}

/// What the selection supports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CorroborationStatus {
    /// Enough independent applicable replayable units were selected for the
    /// declared requirement.
    Ready,
    /// Fewer units than declared: the broader claim remains unsupported. A
    /// workload without the mechanism is not evidence against it, and a
    /// missing retained artifact cannot be replaced by a summary or an
    /// assumed saving.
    Inconclusive(String),
}

/// The deterministic corroboration selection over retained tasks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorroborationSelection {
    pub schema: u32,
    pub required_units: u32,
    pub status: CorroborationStatus,
    /// At most `required_units` units, in a fixed order independent of the
    /// caller's candidate order, so selection never extends the declared plan.
    pub units: Vec<CorroborationUnit>,
    pub excluded: Vec<ExcludedUnit>,
}

impl CorroborationSelection {
    /// True when the declared corroboration requirement is satisfied.
    pub fn is_ready(&self) -> bool {
        matches!(self.status, CorroborationStatus::Ready)
    }
}

/// Select corroboration units from retained completed real tasks. Selection is
/// order-independent: applicable, independent, replayable units are ordered by
/// task identity and only the declared number is returned. Fewer usable units
/// than declared produce an explicit inconclusive status, never a rejection or
/// a fabricated summary.
pub fn select_corroboration(
    candidates: &[RetainedTask],
    requirement: &CorroborationRequirement,
) -> io::Result<CorroborationSelection> {
    if requirement.required_units == 0 {
        return Err(invalid(
            "corroboration requires at least one independent unit",
        ));
    }
    bounded("mechanism", &requirement.mechanism, 96)?;
    bounded("conditions", &requirement.conditions, 96)?;
    let mut excluded = Vec::new();
    let mut admissible: Vec<&RetainedTask> = Vec::new();
    for candidate in candidates {
        candidate.validate()?;
        let already_planned = requirement
            .excluded
            .iter()
            .any(|identity| identity == &candidate.case_id || identity == &candidate.owner);
        if candidate.mechanism != requirement.mechanism
            || candidate.conditions != requirement.conditions
        {
            excluded.push(ExcludedUnit {
                owner: candidate.owner.clone(),
                case_id: candidate.case_id.clone(),
                reason: ExclusionReason::NotApplicable,
            });
        } else if already_planned {
            excluded.push(ExcludedUnit {
                owner: candidate.owner.clone(),
                case_id: candidate.case_id.clone(),
                reason: ExclusionReason::AlreadyUsed,
            });
        } else if let Err(error) = task_worktree::verify_frozen_pristine(&candidate.replay) {
            excluded.push(ExcludedUnit {
                owner: candidate.owner.clone(),
                case_id: candidate.case_id.clone(),
                reason: ExclusionReason::NotReplayable {
                    detail: error.to_string(),
                },
            });
        } else {
            admissible.push(candidate);
        }
    }
    admissible.sort_by(|left, right| {
        (
            left.case_id.as_str(),
            left.owner.as_str(),
            left.replay.tree_sha256.as_str(),
        )
            .cmp(&(
                right.case_id.as_str(),
                right.owner.as_str(),
                right.replay.tree_sha256.as_str(),
            ))
    });
    let mut units = Vec::new();
    let mut used_cases = BTreeSet::new();
    let mut used_trees = BTreeSet::new();
    for candidate in admissible {
        if units.len() >= requirement.required_units as usize {
            break;
        }
        if !used_cases.insert(candidate.case_id.clone())
            || !used_trees.insert(candidate.replay.tree_sha256.clone())
        {
            excluded.push(ExcludedUnit {
                owner: candidate.owner.clone(),
                case_id: candidate.case_id.clone(),
                reason: ExclusionReason::AlreadyUsed,
            });
            continue;
        }
        units.push(CorroborationUnit {
            owner: candidate.owner.clone(),
            case_id: candidate.case_id.clone(),
            experiment: candidate.experiment.clone(),
            mechanism: candidate.mechanism.clone(),
            conditions: candidate.conditions.clone(),
            revision: candidate.replay.revision.clone(),
            tree_sha256: candidate.replay.tree_sha256.clone(),
        });
    }
    let status = if units.len() >= requirement.required_units as usize {
        CorroborationStatus::Ready
    } else {
        CorroborationStatus::Inconclusive(format!(
            "fewer applicable independent replayable retained tasks than the declared corroboration requirement (required {}, admissible {}); the broader claim remains unsupported, and a workload that does not exercise the mechanism is not evidence against it",
            requirement.required_units,
            units.len()
        ))
    };
    Ok(CorroborationSelection {
        schema: EXPERIMENT_SCHEMA,
        required_units: requirement.required_units,
        status,
        units,
        excluded,
    })
}
