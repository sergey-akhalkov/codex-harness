//! Durable controller state for one explicitly started improvement run.
//!
//! This module owns the *machine-owned* bookkeeping of `codex-harness improve`:
//! the explicit local run inputs ([`RunSpec`]), the bounded recoverable phase
//! cursor ([`Cursor`]), run ownership, the model-free dispatch and removal
//! gates and the stop/resume reconciliation vocabulary. Beads remains the
//! hypothesis and decision owner and OpenSpec remains the planning owner; the
//! cursor only references their identities and never becomes a second
//! hypothesis journal or decision ledger.
//!
//! Comparison execution and frozen runtime preparation are separate owners.
//! Phases that need them (`baseline-attempt`, `candidate-attempt`, acceptance,
//! decision, activation) are reachable only by recording an actual effect; a
//! controller slice that cannot perform that effect reports the phase as
//! explicitly pending instead of advancing.

use crate::process::{Cancellation, Deadline, ExclusiveFileLock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    time::Duration,
};

pub const RUN_SCHEMA: u32 = 1;
pub const CURSOR_SCHEMA: u32 = 1;
pub const OWNER_SCHEMA: u32 = 1;
pub const VARIANTS_SCHEMA: u32 = 1;
pub const MAX_RUN_SPEC_BYTES: u64 = 256 * 1024;
pub const MAX_CURSOR_BYTES: u64 = 1024 * 1024;
/// The cursor is bounded recovery data: old effects are dropped (with a
/// counted loss) instead of growing without limit. Durable evidence lives on
/// the board and in the retained run files, not in this history.
pub const MAX_EFFECTS: usize = 256;
pub const MAX_ATTEMPTS: usize = 64;
pub const MAX_SCOPE_ENTRIES: usize = 32;
pub const MAX_OWNER_BYTES: usize = 64;
/// How long a mutating command waits for the exclusive run-mutation guard
/// before reporting the busy controller instead of acting on stale state.
pub const MUTATION_LOCK_WAIT: Duration = Duration::from_secs(30);

const MAX_TOKEN: usize = 200;
const MAX_REASON: usize = 1024;
/// The deterministic dispatch-owner label budget. The native TUI truncates
/// long console captions, so a controller-authored owner must stay short; a
/// digest suffix keeps distinct runs distinct.
pub const MAX_DISPATCH_OWNER: usize = 32;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn bounded_read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(invalid(format!("{} is not a regular file", path.display())));
    }
    if metadata.len() > limit {
        return Err(invalid(format!(
            "{} exceeds the {} byte bound of its owner",
            path.display(),
            limit
        )));
    }
    fs::read(path)
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    if bytes.len() as u64 > MAX_CURSOR_BYTES {
        return Err(invalid(format!(
            "{} would exceed the durable state bound",
            path.display()
        )));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("run state path has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, &bytes)?;
    fs::rename(&temp, path)
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, limit: u64) -> io::Result<T> {
    let bytes = bounded_read(path, limit)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("{} is not valid JSON: {error}", path.display())))
}

fn token(name: &str, value: &str, max: usize) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > max
        || trimmed.starts_with('-')
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        || matches!(trimmed, "." | "..")
    {
        return Err(invalid(format!("invalid {name}")));
    }
    Ok(trimmed.to_owned())
}

fn line(name: &str, value: &str, max: usize) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > max || trimmed.contains(['\n', '\r']) {
        return Err(invalid(format!("missing, oversized or multiline {name}")));
    }
    Ok(trimmed.to_owned())
}

fn absolute_directory(name: &str, path: &Path) -> io::Result<()> {
    if !path.is_absolute() || !path.is_dir() {
        return Err(invalid(format!(
            "{name} must be an existing absolute directory: {}",
            path.display()
        )));
    }
    Ok(())
}

/// A relative scope entry: no traversal, no drive/UNC prefix, no `.` and no
/// repository metadata directories.
fn relative_scope_entry(value: &str) -> io::Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 240 {
        return Err(invalid(
            "writable scope entries must be short relative paths",
        ));
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(invalid(format!(
            "writable scope entry {trimmed} must be relative to the run project"
        )));
    }
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                if matches!(name.as_ref(), ".git" | ".beads") {
                    return Err(invalid(format!(
                        "writable scope entry {trimmed} may not cover repository metadata"
                    )));
                }
            }
            _ => {
                return Err(invalid(format!(
                    "writable scope entry {trimmed} contains a traversal or non-relative component"
                )));
            }
        }
    }
    Ok(())
}

/// One publication/activation stage the run authority permits.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum PublicationStage {
    Experiment,
    Integration,
    Publication,
}

