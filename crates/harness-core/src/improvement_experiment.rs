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
