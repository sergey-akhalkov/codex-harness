//! Model-free intake of accepted board closures and durable two-stage
//! reflection state.
//!
//! The controller owns no model dispatch and creates no worktree. It reads the
//! native beads event journal, binds one accepted ordinary closure to one
//! durable record, and exposes the exact source sessions a lead-owned runner
//! must fork. Stage results, delivery acknowledgement and cost accounting are
//! persisted before the next stage becomes eligible.

use crate::board_cli;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::Output,
};

pub const PURPOSE_METADATA: &str = "harness.purpose";
pub const ACCEPTANCE_METADATA: &str = "harness.reflection.acceptance";
/// Exact `bd close --reason` value for an accepted ordinary closure.
pub const ACCEPTED_CLOSE_REASON: &str = "accepted";
pub const ACCEPTANCE_SCHEMA: u32 = 1;
pub const STATE_SCHEMA: u32 = 1;
pub const JOURNAL_CONFIG_KEY: &str = "events-journal";
pub const MAX_TEXT_BYTES: usize = 4_096;
pub const MAX_ACCEPTANCE_BYTES: usize = 16_384;
pub const MAX_SUMMARY_BYTES: usize = 4_096;
pub const MAX_COVERAGE_ITEMS: usize = 16;
pub const MAX_ANCESTORS: usize = 32;
pub const MAX_RECONCILE_RECORDS: usize = 64;
pub const MAX_POLL_REFUSALS: usize = 64;
const MAX_HEAD_PROBES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Purpose {
    Ordinary,
    Reflection,
    Improvement,
}

impl Purpose {
    pub fn parse(value: &str) -> io::Result<Self> {
        match value.trim() {
            "ordinary" => Ok(Self::Ordinary),
            "reflection" => Ok(Self::Reflection),
            "improvement" => Ok(Self::Improvement),
            other => Err(invalid(&format!(
                "purpose '{other}' is not ordinary, reflection or improvement"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary",
            Self::Reflection => "reflection",
            Self::Improvement => "improvement",
        }
    }

    pub fn triggers_reflection(self) -> bool {
        self == Self::Ordinary
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Executor,
    Lead,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Executor => "executor",
            Self::Lead => "lead",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Finding {
    Findings,
    NoFinding,
    Inconclusive,
}

/// Strict lead acceptance written on the accepted task. `outcome` is kept as a
/// string so a contradictory value (cancelled/rejected) yields an actionable
/// refusal instead of a generic deserialization error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceBinding {
    pub schema: u32,
    pub outcome: String,
    pub purpose: Purpose,
    pub task: String,
    pub run: String,
    pub assignment: String,
    pub result: String,
    pub revision: String,
    pub executor_sessions: Vec<String>,
    pub lead_session: String,
    pub approved_by: String,
    /// Exact native session of the accepted executor conversation. Absent
    /// means the accepted run's own conversation is not established, so
    /// automatic reflection refuses instead of choosing from the set.
    #[serde(default)]
    pub executor_session: Option<String>,
    /// Exact completed native turn of the accepted executor run, copied from
    /// the run's own observation record. Never inferred from recency.
    #[serde(default)]
    pub executor_turn: Option<String>,
    /// Exact native turn of the accepting lead conversation. When the lead
    /// accepts inside a still-running turn, this names that turn and the lead
    /// stage waits for its completion before forking.
    #[serde(default)]
    pub lead_turn: Option<String>,
}

#[derive(Serialize)]
struct AcceptanceIdentity<'a> {
    schema: u32,
    purpose: Purpose,
    task: &'a str,
    run: &'a str,
    assignment: &'a str,
    result: &'a str,
    revision: &'a str,
    executor_sessions: &'a [String],
}

impl AcceptanceBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task: impl Into<String>,
        run: impl Into<String>,
        assignment: impl Into<String>,
        result: impl Into<String>,
        revision: impl Into<String>,
        executor_sessions: Vec<String>,
        lead_session: impl Into<String>,
        approved_by: impl Into<String>,
        purpose: Purpose,
    ) -> Self {
        Self {
            schema: ACCEPTANCE_SCHEMA,
            outcome: "accepted".to_owned(),
            purpose,
            task: task.into(),
            run: run.into(),
            assignment: assignment.into(),
            result: result.into(),
            revision: revision.into(),
            executor_sessions,
            lead_session: lead_session.into(),
            approved_by: approved_by.into(),
            executor_session: None,
            executor_turn: None,
            lead_turn: None,
        }
    }

    /// Binds the exact native identities this acceptance was derived from.
    /// The turn fields deliberately do not participate in `acceptance_id`: a
    /// lead turn may complete after the closure, and the same accepted work
    /// must stay one idempotent record.
    pub fn with_turns(
        mut self,
        executor_session: Option<String>,
        executor_turn: Option<String>,
        lead_turn: Option<String>,
    ) -> Self {
        self.executor_session = executor_session;
        self.executor_turn = executor_turn;
        self.lead_turn = lead_turn;
        self
    }

    pub fn normalized(mut self) -> io::Result<Self> {
        if self.schema != ACCEPTANCE_SCHEMA {
            return Err(invalid(&format!(
                "acceptance schema {} is unsupported; expected {ACCEPTANCE_SCHEMA}",
                self.schema
            )));
        }
        for (name, value) in [
            ("outcome", &mut self.outcome),
            ("task", &mut self.task),
            ("run", &mut self.run),
            ("assignment", &mut self.assignment),
            ("result", &mut self.result),
            ("revision", &mut self.revision),
            ("lead_session", &mut self.lead_session),
            ("approved_by", &mut self.approved_by),
        ] {
            *value = value.trim().to_owned();
            require_text(name, value, MAX_TEXT_BYTES)?;
        }
        if self.outcome != "accepted" {
            return Err(invalid(&format!(
                "acceptance outcome '{}' is not accepted",
                self.outcome
            )));
        }
        if self.executor_sessions.is_empty() {
            return Err(invalid("acceptance executor_sessions is empty"));
        }
        let mut sessions = BTreeSet::new();
        for session in &self.executor_sessions {
            let session = session.trim();
            require_text("executor_sessions", session, MAX_TEXT_BYTES)?;
            sessions.insert(session.to_owned());
        }
        self.executor_sessions = sessions.into_iter().collect();
        fn normalize_identity(slot: &mut Option<String>, name: &str) -> io::Result<()> {
            if let Some(value) = slot.as_mut() {
                let trimmed = value.trim().to_owned();
                if trimmed.is_empty() {
                    *slot = None;
                } else {
                    require_text(name, &trimmed, MAX_TEXT_BYTES)?;
                    *value = trimmed;
                }
            }
            Ok(())
        }
        normalize_identity(&mut self.executor_session, "executor_session")?;
        normalize_identity(&mut self.executor_turn, "executor_turn")?;
        normalize_identity(&mut self.lead_turn, "lead_turn")?;
        if let Some(session) = &self.executor_session
            && !self.executor_sessions.iter().any(|item| item == session)
        {
            return Err(invalid(&format!(
                "acceptance executor_session '{session}' is not one of executor_sessions"
            )));
        }
        let size = serde_json::to_vec(&self)?.len();
        if size > MAX_ACCEPTANCE_BYTES {
            return Err(invalid(&format!(
                "acceptance metadata is {size} bytes; the limit is {MAX_ACCEPTANCE_BYTES}"
            )));
        }
        Ok(self)
    }

    pub fn acceptance_id(&self) -> io::Result<String> {
        let normalized = self.clone().normalized()?;
        let identity = AcceptanceIdentity {
            schema: normalized.schema,
            purpose: normalized.purpose,
            task: &normalized.task,
            run: &normalized.run,
            assignment: &normalized.assignment,
            result: &normalized.result,
            revision: &normalized.revision,
            executor_sessions: &normalized.executor_sessions,
        };
        Ok(format!(
            "acceptance-{}",
            hex_digest(&serde_json::to_vec(&identity)?)
        ))
    }
}

pub fn purpose_metadata(purpose: Purpose) -> BTreeMap<String, String> {
    BTreeMap::from([(PURPOSE_METADATA.to_owned(), purpose.as_str().to_owned())])
}

pub fn acceptance_metadata(binding: &AcceptanceBinding) -> io::Result<BTreeMap<String, String>> {
    let binding = binding.clone().normalized()?;
    let mut metadata = purpose_metadata(binding.purpose);
    metadata.insert(
        ACCEPTANCE_METADATA.to_owned(),
        serde_json::to_string(&binding)?,
    );
    Ok(metadata)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub project_id: String,
    pub beads_dir: String,
    pub database: String,
    pub repo_root: String,
    pub branch: String,
    pub commit: String,
}

impl SourceIdentity {
    pub fn stable_key(&self) -> String {
        hex_digest(
            format!(
                "{}\0{}\0{}",
                normalize(&self.project_id),
                normalize(&self.beads_dir),
                normalize(&self.database)
            )
            .as_bytes(),
        )
    }

    pub fn same_scope(&self, other: &Self) -> bool {
        normalize(&self.project_id) == normalize(&other.project_id)
            && normalize(&self.beads_dir) == normalize(&other.beads_dir)
            && normalize(&self.database) == normalize(&other.database)
            && normalize(&self.repo_root) == normalize(&other.repo_root)
    }
}

/// Reads the canonical board scope and current board revision through the
/// native `bd` commands. Ordinary dispatch resolves a declared consumer-board
/// task through this same reader before it claims a slot or starts a model.
pub fn board_source_identity(bd: &Path, project: &Path) -> io::Result<SourceIdentity> {
    let context = board_json(bd, project, &["context", "--json"])?;
    let vc = board_json(bd, project, &["vc", "status", "--json"])?;
    Ok(SourceIdentity {
        project_id: required_string(&context, "project_id")?,
        beads_dir: required_string(&context, "beads_dir")?,
        database: required_string(&context, "database")?,
        repo_root: context
            .get("repo_root")
            .or_else(|| context.get("cwd_repo_root"))
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("bd context has no repo_root"))?
            .to_owned(),
        branch: required_string(&vc, "branch")?,
        commit: required_string(&vc, "commit")?,
    })
}

fn board_json(bd: &Path, project: &Path, args: &[&str]) -> io::Result<Value> {
    let out = board_cli::run(bd, project, args)?;
    if !out.status.success() {
        return Err(board_failure("bd", args, &out));
    }
    serde_json::from_slice(&out.stdout).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bd {} did not return JSON: {error}", args.join(" ")),
        )
    })
}