impl PublicationStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Experiment => "experiment",
            Self::Integration => "integration",
            Self::Publication => "publication",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BoardInputs {
    /// The bd executable that owns the hypothesis cards.
    pub bd: PathBuf,
    /// The project whose board holds the hypothesis card.
    pub project: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunnerInputs {
    /// The dispatch profile; the complete model/effort selection comes from
    /// the installed profile binding, never from a per-assignment override.
    pub profile: String,
    /// The operator-declared effective model, when a specific one is expected.
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub reasoning_effort: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RemovalScope {
    /// The reviewed proposal reference (for example the linked OpenSpec change).
    pub proposal: String,
    /// The named removal target.
    pub target: String,
}

/// The explicit local run inputs. Every field is supplied by the operator as
/// private run data; nothing here is a checked-in endpoint or machine path.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunSpec {
    pub schema: u32,
    /// Stable run identity; also the default dispatch-owner stem.
    pub run: String,
    /// The source project the hypothesis is implemented in.
    pub project: PathBuf,
    /// The Codex home whose launcher and profile binding own dispatch.
    pub codex_home: PathBuf,
    pub board: BoardInputs,
    /// The OpenSpec planning target for this hypothesis.
    pub specification: crate::improvement_spec::Specification,
    /// The hypothesis card this run investigates.
    pub hypothesis_item: String,
    /// The frozen experiment contract the planning receipt qualifies.
    pub experiment: crate::improvement_spec::ExperimentContract,
    /// The committed base revision the run is frozen to.
    pub base_revision: String,
    /// The candidate's writable scope, relative to `project`.
    pub writable_scope: Vec<String>,
    /// The visible-dispatch profile, absent while model inputs are pending.
    pub runner: Option<RunnerInputs>,
    /// The declared local comparison runtime, when the experiment compares it.
    pub local_runner: Option<crate::outcome_qualification::LocalRunner>,
    /// The local qualification record produced by the qualification owner.
    pub qualification: Option<PathBuf>,
    /// The stages this run's authority permits.
    pub publication_scope: Vec<PublicationStage>,
    /// The independent acceptance/oracle reference, outside candidate writes.
    pub oracle: String,
    /// The removal treatment this run evaluates, when it is a removal.
    pub removal: Option<RemovalScope>,
}

impl RunSpec {
    pub fn load(path: &Path) -> io::Result<Self> {
        let spec: Self = read_json(path, MAX_RUN_SPEC_BYTES)?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> io::Result<()> {
        if self.schema != RUN_SCHEMA {
            return Err(invalid(format!(
                "run inputs declare schema {}; this controller reads schema {RUN_SCHEMA}",
                self.schema
            )));
        }
        token("run id", &self.run, MAX_OWNER_BYTES)?;
        token("hypothesis item", &self.hypothesis_item, MAX_TOKEN)?;
        if self.base_revision.len() < 7
            || self.base_revision.len() > 64
            || !self.base_revision.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid(
                "base_revision must be a 7-64 character hexadecimal commit revision",
            ));
        }
        absolute_directory("project", &self.project)?;
        absolute_directory("codex_home", &self.codex_home)?;
        absolute_directory("board project", &self.board.project)?;
        if !self.board.bd.is_file() {
            return Err(invalid(format!(
                "board bd executable is missing at {}",
                self.board.bd.display()
            )));
        }
        if self.specification.project != self.project {
            return Err(invalid(
                "the planning target project does not match the run project",
            ));
        }
        absolute_directory("planning root", &self.specification.planning_root)?;
        token("change", &self.specification.change, MAX_TOKEN)?;
        if let Some(store) = &self.specification.store {
            token("store", store, MAX_TOKEN)?;
        }
        self.experiment.validate()?;
        if self.writable_scope.is_empty() || self.writable_scope.len() > MAX_SCOPE_ENTRIES {
            return Err(invalid(
                "writable_scope must name 1-32 relative paths inside the project",
            ));
        }
        for entry in &self.writable_scope {
            relative_scope_entry(entry)?;
        }
        if let Some(runner) = &self.runner {
            token("runner profile", &runner.profile, MAX_OWNER_BYTES)?;
            if runner.model.is_some() != runner.model_provider.is_some() {
                return Err(invalid(
                    "runner model and model_provider must be declared together",
                ));
            }
            if let Some(model) = &runner.model {
                line("runner model", model, MAX_TOKEN)?;
            }
            if let Some(provider) = &runner.model_provider {
                line("runner model provider", provider, MAX_TOKEN)?;
            }
            if let Some(effort) = &runner.reasoning_effort {
                line("runner reasoning effort", effort, MAX_TOKEN)?;
            }
        }
        if self.local_runner.is_some() && self.qualification.is_none() {
            return Err(invalid(
                "a declared local runner requires the qualification record path",
            ));
        }
        if let Some(path) = &self.qualification
            && !path.is_absolute()
        {
            return Err(invalid("the qualification record path must be absolute"));
        }
        let mut stages = self.publication_scope.clone();
        stages.sort_unstable();
        stages.dedup();
        if stages.len() != self.publication_scope.len() {
            return Err(invalid("publication_scope names a stage more than once"));
        }
        line("oracle reference", &self.oracle, MAX_REASON)?;
        if let Some(removal) = &self.removal {
            token("removal proposal", &removal.proposal, MAX_TOKEN)?;
            token("removal target", &removal.target, MAX_TOKEN)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> io::Result<String> {
        let bytes = serde_json::to_vec(self)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    pub fn permits(&self, stage: PublicationStage) -> bool {
        self.publication_scope.contains(&stage)
    }

    /// Protects the running supervisor, the frozen policy, the planning
    /// artifacts and the independent oracle from candidate writes. A candidate
    /// cannot activate itself, rewrite its decision policy or weaken its own
    /// checks during the experiment.
    pub fn supervisor_gate(
        &self,
        run_dir: &Path,
        change_root: &Path,
        oracle: &str,
    ) -> io::Result<()> {
        let oracle_path = absolute_path_reference(oracle);
        let project = &self.project;
        for entry in &self.writable_scope {
            let scope = project.join(entry);
            for (name, protected) in [("run state", run_dir), ("planning artifacts", change_root)] {
                // Overlap in either direction: a scope inside the protected
                // root writes it, and a scope that is an ancestor of it
                // reaches it through its own subtree.
                if protected.starts_with(&scope) || scope.starts_with(protected) {
                    return Err(invalid(format!(
                        "writable scope {entry} covers the {name} at {}; a candidate cannot rewrite its own supervisor, policy or acceptance",
                        protected.display()
                    )));
                }
            }
            if let Some(oracle_path) = &oracle_path
                && (oracle_path.starts_with(&scope) || scope.starts_with(oracle_path))
            {
                return Err(invalid(format!(
                    "writable scope {entry} covers the independent oracle at {}; candidate writes cannot reach acceptance inputs",
                    oracle_path.display()
                )));
            }
        }
        Ok(())
    }
}

/// Interprets a reference as a filesystem path only when it is an absolute
/// path; a textual reference such as `outcome-oracle:request` stays opaque.
pub fn absolute_path_reference(reference: &str) -> Option<PathBuf> {
    let path = PathBuf::from(reference);
    path.is_absolute().then_some(path)
}

/// The recoverable phase cursor. `Blocked`/`Idle`/`Stopped` are conditions of
/// the surrounding phase, not a second hypothesis lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Planning,
    CandidateReady,
    BaselineAttempt,
    CandidateAttempt,
    Acceptance,
    DecisionRecorded,
    ActivationConfirmed,
    Idle,
    Blocked,
    Stopped,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::CandidateReady => "candidate-ready",
            Self::BaselineAttempt => "baseline-attempt",
            Self::CandidateAttempt => "candidate-attempt",
            Self::Acceptance => "acceptance",
            Self::DecisionRecorded => "decision-recorded",
            Self::ActivationConfirmed => "activation-confirmed",
            Self::Idle => "idle",
            Self::Blocked => "blocked",
            Self::Stopped => "stopped",
        }
    }

    /// A phase that needs the comparison/runtime-preparation owners. This
    /// controller slice never advances into one without a recorded effect.
    pub fn requires_comparison_owner(self) -> bool {
        matches!(
            self,
            Self::CandidateReady
                | Self::BaselineAttempt
                | Self::CandidateAttempt
                | Self::Acceptance
                | Self::DecisionRecorded
                | Self::ActivationConfirmed
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AttemptRole {
    Investigator,
    Implementer,
    Baseline,
    Candidate,
}

impl AttemptRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Investigator => "investigator",
            Self::Implementer => "implementer",
            Self::Baseline => "baseline",
            Self::Candidate => "candidate",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Self::Investigator => "inv",
            Self::Implementer => "impl",
            Self::Baseline => "base",
            Self::Candidate => "cand",
        }
    }

    /// An arm whose conversation is a measured comparison attempt.
    pub fn is_measured(self) -> bool {
        matches!(self, Self::Baseline | Self::Candidate)
    }

    /// A conversation that applies the candidate treatment, including any
    /// declared removal, to the experiment.
    pub fn applies_treatment(self) -> bool {
        matches!(self, Self::Implementer | Self::Candidate)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AttemptState {
    /// Recorded before the visible surface opened; never inference evidence.
    Requested,
    /// The owned dispatch accepted the assignment; the outcome is unknown
    /// until the receipt records a terminal state.
    Started,
    Completed,
    Failed,
    Interrupted,
    Stopped,
    /// The outcome is unknown; the attempt must be reconciled through the
    /// owning dispatcher and is never resubmitted automatically.
    Unknown,
}

impl AttemptState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }

    /// An attempt whose model effect may still be running or unresolved.
    pub fn is_in_flight(self) -> bool {
        matches!(self, Self::Requested | Self::Started)
    }

    pub fn is_terminal(self) -> bool {
        !self.is_in_flight()
    }
}

/// The outcome the owning dispatcher's receipt reported for one attempt. The
/// mapping from the native receipt is performed by the CLI; the core keeps
/// this vocabulary so the state machine is testable without a receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservedOutcome {
    Completed,
    Failed,
    Interrupted,
    Stopped,
    /// The recorded host still runs: the attempt stays active.
    Active,
    /// No terminal record and no live host: the outcome is unknown.
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub id: String,
    pub role: AttemptRole,
    /// The `executor` owner label; bounded so the titled surface stays
    /// verifiable by the frontend attach guard.
    pub owner: String,
    /// The exact console/tab title the dispatch used.
    pub title: String,
    pub profile: String,
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub reasoning_effort: Option<String>,
    /// The bound workspace the assignment was validated against.
    pub checkout: Option<PathBuf>,
    pub assignment: Option<PathBuf>,
    pub receipt: Option<PathBuf>,
    pub result: Option<PathBuf>,
    pub detail: Option<PathBuf>,
    pub state: AttemptState,
    pub reason: Option<String>,
    /// Why a completed attempt cannot be reused without remeasurement.
    #[serde(default)]
    pub reuse_refused: Option<String>,
    pub started_ms: u64,
    pub updated_ms: u64,
}

impl Attempt {
    pub fn settle(&mut self, outcome: ObservedOutcome, at_ms: u64) {
        let (state, reason) = match outcome {
            ObservedOutcome::Completed => (AttemptState::Completed, None),
            ObservedOutcome::Failed => (AttemptState::Failed, None),
            ObservedOutcome::Interrupted => (AttemptState::Interrupted, None),
            ObservedOutcome::Stopped => (AttemptState::Stopped, None),
            ObservedOutcome::Active => return,
            ObservedOutcome::Unknown => (
                AttemptState::Unknown,
                Some(
                    "the recorded outcome is unknown; reconcile through the owning dispatcher before reuse and do not resubmit"
                        .to_owned(),
                ),
            ),
        };
        self.state = state;
        if let Some(reason) = reason {
            self.reason = Some(reason);
        }
        self.updated_ms = at_ms;
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EffectKind {
    PlanningQualified,
    DispatchPrepared,
    DispatchAccepted,
    DispatchRefused,
    RemovalChecked,
    VariantSelected,
    VariantsUnavailable,
    StopRequested,
    /// One verified owned-process cleanup performed by the exact-identity
    /// executor stop owner, or the explicit retention when cleanup could not
    /// be established.
    OwnedCleanup,
    OwnershipTaken,
    Resumed,
    Reconciled,
    RemeasurementRequired,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    pub kind: EffectKind,
    pub detail: String,
    pub at_ms: u64,
}

/// The durable cursor: bounded recovery data referencing the board, planning
/// and runtime identities. It never records a hypothesis decision.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub schema: u32,
    pub run: String,
    pub spec_digest: String,
    /// The resolved planning change root, frozen at start.
    pub change_root: PathBuf,
    pub phase: Phase,
    pub previous_phase: Option<Phase>,
    /// Why the run is blocked or stopped.
    pub condition: Option<String>,
    pub hypothesis_item: String,
    pub experiment: String,
    /// The reviewed removal proposal digest frozen at start, when the run is a
    /// removal treatment. A later proposal version with any changed reviewed
    /// field invalidates it.
    pub removal_frozen: Option<String>,
    pub attempts: Vec<Attempt>,
    pub effects: Vec<Effect>,
    /// Effects dropped from the bounded history; the durable owners keep the
    /// evidence, this counter keeps the loss visible.
    #[serde(default)]
    pub effects_dropped: u64,
    pub selected_variant: Option<String>,
    pub selected_runtime: Option<PathBuf>,
    pub selected_identity: Option<String>,
    pub updated_ms: u64,
}

impl Cursor {
    pub fn new(run: &str, spec_digest: String, change_root: PathBuf, item: &str) -> Self {
        Self {
            schema: CURSOR_SCHEMA,
            run: run.to_owned(),
            spec_digest,
            change_root,
            phase: Phase::Planning,
            previous_phase: None,
            condition: None,
            hypothesis_item: item.to_owned(),
            experiment: format!("{run}-experiment-1"),
            removal_frozen: None,
            attempts: Vec::new(),
            effects: Vec::new(),
            effects_dropped: 0,
            selected_variant: None,
            selected_runtime: None,
            selected_identity: None,
            updated_ms: now_ms(),
        }
    }