fn default_max_active_stages() -> u32 {
    2
}
fn default_max_attempts_per_stage() -> u32 {
    3
}
fn default_max_turns_per_stage() -> u64 {
    24
}
fn default_max_tokens_per_stage() -> u64 {
    120_000
}
fn default_max_tokens_per_record() -> u64 {
    200_000
}
fn default_max_investigations_per_stage() -> u64 {
    64
}
fn default_max_investigations_per_record() -> u64 {
    128
}
fn default_max_context_bytes_per_stage() -> u64 {
    1024 * 1024
}
fn default_max_context_bytes_per_record() -> u64 {
    2 * 1024 * 1024
}
fn default_reconciled() -> bool {
    true
}
fn default_max_events_per_poll() -> u32 {
    500
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReflectionLimits {
    pub max_active_stages: u32,
    pub max_attempts_per_stage: u32,
    pub max_turns_per_stage: u64,
    pub max_tokens_per_stage: u64,
    pub max_tokens_per_record: u64,
    pub max_investigations_per_stage: u64,
    pub max_investigations_per_record: u64,
    pub max_context_bytes_per_stage: u64,
    pub max_context_bytes_per_record: u64,
    pub max_events_per_poll: u32,
}

impl Default for ReflectionLimits {
    fn default() -> Self {
        Self {
            max_active_stages: default_max_active_stages(),
            max_attempts_per_stage: default_max_attempts_per_stage(),
            max_turns_per_stage: default_max_turns_per_stage(),
            max_tokens_per_stage: default_max_tokens_per_stage(),
            max_tokens_per_record: default_max_tokens_per_record(),
            max_investigations_per_stage: default_max_investigations_per_stage(),
            max_investigations_per_record: default_max_investigations_per_record(),
            max_context_bytes_per_stage: default_max_context_bytes_per_stage(),
            max_context_bytes_per_record: default_max_context_bytes_per_record(),
            max_events_per_poll: default_max_events_per_poll(),
        }
    }
}

impl ReflectionLimits {
    pub fn validate(&self) -> io::Result<()> {
        if self.max_active_stages == 0 || self.max_active_stages > 8 {
            return Err(invalid(
                "reflection max_active_stages must be between 1 and 8",
            ));
        }
        if self.max_attempts_per_stage == 0 || self.max_attempts_per_stage > 16 {
            return Err(invalid(
                "reflection max_attempts_per_stage must be between 1 and 16",
            ));
        }
        if self.max_turns_per_stage == 0 || self.max_turns_per_stage > 1_000 {
            return Err(invalid(
                "reflection max_turns_per_stage must be between 1 and 1000",
            ));
        }
        if self.max_tokens_per_stage == 0 || self.max_tokens_per_stage > 10_000_000 {
            return Err(invalid(
                "reflection max_tokens_per_stage must be between 1 and 10000000",
            ));
        }
        if self.max_tokens_per_record < self.max_tokens_per_stage
            || self.max_tokens_per_record > 20_000_000
        {
            return Err(invalid(
                "reflection max_tokens_per_record must cover one stage and be at most 20000000",
            ));
        }
        if self.max_investigations_per_stage == 0 || self.max_investigations_per_stage > 10_000 {
            return Err(invalid(
                "reflection max_investigations_per_stage must be between 1 and 10000",
            ));
        }
        if self.max_investigations_per_record < self.max_investigations_per_stage
            || self.max_investigations_per_record > 20_000
        {
            return Err(invalid(
                "reflection max_investigations_per_record must cover one stage and be at most 20000",
            ));
        }
        if self.max_context_bytes_per_stage == 0
            || self.max_context_bytes_per_stage > 64 * 1024 * 1024
        {
            return Err(invalid(
                "reflection max_context_bytes_per_stage must be between 1 and 67108864",
            ));
        }
        if self.max_context_bytes_per_record < self.max_context_bytes_per_stage
            || self.max_context_bytes_per_record > 128 * 1024 * 1024
        {
            return Err(invalid(
                "reflection max_context_bytes_per_record must cover one stage and be at most 134217728",
            ));
        }
        if self.max_events_per_poll == 0 || self.max_events_per_poll > 5_000 {
            return Err(invalid(
                "reflection max_events_per_poll must be between 1 and 5000",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleStatus {
    Active,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleBinding {
    /// Stable for the project board lifecycle. A supported lead/run rebind
    /// changes only `revision`.
    pub id: String,
    pub revision: u64,
    pub lead_session: String,
    pub run: Option<String>,
    pub activated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalOwnership {
    PreexistingEnabled,
    EnabledByController,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalPolicy {
    RequireExisting,
    EnableIfMissing,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StageUsage {
    pub attempts: u32,
    pub turns: u64,
    pub tokens: u64,
    pub investigations: u64,
    pub context_bytes: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UsageDelta {
    pub turns: u64,
    pub tokens: u64,
}

/// Work observed by native stage signals that is not a token counter. It is
/// recorded beside token usage and is never replaced by an estimate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StageWork {
    pub investigations: u64,
    pub context_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UsageTotals {
    pub attempts: u32,
    pub turns: u64,
    pub tokens: u64,
    pub investigations: u64,
    pub context_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetLimit {
    StageAttempts,
    StageTurns,
    StageTokens,
    RecordTokens,
    StageInvestigations,
    RecordInvestigations,
    StageContextBytes,
    RecordContextBytes,
}

impl BudgetLimit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StageAttempts => "stage_attempts",
            Self::StageTurns => "stage_turns",
            Self::StageTokens => "stage_tokens",
            Self::RecordTokens => "record_tokens",
            Self::StageInvestigations => "stage_investigations",
            Self::RecordInvestigations => "record_investigations",
            Self::StageContextBytes => "stage_context_bytes",
            Self::RecordContextBytes => "record_context_bytes",
        }
    }

    fn configured(self, limits: &ReflectionLimits) -> u64 {
        match self {
            Self::StageAttempts => u64::from(limits.max_attempts_per_stage),
            Self::StageTurns => limits.max_turns_per_stage,
            Self::StageTokens => limits.max_tokens_per_stage,
            Self::RecordTokens => limits.max_tokens_per_record,
            Self::StageInvestigations => limits.max_investigations_per_stage,
            Self::RecordInvestigations => limits.max_investigations_per_record,
            Self::StageContextBytes => limits.max_context_bytes_per_stage,
            Self::RecordContextBytes => limits.max_context_bytes_per_record,
        }
    }
}

/// A work stoppage caused by a configured allowance. The record remains
/// inspectable and the prior usage stays charged; retrying requires the named
/// allowance to be raised above the observed value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BudgetPause {
    pub limit: BudgetLimit,
    pub observed: u64,
    pub allowed: u64,
    pub missing_fact: String,
    pub continuation_choice: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

impl BudgetPause {
    fn validate(&self) -> io::Result<()> {
        if self.allowed == 0 || self.observed <= self.allowed {
            return Err(invalid(
                "a reflection budget pause must record an observed value above a nonzero allowance",
            ));
        }
        bounded_text(
            "budget pause missing fact",
            &self.missing_fact,
            MAX_TEXT_BYTES,
        )?;
        bounded_text(
            "budget pause continuation",
            &self.continuation_choice,
            MAX_TEXT_BYTES,
        )?;
        if self.evidence.len() > MAX_COVERAGE_ITEMS {
            return Err(invalid(&format!(
                "budget pause has {} evidence items; the limit is {MAX_COVERAGE_ITEMS}",
                self.evidence.len()
            )));
        }
        for item in &self.evidence {
            bounded_text("budget pause evidence", item, MAX_TEXT_BYTES)?;
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        format!(
            "budget pause: {} observed {} exceeds allowed {}; missing fact: {}; continuation: {}",
            self.limit.as_str(),
            self.observed,
            self.allowed,
            self.missing_fact,
            self.continuation_choice
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageOutcome {
    pub finding: Finding,
    pub summary: String,
    pub coverage: Vec<String>,
    pub retained_at: String,
    pub session: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageState {
    #[default]
    Pending,
    InFlight,
    Retained,
    Blocked,
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StageRecord {
    pub state: StageState,
    pub usage: StageUsage,
    pub outcome: Option<StageOutcome>,
    pub usage_reconciled: bool,
    /// Investigation and supplied-context work is reconciled separately from
    /// token usage. Older records without this field predate split work
    /// reconciliation and remain recoverable as reconciled.
    #[serde(default = "default_reconciled")]
    pub work_reconciled: bool,
    #[serde(default)]
    pub pause: Option<BudgetPause>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    #[default]
    Pending,
    Consumed,
    Unknown,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReflectionRecord {
    pub acceptance_id: String,
    pub binding: AcceptanceBinding,
    pub event_seq: u64,
    pub event_ts: String,
    pub event_actor: String,
    pub task_revision: String,
    pub executor: StageRecord,
    pub lead: StageRecord,
    pub delivery: DeliveryState,
    pub delivery_receipt: Option<String>,
    pub retained_at: String,
}

impl ReflectionRecord {
    pub fn usage_totals(&self) -> UsageTotals {
        UsageTotals {
            attempts: self
                .executor
                .usage
                .attempts
                .saturating_add(self.lead.usage.attempts),
            turns: self
                .executor
                .usage
                .turns
                .saturating_add(self.lead.usage.turns),
            tokens: self
                .executor
                .usage
                .tokens
                .saturating_add(self.lead.usage.tokens),
            investigations: self
                .executor
                .usage
                .investigations
                .saturating_add(self.lead.usage.investigations),
            context_bytes: self
                .executor
                .usage
                .context_bytes
                .saturating_add(self.lead.usage.context_bytes),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryKind {
    SourceChanged,
    BranchChanged,
    ReplicaChanged,
    EventGap,
    InvalidEvent,
    TaskRead,
    TaskDrift,
    InterruptedStage,
    UnknownOutcome,
    JournalUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskBindingStatus {
    Bound,
    Missing,
    Drifted,
    ReadError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReconciliation {
    pub acceptance_id: String,
    pub task_id: String,
    pub status: TaskBindingStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryState {
    pub kind: RecoveryKind,
    pub detail: String,
    pub observed_source: Option<SourceIdentity>,
    pub floor: Option<u64>,
    pub head: Option<u64>,
    pub records: Vec<TaskReconciliation>,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryResolution {
    Rebaselined { checkpoint: u64 },
    Resumed { checkpoint: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollStamp {
    pub polled_at: String,
    pub checkpoint: u64,
    pub events_seen: u64,
    pub accepted: u64,
    pub ignored: u64,
    pub duplicates: u64,
    pub model_calls: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    MissingPurpose,
    UnknownPurpose,
    NonTriggerPurpose,
    MissingAcceptance,
    InvalidAcceptance,
    OutcomeNotAccepted,
    CloseReasonNotAccepted,
    TaskMismatch,
    ActorMismatch,
    WrongLead,
    TaskDrift,
    LineageUnavailable,
    AncestorLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventRefusal {
    pub seq: u64,
    pub task: String,
    pub code: RefusalCode,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReflectionState {
    pub schema: u32,
    pub lifecycle: LifecycleStatus,
    pub binding: LifecycleBinding,
    pub source: SourceIdentity,
    pub journal: JournalOwnership,
    pub checkpoint: u64,
    pub limits: ReflectionLimits,
    pub records: BTreeMap<String, ReflectionRecord>,
    pub recovery: Option<RecoveryState>,
    pub last_poll: Option<PollStamp>,
    pub refusals: Vec<EventRefusal>,
    pub stopped_at: Option<String>,
    pub last_recovery_evidence: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationRequest {
    pub lead_session: String,
    pub run: Option<String>,
    /// Explicit current head. Omission performs bounded journal-head probes.
    pub boundary: Option<u64>,
    pub journal: JournalPolicy,
    pub limits: Option<ReflectionLimits>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActivationReceipt {
    pub binding_id: String,
    pub revision: u64,
    pub checkpoint: u64,
    pub source: SourceIdentity,
    pub reused: bool,
    pub journal: JournalOwnership,
    pub bound_records: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReflectionJob {
    pub acceptance_id: String,
    pub stage: Stage,
    pub task_id: String,
    pub run: String,
    pub source_sessions: Vec<String>,
    pub lead_session: String,
    pub executor_session: Option<String>,
    pub executor_turn: Option<String>,
    pub lead_turn: Option<String>,
    pub context: Vec<String>,
    pub usage: StageUsage,
    pub remaining_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PollReceipt {
    pub checkpoint: u64,
    pub model_calls: u32,
    pub events_seen: u64,
    pub accepted: u64,
    pub ignored: u64,
    pub duplicates: u64,
    pub jobs: Vec<ReflectionJob>,
    pub recovery: Option<RecoveryState>,
    pub refusals: Vec<EventRefusal>,
}

enum PollOutcome {
    Inactive,
    Stopped,
    Receipt(Box<PollReceipt>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StopReceipt {
    pub binding_id: String,
    pub revision: u64,
    pub checkpoint: u64,
    pub stopped_at: String,
    pub retained_records: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageStart {
    pub acceptance_id: String,
    pub stage: Stage,
    pub attempt: u32,
    pub source_sessions: Vec<String>,
    pub lead_session: String,
    pub executor_session: Option<String>,
    pub executor_turn: Option<String>,
    pub lead_turn: Option<String>,
    pub usage: StageUsage,
    pub admission: StageAdmission,
    pub remaining_tokens: u64,
}

/// The cumulative allowance state visible before a new native stage attempt.
/// Native enforcement uses both the stage and accepted-record counters so a
/// tool or context overrun cannot wait for a later admission check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageAdmission {
    pub stage_turns_used: u64,
    pub stage_turns_limit: u64,
    pub stage_tokens_used: u64,
    pub stage_tokens_limit: u64,
    pub stage_investigations_used: u64,
    pub stage_investigations_limit: u64,
    pub stage_context_bytes_used: u64,
    pub stage_context_bytes_limit: u64,
    pub record_investigations_used: u64,
    pub record_investigations_limit: u64,
    pub record_context_bytes_used: u64,
    pub record_context_bytes_limit: u64,
    pub record_tokens_used: u64,
    pub record_tokens_limit: u64,
}

impl StageAdmission {
    pub fn new(limits: &ReflectionLimits, stage: &StageUsage, record: &UsageTotals) -> Self {
        Self {
            stage_turns_used: stage.turns,
            stage_turns_limit: limits.max_turns_per_stage,
            stage_tokens_used: stage.tokens,
            stage_tokens_limit: limits.max_tokens_per_stage,
            stage_investigations_used: stage.investigations,
            stage_investigations_limit: limits.max_investigations_per_stage,
            stage_context_bytes_used: stage.context_bytes,
            stage_context_bytes_limit: limits.max_context_bytes_per_stage,
            record_investigations_used: record.investigations,
            record_investigations_limit: limits.max_investigations_per_record,
            record_context_bytes_used: record.context_bytes,
            record_context_bytes_limit: limits.max_context_bytes_per_record,
            record_tokens_used: record.tokens,
            record_tokens_limit: limits.max_tokens_per_record,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageReceipt {
    pub acceptance_id: String,
    pub stage: Stage,
    pub state: StageState,
    pub usage: StageUsage,
    pub delivery: DeliveryState,
    pub next_stage_eligible: bool,
    pub record_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeliveryReceipt {
    pub acceptance_id: String,
    pub delivery: DeliveryState,
    pub receipt: String,
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveryReceipt {
    pub binding_id: String,
    pub revision: u64,
    pub checkpoint: u64,
    pub journal_restored: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingStatus {
    Active,
    Inactive,
    Stopped,
    WrongProject,
    WrongLead,
    UnrelatedWork,
    StaleSource,
    RecoveryPending,
    InvalidState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BindingCheck {
    pub status: BindingStatus,
    pub binding_id: Option<String>,
    pub revision: u64,
    pub source: Option<SourceIdentity>,
    pub lead_session: Option<String>,
    pub run: Option<String>,
    pub checkpoint: Option<u64>,
    pub matched_acceptance_id: Option<String>,
    pub work: Option<String>,
    pub detail: String,
}

impl BindingCheck {
    pub fn is_active(&self) -> bool {
        self.status == BindingStatus::Active
    }
}

pub struct ReflectionController {
    bd: PathBuf,
    project: PathBuf,
    codex_home: PathBuf,
}

impl ReflectionController {
    pub fn new(
        bd: impl Into<PathBuf>,
        project: impl Into<PathBuf>,
        codex_home: impl Into<PathBuf>,
    ) -> Self {
        Self {
            bd: bd.into(),
            project: project.into(),
            codex_home: codex_home.into(),
        }
    }

    /// The retained lifecycle file for the current native board source.
    pub fn state_file(&self) -> io::Result<PathBuf> {
        let source = self.current_source()?;
        Ok(self
            .codex_home
            .join("harness/delivery-reflection")
            .join(format!("{}.json", source.stable_key())))
    }

    /// Reads retained state without creating directories, locks or files.
    pub fn stored_state(&self) -> io::Result<Option<ReflectionState>> {
        let path = self.state_file()?;
        match fs::read(&path) {
            Ok(bytes) => {
                let state: ReflectionState = serde_json::from_slice(&bytes).map_err(|error| {
                    invalid(&format!(
                        "reflection state {} is invalid JSON: {error}",
                        path.display()
                    ))
                })?;
                validate_state(&state)?;
                Ok(Some(state))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn activate(&self, request: &ActivationRequest) -> io::Result<ActivationReceipt> {
        require_text("lead_session", &request.lead_session, MAX_TEXT_BYTES)?;
        if let Some(run) = &request.run {
            require_text("run", run, MAX_TEXT_BYTES)?;
        }
        if let Some(limits) = &request.limits {
            limits.validate()?;
        }
        let source = self.current_source()?;
        let path = self.state_path(&source);
        let _lock = self.lock(&path)?;
        let existing = self.read_state_at(&path)?;
        if let Some(state) = &existing {
            if !state.source.same_scope(&source) {
                return Err(conflict(&format!(
                    "reflection state is bound to project {} / {}, not the current board scope; preserve it and use the matching project",
                    state.source.project_id, state.source.beads_dir
                )));
            }
            if normalize(&state.source.branch) != normalize(&source.branch) {
                return Err(recovery_error(
                    "source branch changed; call rebaseline_current after scoped reconciliation, or resume_from_floor when the recorded gap has a floor",
                ));
            }
            if state.lifecycle == LifecycleStatus::Active
                && !state
                    .binding
                    .lead_session
                    .trim()
                    .eq(request.lead_session.trim())
                && state.records.values().any(|record| {
                    matches!(
                        record.executor.state,
                        StageState::InFlight | StageState::Unknown
                    ) || matches!(
                        record.lead.state,
                        StageState::InFlight | StageState::Unknown
                    )
                })
            {
                return Err(conflict(
                    "a reflection stage is in flight or unknown; reconcile it before rebinding the lead",
                ));
            }
        }
        let journal = self.ensure_journal(existing.as_ref(), request.journal)?;
        let head = match request.boundary {
            Some(boundary) => boundary,
            None => self.probe_head(existing.as_ref().map_or(0, |state| state.checkpoint))?,
        };
        if let Some(boundary) = request.boundary {
            let current = self.probe_head(existing.as_ref().map_or(0, |state| state.checkpoint))?;
            if boundary != current {
                return Err(conflict(&format!(
                    "activation boundary {boundary} is not the current journal head {current}; omit the boundary to bind the current head"
                )));
            }
        }
        let reused = existing.is_some();
        let now = now();
        let binding_id = source.stable_key();
        let state = match existing {
            Some(mut state) => {
                if state.binding.lead_session != request.lead_session.trim()
                    || state.binding.run.as_deref() != request.run.as_deref()
                    || state.lifecycle == LifecycleStatus::Stopped
                {
                    state.binding.revision = state.binding.revision.saturating_add(1);
                    state.binding.lead_session = request.lead_session.trim().to_owned();
                    state.binding.run = request.run.as_ref().map(|run| run.trim().to_owned());
                    state.binding.activated_at = now.clone();
                }
                if let Some(limits) = &request.limits {
                    state.limits = limits.clone();
                }
                state.lifecycle = LifecycleStatus::Active;
                state.stopped_at = None;
                state.source = source.clone();
                state.journal = journal;
                state
            }
            None => ReflectionState {
                schema: STATE_SCHEMA,
                lifecycle: LifecycleStatus::Active,
                binding: LifecycleBinding {
                    id: binding_id.clone(),
                    revision: 1,
                    lead_session: request.lead_session.trim().to_owned(),
                    run: request.run.as_ref().map(|run| run.trim().to_owned()),
                    activated_at: now,
                },
                source: source.clone(),
                journal,
                checkpoint: head,
                limits: request.limits.clone().unwrap_or_default(),
                records: BTreeMap::new(),
                recovery: None,
                last_poll: None,
                refusals: Vec::new(),
                stopped_at: None,
                last_recovery_evidence: None,
            },
        };
        let receipt = ActivationReceipt {
            binding_id: state.binding.id.clone(),
            revision: state.binding.revision,
            checkpoint: state.checkpoint,
            source: state.source.clone(),
            reused,
            journal: state.journal,
            bound_records: state.records.len(),
        };
        self.write_state_at(&path, &state)?;
        Ok(receipt)
    }

    fn state_path(&self, source: &SourceIdentity) -> PathBuf {
        self.codex_home
            .join("harness/delivery-reflection")
            .join(format!("{}.json", source.stable_key()))
    }

    fn read_state_at(&self, path: &Path) -> io::Result<Option<ReflectionState>> {
        match fs::read(path) {
            Ok(bytes) => {
                let state: ReflectionState = serde_json::from_slice(&bytes).map_err(|error| {
                    invalid(&format!(
                        "reflection state {} is invalid JSON: {error}",
                        path.display()
                    ))
                })?;
                validate_state(&state)?;
                Ok(Some(state))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn write_state_at(&self, path: &Path, state: &ReflectionState) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let bytes = serde_json::to_vec_pretty(state)?;
        let tmp = path.with_extension("json.tmp");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    fn lock(&self, path: &Path) -> io::Result<crate::process::ExclusiveFileLock> {
        let lock_path = path.with_extension("json.lock");
        if let Some(dir) = lock_path.parent() {
            fs::create_dir_all(dir)?;
        }
        crate::process::ExclusiveFileLock::try_acquire(&lock_path)?.ok_or_else(|| {
            conflict(&format!(
                "reflection state {} is locked by another controller; retry after that operation finishes",
                path.display()
            ))
        })
    }

    fn current_source(&self) -> io::Result<SourceIdentity> {
        board_source_identity(&self.bd, &self.project)
    }

    fn json(&self, args: &[&str]) -> io::Result<Value> {
        let out = board_cli::run(&self.bd, &self.project, args)?;
        if !out.status.success() {
            return Err(board_failure("bd", args, &out));
        }
        serde_json::from_slice(&out.stdout).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bd {} did not return JSON: {error}", args.join(" ")),
            )
        })
    }

    fn ensure_journal(
        &self,
        existing: Option<&ReflectionState>,
        policy: JournalPolicy,
    ) -> io::Result<JournalOwnership> {
        match self.journal_value()? {
            Some(true) => Ok(existing
                .map(|state| state.journal)
                .unwrap_or(JournalOwnership::PreexistingEnabled)),
            Some(false) => Err(conflict(
                "events-journal is explicitly false; reflection activation preserves that setting and cannot consume accepted closures until an owner enables it",
            )),
            None => match policy {
                JournalPolicy::RequireExisting => Err(conflict(
                    "events-journal is not enabled; enable it with 'bd config set events-journal true' or activate with JournalPolicy::EnableIfMissing",
                )),
                JournalPolicy::EnableIfMissing => {
                    let out = board_cli::run(
                        &self.bd,
                        &self.project,
                        &["config", "set", JOURNAL_CONFIG_KEY, "true"],
                    )?;
                    if !out.status.success() {
                        return Err(board_failure("bd config set", &["events-journal"], &out));
                    }
                    if self.journal_value()? != Some(true) {
                        return Err(conflict(
                            "bd accepted events-journal setup but the setting is not true; reflection remains inactive",
                        ));
                    }
                    Ok(JournalOwnership::EnabledByController)
                }
            },
        }
    }

    fn journal_value(&self) -> io::Result<Option<bool>> {
        let values = self.json(&["config", "show", "--json"])?;
        let Some(entry) = values.as_array().and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.get("key").and_then(Value::as_str) == Some(JOURNAL_CONFIG_KEY))
        }) else {
            return Ok(None);
        };
        let text = entry
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let source = entry
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        match text {
            "" => Ok(None),
            "true" => Ok(Some(true)),
            "false" if source == "default" => Ok(None),
            "false" => Ok(Some(false)),
            other => Err(invalid(&format!(
                "events-journal has unsupported value '{other}'; expected true or false"
            ))),
        }
    }

    /// Finds the highest committed journal sequence with bounded one-record
    /// probes. There is no native head query; no full history is read.
    fn probe_head(&self, checkpoint: u64) -> io::Result<u64> {
        let mut low = checkpoint;
        let initial = self.tail(low, 1)?;
        if let Some((floor, head)) = initial.truncated {
            return Err(recovery_error(&format!(
                "journal checkpoint {checkpoint} precedes the retained floor {floor} at head {head}; reconcile the retained records and pass a current boundary"
            )));
        }
        if initial.events.is_empty() {
            return Ok(low);
        }
        let mut high = low.saturating_add(1);
        for _ in 0..MAX_HEAD_PROBES {
            let page = self.tail(high, 1)?;
            if let Some((floor, head)) = page.truncated {
                return Err(recovery_error(&format!(
                    "journal head probing at {high} crossed the retained floor {floor} at head {head}; pass the current boundary explicitly"
                )));
            }
            if page.events.is_empty() {
                while low.saturating_add(1) < high {
                    let mid = low + (high - low) / 2;
                    let page = self.tail(mid, 1)?;
                    if let Some((floor, head)) = page.truncated {
                        return Err(recovery_error(&format!(
                            "journal head probing at {mid} crossed the retained floor {floor} at head {head}; pass the current boundary explicitly"
                        )));
                    }
                    if page.events.is_empty() {
                        high = mid;
                    } else {
                        low = mid;
                    }
                }
                return Ok(low);
            }
            high = high.checked_mul(2).unwrap_or(i64::MAX as u64);
            if high >= i64::MAX as u64 {
                return Err(conflict(
                    "journal head could not be bounded within the supported probe limit",
                ));
            }
        }
        Err(conflict(
            "journal head probing exceeded its finite bound; pass the current head explicitly",
        ))
    }

    fn tail(&self, since: u64, limit: u32) -> io::Result<TailPage> {
        if since > i64::MAX as u64 {
            return Err(invalid(
                "journal checkpoint exceeds the supported i64 sequence range",
            ));
        }
        let since = since.to_string();
        let limit = limit.to_string();
        let out = board_cli::run(
            &self.bd,
            &self.project,
            &[
                "events", "tail", "--since", &since, "--limit", &limit, "--json",
            ],
        )?;
        if out.status.success() {
            let mut events = Vec::new();
            for line in out.stdout.split(|byte| *byte == b'\n') {
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let event = serde_json::from_slice(line).map_err(|error| {
                    invalid(&format!("bd events tail returned malformed JSON: {error}"))
                })?;
                events.push(event);
            }
            return Ok(TailPage {
                events,
                truncated: None,
            });
        }
        if let Some((floor, head)) = truncated_bounds(&out) {
            return Ok(TailPage {
                events: Vec::new(),
                truncated: Some((floor, head)),
            });
        }
        Err(board_failure(
            "bd events tail",
            &["--since", since.as_str()],
            &out,
        ))
    }

    /// Reads new committed closures without launching a model. Any recovery
    /// condition stops consumption at the retained checkpoint.
    pub fn poll(&self) -> io::Result<PollReceipt> {
        match self.poll_outcome()? {
            PollOutcome::Receipt(receipt) => Ok(*receipt),
            PollOutcome::Inactive => Err(conflict(
                "delivery reflection is inactive; activate it before polling",
            )),
            PollOutcome::Stopped => Err(conflict(
                "delivery reflection is stopped; activate it before polling",
            )),
        }
    }

    /// Polls while the lifecycle is retained and active. A normal stop observed
    /// before any stage dispatch is a typed no-op, not a failed consumer.
    pub fn poll_if_active(&self) -> io::Result<Option<PollReceipt>> {
        match self.poll_outcome()? {
            PollOutcome::Receipt(receipt) => Ok(Some(*receipt)),
            PollOutcome::Inactive | PollOutcome::Stopped => Ok(None),
        }
    }

    fn poll_outcome(&self) -> io::Result<PollOutcome> {
        let current = self.current_source()?;
        let path = self.state_path(&current);
        let _lock = self.lock(&path)?;
        let Some(mut state) = self.read_state_at(&path)? else {
            return Ok(PollOutcome::Inactive);
        };
        if state.lifecycle != LifecycleStatus::Active {
            return Ok(PollOutcome::Stopped);
        }
        if state.recovery.is_none() && !state.source.same_scope(&current) {
            let kind = if normalize(&state.source.project_id) == normalize(&current.project_id)
                && normalize(&state.source.database) == normalize(&current.database)
            {
                RecoveryKind::SourceChanged
            } else {
                RecoveryKind::ReplicaChanged
            };
            let detail = format!(
                "current board source {} / {} / {} differs from retained {} / {} / {}",
                current.project_id,
                current.beads_dir,
                current.repo_root,
                state.source.project_id,
                state.source.beads_dir,
                state.source.repo_root
            );
            self.pause_for_recovery(&mut state, kind, detail, None, Some(current.clone()), None)?;
        } else if state.recovery.is_none()
            && normalize(&state.source.branch) != normalize(&current.branch)
        {
            let detail = format!(
                "journal branch changed from '{}' to '{}'; journal sequence spaces are branch-local",
                state.source.branch, current.branch
            );
            self.pause_for_recovery(
                &mut state,
                RecoveryKind::BranchChanged,
                detail,
                None,
                Some(current.clone()),
                None,
            )?;
        }
        if let Some(recovery) = &state.recovery {
            self.write_state_at(&path, &state)?;
            return Ok(PollOutcome::Receipt(Box::new(poll_receipt(
                &state,
                0,
                0,
                0,
                0,
                Vec::new(),
                Some(recovery.clone()),
            ))));
        }
        let page = match self.tail(state.checkpoint, state.limits.max_events_per_poll) {
            Ok(page) => page,
            Err(error) => {
                let detail = format!(
                    "bd events tail is unavailable at checkpoint {}; preserve retained records and re-enable or repair the board journal before rebaselining: {error}",
                    state.checkpoint
                );
                self.pause_for_recovery(
                    &mut state,
                    RecoveryKind::JournalUnavailable,
                    detail,
                    None,
                    Some(current.clone()),
                    None,
                )?;
                self.write_state_at(&path, &state)?;
                return Ok(PollOutcome::Receipt(Box::new(poll_receipt(
                    &state,
                    0,
                    0,
                    0,
                    0,
                    Vec::new(),
                    Some(state.recovery.clone().expect("recovery was set")),
                ))));
            }
        };
        if let Some((floor, head)) = page.truncated {
            let detail = format!(
                "journal records after checkpoint {} were pruned; retained floor {floor}, head {head}",
                state.checkpoint
            );
            self.pause_for_recovery(
                &mut state,
                RecoveryKind::EventGap,
                detail,
                Some(floor),
                None,
                Some(head),
            )?;
            self.write_state_at(&path, &state)?;
            return Ok(PollOutcome::Receipt(Box::new(poll_receipt(
                &state,
                0,
                0,
                0,
                0,
                Vec::new(),
                Some(state.recovery.clone().expect("recovery was set")),
            ))));
        }
        let events_seen = page.events.len() as u64;
        let mut accepted = 0u64;
        let mut ignored = 0u64;
        let mut duplicates = 0u64;
        let mut refusals = Vec::new();
        let mut expected = state.checkpoint.saturating_add(1);
        for event in page.events {
            if event.seq != expected {
                let detail = format!(
                    "journal sequence gap: expected {expected}, received {}; checkpoint remains {}",
                    event.seq, state.checkpoint
                );
                self.pause_for_recovery(
                    &mut state,
                    RecoveryKind::EventGap,
                    detail,
                    None,
                    Some(current.clone()),
                    Some(event.seq),
                )?;
                break;
            }
            match self.evaluate_event(&state, &event) {
                Ok(EvaluatedEvent::Ignored) => {
                    ignored += 1;
                }
                Ok(EvaluatedEvent::Refused(refusal)) => {
                    ignored += 1;
                    retain_refusal(&mut state.refusals, refusal.clone());
                    refusals.push(refusal);
                }
                Ok(EvaluatedEvent::Eligible {
                    acceptance_id,
                    binding,
                    task_revision,
                }) => {
                    if state.records.contains_key(&acceptance_id) {
                        duplicates += 1;
                    } else {
                        let now = now();
                        state.records.insert(
                            acceptance_id.clone(),
                            ReflectionRecord {
                                acceptance_id,
                                binding: *binding,
                                event_seq: event.seq,
                                event_ts: event.ts.clone(),
                                event_actor: event.actor.clone(),
                                task_revision,
                                executor: StageRecord::default(),
                                lead: StageRecord::default(),
                                delivery: DeliveryState::Pending,
                                delivery_receipt: None,
                                retained_at: now,
                            },
                        );
                        accepted += 1;
                    }
                }
                Err(error) => {
                    let detail = format!(
                        "accepted closure {} could not be reconciled: {error}; checkpoint remains {}",
                        event.issue_id, state.checkpoint
                    );
                    self.pause_for_recovery(
                        &mut state,
                        RecoveryKind::TaskRead,
                        detail,
                        None,
                        None,
                        Some(event.seq),
                    )?;
                    break;
                }
            }
            state.checkpoint = event.seq;
            expected = event.seq.saturating_add(1);
        }
        if state.recovery.is_none()
            && page_commit_changed_without_events(&state, &current, events_seen)
        {
            let detail = format!(
                "board commit changed from {} to {} without a journal record; a sync/import or raw write may have bypassed the journal",
                state.source.commit, current.commit
            );
            self.pause_for_recovery(
                &mut state,
                RecoveryKind::ReplicaChanged,
                detail,
                None,
                Some(current.clone()),
                None,
            )?;
        } else if state.recovery.is_none() {
            state.source.commit = current.commit;
            state.source.branch = current.branch;
            state.source.repo_root = current.repo_root;
        }
        state.last_poll = Some(PollStamp {
            polled_at: now(),
            checkpoint: state.checkpoint,
            events_seen,
            accepted,
            ignored,
            duplicates,
            model_calls: 0,
        });
        let jobs = pending_jobs_from_state(&state);
        let recovery = state.recovery.clone();
        self.write_state_at(&path, &state)?;
        Ok(PollOutcome::Receipt(Box::new(poll_receipt(
            &state,
            accepted,
            ignored,
            duplicates,
            events_seen,
            jobs,
            recovery,
        ))))
    }

    fn pause_for_recovery(
        &self,
        state: &mut ReflectionState,
        kind: RecoveryKind,
        detail: String,
        floor: Option<u64>,
        observed_source: Option<SourceIdentity>,
        head: Option<u64>,
    ) -> io::Result<()> {
        let records = self.reconcile_records(state);
        state.recovery = Some(RecoveryState {
            kind,
            detail,
            observed_source,
            floor,
            head,
            records,
            recorded_at: now(),
        });
        Ok(())
    }

    fn evaluate_event(
        &self,
        state: &ReflectionState,
        event: &RawEvent,
    ) -> io::Result<EvaluatedEvent> {
        if event.op != "close" {
            return Ok(EvaluatedEvent::Ignored);
        }
        let Some(issue) = &event.issue else {
            return Ok(EvaluatedEvent::Ignored);
        };
        if issue.get("id").and_then(Value::as_str) != Some(event.issue_id.as_str())
            || issue.get("status").and_then(Value::as_str) != Some("closed")
        {
            return Ok(EvaluatedEvent::Ignored);
        }
        let metadata = metadata_of(issue);
        let Some(raw_acceptance) = metadata.get(ACCEPTANCE_METADATA) else {
            return Ok(EvaluatedEvent::Ignored);
        };
        let close_reason = issue
            .get("close_reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if close_reason != ACCEPTED_CLOSE_REASON {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::CloseReasonNotAccepted,
                format!(
                    "close reason '{close_reason}' is not accepted; lead review, cancellation and other closures are not acceptance"
                ),
            )));
        }
        let Some(raw_purpose) = metadata.get(PURPOSE_METADATA) else {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::MissingPurpose,
                "accepted work has no harness.purpose; purpose is never inferred as ordinary",
            )));
        };
        let purpose = match Purpose::parse(raw_purpose) {
            Ok(purpose) => purpose,
            Err(error) => {
                return Ok(EvaluatedEvent::Refused(refusal(
                    event,
                    RefusalCode::UnknownPurpose,
                    error.to_string(),
                )));
            }
        };
        if !purpose.triggers_reflection() {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::NonTriggerPurpose,
                format!("{} work never auto-triggers reflection", purpose.as_str()),
            )));
        }
        let parsed: AcceptanceBinding = match serde_json::from_str(raw_acceptance) {
            Ok(binding) => binding,
            Err(error) => {
                return Ok(EvaluatedEvent::Refused(refusal(
                    event,
                    RefusalCode::InvalidAcceptance,
                    format!(
                        "acceptance metadata is not strict schema {ACCEPTANCE_SCHEMA} JSON: {error}"
                    ),
                )));
            }
        };
        if parsed.outcome.trim() != "accepted" {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::OutcomeNotAccepted,
                format!("acceptance outcome '{}' is not accepted", parsed.outcome),
            )));
        }
        let binding = match parsed.normalized() {
            Ok(binding) => binding,
            Err(error) => {
                return Ok(EvaluatedEvent::Refused(refusal(
                    event,
                    RefusalCode::InvalidAcceptance,
                    error.to_string(),
                )));
            }
        };
        if binding.purpose != purpose {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::UnknownPurpose,
                format!(
                    "task purpose '{}' contradicts acceptance purpose '{}'",
                    purpose.as_str(),
                    binding.purpose.as_str()
                ),
            )));
        }
        if binding.task != event.issue_id {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::TaskMismatch,
                format!(
                    "acceptance task '{}' does not match closed task '{}'",
                    binding.task, event.issue_id
                ),
            )));
        }
        if binding.approved_by != event.actor.trim() {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::ActorMismatch,
                format!(
                    "close actor '{}' is not acceptance approver '{}'",
                    event.actor, binding.approved_by
                ),
            )));
        }
        if binding.lead_session != state.binding.lead_session {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::WrongLead,
                format!(
                    "acceptance lead '{}' is not active lead '{}'",
                    binding.lead_session, state.binding.lead_session
                ),
            )));
        }
        let acceptance_id = match binding.acceptance_id() {
            Ok(id) => id,
            Err(error) => {
                return Ok(EvaluatedEvent::Refused(refusal(
                    event,
                    RefusalCode::InvalidAcceptance,
                    error.to_string(),
                )));
            }
        };
        let task = self.task_by_id(&event.issue_id)?;
        if task.get("status").and_then(Value::as_str) != Some("closed") {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::TaskDrift,
                "the task is no longer closed; a historical close cannot start reflection after reuse",
            )));
        }
        if task
            .get("close_reason")
            .and_then(Value::as_str)
            .map(str::trim)
            != Some(ACCEPTED_CLOSE_REASON)
        {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::CloseReasonNotAccepted,
                "the task's current close reason is not accepted; a later non-accepting closure supersedes this event",
            )));
        }
        let current = metadata_of(&task);
        let current_binding = current
            .get(ACCEPTANCE_METADATA)
            .and_then(|raw| serde_json::from_str::<AcceptanceBinding>(raw).ok())
            .and_then(|binding| binding.normalized().ok())
            .and_then(|binding| binding.acceptance_id().ok());
        if current_binding.as_deref() != Some(acceptance_id.as_str()) {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::TaskDrift,
                "the task's current acceptance metadata differs from the close event",
            )));
        }
        if current.get(PURPOSE_METADATA).map(String::as_str) != Some(purpose.as_str()) {
            return Ok(EvaluatedEvent::Refused(refusal(
                event,
                RefusalCode::TaskDrift,
                "the task's current purpose differs from the close event",
            )));
        }
        if let Some((code, detail)) = self.purpose_lineage_refusal(&task)? {
            return Ok(EvaluatedEvent::Refused(refusal(event, code, detail)));
        }
        let task_revision = required_string(&task, "revision")?;
        Ok(EvaluatedEvent::Eligible {
            acceptance_id,
            binding: Box::new(binding),
            task_revision,
        })
    }

    fn task_by_id(&self, id: &str) -> io::Result<Value> {
        require_text("task id", id, MAX_TEXT_BYTES)?;
        let out = board_cli::run(&self.bd, &self.project, &["show", id, "--json"])?;
        if !out.status.success() {
            return Err(board_failure("bd show", &[id], &out));
        }
        let value: Value = serde_json::from_slice(&out.stdout).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bd show {id} did not return JSON: {error}"),
            )
        })?;
        value
            .as_array()
            .and_then(|items| items.first())
            .cloned()
            .ok_or_else(|| invalid(&format!("bd show {id} returned no task record")))
    }

    fn reconcile_records(&self, state: &ReflectionState) -> Vec<TaskReconciliation> {
        let mut results = Vec::new();
        for (index, record) in state.records.values().enumerate() {
            if index >= MAX_RECONCILE_RECORDS {
                break;
            }
            let task_id = record.binding.task.clone();
            match self.task_by_id(&task_id) {
                Err(error) => results.push(TaskReconciliation {
                    acceptance_id: record.acceptance_id.clone(),
                    task_id,
                    status: TaskBindingStatus::ReadError,
                    detail: error.to_string(),
                }),
                Ok(task) => {
                    let current = metadata_of(&task)
                        .get(ACCEPTANCE_METADATA)
                        .and_then(|raw| serde_json::from_str::<AcceptanceBinding>(raw).ok())
                        .and_then(|binding| binding.normalized().ok())
                        .and_then(|binding| binding.acceptance_id().ok());
                    if current.as_deref() == Some(record.acceptance_id.as_str()) {
                        results.push(TaskReconciliation {
                            acceptance_id: record.acceptance_id.clone(),
                            task_id,
                            status: TaskBindingStatus::Bound,
                            detail: "current acceptance identity matches the retained record"
                                .to_owned(),
                        });
                    } else {
                        results.push(TaskReconciliation {
                            acceptance_id: record.acceptance_id.clone(),
                            task_id,
                            status: TaskBindingStatus::Drifted,
                            detail:
                                "current task acceptance identity differs from the retained record"
                                    .to_owned(),
                        });
                    }
                }
            }
        }
        results
    }

    pub fn record(&self, acceptance_id: &str) -> io::Result<Option<ReflectionRecord>> {
        Ok(self
            .stored_state()?
            .and_then(|state| state.records.get(acceptance_id).cloned()))
    }

    pub fn pending_jobs(&self) -> io::Result<Vec<ReflectionJob>> {
        let state = self
            .stored_state()?
            .ok_or_else(|| conflict("delivery reflection is inactive"))?;
        if state.lifecycle != LifecycleStatus::Active || state.recovery.is_some() {
            return Ok(Vec::new());
        }
        Ok(pending_jobs_from_state(&state))
    }

    /// Marks one eligible stage in flight and exposes its source sessions.
    /// This reserves no model slot and creates no worktree.
    pub fn begin_stage(&self, acceptance_id: &str, stage: Stage) -> io::Result<StageStart> {
        let current = self.current_source()?;
        let path = self.state_path(&current);
        let _lock = self.lock(&path)?;
        let mut state = self.active_state(&path)?;
        if state.recovery.is_some() {
            return Err(recovery_error(
                "reflection recovery is pending; reconcile the retained records before beginning a stage",
            ));
        }
        if !state.source.same_scope(&current)
            || normalize(&state.source.branch) != normalize(&current.branch)
        {
            return Err(recovery_error(
                "board source or branch changed; poll and reconcile before beginning a stage",
            ));
        }
        // Admission may wait after poll. Revalidate the accepted work and its
        // purpose lineage before spending an attempt or starting a model.
        let binding_check =
            self.check_active_binding(&state.binding.lead_session, Some(acceptance_id))?;
        if !binding_check.is_active() {
            return Err(conflict(&format!(
                "cannot begin {} reflection: {}",
                stage.as_str(),
                binding_check.detail
            )));
        }
        let active = state
            .records
            .values()
            .map(|record| {
                usize::from(record.executor.state == StageState::InFlight)
                    + usize::from(record.lead.state == StageState::InFlight)
            })
            .sum::<usize>();
        if active >= state.limits.max_active_stages as usize {
            return Err(conflict(&format!(
                "{} reflection stages are already in flight; configured capacity is {}",
                active, state.limits.max_active_stages
            )));
        }
        let limits = state.limits.clone();
        let record = state
            .records
            .get_mut(acceptance_id)
            .ok_or_else(|| invalid(&format!("unknown acceptance id '{acceptance_id}'")))?;
        let record_totals = record.usage_totals();
        let record_tokens = record_totals.tokens;
        let remaining_tokens = limits.max_tokens_per_record.saturating_sub(record_tokens);
        let slot = match stage {
            Stage::Executor => {
                if record.executor.state != StageState::Pending {
                    return Err(conflict(&format!(
                        "executor reflection is {:?}, not pending",
                        record.executor.state
                    )));
                }
                &mut record.executor
            }
            Stage::Lead => {
                if record.executor.state != StageState::Retained {
                    return Err(conflict(
                        "lead reflection is ineligible until the executor outcome is retained",
                    ));
                }
                if !record.executor.usage_reconciled {
                    return Err(conflict(
                        "lead reflection is ineligible until the executor stage's new-turn usage is reconciled; record the measured or explicitly unknown cost first",
                    ));
                }
                if !record.executor.work_reconciled {
                    return Err(conflict(
                        "lead reflection is ineligible until the executor stage's observed investigation/context work is reconciled; record the measured work first",
                    ));
                }
                if record.lead.state != StageState::Pending
                    || record.delivery == DeliveryState::Consumed
                {
                    return Err(conflict(&format!(
                        "lead reflection is {:?} with delivery {:?}, not pending",
                        record.lead.state, record.delivery
                    )));
                }
                &mut record.lead
            }
        };
        if let Some(pause) = &slot.pause {
            return Err(conflict(&format!(
                "{} reflection remains paused; configure the named allowance above the observed value, reconcile usage, and call retry_stage before another attempt: {}",
                stage.as_str(),
                pause.summary()
            )));
        }
        if slot.usage.attempts >= limits.max_attempts_per_stage {
            return Err(conflict(&format!(
                "{} reflection exceeded its {} attempt allowance",
                stage.as_str(),
                limits.max_attempts_per_stage
            )));
        }
        if slot.usage.turns >= limits.max_turns_per_stage {
            return Err(conflict(&format!(
                "{} reflection reached its {} turn allowance",
                stage.as_str(),
                limits.max_turns_per_stage
            )));
        }
        if slot.usage.tokens >= limits.max_tokens_per_stage {
            return Err(conflict(&format!(
                "{} reflection reached its {} token allowance",
                stage.as_str(),
                limits.max_tokens_per_stage
            )));
        }
        if record_tokens >= limits.max_tokens_per_record {
            return Err(conflict(&format!(
                "accepted task reached its {} token allowance across both stages",
                limits.max_tokens_per_record
            )));
        }
        if slot.usage.investigations >= limits.max_investigations_per_stage {
            return Err(conflict(&format!(
                "{} reflection reached its {} investigation allowance",
                stage.as_str(),
                limits.max_investigations_per_stage
            )));
        }
        if record_totals.investigations >= limits.max_investigations_per_record {
            return Err(conflict(&format!(
                "accepted task reached its {} investigation allowance across both stages",
                limits.max_investigations_per_record
            )));
        }
        if slot.usage.context_bytes >= limits.max_context_bytes_per_stage {
            return Err(conflict(&format!(
                "{} reflection reached its {} supplied-context byte allowance",
                stage.as_str(),
                limits.max_context_bytes_per_stage
            )));
        }
        if record_totals.context_bytes >= limits.max_context_bytes_per_record {
            return Err(conflict(&format!(
                "accepted task reached its {} supplied-context byte allowance across both stages",
                limits.max_context_bytes_per_record
            )));
        }
        let admission = StageAdmission::new(&limits, &slot.usage, &record_totals);
        slot.usage.attempts = slot.usage.attempts.saturating_add(1);
        slot.state = StageState::InFlight;
        slot.usage_reconciled = false;
        slot.work_reconciled = false;
        let source_sessions = match stage {
            Stage::Executor => record.binding.executor_sessions.clone(),
            Stage::Lead => vec![state.binding.lead_session.clone()],
        };
        let start = StageStart {
            acceptance_id: record.acceptance_id.clone(),
            stage,
            attempt: slot.usage.attempts,
            source_sessions,
            lead_session: record.binding.lead_session.clone(),
            executor_session: record.binding.executor_session.clone(),
            executor_turn: record.binding.executor_turn.clone(),
            lead_turn: record.binding.lead_turn.clone(),
            usage: slot.usage.clone(),
            admission,
            remaining_tokens,
        };
        self.write_state_at(&path, &state)?;
        Ok(start)
    }

    pub fn retain_stage(
        &self,
        acceptance_id: &str,
        stage: Stage,
        outcome: StageOutcome,
        usage: UsageDelta,
    ) -> io::Result<StageReceipt> {
        self.retain_stage_with_work(acceptance_id, stage, outcome, usage, StageWork::default())
    }

    /// Retains a completed stage together with its observed investigation and
    /// supplied-context work. Prior attempts remain charged.
    pub fn retain_stage_with_work(
        &self,
        acceptance_id: &str,
        stage: Stage,
        outcome: StageOutcome,
        usage: UsageDelta,
        work: StageWork,
    ) -> io::Result<StageReceipt> {
        self.retain_stage_with_work_reconciliation(
            acceptance_id,
            stage,
            outcome,
            usage,
            true,
            work,
            true,
            "",
        )
    }

    /// Retains a completed stage with explicit token and observed-work
    /// reconciliation state. A result may be retained while one dimension is
    /// still unknown, but dependent stages remain closed until both are
    /// recorded without estimates.
    #[allow(clippy::too_many_arguments)]
    pub fn retain_stage_with_work_reconciliation(
        &self,
        acceptance_id: &str,
        stage: Stage,
        mut outcome: StageOutcome,
        usage: UsageDelta,
        usage_reconciled: bool,
        work: StageWork,
        work_reconciled: bool,
        detail: impl Into<String>,
    ) -> io::Result<StageReceipt> {
        normalize_outcome(&mut outcome)?;
        let detail = detail.into();
        let detail = if detail.trim().is_empty() {
            None
        } else {
            Some(bounded_text("usage uncertainty", &detail, MAX_TEXT_BYTES)?)
        };
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self.active_state(&path)?;
        let (record_tokens, next_stage_eligible) = {
            let record = state
                .records
                .get_mut(acceptance_id)
                .ok_or_else(|| invalid(&format!("unknown acceptance id '{acceptance_id}'")))?;
            let slot = stage_slot(record, stage);
            if slot.state != StageState::InFlight {
                return Err(conflict(&format!(
                    "{} reflection is {:?}, not in flight",
                    stage.as_str(),
                    slot.state
                )));
            }
            add_usage(slot, usage, work);
            slot.state = StageState::Retained;
            slot.usage.last_error = detail;
            slot.usage_reconciled = usage_reconciled;
            slot.work_reconciled = work_reconciled;
            slot.outcome = Some(outcome);
            slot.pause = None;
            if stage == Stage::Lead {
                record.delivery = DeliveryState::Pending;
            }
            let record_tokens = record.usage_totals().tokens;
            let next = lead_stage_eligible(record);
            (record_tokens, next)
        };
        let receipt = {
            let record = &state.records[acceptance_id];
            stage_receipt(record, stage, next_stage_eligible, record_tokens)
        };
        self.write_state_at(&path, &state)?;
        Ok(receipt)
    }

    /// Retains a completed stage's result while its exact new-turn cost is
    /// still unreconciled. The result survives, but lead admission stays
    /// closed until `record_unknown_usage` records the measured or explicitly
    /// unknown cost.
    pub fn retain_stage_unreconciled(
        &self,
        acceptance_id: &str,
        stage: Stage,
        outcome: StageOutcome,
        usage: UsageDelta,
        detail: impl Into<String>,
    ) -> io::Result<StageReceipt> {
        self.retain_stage_unreconciled_with_work(
            acceptance_id,
            stage,
            outcome,
            usage,
            StageWork::default(),
            true,
            detail,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn retain_stage_unreconciled_with_work(
        &self,
        acceptance_id: &str,
        stage: Stage,
        outcome: StageOutcome,
        usage: UsageDelta,
        work: StageWork,
        work_reconciled: bool,
        detail: impl Into<String>,
    ) -> io::Result<StageReceipt> {
        self.retain_stage_with_work_reconciliation(
            acceptance_id,
            stage,
            outcome,
            usage,
            false,
            work,
            work_reconciled,
            detail,
        )
    }

    pub fn fail_stage(
        &self,
        acceptance_id: &str,
        stage: Stage,
        error: impl Into<String>,
        usage: UsageDelta,
    ) -> io::Result<StageReceipt> {
        let error = bounded_text("stage error", &error.into(), MAX_TEXT_BYTES)?;
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self.active_state(&path)?;
        let record_tokens = {
            let record = state
                .records
                .get_mut(acceptance_id)
                .ok_or_else(|| invalid(&format!("unknown acceptance id '{acceptance_id}'")))?;
            let slot = stage_slot(record, stage);
            if slot.state != StageState::InFlight {
                return Err(conflict(&format!(
                    "{} reflection is {:?}, not in flight",
                    stage.as_str(),
                    slot.state
                )));
            }
            add_usage(slot, usage, StageWork::default());
            slot.state = StageState::Blocked;
            slot.usage.last_error = Some(error);
            slot.usage_reconciled = true;
            slot.work_reconciled = true;
            slot.outcome = None;
            slot.pause = None;
            record.usage_totals().tokens
        };
        let receipt = {
            let record = &state.records[acceptance_id];
            stage_receipt(record, stage, false, record_tokens)
        };
        self.write_state_at(&path, &state)?;
        Ok(receipt)
    }

    /// Stops one in-flight stage on a configured allowance. The observed work
    /// and any exact token delta remain charged. If token usage is not known,
    /// the pause stays unreconciled and blocks all dependent admission.
    #[allow(clippy::too_many_arguments)]
    pub fn pause_stage(
        &self,
        acceptance_id: &str,
        stage: Stage,
        pause: BudgetPause,
        usage: UsageDelta,
        work: StageWork,
        usage_reconciled: bool,
        work_reconciled: bool,
        usage_unknown_reason: Option<&str>,
        work_unknown_reason: Option<&str>,
    ) -> io::Result<StageReceipt> {
        pause.validate()?;
        let mut detail = pause.summary();
        if !usage_reconciled {
            let reason =
                usage_unknown_reason.unwrap_or("the exact new-turn token usage is unavailable");
            detail.push_str(&format!(
                "; token usage remains unreconciled and blocks dependent work: {reason}"
            ));
        }
        if !work_reconciled {
            let reason = work_unknown_reason
                .unwrap_or("the observed investigation and supplied-context work is unavailable");
            detail.push_str(&format!(
                "; observed work remains unreconciled and blocks dependent work: {reason}"
            ));
        }
        let detail = bounded_text("budget pause detail", &detail, MAX_TEXT_BYTES)?;
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self.active_state(&path)?;
        let record_tokens = {
            let record = state
                .records
                .get_mut(acceptance_id)
                .ok_or_else(|| invalid(&format!("unknown acceptance id '{acceptance_id}'")))?;
            let slot = stage_slot(record, stage);
            if slot.state != StageState::InFlight {
                return Err(conflict(&format!(
                    "{} reflection is {:?}, not in flight",
                    stage.as_str(),
                    slot.state
                )));
            }
            add_usage(slot, usage, work);
            slot.state = StageState::Blocked;
            slot.usage.last_error = Some(detail);
            slot.usage_reconciled = usage_reconciled;
            slot.work_reconciled = work_reconciled;
            slot.outcome = None;
            slot.pause = Some(pause);
            record.usage_totals().tokens
        };
        let receipt = {
            let record = &state.records[acceptance_id];
            stage_receipt(record, stage, false, record_tokens)
        };
        self.write_state_at(&path, &state)?;
        Ok(receipt)
    }

    pub fn mark_stage_unknown(
        &self,
        acceptance_id: &str,
        stage: Stage,
        error: impl Into<String>,
    ) -> io::Result<StageReceipt> {
        self.mark_stage_unknown_with_usage(acceptance_id, stage, error, UsageDelta::default())
    }

    /// Marks one stage unknown while retaining the work its submitted turn
    /// consumed. Usage stays unreconciled, so dependent stages remain closed.
    pub fn mark_stage_unknown_with_usage(
        &self,
        acceptance_id: &str,
        stage: Stage,
        error: impl Into<String>,
        usage: UsageDelta,
    ) -> io::Result<StageReceipt> {
        self.mark_stage_unknown_with_work_and_usage(
            acceptance_id,
            stage,
            error,
            usage,
            StageWork::default(),
            true,
        )
    }

    /// Marks one stage unknown with both its submitted token delta and any
    /// measured investigation/context work. Token usage remains
    /// unreconciled; work is reconciled only when the caller measured it.
    pub fn mark_stage_unknown_with_work_and_usage(
        &self,
        acceptance_id: &str,
        stage: Stage,
        error: impl Into<String>,
        usage: UsageDelta,
        work: StageWork,
        work_reconciled: bool,
    ) -> io::Result<StageReceipt> {
        let error = bounded_text("stage uncertainty", &error.into(), MAX_TEXT_BYTES)?;
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self.active_state(&path)?;
        let record_tokens = {
            let record = state
                .records
                .get_mut(acceptance_id)
                .ok_or_else(|| invalid(&format!("unknown acceptance id '{acceptance_id}'")))?;
            let slot = stage_slot(record, stage);
            if slot.state != StageState::InFlight {
                return Err(conflict(&format!(
                    "{} reflection is {:?}, not in flight",
                    stage.as_str(),
                    slot.state
                )));
            }
            add_usage(slot, usage, work);
            slot.state = StageState::Unknown;
            slot.usage.last_error = Some(error);
            slot.usage_reconciled = false;
            slot.work_reconciled = work_reconciled;
            record.usage_totals().tokens
        };
        let receipt = {
            let record = &state.records[acceptance_id];
            stage_receipt(record, stage, false, record_tokens)
        };
        self.write_state_at(&path, &state)?;
        Ok(receipt)
    }

    pub fn record_unknown_usage(
        &self,
        acceptance_id: &str,
        stage: Stage,
        usage: UsageDelta,
    ) -> io::Result<StageReceipt> {
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self.active_state(&path)?;
        let (record_tokens, next_stage_eligible) = {
            let record = state
                .records
                .get_mut(acceptance_id)
                .ok_or_else(|| invalid(&format!("unknown acceptance id '{acceptance_id}'")))?;
            let slot = stage_slot(record, stage);
            let reconcilable = slot.state == StageState::Unknown
                || (slot.state == StageState::Retained && !slot.usage_reconciled)
                || (slot.state == StageState::Blocked
                    && slot.pause.is_some()
                    && !slot.usage_reconciled);
            if !reconcilable {
                return Err(conflict(&format!(
                    "{} reflection is {:?} with usage reconciled {}, not unknown or retained-unreconciled",
                    stage.as_str(),
                    slot.state,
                    slot.usage_reconciled
                )));
            }
            add_usage(slot, usage, StageWork::default());
            slot.usage_reconciled = true;
            if slot.work_reconciled {
                slot.usage.last_error = None;
            }
            let record_tokens = record.usage_totals().tokens;
            let next = stage == Stage::Executor && lead_stage_eligible(record);
            (record_tokens, next)
        };
        let receipt = {
            let record = &state.records[acceptance_id];
            stage_receipt(record, stage, next_stage_eligible, record_tokens)
        };
        self.write_state_at(&path, &state)?;
        Ok(receipt)
    }

    /// Records measured investigation and supplied-context work for a stage
    /// whose work signal was incomplete. Pass only observed values, never an
    /// estimate. Token reconciliation remains a separate requirement.
    pub fn record_observed_work(
        &self,
        acceptance_id: &str,
        stage: Stage,
        work: StageWork,
    ) -> io::Result<StageReceipt> {
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self.active_state(&path)?;
        let (record_tokens, next_stage_eligible) = {
            let record = state
                .records
                .get_mut(acceptance_id)
                .ok_or_else(|| invalid(&format!("unknown acceptance id '{acceptance_id}'")))?;
            let slot = stage_slot(record, stage);
            let reconcilable = !slot.work_reconciled
                && (slot.state == StageState::Unknown
                    || slot.state == StageState::Retained
                    || (slot.state == StageState::Blocked && slot.pause.is_some()));
            if !reconcilable {
                return Err(conflict(&format!(
                    "{} reflection is {:?} with observed work already reconciled {}; no additional work was charged",
                    stage.as_str(),
                    slot.state,
                    slot.work_reconciled
                )));
            }
            add_usage(slot, UsageDelta::default(), work);
            slot.work_reconciled = true;
            if slot.usage_reconciled {
                slot.usage.last_error = None;
            }
            let record_tokens = record.usage_totals().tokens;
            let next = stage == Stage::Executor && lead_stage_eligible(record);
            (record_tokens, next)
        };
        let receipt = {
            let record = &state.records[acceptance_id];
            stage_receipt(record, stage, next_stage_eligible, record_tokens)
        };
        self.write_state_at(&path, &state)?;
        Ok(receipt)
    }

    pub fn retry_stage(&self, acceptance_id: &str, stage: Stage) -> io::Result<StageReceipt> {
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self.active_state(&path)?;
        let limits = state.limits.clone();
        let max_attempts = limits.max_attempts_per_stage;
        let record_tokens = {
            let record = state
                .records
                .get_mut(acceptance_id)
                .ok_or_else(|| invalid(&format!("unknown acceptance id '{acceptance_id}'")))?;
            let slot = stage_slot(record, stage);
            if !matches!(slot.state, StageState::Unknown | StageState::Blocked) {
                return Err(conflict(&format!(
                    "{} reflection is {:?}; retry requires unknown or blocked",
                    stage.as_str(),
                    slot.state
                )));
            }
            if slot.state == StageState::Unknown && !slot.usage_reconciled {
                return Err(conflict(
                    "the unknown stage outcome must have its usage reconciled with record_unknown_usage before retry",
                ));
            }
            if slot.state == StageState::Unknown && !slot.work_reconciled {
                return Err(conflict(
                    "the unknown stage's observed investigation/context work must be reconciled with record_observed_work before retry",
                ));
            }
            if slot.state == StageState::Blocked {
                let pause = slot.pause.as_ref().ok_or_else(|| {
                    conflict(&format!(
                        "{} reflection is blocked without a retained budget pause; reconcile its cause before retry",
                        stage.as_str()
                    ))
                })?;
                if !slot.usage_reconciled {
                    return Err(conflict(
                        "the paused stage's new-turn usage must be reconciled with record_unknown_usage before retry",
                    ));
                }
                if !slot.work_reconciled {
                    return Err(conflict(
                        "the paused stage's observed investigation/context work must be reconciled with record_observed_work before retry",
                    ));
                }
                let configured = pause.limit.configured(&limits);
                if configured <= pause.observed {
                    return Err(conflict(&format!(
                        "{} reflection remains paused at {} observed {}; configure that allowance above the observed value before retry",
                        stage.as_str(),
                        pause.limit.as_str(),
                        pause.observed
                    )));
                }
            }
            if slot.usage.attempts >= max_attempts {
                return Err(conflict(&format!(
                    "{} reflection reached its {max_attempts} attempt allowance",
                    stage.as_str()
                )));
            }
            slot.state = StageState::Pending;
            slot.outcome = None;
            slot.pause = None;
            slot.usage.last_error = None;
            record.usage_totals().tokens
        };
        let receipt = {
            let record = &state.records[acceptance_id];
            stage_receipt(record, stage, false, record_tokens)
        };
        self.write_state_at(&path, &state)?;
        Ok(receipt)
    }

    pub fn observe_delivery(
        &self,
        acceptance_id: &str,
        receipt: impl Into<String>,
    ) -> io::Result<DeliveryReceipt> {
        let receipt = bounded_text("delivery receipt", &receipt.into(), MAX_TEXT_BYTES)?;
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self.active_state(&path)?;
        let duplicate = {
            let record = state
                .records
                .get_mut(acceptance_id)
                .ok_or_else(|| invalid(&format!("unknown acceptance id '{acceptance_id}'")))?;
            if record.lead.state != StageState::Retained {
                return Err(conflict(
                    "delivery is only eligible after the lead outcome is retained",
                ));
            }
            if record.delivery == DeliveryState::Consumed {
                if record.delivery_receipt.as_deref() == Some(receipt.as_str()) {
                    true
                } else {
                    return Err(conflict(&format!(
                        "delivery was already consumed with receipt '{}'",
                        record.delivery_receipt.as_deref().unwrap_or("<missing>")
                    )));
                }
            } else {
                record.delivery = DeliveryState::Consumed;
                record.delivery_receipt = Some(receipt.clone());
                false
            }
        };
        self.write_state_at(&path, &state)?;
        Ok(DeliveryReceipt {
            acceptance_id: acceptance_id.to_owned(),
            delivery: DeliveryState::Consumed,
            receipt,
            duplicate,
        })
    }

    pub fn mark_delivery_unknown(
        &self,
        acceptance_id: &str,
        detail: impl Into<String>,
    ) -> io::Result<StageReceipt> {
        let detail = bounded_text("delivery uncertainty", &detail.into(), MAX_TEXT_BYTES)?;
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self.active_state(&path)?;
        let record_tokens = {
            let record = state
                .records
                .get_mut(acceptance_id)
                .ok_or_else(|| invalid(&format!("unknown acceptance id '{acceptance_id}'")))?;
            if record.lead.state != StageState::Retained {
                return Err(conflict(
                    "delivery uncertainty requires a retained lead outcome",
                ));
            }
            if record.delivery == DeliveryState::Consumed {
                return Err(conflict("delivery is already conclusively consumed"));
            }
            record.delivery = DeliveryState::Unknown;
            record.lead.usage.last_error = Some(detail);
            record.usage_totals().tokens
        };
        let receipt = {
            let record = &state.records[acceptance_id];
            stage_receipt(record, Stage::Lead, false, record_tokens)
        };
        self.write_state_at(&path, &state)?;
        Ok(receipt)
    }

    pub fn configure_limits(&self, limits: &ReflectionLimits) -> io::Result<()> {
        limits.validate()?;
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self.active_state(&path)?;
        if state.records.values().any(|record| {
            matches!(
                record.executor.state,
                StageState::InFlight | StageState::Unknown
            ) || matches!(
                record.lead.state,
                StageState::InFlight | StageState::Unknown
            )
        }) {
            return Err(conflict(
                "reconcile in-flight or unknown reflection stages before changing limits",
            ));
        }
        state.limits = limits.clone();
        self.write_state_at(&path, &state)?;
        Ok(())
    }

    pub fn stop(&self) -> io::Result<StopReceipt> {
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self.read_state_at(&path)?.ok_or_else(|| {
            conflict("delivery reflection is inactive; there is no lifecycle to stop")
        })?;
        let stopped_at = now();
        state.lifecycle = LifecycleStatus::Stopped;
        state.stopped_at = Some(stopped_at.clone());
        self.write_state_at(&path, &state)?;
        Ok(StopReceipt {
            binding_id: state.binding.id,
            revision: state.binding.revision,
            checkpoint: state.checkpoint,
            stopped_at,
            retained_records: state.records.len(),
        })
    }

    /// Removes only a journal setting this controller enabled. Retained
    /// records and another consumer's preexisting setting are preserved.
    pub fn restore_owned_journal(&self) -> io::Result<RecoveryReceipt> {
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let state = self.read_state_at(&path)?.ok_or_else(|| {
            conflict("delivery reflection is inactive; there is no owned journal setting")
        })?;
        if state.lifecycle != LifecycleStatus::Stopped {
            return Err(conflict(
                "stop delivery reflection before restoring the journal setting",
            ));
        }
        let (journal_restored, detail) = match state.journal {
            JournalOwnership::PreexistingEnabled => (
                false,
                "events-journal predated activation; leaving it unchanged".to_owned(),
            ),
            JournalOwnership::EnabledByController => match self.journal_value()? {
                Some(true) => {
                    let out = board_cli::run(
                        &self.bd,
                        &self.project,
                        &["config", "unset", JOURNAL_CONFIG_KEY],
                    )?;
                    if !out.status.success() {
                        return Err(board_failure("bd config unset", &["events-journal"], &out));
                    }
                    if self.journal_value()? == Some(true) {
                        return Err(conflict(
                            "bd accepted events-journal removal but the setting is still true; the controller-owned setting was not restored",
                        ));
                    }
                    (
                        true,
                        "removed the controller-owned events-journal setting".to_owned(),
                    )
                }
                Some(false) => (
                    false,
                    "the controller-owned events-journal setting is already false".to_owned(),
                ),
                None => (
                    false,
                    "the controller-owned events-journal setting was removed by another owner; leaving it absent".to_owned(),
                ),
            },
        };
        Ok(RecoveryReceipt {
            binding_id: state.binding.id,
            revision: state.binding.revision,
            checkpoint: state.checkpoint,
            journal_restored,
            detail,
        })
    }

    pub fn rebaseline_current(&self, evidence: impl Into<String>) -> io::Result<RecoveryReceipt> {
        let evidence = bounded_text("recovery evidence", &evidence.into(), MAX_TEXT_BYTES)?;
        let current = self.current_source()?;
        let path = self.state_path(&current);
        let _lock = self.lock(&path)?;
        let mut state = self.read_state_at(&path)?.ok_or_else(|| {
            conflict("delivery reflection is inactive; there is no recovery to resolve")
        })?;
        if state.lifecycle != LifecycleStatus::Active {
            return Err(conflict(
                "activate delivery reflection before rebaselining recovery",
            ));
        }
        let recovery = state
            .recovery
            .as_ref()
            .ok_or_else(|| conflict("delivery reflection has no pending recovery"))?
            .clone();
        let records = self.reconcile_records(&state);
        if records
            .iter()
            .any(|record| record.status != TaskBindingStatus::Bound)
        {
            state.recovery = Some(RecoveryState {
                records,
                recorded_at: now(),
                ..recovery
            });
            self.write_state_at(&path, &state)?;
            return Err(recovery_error(
                "retained task identity drifted or could not be read; preserve the records and repair the board source before rebaselining",
            ));
        }
        let anchor = recovery.floor.unwrap_or(state.checkpoint);
        let head = self.probe_head(anchor)?;
        state.checkpoint = head;
        state.source = current;
        state.recovery = None;
        state.binding.revision = state.binding.revision.saturating_add(1);
        state.last_recovery_evidence = Some(evidence.clone());
        let receipt = RecoveryReceipt {
            binding_id: state.binding.id.clone(),
            revision: state.binding.revision,
            checkpoint: head,
            journal_restored: false,
            detail: format!("rebaselined after scoped reconciliation: {evidence}"),
        };
        self.write_state_at(&path, &state)?;
        Ok(receipt)
    }

    pub fn resume_from_floor(&self) -> io::Result<RecoveryReceipt> {
        let path = self.state_path(&self.current_source()?);
        let _lock = self.lock(&path)?;
        let mut state = self
            .read_state_at(&path)?
            .ok_or_else(|| conflict("delivery reflection is inactive"))?;
        let recovery = state
            .recovery
            .as_ref()
            .ok_or_else(|| conflict("delivery reflection has no pending recovery"))?;
        if recovery.kind != RecoveryKind::EventGap || recovery.floor.is_none() {
            return Err(conflict(
                "resume_from_floor is only available for a truncated journal with a recorded floor",
            ));
        }
        let floor = recovery.floor.expect("checked above");
        let checkpoint = floor.saturating_sub(1);
        state.checkpoint = checkpoint;
        state.recovery = None;
        state.binding.revision = state.binding.revision.saturating_add(1);
        let receipt = RecoveryReceipt {
            binding_id: state.binding.id.clone(),
            revision: state.binding.revision,
            checkpoint,
            journal_restored: false,
            detail: format!("resuming at retained journal floor {floor}"),
        };
        self.write_state_at(&path, &state)?;
        Ok(receipt)
    }

    fn purpose_lineage_refusal(&self, task: &Value) -> io::Result<Option<(RefusalCode, String)>> {
        let mut parent = task
            .get("parent")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mut seen = BTreeSet::new();
        let mut depth = 0usize;
        while let Some(parent_id) = parent.take() {
            if depth >= MAX_ANCESTORS {
                return Ok(Some((
                    RefusalCode::AncestorLimit,
                    format!("purpose lineage exceeds {MAX_ANCESTORS} ancestors at '{parent_id}'"),
                )));
            }
            if !seen.insert(parent_id.clone()) {
                return Ok(Some((
                    RefusalCode::LineageUnavailable,
                    format!("purpose lineage repeats '{parent_id}'"),
                )));
            }
            let ancestor = self.task_by_id(&parent_id)?;
            let ancestor_metadata = metadata_of(&ancestor);
            if let Some(value) = ancestor_metadata.get(PURPOSE_METADATA) {
                let ancestor_purpose = match Purpose::parse(value) {
                    Ok(purpose) => purpose,
                    Err(error) => {
                        return Ok(Some((
                            RefusalCode::UnknownPurpose,
                            format!("ancestor '{parent_id}': {error}"),
                        )));
                    }
                };
                if !ancestor_purpose.triggers_reflection() {
                    return Ok(Some((
                        RefusalCode::NonTriggerPurpose,
                        format!(
                            "ancestor '{parent_id}' has {} purpose; descendants never auto-trigger",
                            ancestor_purpose.as_str()
                        ),
                    )));
                }
            }
            parent = ancestor
                .get("parent")
                .and_then(Value::as_str)
                .map(str::to_owned);
            depth += 1;
        }
        Ok(None)
    }

    /// Model-free read-only validation used by verified-improvement admission.
    /// This creates no directory or lock, advances no checkpoint and reports
    /// every stable binding field it could read. Ordinary journaled board
    /// writes may advance the source commit; commit churn alone does not stale
    /// the binding, and `poll` owns sync/gap recovery. When `work` is given,
    /// the matched task is re-read and its current acceptance identity and
    /// purpose must still match the retained record.
    pub fn check_active_binding(
        &self,
        expected_lead: &str,
        work: Option<&str>,
    ) -> io::Result<BindingCheck> {
        let current = self.current_source()?;
        let path = self.state_path(&current);
        let state = match self.read_state_at(&path) {
            Ok(Some(state)) => state,
            Ok(None) => {
                return Ok(BindingCheck {
                    status: BindingStatus::Inactive,
                    binding_id: None,
                    revision: 0,
                    source: Some(current),
                    lead_session: None,
                    run: None,
                    checkpoint: None,
                    matched_acceptance_id: None,
                    work: work.map(str::to_owned),
                    detail: "no delivery-reflection lifecycle is retained for this board"
                        .to_owned(),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                return Ok(BindingCheck {
                    status: BindingStatus::InvalidState,
                    binding_id: None,
                    revision: 0,
                    source: Some(current),
                    lead_session: None,
                    run: None,
                    checkpoint: None,
                    matched_acceptance_id: None,
                    work: work.map(str::to_owned),
                    detail: error.to_string(),
                });
            }
            Err(error) => return Err(error),
        };
        let mut check = BindingCheck {
            status: BindingStatus::Active,
            binding_id: Some(state.binding.id.clone()),
            revision: state.binding.revision,
            source: Some(state.source.clone()),
            lead_session: Some(state.binding.lead_session.clone()),
            run: state.binding.run.clone(),
            checkpoint: Some(state.checkpoint),
            matched_acceptance_id: None,
            work: work.map(str::to_owned),
            detail: String::new(),
        };
        if !state.source.same_scope(&current) {
            check.status = BindingStatus::WrongProject;
            check.detail = format!(
                "binding project/beads/database {} / {} / {} does not match current {} / {} / {}",
                state.source.project_id,
                state.source.beads_dir,
                state.source.database,
                current.project_id,
                current.beads_dir,
                current.database
            );
            return Ok(check);
        }
        if state.lifecycle != LifecycleStatus::Active {
            check.status = BindingStatus::Stopped;
            check.detail = "delivery reflection is stopped".to_owned();
            return Ok(check);
        }
        if expected_lead.trim().is_empty() || expected_lead.trim() != state.binding.lead_session {
            check.status = BindingStatus::WrongLead;
            check.detail = format!(
                "active lead is '{}', not expected '{}'",
                state.binding.lead_session, expected_lead
            );
            return Ok(check);
        }
        if let Some(recovery) = &state.recovery {
            check.status = BindingStatus::RecoveryPending;
            check.detail = format!("{:?} recovery: {}", recovery.kind, recovery.detail);
            return Ok(check);
        }
        if normalize(&state.source.branch) != normalize(&current.branch) {
            check.status = BindingStatus::StaleSource;
            check.detail = format!(
                "binding branch '{}' differs from current branch '{}'",
                state.source.branch, current.branch
            );
            return Ok(check);
        }
        if let Some(work) = work {
            let work = work.trim();
            let mut candidates = state
                .records
                .values()
                .filter(|record| work_matches(record, work));
            let Some(first) = candidates.next() else {
                check.status = BindingStatus::UnrelatedWork;
                check.detail = format!(
                    "work '{work}' matches no retained acceptance, task, run, assignment or result"
                );
                return Ok(check);
            };
            let task_id = first.binding.task.clone();
            if candidates.any(|record| record.binding.task != task_id) {
                check.status = BindingStatus::StaleSource;
                check.detail =
                    format!("work '{work}' matches retained acceptances for more than one task");
                return Ok(check);
            }
            let task = self.task_by_id(&task_id)?;
            if task.get("status").and_then(Value::as_str) != Some("closed") {
                check.status = BindingStatus::StaleSource;
                check.detail =
                    format!("retained work '{work}' no longer refers to a closed task '{task_id}'");
                return Ok(check);
            }
            if task
                .get("close_reason")
                .and_then(Value::as_str)
                .map(str::trim)
                != Some(ACCEPTED_CLOSE_REASON)
            {
                check.status = BindingStatus::StaleSource;
                check.detail = format!("task '{task_id}' no longer has an accepted close reason");
                return Ok(check);
            }
            let current_metadata = metadata_of(&task);
            let current_binding = current_metadata
                .get(ACCEPTANCE_METADATA)
                .and_then(|raw| serde_json::from_str::<AcceptanceBinding>(raw).ok())
                .and_then(|binding| binding.normalized().ok())
                .and_then(|binding| binding.acceptance_id().ok());
            let Some(record) = current_binding
                .as_deref()
                .and_then(|id| state.records.get(id))
            else {
                check.status = BindingStatus::StaleSource;
                check.detail = format!(
                    "task '{task_id}' no longer carries an acceptance retained by this binding"
                );
                return Ok(check);
            };
            if !work_matches(record, work) {
                check.status = BindingStatus::StaleSource;
                check.detail = format!(
                    "work '{work}' refers to a superseded acceptance; current acceptance is '{}'",
                    record.acceptance_id
                );
                return Ok(check);
            }
            if current_metadata.get(PURPOSE_METADATA).map(String::as_str)
                != Some(record.binding.purpose.as_str())
            {
                check.status = BindingStatus::StaleSource;
                check.detail = format!(
                    "task '{task_id}' now carries a different purpose than retained acceptance '{}'",
                    record.acceptance_id
                );
                return Ok(check);
            }
            if let Some((_, detail)) = self.purpose_lineage_refusal(&task)? {
                check.status = BindingStatus::StaleSource;
                check.detail = detail;
                return Ok(check);
            }
            check.matched_acceptance_id = Some(record.acceptance_id.clone());
        }
        check.detail =
            "active reflection binding matches the exact project, lead and work".to_owned();
        Ok(check)
    }

    fn active_state(&self, path: &Path) -> io::Result<ReflectionState> {
        let state = self
            .read_state_at(path)?
            .ok_or_else(|| conflict("delivery reflection is inactive; activate it first"))?;
        if state.lifecycle != LifecycleStatus::Active {
            return Err(conflict(
                "delivery reflection is stopped; activate it before this operation",
            ));
        }
        Ok(state)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RawEvent {
    seq: u64,
    ts: String,
    op: String,
    issue_id: String,
    actor: String,
    issue: Option<Value>,
}

struct TailPage {
    events: Vec<RawEvent>,
    truncated: Option<(u64, u64)>,
}

enum EvaluatedEvent {
    Ignored,
    Refused(EventRefusal),
    Eligible {
        acceptance_id: String,
        binding: Box<AcceptanceBinding>,
        task_revision: String,
    },
}

fn stage_slot(record: &mut ReflectionRecord, stage: Stage) -> &mut StageRecord {
    match stage {
        Stage::Executor => &mut record.executor,
        Stage::Lead => &mut record.lead,
    }
}

fn add_usage(slot: &mut StageRecord, usage: UsageDelta, work: StageWork) {
    slot.usage.turns = slot.usage.turns.saturating_add(usage.turns);
    slot.usage.tokens = slot.usage.tokens.saturating_add(usage.tokens);
    slot.usage.investigations = slot
        .usage
        .investigations
        .saturating_add(work.investigations);
    slot.usage.context_bytes = slot.usage.context_bytes.saturating_add(work.context_bytes);
}

fn normalize_outcome(outcome: &mut StageOutcome) -> io::Result<()> {
    outcome.summary = bounded_text("stage summary", &outcome.summary, MAX_SUMMARY_BYTES)?;
    if outcome.coverage.len() > MAX_COVERAGE_ITEMS {
        return Err(invalid(&format!(
            "stage coverage has {} items; the limit is {MAX_COVERAGE_ITEMS}",
            outcome.coverage.len()
        )));
    }
    for item in &mut outcome.coverage {
        *item = bounded_text("stage coverage item", item, MAX_TEXT_BYTES)?;
    }
    if outcome.retained_at.trim().is_empty() {
        outcome.retained_at = now();
    } else {
        outcome.retained_at =
            bounded_text("stage retained_at", &outcome.retained_at, MAX_TEXT_BYTES)?;
    }
    if let Some(session) = &mut outcome.session {
        *session = bounded_text("stage session", session, MAX_TEXT_BYTES)?;
    }
    Ok(())
}

/// The lead stage may begin only after the executor outcome is retained and
/// its new-turn cost is reconciled; an unreconciled cost pauses admission
/// instead of resetting or skipping it.
fn lead_stage_eligible(record: &ReflectionRecord) -> bool {
    record.executor.state == StageState::Retained
        && record.executor.usage_reconciled
        && record.executor.work_reconciled
        && record.lead.state == StageState::Pending
        && record.delivery != DeliveryState::Consumed
}

fn stage_receipt(
    record: &ReflectionRecord,
    stage: Stage,
    next_stage_eligible: bool,
    record_tokens: u64,
) -> StageReceipt {
    let slot = match stage {
        Stage::Executor => &record.executor,
        Stage::Lead => &record.lead,
    };
    StageReceipt {
        acceptance_id: record.acceptance_id.clone(),
        stage,
        state: slot.state,
        usage: slot.usage.clone(),
        delivery: record.delivery,
        next_stage_eligible,
        record_tokens,
    }
}

fn pending_jobs_from_state(state: &ReflectionState) -> Vec<ReflectionJob> {
    let mut jobs = Vec::new();
    for record in state.records.values() {
        if record.executor.state == StageState::Pending {
            jobs.push(reflection_job(state, record, Stage::Executor));
        }
        if record.executor.state == StageState::Retained
            && record.lead.state == StageState::Pending
            && record.delivery != DeliveryState::Consumed
        {
            jobs.push(reflection_job(state, record, Stage::Lead));
        }
    }
    jobs
}

fn reflection_job(
    state: &ReflectionState,
    record: &ReflectionRecord,
    stage: Stage,
) -> ReflectionJob {
    let slot = match stage {
        Stage::Executor => &record.executor,
        Stage::Lead => &record.lead,
    };
    let mut context = vec![
        format!("task={}", record.binding.task),
        format!("run={}", record.binding.run),
        format!("assignment={}", record.binding.assignment),
        format!("result={}", record.binding.result),
        format!("revision={}", record.binding.revision),
    ];
    if stage == Stage::Lead
        && let Some(outcome) = &record.executor.outcome
    {
        context.push(format!("executor_finding={:?}", outcome.finding));
        context.push(format!("executor_summary={}", outcome.summary));
        for item in &outcome.coverage {
            context.push(format!("executor_coverage={item}"));
        }
    }
    let source_sessions = match stage {
        Stage::Executor => record.binding.executor_sessions.clone(),
        Stage::Lead => vec![state.binding.lead_session.clone()],
    };
    ReflectionJob {
        acceptance_id: record.acceptance_id.clone(),
        stage,
        task_id: record.binding.task.clone(),
        run: record.binding.run.clone(),
        source_sessions,
        lead_session: record.binding.lead_session.clone(),
        executor_session: record.binding.executor_session.clone(),
        executor_turn: record.binding.executor_turn.clone(),
        lead_turn: record.binding.lead_turn.clone(),
        context,
        usage: slot.usage.clone(),
        remaining_tokens: state
            .limits
            .max_tokens_per_record
            .saturating_sub(record.usage_totals().tokens),
    }
}

fn work_matches(record: &ReflectionRecord, work: &str) -> bool {
    record.acceptance_id == work
        || record.binding.task == work
        || record.binding.run == work
        || record.binding.assignment == work
        || record.binding.result == work
}

fn poll_receipt(
    state: &ReflectionState,
    accepted: u64,
    ignored: u64,
    duplicates: u64,
    events_seen: u64,
    jobs: Vec<ReflectionJob>,
    recovery: Option<RecoveryState>,
) -> PollReceipt {
    PollReceipt {
        checkpoint: state.checkpoint,
        model_calls: 0,
        events_seen,
        accepted,
        ignored,
        duplicates,
        jobs,
        recovery,
        refusals: state
            .refusals
            .iter()
            .rev()
            .take(MAX_POLL_REFUSALS)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
    }
}

fn page_commit_changed_without_events(
    state: &ReflectionState,
    current: &SourceIdentity,
    events_seen: u64,
) -> bool {
    events_seen == 0
        && !state.source.commit.is_empty()
        && !current.commit.is_empty()
        && state.source.commit != current.commit
}

fn retain_refusal(refusals: &mut Vec<EventRefusal>, refusal: EventRefusal) {
    refusals.push(refusal);
    if refusals.len() > MAX_POLL_REFUSALS {
        let excess = refusals.len() - MAX_POLL_REFUSALS;
        refusals.drain(0..excess);
    }
}

fn refusal(event: &RawEvent, code: RefusalCode, detail: impl Into<String>) -> EventRefusal {
    EventRefusal {
        seq: event.seq,
        task: event.issue_id.clone(),
        code,
        detail: detail.into(),
    }
}

fn metadata_of(value: &Value) -> BTreeMap<String, String> {
    value
        .get("metadata")
        .and_then(Value::as_object)
        .map(|metadata| {
            metadata
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn required_string(value: &Value, field: &str) -> io::Result<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(&format!("missing bd field {field}")))
}

fn validate_state(state: &ReflectionState) -> io::Result<()> {
    if state.schema != STATE_SCHEMA {
        return Err(invalid(&format!(
            "reflection state schema {} is unsupported; expected {STATE_SCHEMA}",
            state.schema
        )));
    }
    if state.binding.id.trim().is_empty() || state.binding.revision == 0 {
        return Err(invalid(
            "reflection lifecycle binding identity is incomplete",
        ));
    }
    require_text(
        "binding lead_session",
        &state.binding.lead_session,
        MAX_TEXT_BYTES,
    )?;
    state.limits.validate()?;
    for (id, record) in &state.records {
        if id != &record.acceptance_id {
            return Err(invalid(
                "reflection record key differs from its acceptance identity",
            ));
        }
        if record.binding.acceptance_id()? != *id {
            return Err(invalid(
                "reflection record acceptance identity is not stable",
            ));
        }
        if record.binding.purpose != Purpose::Ordinary {
            return Err(invalid(
                "reflection records may contain only ordinary accepted work",
            ));
        }
    }
    Ok(())
}

fn truncated_bounds(out: &Output) -> Option<(u64, u64)> {
    for bytes in [&out.stdout, &out.stderr] {
        if let Some(bounds) = truncated_bounds_in(bytes) {
            return Some(bounds);
        }
    }
    None
}

fn truncated_bounds_in(bytes: &[u8]) -> Option<(u64, u64)> {
    let text = String::from_utf8_lossy(bytes);
    let mut search = text.as_ref();
    while let Some(start) = search.find('{') {
        let candidate = &search[start..];
        for (end, _) in candidate.match_indices('}') {
            let Ok(value) = serde_json::from_str::<Value>(&candidate[..=end]) else {
                continue;
            };
            if value.get("code").and_then(Value::as_str) == Some("events_journal_truncated") {
                return Some((value.get("floor")?.as_u64()?, value.get("head")?.as_u64()?));
            }
        }
        search = &candidate[1..];
    }
    None
}

fn board_failure(op: &str, args: &[&str], out: &Output) -> io::Error {
    io::Error::other(format!(
        "{op} {} failed: {} {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

fn require_text(name: &str, value: &str, limit: usize) -> io::Result<()> {
    if value.trim().is_empty() {
        return Err(invalid(&format!("{name} is empty")));
    }
    if value.len() > limit {
        return Err(invalid(&format!(
            "{name} is {} bytes; the limit is {limit}",
            value.len()
        )));
    }
    Ok(())
}

fn bounded_text(name: &str, value: &str, limit: usize) -> io::Result<String> {
    let value = value.trim();
    require_text(name, value, limit)?;
    Ok(value.to_owned())
}

fn normalize(value: &str) -> String {
    value.trim().replace('\\', "/").to_ascii_lowercase()
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn now() -> String {
    chrono::DateTime::<Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn conflict(message: &str) -> io::Error {
    io::Error::other(message)
}

fn recovery_error(message: &str) -> io::Error {
    io::Error::other(format!("reflection recovery required: {message}"))
}