    pub fn effect(&mut self, kind: EffectKind, detail: impl Into<String>) {
        self.effects.push(Effect {
            kind,
            detail: detail.into(),
            at_ms: now_ms(),
        });
        if self.effects.len() > MAX_EFFECTS {
            let drop = self.effects.len() - MAX_EFFECTS;
            self.effects.drain(..drop);
            self.effects_dropped += drop as u64;
        }
        self.updated_ms = now_ms();
    }

    pub fn block(&mut self, reason: impl Into<String>) {
        if self.phase != Phase::Blocked {
            self.previous_phase = Some(self.phase);
            self.phase = Phase::Blocked;
        }
        self.condition = Some(reason.into());
        self.updated_ms = now_ms();
    }

    pub fn clear_blocked(&mut self) {
        if self.phase == Phase::Blocked {
            self.phase = self.previous_phase.take().unwrap_or(Phase::Planning);
            self.condition = None;
        }
        self.updated_ms = now_ms();
    }

    pub fn stop(&mut self, reason: &str) {
        if self.phase != Phase::Stopped {
            self.previous_phase = Some(self.phase);
        }
        self.phase = Phase::Stopped;
        self.condition = Some(reason.to_owned());
        for attempt in &mut self.attempts {
            if attempt.state.is_in_flight() {
                attempt.state = AttemptState::Unknown;
                attempt.reason = Some(
                    "the run was stopped while this attempt was in flight; its outcome must be reconciled through the owning dispatcher and is never replayed"
                        .to_owned(),
                );
                attempt.updated_ms = now_ms();
            }
        }
        self.effect(EffectKind::StopRequested, reason);
    }

    /// Restores the phase a stop suspended. Blocked reasons from
    /// reconciliation remain the caller's input; this only unwinds `Stopped`.
    pub fn resume_phase(&mut self) -> Phase {
        if self.phase == Phase::Stopped {
            self.phase = self.previous_phase.take().unwrap_or(Phase::Planning);
            self.condition = None;
        }
        self.updated_ms = now_ms();
        self.phase
    }

    pub fn attempt(&self, id: &str) -> Option<&Attempt> {
        self.attempts.iter().find(|attempt| attempt.id == id)
    }

    pub fn active_attempt(&self) -> Option<&Attempt> {
        self.attempts
            .iter()
            .find(|attempt| attempt.state.is_in_flight())
    }

    pub fn unresolved_attempts(&self) -> Vec<&Attempt> {
        self.attempts
            .iter()
            .filter(|attempt| attempt.state == AttemptState::Unknown)
            .collect()
    }

    /// The attempts a resume must reconcile from their exact receipts: every
    /// in-flight attempt plus every attempt whose outcome is still unknown.
    /// A stopped run retains its in-flight effects as unknown, and a later
    /// authoritative receipt completion or failure settles them once; a truly
    /// unobserved attempt stays unknown and is never replayed.
    pub fn attempts_requiring_reconciliation(&self) -> Vec<&Attempt> {
        self.attempts
            .iter()
            .filter(|attempt| {
                attempt.state.is_in_flight() || attempt.state == AttemptState::Unknown
            })
            .collect()
    }

    pub fn push_attempt(&mut self, attempt: Attempt) -> io::Result<()> {
        if self.attempts.len() >= MAX_ATTEMPTS {
            return Err(invalid(format!(
                "the run already recorded {MAX_ATTEMPTS} attempts; retention is bounded and the cursor refuses further growth"
            )));
        }
        self.attempts.push(attempt);
        self.updated_ms = now_ms();
        Ok(())
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// One process owner of a run. A recorded owner that is verifiably gone may be
/// taken over by `resume`; a live one blocks every mutating operation.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OwnerRecord {
    pub schema: u32,
    pub run: String,
    pub pid: u32,
    /// Windows FILETIME creation time; a bare pid is not an identity.
    pub created: u64,
    pub program: PathBuf,
    pub claimed_ms: u64,
}

impl OwnerRecord {
    pub fn current(run: &str) -> io::Result<Self> {
        let (pid, created, program) = current_process_identity()?;
        Ok(Self {
            schema: OWNER_SCHEMA,
            run: run.to_owned(),
            pid,
            created,
            program,
            claimed_ms: now_ms(),
        })
    }
}

/// The resolved ownership of one mutation: the record this command wrote and
/// the record it replaced, when one was recorded. The exclusive run-mutation
/// guard - not pid liveness - is the mutual exclusion, so a previously
/// recorded owner that still runs is reported as a takeover instead of
/// blocking the serialized writer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ownership {
    pub owner: OwnerRecord,
    pub previous: Option<OwnerRecord>,
    /// Whether the replaced owner still ran when it was replaced.
    pub previous_live: Option<bool>,
}

impl Ownership {
    pub fn takeover_note(&self) -> Option<String> {
        let previous = self.previous.as_ref()?;
        if previous.pid == self.owner.pid
            && previous.created == self.owner.created
            && previous.program == self.owner.program
        {
            return None;
        }
        Some(format!(
            "ownership taken over from the previously recorded owner pid {} ({}, {}) under the exclusive run-mutation guard",
            previous.pid,
            previous.program.display(),
            match self.previous_live {
                Some(true) => "still running",
                Some(false) => "no longer running",
                None => "liveness unverifiable",
            }
        ))
    }
}

#[cfg(windows)]
pub fn current_process_identity() -> io::Result<(u32, u64, PathBuf)> {
    let program = std::env::current_exe()?.canonicalize()?;
    let user = crate::process_service::current_user()?;
    let identity =
        crate::process_service::ServiceProcess::observe(std::process::id(), &program, 0, &user)?
            .identity();
    Ok((identity.pid, identity.creation_time, program))
}

#[cfg(not(windows))]
pub fn current_process_identity() -> io::Result<(u32, u64, PathBuf)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native process identity is unavailable on this platform",
    ))
}

/// Whether the recorded owner still runs. `None` means liveness cannot be
/// established (no native process identity owner, access failure or an image
/// mismatch), and the caller must refuse takeover instead of guessing.
#[cfg(windows)]
pub fn owner_is_live(record: &OwnerRecord) -> io::Result<Option<bool>> {
    let user = crate::process_service::current_user()?;
    match crate::process_service::ServiceProcess::inspect(
        crate::process::ProcessIdentity {
            pid: record.pid,
            creation_time: record.created,
        },
        &record.program,
        &user,
    ) {
        Ok(Some(_)) => Ok(Some(true)),
        Ok(None) => Ok(Some(false)),
        Err(_) => Ok(None),
    }
}

#[cfg(not(windows))]
pub fn owner_is_live(_record: &OwnerRecord) -> io::Result<Option<bool>> {
    Ok(None)
}

/// The private run store: one directory per run holding only the frozen spec,
/// the planning receipt, the cursor, the owner record and retained dispatch
/// assignments. It never becomes a hypothesis queue or decision journal.
#[derive(Debug)]
pub struct RunStore {
    root: PathBuf,
}

pub const SPEC_FILE: &str = "spec.json";
pub const PLANNING_FILE: &str = "planning.json";
pub const CURSOR_FILE: &str = "cursor.json";
pub const OWNER_FILE: &str = "owner.json";
pub const VARIANTS_FILE: &str = "variants.json";
pub const ASSIGNMENTS_DIR: &str = "assignments";
/// The stable lock file for the exclusive run-mutation guard. Every mutating
/// operation acquires it *before* reading run state, so two commands can
/// neither create duplicate run ownership nor overwrite each other's cursor
/// with a stale read-modify-write.
pub const MUTATION_LOCK_FILE: &str = "mutation.lock";

/// The exclusive run-mutation guard: a native whole-file OS lock held across
/// the fresh state read, the gates, the recorded effects and the persisted
/// state. Dropping it releases the lock; read-only `status` does not take it.
#[derive(Debug)]
pub struct RunMutation {
    _lock: ExclusiveFileLock,
}

fn acquire_mutation_lock(root: &Path, wait: Duration) -> io::Result<RunMutation> {
    let path = root.join(MUTATION_LOCK_FILE);
    let deadline = Deadline::after(wait)?;
    ExclusiveFileLock::acquire(&path, deadline, &Cancellation::default())
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "the exclusive run-mutation guard at {} was not acquired within {:.1}s: {error}; another controller or command owns this run, so no state was read or changed",
                    path.display(),
                    wait.as_secs_f32()
                ),
            )
        })
        .map(|lock| RunMutation { _lock: lock })
}

impl RunStore {
    /// Creates the run directory (when needed) and acquires the exclusive
    /// mutation guard *before* any run state is read or written. An existing
    /// store is a duplicate run ownership: the second `start` is refused with
    /// the resume remedy.
    pub fn create(
        root: &Path,
        spec: &RunSpec,
        spec_digest: &str,
        change_root: &Path,
    ) -> io::Result<Self> {
        let (store, _guard) = Self::lock_new(root)?;
        store.create_locked(spec, spec_digest, change_root)?;
        store.claim_ownership(&spec.run)?;
        Ok(store)
    }

    /// Acquires the exclusive mutation guard for a run that does not exist yet.
    /// The caller must hold the guard across creation, ownership claiming and
    /// every subsequent state read or write.
    pub fn lock_new(root: &Path) -> io::Result<(Self, RunMutation)> {
        Self::lock_new_within(root, MUTATION_LOCK_WAIT)
    }

    pub fn lock_new_within(root: &Path, wait: Duration) -> io::Result<(Self, RunMutation)> {
        if !root.is_absolute() {
            return Err(invalid("the run directory must be absolute"));
        }
        fs::create_dir_all(root)?;
        let guard = acquire_mutation_lock(root, wait)?;
        let store = Self {
            root: root.to_path_buf(),
        };
        Ok((store, guard))
    }

    /// Opens an existing run and acquires the exclusive mutation guard before
    /// the caller reads any state.
    pub fn open_locked(root: &Path) -> io::Result<(Self, RunMutation)> {
        Self::open_locked_within(root, MUTATION_LOCK_WAIT)
    }

    pub fn open_locked_within(root: &Path, wait: Duration) -> io::Result<(Self, RunMutation)> {
        let store = Self::open(root)?;
        let guard = acquire_mutation_lock(&store.root, wait)?;
        Ok((store, guard))
    }

    /// Writes a fresh store under an already-held mutation guard. Repeating
    /// this for an existing run is the duplicate-ownership refusal.
    pub fn create_locked(
        &self,
        spec: &RunSpec,
        spec_digest: &str,
        change_root: &Path,
    ) -> io::Result<()> {
        if self.root.join(SPEC_FILE).exists() || self.root.join(CURSOR_FILE).exists() {
            return Err(invalid(format!(
                "run state already exists at {}; a second start would duplicate run ownership - use `improve resume` to take over the interrupted or stopped run",
                self.root.display()
            )));
        }
        fs::create_dir_all(self.root.join(ASSIGNMENTS_DIR))?;
        write_json_atomic(&self.spec_path(), spec)?;
        self.save_cursor(&Cursor::new(
            &spec.run,
            spec_digest.to_owned(),
            change_root.to_path_buf(),
            &spec.hypothesis_item,
        ))
    }

    pub fn open(root: &Path) -> io::Result<Self> {
        if !root.is_absolute() {
            return Err(invalid("the run directory must be absolute"));
        }
        if !root.join(SPEC_FILE).is_file() {
            return Err(invalid(format!(
                "no improvement run exists at {} (missing {}); `improve start` creates it",
                root.display(),
                SPEC_FILE
            )));
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn spec_path(&self) -> PathBuf {
        self.root.join(SPEC_FILE)
    }

    pub fn planning_path(&self) -> PathBuf {
        self.root.join(PLANNING_FILE)
    }

    pub fn cursor_path(&self) -> PathBuf {
        self.root.join(CURSOR_FILE)
    }

    pub fn owner_path(&self) -> PathBuf {
        self.root.join(OWNER_FILE)
    }

    pub fn variants_path(&self) -> PathBuf {
        self.root.join(VARIANTS_FILE)
    }

    pub fn assignments_dir(&self) -> PathBuf {
        self.root.join(ASSIGNMENTS_DIR)
    }

    pub fn spec(&self) -> io::Result<RunSpec> {
        RunSpec::load(&self.spec_path())
    }

    pub fn cursor(&self) -> io::Result<Cursor> {
        let cursor: Cursor = read_json(&self.cursor_path(), MAX_CURSOR_BYTES)?;
        if cursor.schema != CURSOR_SCHEMA {
            return Err(invalid("the run cursor has an unsupported schema"));
        }
        Ok(cursor)
    }

    pub fn save_cursor(&self, cursor: &Cursor) -> io::Result<()> {
        write_json_atomic(&self.cursor_path(), cursor)
    }

    pub fn planning(&self) -> io::Result<crate::improvement_spec::PlanningReceipt> {
        read_json(&self.planning_path(), MAX_RUN_SPEC_BYTES)
    }

    pub fn save_planning(
        &self,
        receipt: &crate::improvement_spec::PlanningReceipt,
    ) -> io::Result<()> {
        write_json_atomic(&self.planning_path(), receipt)
    }

    pub fn owner(&self) -> io::Result<Option<OwnerRecord>> {
        match read_json::<OwnerRecord>(&self.owner_path(), 64 * 1024) {
            Ok(record) => Ok(Some(record)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Records this process as the run's owner. The caller holds the exclusive
    /// run-mutation guard, so the replacement is serialized; a previously
    /// recorded owner is reported (and whether it still ran) rather than
    /// refusing the takeover, because liveness cannot prove that the old owner
    /// still holds the guard.
    pub fn claim_ownership(&self, run: &str) -> io::Result<Ownership> {
        let previous = self.owner()?;
        if let Some(previous) = &previous
            && previous.run != run
        {
            return Err(invalid(format!(
                "run state records owner run={} instead of {run}",
                previous.run
            )));
        }
        let previous_live = match &previous {
            Some(previous) => owner_is_live(previous)?,
            None => None,
        };
        let owner = OwnerRecord::current(run)?;
        write_json_atomic(&self.owner_path(), &owner)?;
        Ok(Ownership {
            owner,
            previous,
            previous_live,
        })
    }
}

/// One prepared runtime of the baseline/candidate pair. Preparation itself is
/// a separate owner; selection only consumes the recorded artifact identities.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    /// The native build selection state that owns this runtime.
    pub state: PathBuf,
    /// The prepared immutable build published inside that state.
    pub build: PathBuf,
    /// The declared identity from preparation; verified against the build
    /// record before selection.
    #[serde(default)]
    pub identity: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VariantSet {
    pub schema: u32,
    pub baseline: Variant,
    pub candidate: Variant,
}

impl VariantSet {
    pub fn load(path: &Path) -> io::Result<Self> {
        let set: Self = read_json(path, MAX_RUN_SPEC_BYTES)?;
        if set.schema != VARIANTS_SCHEMA {
            return Err(invalid("prepared variants declare an unsupported schema"));
        }
        for variant in [&set.baseline, &set.candidate] {
            if !variant.state.is_absolute() || !variant.build.is_absolute() {
                return Err(invalid(
                    "prepared runtime state and build paths must be absolute",
                ));
            }
        }
        Ok(set)
    }

    pub fn named(&self, name: &str) -> Option<&Variant> {
        match name {
            "baseline" => Some(&self.baseline),
            "candidate" => Some(&self.candidate),
            _ => None,
        }
    }
}

/// The states of the removal gate. Removal consent is separate from benefit:
/// every removal effect checks the *current* board decision and the frozen
/// reviewed proposal identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemovalGate {
    Authorized {
        reviewed: String,
    },
    /// A current decision exists and is a refusal: do not repeat the request
    /// without a new evidential basis or user instruction.
    Refused {
        reviewed: Option<String>,
    },
    Withdrawn {
        reviewed: Option<String>,
    },
    /// Waiting for input: no decision, an incomplete proposal version, a
    /// changed proposal version or an uncovered action.
    Pending {
        reason: String,
    },
}

/// The removal gate used by every removal effect boundary. It resolves the
/// current authority for one exact item/proposal/target/action against the live
/// board comments, and binds it to the digest frozen at run start. Any changed
/// reviewed field - including the bounded prose detail - requires a fresh
/// decision before the effect; a missing, refused or withdrawn decision blocks
/// it, and no benefit verdict is consulted as consent.
pub fn removal_gate_at(
    item: &str,
    request: &crate::board_hypothesis::AuthorityRequest,
    comments: &[String],
    frozen_reviewed: Option<&str>,
) -> RemovalGate {
    use crate::board_hypothesis::{
        RemovalAuthority, parse_removal_proposals, reviewed_proposal_digest,
    };
    let current = parse_removal_proposals(comments)
        .into_iter()
        .rfind(|proposal| {
            proposal.item == item
                && proposal.proposal == request.proposal
                && proposal.target == request.target
        });
    if let Some(frozen) = frozen_reviewed {
        match &current {
            Some(proposal) if reviewed_proposal_digest(proposal) == frozen => {}
            Some(_) => {
                return RemovalGate::Pending {
                    reason: format!(
                        "the reviewed proposal for proposal={} target={} changed after the frozen approval; a fresh decision is required",
                        request.proposal, request.target
                    ),
                };
            }
            None => {
                return RemovalGate::Pending {
                    reason: format!(
                        "the frozen removal proposal for proposal={} target={} is no longer recorded; a fresh decision is required",
                        request.proposal, request.target
                    ),
                };
            }
        }
    } else if current.is_none() {
        return RemovalGate::Pending {
            reason: format!(
                "no reviewed removal proposal for proposal={} target={} is recorded; prepare the proposal and obtain the user's decision",
                request.proposal, request.target
            ),
        };
    }
    match crate::board_hypothesis::removal_authority(comments, item, request) {
        RemovalAuthority::Authorized { record } => RemovalGate::Authorized {
            reviewed: record.reviewed.unwrap_or_default(),
        },
        RemovalAuthority::Refused { record } => RemovalGate::Refused {
            reviewed: record.reviewed,
        },
        RemovalAuthority::Withdrawn { record } => RemovalGate::Withdrawn {
            reviewed: record.reviewed,
        },
        RemovalAuthority::Missing => RemovalGate::Pending {
            reason: format!(
                "no removal decision for proposal={} target={} action={} is recorded; the missing approval blocks this effect",
                request.proposal,
                request.target,
                request.action.as_str()
            ),
        },
        RemovalAuthority::NotCovered { reason, .. } => RemovalGate::Pending { reason },
    }
}

/// What the dispatch gate decided for the next bounded conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchGate {
    Ready,
    Blocked { reason: String },
}

/// Inputs the gate needs that the cursor cannot know by itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DispatchFacts {
    /// `None` while model inputs are pending.
    pub runner_declared: bool,
    /// The installed launcher the visible surface needs; `None` when missing.
    pub launcher: Option<PathBuf>,
    /// The resolved profile binding error, when the profile is unavailable.
    pub binding_error: Option<String>,
    /// A refusal produced by the qualification owner, when strict comparison
    /// inputs are missing or unqualified.
    pub qualification_block: Option<String>,
    /// The current removal gate, when the run is a removal treatment.
    pub removal: Option<RemovalGate>,
    /// Whether any prior dispatch lost its required surface without an
    /// explicit stop: new model work must stay suspended.
    pub surface_loss: Option<String>,
}

/// The model-free dispatch gate. Actual model dispatch may only proceed from
/// [`DispatchGate::Ready`]; every blocked reason names the missing fact and no
/// hidden fallback is attempted. Local qualification blocks the measured arms;
/// a declared removal blocks the conversations that apply the treatment, not
/// the bounded investigator.
pub fn dispatch_gate(cursor: &Cursor, role: AttemptRole, facts: &DispatchFacts) -> DispatchGate {
    if let Some(active) = cursor.active_attempt() {
        return DispatchGate::Blocked {
            reason: format!(
                "attempt {} ({}) is still recorded as {} ; the frozen runtime is kept until it finishes or is explicitly cancelled",
                active.id,
                active.role.as_str(),
                active.state.as_str()
            ),
        };
    }
    let unresolved = cursor.unresolved_attempts();
    if !unresolved.is_empty() {
        let ids: Vec<&str> = unresolved
            .iter()
            .map(|attempt| attempt.id.as_str())
            .collect();
        return DispatchGate::Blocked {
            reason: format!(
                "unknown outcome for attempt(s) {}: reconcile through the owning dispatcher (`executor watch`/`executor stop`) before any new dispatch; an unknown attempt is never resubmitted",
                ids.join(", ")
            ),
        };
    }
    if let Some(reason) = &facts.surface_loss {
        return DispatchGate::Blocked {
            reason: format!(
                "missing visibility: {reason}; no new model work is started until the surface is restored or the run is explicitly stopped, and no hidden fallback is used"
            ),
        };
    }
    if !facts.runner_declared {
        return DispatchGate::Blocked {
            reason: "model inputs are pending: the run declares no runner profile, so only model-free preparation is performed"
                .to_owned(),
        };
    }
    if let Some(error) = &facts.binding_error {
        return DispatchGate::Blocked {
            reason: format!("the dispatch profile is unusable: {error}"),
        };
    }
    let Some(launcher) = &facts.launcher else {
        return DispatchGate::Blocked {
            reason:
                "missing visibility: the installed Codex launcher is missing, so no titled conversation surface can open"
                    .to_owned(),
        };
    };
    if !launcher.is_file() {
        return DispatchGate::Blocked {
            reason: format!(
                "missing visibility: the installed Codex launcher at {} is missing",
                launcher.display()
            ),
        };
    }
    if role.is_measured()
        && let Some(block) = &facts.qualification_block
    {
        return DispatchGate::Blocked {
            reason: format!("local qualification blocks dependent model dispatch: {block}"),
        };
    }
    if role.applies_treatment() {
        match &facts.removal {
            Some(RemovalGate::Authorized { .. }) | None => {}
            Some(RemovalGate::Refused { .. }) => {
                return DispatchGate::Blocked {
                    reason: "the user declined this removal; the dependent removal effect stays blocked and the request is not repeated without a new evidential basis"
                        .to_owned(),
                };
            }
            Some(RemovalGate::Withdrawn { .. }) => {
                return DispatchGate::Blocked {
                    reason: "the removal approval was withdrawn; the dependent removal effect stays blocked until a new decision".to_owned(),
                };
            }
            Some(RemovalGate::Pending { reason }) => {
                return DispatchGate::Blocked {
                    reason: format!("removal approval is pending: {reason}"),
                };
            }
        }
    }
    DispatchGate::Ready
}

/// The removal gate of a run that declares a removal treatment, resolved
/// against the live comments; `None` for ordinary runs.
pub fn declared_removal_gate(
    spec: &RunSpec,
    cursor: &Cursor,
    comments: &[String],
    action: crate::board_hypothesis::RemovalAction,
) -> Option<RemovalGate> {
    let removal = spec.removal.as_ref()?;
    let request = crate::board_hypothesis::AuthorityRequest {
        proposal: removal.proposal.clone(),
        target: removal.target.clone(),
        action,
    };
    Some(removal_gate_at(
        &spec.hypothesis_item,
        &request,
        comments,
        cursor.removal_frozen.as_deref(),
    ))
}

/// The digest to freeze at start for a declared removal: the current complete
/// proposal version, when one is already recorded.
pub fn frozen_removal_digest(spec: &RunSpec, comments: &[String]) -> Option<String> {
    let removal = spec.removal.as_ref()?;
    let proposal = crate::board_hypothesis::parse_removal_proposals(comments)
        .into_iter()
        .rfind(|proposal| {
            proposal.item == spec.hypothesis_item
                && proposal.proposal == removal.proposal
                && proposal.target == removal.target
        })?;
    Some(crate::board_hypothesis::reviewed_proposal_digest(&proposal))
}

/// A deterministic, bounded dispatch owner for one attempt. Distinct runs and
/// roles stay distinct through a digest suffix while the title remains short
/// enough for the native TUI caption the frontend attach guard verifies.
pub fn dispatch_owner(run: &str, role: AttemptRole, ordinal: u32) -> String {
    let candidate = format!("{run}-{}-{ordinal}", role.short());
    if candidate.len() <= MAX_DISPATCH_OWNER {
        return candidate;
    }
    let digest = format!("{:x}", Sha256::digest(run.as_bytes()));
    let head: String = run
        .chars()
        .take(12)
        .collect::<String>()
        .trim_end_matches('-')
        .to_owned();
    let owner = format!("{}-{}-{ordinal}-{}", head, role.short(), &digest[..8]);
    if owner.len() <= MAX_DISPATCH_OWNER {
        owner
    } else {
        format!("imp-{}-{}-{ordinal}", &digest[..12], role.short())
    }
}

/// The reconciled result of a resume over the recorded attempts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResumeReport {
    /// Attempts whose host still runs; they keep the runtime frozen.
    pub active: Vec<String>,
    /// Unknown attempts whose exact receipt still reports a live host: the
    /// outcome is retained as unknown, never guessed and never replayed.
    pub retained_live: Vec<String>,
    /// Attempts settled just now from their receipts.
    pub settled: Vec<String>,
    /// Attempts whose outcome remains unknown; they are never replayed.
    pub unknown: Vec<String>,
    /// Completed attempts refused for reuse, with the changed condition.
    pub remeasure: Vec<(String, String)>,
}

impl ResumeReport {
    pub fn condition(&self) -> Option<String> {
        let mut parts = Vec::new();
        if !self.active.is_empty() {
            parts.push(format!(
                "attempt(s) {} are still active; the frozen runtime is kept until they finish or are stopped",
                self.active.join(", ")
            ));
        }
        if !self.retained_live.is_empty() {
            parts.push(format!(
                "attempt(s) {} are retained live: their recorded host still runs, their outcome stays unknown and the frozen runtime is kept",
                self.retained_live.join(", ")
            ));
        }
        if !self.unknown.is_empty() {
            parts.push(format!(
                "attempt(s) {} have unknown outcomes; reconcile through the owning dispatcher before dependent work and never resubmit them",
                self.unknown.join(", ")
            ));
        }
        if !self.remeasure.is_empty() {
            let named: Vec<String> = self
                .remeasure
                .iter()
                .map(|(id, why)| format!("{id} ({why})"))
                .collect();
            parts.push(format!(
                "completed attempt(s) {} cannot be reused without remeasurement",
                named.join(", ")
            ));
        }
        (!parts.is_empty()).then(|| parts.join("; "))
    }
}

/// Settlement rules for a resume: a completed arm is reusable only while its
/// planning inputs still validate; anything unresolved stays unknown.
pub fn settle_completed_reuse(attempt: &mut Attempt, planning_valid: Result<(), String>) -> bool {
    if attempt.state != AttemptState::Completed {
        return false;
    }
    match planning_valid {
        Ok(()) => {
            attempt.reuse_refused = None;
            true
        }
        Err(reason) => {
            attempt.reuse_refused = Some(reason.clone());
            false
        }
    }
}

/// Selection gate: an active measured attempt keeps its frozen runtime, and a
/// candidate that is a removal treatment needs current experimental authority.
pub fn selection_gate(
    cursor: &Cursor,
    variant: &str,
    removal: Option<&RemovalGate>,
) -> Result<(), String> {
    if let Some(active) = cursor.active_attempt() {
        return Err(format!(
            "attempt {} ({}) is active; selection is refused while a measured attempt keeps its frozen runtime",
            active.id,
            active.role.as_str()
        ));
    }
    if !cursor.unresolved_attempts().is_empty() {
        return Err(
            "an attempt with an unknown outcome must be reconciled before another runtime is selected"
                .to_owned(),
        );
    }
    if variant == "candidate"
        && let Some(gate) = removal
    {
        match gate {
            RemovalGate::Authorized { .. } => {}
            RemovalGate::Refused { .. } => {
                return Err(
                    "the user declined this removal; the candidate cannot be applied and the request is not repeated without a new evidential basis"
                        .to_owned(),
                );
            }
            RemovalGate::Withdrawn { .. } => {
                return Err(
                    "the removal approval was withdrawn; selecting the candidate treatment waits for a new decision"
                        .to_owned(),
                );
            }
            RemovalGate::Pending { reason } => {
                return Err(format!("removal approval is pending: {reason}"));
            }
        }
    }
    Ok(())
}

#[cfg(all(test, windows))]
#[path = "improvement_loop_tests.rs"]
mod improvement_loop_tests;
