//! Native executor run observation.
//!
//! The installed CLI's `codex exec --json` event stream (CLI 0.155.1 shape:
//! `thread.started`, `turn.started`, `item.started|updated|completed`,
//! `turn.completed`, `turn.failed`, `error`) is authoritative for identity
//! and state: `thread.started` supplies the exact native session, item events
//! supply messages and tool activity, and the turn events supply the terminal
//! state. `--output-last-message` supplies the returned result file.
//!
//! This module renders that stream onto the session's own visible terminal
//! surface, records the lifecycle in the dispatch receipt the dispatcher,
//! watcher and resume path already own, and turns the recorded state into
//! bounded review data. It never guesses an identity from rollout filenames,
//! never selects a session by recency and never inspects opaque reasoning
//! state.

use harness_core::process::{
    Cancellation, CommandSpec, Deadline, ExclusiveFileLock, Job, Limits, ProcessIdentity,
};
use harness_core::process_service::ServiceProcess;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs, io,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[path = "executor_cache.rs"]
pub(crate) mod cache;

/// Stop only the child tree owned by this host. Preserve the receipt, checkout
/// and rollout, and distinguish verified termination from incomplete cleanup.
pub(crate) fn stop_cache_run(
    receipt: &Path,
    tracker: &mut RunTracker,
    job: Job,
    loss: cache::CacheLoss,
) -> io::Result<i32> {
    let (state, cleanup) = match job.terminate(1, JOB_CLEANUP) {
        Ok(snapshot) if snapshot.active_processes == 0 => (
            STATE_STOPPED,
            "owned child tree terminated; no process remained".to_owned(),
        ),
        Ok(snapshot) => (
            STATE_PARTIAL_STOP,
            format!(
                "{} owned processes remain; use executor stop and inspect the recorded run",
                snapshot.active_processes
            ),
        ),
        Err(error) => (
            STATE_PARTIAL_STOP,
            format!(
                "termination could not be verified: {error}; use executor stop and inspect the recorded run"
            ),
        ),
    };
    let value = read_receipt_value(receipt)?;
    let quote = |text: &str| format!("'{}'", text.replace('\'', "''"));
    let recovery = match (value["slot"]["source"].as_str(), value["slot"]["index"].as_u64(),
        value["slot"]["owner"].as_str(), tracker.observation.session.as_deref()) {
        (Some(source), Some(slot), Some(owner), Some(session)) => {
            let home = harness_core::native_launcher::codex_home()?;
            let repository = value["slot"]["repository"].as_str().unwrap_or(source);
            let repository_flag = if source.eq_ignore_ascii_case(repository) {
                String::new()
            } else {
                format!(" --repository {}", quote(repository))
            };
            format!("Lead action required: review preserved partial work, then start a NEW session for the original assignment in the SAME worktree with `codex-harness executor restart --source {} --codex-home {} --slot {slot} --owner {} --session {}{repository_flag}`. Do not use spawn/release/reset or resume the expensive history; if this recurs, investigate before another restart", quote(source), quote(&home.to_string_lossy()), quote(owner), quote(session))
        }
        _ => "Lead action required: preserve partial work and start a new conversation in the same checkout for the original task; this receipt has no verified pool address for executor restart".to_owned(),
    };
    let cause = format!("{}; {cleanup}. {recovery}", loss.0);
    tracker.observation.state = state.into();
    tracker.observation.cause = Some(cause.clone());
    tracker.observation.exit_code = None;
    tracker.observation.updated_ms = now_ms();
    with_receipt_lock(receipt, || {
        let mut value = read_receipt_value(receipt)?;
        value["observation"] = serde_json::to_value(&tracker.observation)?;
        value["cacheGuard"] = loss.1;
        value["cacheGuard"]["status"] = Value::String(state.into());
        value["cacheGuard"]["reason"] = Value::String(cause.clone());
        value["cacheGuard"]["recovery"] = Value::String(recovery.clone());
        mark_pending_messages(&mut value, tracker.observation.updated_ms);
        write_receipt(receipt, &value)
    })?;
    println!("result: {state}: {cause}");
    println!(
        "session: {} receipt: {}",
        tracker
            .observation
            .session
            .as_deref()
            .unwrap_or("unrecorded"),
        receipt.display()
    );
    io::stdout().flush()?;
    Ok(EXIT_FAILED)
}

/// Version of the observation record written into a dispatch receipt.
pub(crate) const OBSERVATION_SCHEMA: u32 = 1;
/// Bound on one rendered message or tool line on the visible surface.
pub(crate) const MAX_RENDER_BYTES: usize = 16 * 1024;
/// Bound on the retained raw event stream beside the receipt.
pub(crate) const MAX_STREAM_BYTES: u64 = 4 * 1024 * 1024;
/// Bound on the returned result text `executor watch` prints.
pub(crate) const MAX_REVIEW_BYTES: usize = 4 * 1024;
/// Bound on one read of the final-message file.
pub(crate) const MAX_RESULT_READ: u64 = 16 * 1024;
/// Largest accepted event line; longer lines are dropped as oversize.
const MAX_EVENT_LINE: usize = 512 * 1024;
/// Bound on one text field of an unresolved request reference.
const MAX_REFERENCE_TEXT: usize = 120;
/// Bound on the child's stderr shown on the visible surface after a failure.
pub(crate) const MAX_STDERR_TAIL: usize = 2 * 1024;
/// Bound on how many unresolved request references one observation reports.
/// The reference list an action-required watch result prints stays small even
/// when a receipt holds more requests; the remainder is counted, not hidden.
pub(crate) const MAX_WAITING_REFERENCES: usize = 4;
/// Bound on how long the receipt writer lock waits for another writer.
const LOCK_WAIT: Duration = Duration::from_secs(10);
/// Poll interval of the spool tail while the owned child runs.
const TAIL_POLL: Duration = Duration::from_millis(25);
/// Bounded cleanup budget after the host terminates its owned launcher tree.
const JOB_CLEANUP: Duration = Duration::from_secs(10);

pub(crate) const STATE_ACCEPTED: &str = "dispatch-accepted";
pub(crate) const STATE_STARTED: &str = "native-start";
pub(crate) const STATE_RUNNING: &str = "running";
pub(crate) const STATE_COMPLETED: &str = "completed";
pub(crate) const STATE_FAILED: &str = "failed";
pub(crate) const STATE_DEFECT: &str = "defect";
pub(crate) const STATE_INTERRUPTED: &str = "interrupted";
pub(crate) const STATE_UNOBSERVED: &str = "unobserved";
/// Lifecycle state of a run whose stop was verified end to end.
pub(crate) const STATE_STOPPED: &str = "stopped";
/// Lifecycle state of a stop that verified termination but left a named
/// survivor or an unconfirmed recorded resource: a partial stop is not success.
pub(crate) const STATE_PARTIAL_STOP: &str = "partial-stop";

pub(crate) const COVERAGE_NATIVE: &str = "native";
pub(crate) const COVERAGE_UNAVAILABLE: &str = "unavailable";

/// Version of the stop record written under `stop` in a dispatch receipt.
pub(crate) const STOP_SCHEMA: u32 = 1;
/// Stop records use these outcomes; nothing else is claimed.
pub(crate) const STOP_STOPPED: &str = "stopped";
pub(crate) const STOP_ALREADY_STOPPED: &str = "already-stopped";
pub(crate) const STOP_ALREADY_COMPLETED: &str = "already-completed";
pub(crate) const STOP_PARTIAL: &str = "partial";
pub(crate) const STOP_ERROR: &str = "error";

/// Exit code of an observed run whose process failed or never reached a
/// completed turn while exiting successfully.
pub(crate) const EXIT_FAILED: i32 = 1;
/// Exit code of an observed run that completed but returned an empty or
/// missing final message: an output defect, not a model outage.
pub(crate) const EXIT_DEFECT: i32 = 3;

/// Identity of the process hosting one observed run. A pid alone is not an
/// identity: the creation time and image path must match too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostIdentity {
    pub pid: u32,
    pub created: u64,
    pub program: PathBuf,
}

/// Recorded lifecycle of one executor run, stored under `observation` in the
/// kit-local dispatch receipt (`spawn-<index>.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunObservation {
    #[serde(default)]
    pub schema: u32,
    /// `native` when the run executes `codex exec --json` with a recorded
    /// result file; `unavailable` when the mode exposes no such stream.
    #[serde(default)]
    pub coverage: String,
    /// Why coverage is unavailable (TUI runs, legacy receipts).
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub state: String,
    /// Exact native session id observed from `thread.started`.
    #[serde(default)]
    pub session: Option<String>,
    /// Session recorded before a resume attempt, kept so a failed resume
    /// cannot lose the exact identity of the interrupted conversation.
    #[serde(default)]
    pub previous_session: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub events: u64,
    #[serde(default)]
    pub messages: u64,
    #[serde(default)]
    pub tool_calls: u64,
    /// Completed interaction rounds (completed native turns) observed in this
    /// run: `turn.completed` on the exec route and the committed `completed`
    /// turn lifecycle on the control route. Zero means the observed stream
    /// recorded no completed turn. A receipt written before this counter
    /// existed carries no value, parses with the zero default here, and the
    /// comparison accounting reads the raw receipt and reports the round count
    /// unknown instead of zero.
    #[serde(default)]
    pub rounds: u64,
    /// Completed tool items grouped by operation kind (`commandExecution`,
    /// `mcpToolCall`, `webSearch`, `fileChange`). The values partition the
    /// aggregate [`Self::tool_calls`] counter: both count the same completed
    /// items, the aggregate first and this breakdown by operation. Item
    /// spellings from the exec and control routes normalize to the one
    /// vocabulary.
    #[serde(default)]
    pub tool_operation_counts: BTreeMap<String, u64>,
    #[serde(default)]
    pub malformed: u64,
    /// Failure, defect or interruption cause; named, never fabricated.
    #[serde(default)]
    pub cause: Option<String>,
    /// Identity of the host process that recorded this observation.
    #[serde(default)]
    pub host: Option<HostIdentity>,
    /// Final-message file the CLI writes through `--output-last-message`.
    #[serde(default)]
    pub result: Option<PathBuf>,
    /// Bounded raw event stream retained beside the receipt.
    #[serde(default)]
    pub detail: Option<PathBuf>,
    #[serde(default)]
    pub updated_ms: u64,
    /// Compact QPC facts. Retained after the raw detail file is full.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity: Vec<ActivityMark>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub detail_truncated: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub activity_truncated: bool,
    /// A mark could not be correlated. Not inactivity and not full coverage.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub activity_gap: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<ObservationClock>,
}

/// One host-observed activity boundary in the QPC domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActivityMark {
    pub id: String,
    pub kind: String,
    pub phase: String,
    pub qpc: u64,
    pub frequency: u64,
    pub boot: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_ticks: Option<u64>,
    /// `receipt`: `qpc` is when the host received the notification, an upper
    /// bound on the phase, not the work instant. Native item events carry no
    /// producer timestamp in the exec JSONL shape. App-server notifications
    /// carry `startedAtMs` / `completedAtMs` when the installed server emits
    /// them; those are recorded and are not invented from receipt time. Absent
    /// producer fields keep this receipt bound. It is not a poll interval and
    /// not a scheduling estimate.
    #[serde(default = "receipt_bound")]
    pub bound: String,
    /// Producer start from the notification, when that event carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<i64>,
    /// Producer completion from the notification, when that event carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<i64>,
}

fn receipt_bound() -> String {
    "receipt".to_owned()
}

/// Attempt window in the host's QPC domain. Receipt time is not this window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObservationClock {
    pub frequency: u64,
    pub boot: u64,
    pub started_qpc: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_qpc: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_span: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_span: Option<u64>,
    /// Wall sample paired with `started_qpc`. Absent on older receipts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_filetime: Option<u64>,
    /// Wall sample paired with `ended_qpc`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_filetime: Option<u64>,
}

impl RunObservation {
    /// A run whose process has not started yet.
    pub(crate) fn accepted(result: PathBuf, detail: PathBuf) -> Self {
        Self {
            schema: OBSERVATION_SCHEMA,
            coverage: COVERAGE_NATIVE.into(),
            reason: None,
            state: STATE_ACCEPTED.into(),
            session: None,
            previous_session: None,
            exit_code: None,
            events: 0,
            messages: 0,
            tool_calls: 0,
            rounds: 0,
            tool_operation_counts: BTreeMap::new(),
            malformed: 0,
            cause: None,
            host: None,
            result: Some(result),
            detail: Some(detail),
            updated_ms: now_ms(),
            activity: Vec::new(),
            detail_truncated: false,
            activity_truncated: false,
            activity_gap: false,
            clock: None,
        }
    }

    /// A TUI (or otherwise unobservable) run: the identity and result stay
    /// explicitly unavailable instead of being guessed.
    pub(crate) fn unavailable(reason: &str) -> Self {
        Self {
            schema: OBSERVATION_SCHEMA,
            coverage: COVERAGE_UNAVAILABLE.into(),
            reason: Some(reason.into()),
            state: STATE_UNOBSERVED.into(),
            session: None,
            previous_session: None,
            exit_code: None,
            events: 0,
            messages: 0,
            tool_calls: 0,
            rounds: 0,
            tool_operation_counts: BTreeMap::new(),
            malformed: 0,
            cause: None,
            host: None,
            result: None,
            detail: None,
            updated_ms: now_ms(),
            activity: Vec::new(),
            detail_truncated: false,
            activity_truncated: false,
            activity_gap: false,
            clock: None,
        }
    }

    /// The observation recorded in a receipt document, when it is complete
    /// enough to read. A receipt without one is a legacy receipt.
    pub(crate) fn from_receipt(value: &Value) -> Option<Self> {
        let observation = value.get("observation")?;
        if observation.is_null() {
            return None;
        }
        let parsed: Self = serde_json::from_value(observation.clone()).ok()?;
        if parsed.coverage.is_empty() || parsed.state.is_empty() {
            return None;
        }
        Some(parsed)
    }

    /// The exact session this run observed, falling back to the identity a
    /// resume attempt carried over when the current attempt never started.
    pub(crate) fn recorded_session(&self) -> Option<&str> {
        self.session.as_deref().or(self.previous_session.as_deref())
    }
}

/// One process or surface a stop could not confirm ended, named with the cause
/// and the supported next action instead of a false success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StopSurvivor {
    /// `process`, `terminal-tab` or `console-window`.
    pub kind: String,
    #[serde(default)]
    pub pid: Option<u32>,
    /// Windows FILETIME of the surviving process creation, so a later stop can
    /// re-verify the same process instead of trusting a reused pid.
    #[serde(default)]
    pub created: Option<u64>,
    #[serde(default)]
    pub image: Option<PathBuf>,
    /// Recorded surface identity for a terminal tab or console window.
    #[serde(default)]
    pub surface: Option<String>,
    pub cause: String,
    pub next_action: String,
}

/// Schema of a cleanup failure recorded beside, not inside, the assignment outcome.
pub(crate) const CLEANUP_SCHEMA: u32 = 1;

/// Recovery named with a cleanup failure. It is not a new assignment outcome.
pub(crate) const CLEANUP_RECOVERY: &str = "confirm each surviving owned process by its recorded pid and creation time and end it if it is still running; this cleanup failure does not change the recorded assignment outcome";

/// One owned frontend or backend resource a terminal cleanup could not confirm
/// ended. The assignment outcome stays in the observation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CleanupSurvivor {
    /// `frontend` or `backend`.
    pub kind: String,
    pub pid: u32,
    pub created: u64,
    pub image: PathBuf,
    pub cause: String,
    pub next_action: String,
}

/// A failed owned-surface cleanup. `closed` is false: this record never claims
/// that closure succeeded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CleanupRecord {
    pub schema: u32,
    pub closed: bool,
    pub survivors: Vec<CleanupSurvivor>,
    pub recovery: String,
}

/// Honest stop lifecycle of one executor run, written under `stop` in the
/// kit-local dispatch receipt beside the observation it explains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StopRecord {
    pub schema: u32,
    /// `stopped`, `already-stopped`, `already-completed`, `partial` or `error`.
    pub outcome: String,
    /// Unix milliseconds when the stop was requested and when its outcome was
    /// committed; `duration_ms` is the measured stop path.
    pub requested_ms: u64,
    pub completed_ms: u64,
    pub duration_ms: u64,
    /// The primary honest sentence: what was verified, refused or remains.
    pub detail: String,
    /// Exact host identity this stop verified, when one was recorded.
    #[serde(default)]
    pub host: Option<HostIdentity>,
    /// Native interruption result; `None` when the run has no control endpoint.
    #[serde(default)]
    pub interrupt: Option<String>,
    /// Recorded processes this stop verified gone.
    #[serde(default)]
    pub ended: u64,
    /// Survivors or unverified recorded resources with their next actions.
    #[serde(default)]
    pub survivors: Vec<StopSurvivor>,
    /// Terminal-surface verification text.
    #[serde(default)]
    pub surface: Option<String>,
    /// Queued message ids this stop marked undelivered.
    #[serde(default)]
    pub undelivered: Vec<String>,
    /// How many later stop requests merely reported this recorded outcome; the
    /// first stop's timestamps and duration are preserved across repeats.
    #[serde(default)]
    pub repeats: u64,
    /// Unix milliseconds of the last repeated stop request, when there was one.
    #[serde(default)]
    pub repeated_ms: Option<u64>,
    /// Exit code of the stopped run when one was actually observed. An exit
    /// code that was never observed stays absent, never fabricated.
    #[serde(default)]
    pub exit_code: Option<i32>,
}

/// The lifecycle transition one stop asks its receipt to record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopTransition {
    /// This invocation verified the run stopped: state `stopped`.
    Stopped,
    /// Termination happened but a recorded resource remains: `partial-stop`.
    Partial,
    /// The host ended without a terminal record: `interrupted` with the
    /// unknown-exit-code cause, and no stop claim.
    Unobserved,
    /// This invocation changed nothing (refusal, repeat, completed run).
    Keep,
}

/// Result of committing one stop record under the receipt lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StopCommit {
    /// Effective outcome after the commit-time race check.
    pub outcome: String,
    /// Effective lifecycle state after the commit.
    pub state: String,
    /// The recorded exit code, present only when one was observed.
    pub exit_code: Option<i32>,
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// One parsed event of the native stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NativeEvent {
    ThreadStarted {
        thread_id: String,
    },
    /// `turn_id` is present when the stream names the turn. An id-less event is
    /// the legacy exec JSON shape and belongs to the current turn.
    TurnStarted {
        turn_id: Option<String>,
    },
    TurnCompleted {
        turn_id: Option<String>,
    },
    TurnFailed {
        turn_id: Option<String>,
        message: String,
    },
    Item {
        phase: ItemPhase,
        item: ItemSummary,
    },
    Error {
        message: String,
    },
    Unknown {
        kind: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemPhase {
    Started,
    Updated,
    Completed,
}

/// The fields this build consumes from an item event. Unknown fields are
/// ignored, so a newer stream keeps working for the properties we report.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ItemSummary {
    pub id: Option<String>,
    pub kind: String,
    pub status: Option<String>,
    pub text: Option<String>,
    pub command: Option<String>,
    pub exit_code: Option<i64>,
    pub server: Option<String>,
    pub tool: Option<String>,
    pub query: Option<String>,
    pub changes: Option<usize>,
    pub plan_items: Option<usize>,
}

/// Turn identity carried by an exec JSON event, when the stream names one.
/// Absence is the legacy shape, not a missing current turn.
fn event_turn_id(value: &Value) -> Option<String> {
    value
        .get("turn_id")
        .and_then(Value::as_str)
        .or_else(|| value.get("turnId").and_then(Value::as_str))
        .or_else(|| {
            value
                .get("turn")
                .and_then(|turn| turn.get("id"))
                .and_then(Value::as_str)
        })
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// Parses one JSONL event. The error text is a bounded, human-readable
/// description of the unparsed line; the caller counts and surfaces it.
pub(crate) fn parse_event(line: &str) -> Result<NativeEvent, String> {
    let value: Value = serde_json::from_str(line).map_err(|error| {
        format!(
            "not a JSON event: {error} (line begins {})",
            excerpt(line, 160)
        )
    })?;
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("event without a type: {}", excerpt(line, 160)))?;
    Ok(match kind {
        "thread.started" => NativeEvent::ThreadStarted {
            thread_id: value
                .get("thread_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "thread.started without thread_id".to_owned())?
                .to_owned(),
        },
        "turn.started" => NativeEvent::TurnStarted {
            turn_id: event_turn_id(&value),
        },
        "turn.completed" => NativeEvent::TurnCompleted {
            turn_id: event_turn_id(&value),
        },
        "turn.failed" => NativeEvent::TurnFailed {
            turn_id: event_turn_id(&value),
            message: value["error"]["message"]
                .as_str()
                .unwrap_or("turn failed without a message")
                .to_owned(),
        },
        "error" => NativeEvent::Error {
            message: value["message"]
                .as_str()
                .unwrap_or("event stream error without a message")
                .to_owned(),
        },
        "item.started" | "item.updated" | "item.completed" => {
            let phase = match kind {
                "item.started" => ItemPhase::Started,
                "item.updated" => ItemPhase::Updated,
                _ => ItemPhase::Completed,
            };
            let item = &value["item"];
            let item_kind = item
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{kind} without an item type"))?;
            NativeEvent::Item {
                phase,
                item: ItemSummary {
                    id: item.get("id").and_then(Value::as_str).map(str::to_owned),
                    kind: item_kind.to_owned(),
                    status: item
                        .get("status")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    text: item.get("text").and_then(Value::as_str).map(str::to_owned),
                    command: item
                        .get("command")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    exit_code: item.get("exit_code").and_then(Value::as_i64),
                    server: item
                        .get("server")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    tool: item.get("tool").and_then(Value::as_str).map(str::to_owned),
                    query: item.get("query").and_then(Value::as_str).map(str::to_owned),
                    changes: item.get("changes").and_then(Value::as_array).map(Vec::len),
                    plan_items: item.get("items").and_then(Value::as_array).map(Vec::len),
                },
            }
        }
        other => NativeEvent::Unknown {
            kind: other.to_owned(),
        },
    })
}

/// Renders one event readably for the session's own visible surface: the
/// assignment header is printed by the caller, then messages, tool activity
/// and lifecycle lines arrive here while the turn runs.
pub(crate) fn render_event(event: &NativeEvent, out: &mut dyn Write) -> io::Result<()> {
    let line = match event {
        NativeEvent::ThreadStarted { thread_id } => {
            Some(format!("state: native start (session {thread_id})"))
        }
        NativeEvent::TurnStarted { .. } => Some("state: turn started".to_owned()),
        NativeEvent::TurnCompleted { .. } => Some("state: turn completed".to_owned()),
        NativeEvent::TurnFailed { message, .. } => Some(format!("turn failed: {message}")),
        NativeEvent::Error { message } => Some(format!("error: {message}")),
        NativeEvent::Unknown { kind } => Some(format!("event: {kind}")),
        NativeEvent::Item { phase, item } => match item.kind.as_str() {
            // Reasoning summaries are intentionally not rendered: the run's
            // readable surface carries decisions and activity, not opaque
            // model state.
            "reasoning" => None,
            "agent_message" => match phase {
                ItemPhase::Completed => item
                    .text
                    .as_deref()
                    .map(|text| format!("\nassistant:\n{}\n", excerpt(text, MAX_RENDER_BYTES))),
                ItemPhase::Started | ItemPhase::Updated => None,
            },
            "command_execution" => {
                let command = excerpt(item.command.as_deref().unwrap_or(""), 400);
                match phase {
                    ItemPhase::Started => Some(format!("tool: exec {command}")),
                    ItemPhase::Completed => Some(format!(
                        "tool: exec {} -> {}{}",
                        command,
                        item.status.as_deref().unwrap_or("completed"),
                        item.exit_code
                            .map(|code| format!(" (exit {code})"))
                            .unwrap_or_default()
                    )),
                    ItemPhase::Updated => None,
                }
            }
            "file_change" => match phase {
                ItemPhase::Completed => Some(format!(
                    "files: {} changed ({})",
                    item.changes.unwrap_or(0),
                    item.status.as_deref().unwrap_or("completed")
                )),
                ItemPhase::Started | ItemPhase::Updated => None,
            },
            "mcp_tool_call" => match phase {
                ItemPhase::Completed => Some(format!(
                    "tool: mcp {}/{} -> {}",
                    item.server.as_deref().unwrap_or("?"),
                    item.tool.as_deref().unwrap_or("?"),
                    item.status.as_deref().unwrap_or("completed")
                )),
                ItemPhase::Started => Some(format!(
                    "tool: mcp {}/{}",
                    item.server.as_deref().unwrap_or("?"),
                    item.tool.as_deref().unwrap_or("?")
                )),
                ItemPhase::Updated => None,
            },
            "web_search" => match phase {
                ItemPhase::Started | ItemPhase::Completed => Some(format!(
                    "tool: web search \"{}\"",
                    excerpt(item.query.as_deref().unwrap_or(""), 200)
                )),
                ItemPhase::Updated => None,
            },
            "todo_list" => match phase {
                ItemPhase::Started | ItemPhase::Completed => {
                    Some(format!("plan: {} items", item.plan_items.unwrap_or(0)))
                }
                ItemPhase::Updated => None,
            },
            "error" => item
                .text
                .as_deref()
                .map(|text| format!("item error: {}", excerpt(text, 400))),
            other => match phase {
                ItemPhase::Completed => Some(format!(
                    "item: {other}{}",
                    item.status
                        .as_deref()
                        .map(|status| format!(" ({status})"))
                        .unwrap_or_default()
                )),
                ItemPhase::Started | ItemPhase::Updated => None,
            },
        },
    };
    if let Some(line) = line {
        writeln!(out, "{line}")?;
        out.flush()?;
    }
    Ok(())
}

/// Live tracker of one observed run: applies parsed events to the recorded
/// observation and keeps the transient facts the terminal state needs.
pub(crate) struct RunTracker {
    pub observation: RunObservation,
    saw_thread: bool,
    /// The latest turn this stream accepted. A terminal event for another id
    /// is history and cannot finish the current turn.
    accepted_turn: Option<String>,
    saw_turn_completed: bool,
    /// Stream-level error. A later turn does not erase it.
    failure: Option<String>,
    /// Failure of the current turn. A newer `turn.started` clears it so a
    /// resumed session is not finished by the predecessor's failure.
    turn_failure: Option<String>,
    oversize: u64,
}

/// One completed tool item's operation kind in the receipt's single camelCase
/// vocabulary. The exec route spells items `command_execution` and the control
/// route `commandExecution`; both normalize to the same operation so the
/// per-operation counters stay comparable across routes.
fn tool_operation_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "command_execution" | "commandExecution" | "CommandExecution" => Some("commandExecution"),
        "mcp_tool_call" | "mcpToolCall" | "McpToolCall" => Some("mcpToolCall"),
        "web_search" | "webSearch" | "WebSearch" => Some("webSearch"),
        "file_change" | "fileChange" | "FileChange" => Some("fileChange"),
        _ => None,
    }
}

impl RunTracker {
    pub(crate) fn new(mut observation: RunObservation) -> Self {
        observation.state = STATE_ACCEPTED.into();
        observation.updated_ms = now_ms();
        Self {
            observation,
            saw_thread: false,
            accepted_turn: None,
            saw_turn_completed: false,
            failure: None,
            turn_failure: None,
            oversize: 0,
        }
    }

    /// Applies one parsed event. Returns true when the recorded observation
    /// changed and should be persisted again.
    pub(crate) fn apply(&mut self, event: &NativeEvent) -> bool {
        let before = self.observation.clone();
        self.observation.events += 1;
        match event {
            NativeEvent::ThreadStarted { thread_id } => {
                self.saw_thread = true;
                self.observation.session = Some(thread_id.clone());
                self.observation.state = STATE_STARTED.into();
            }
            NativeEvent::TurnStarted { turn_id } => {
                if self.saw_thread {
                    self.observation.state = STATE_RUNNING.into();
                }
                // A newer turn means the previous terminal event is not this
                // run's outcome. An id-less start is the legacy boundary.
                let changed = match turn_id {
                    Some(id) => self.accepted_turn.as_deref() != Some(id.as_str()),
                    None => true,
                };
                if changed {
                    self.accepted_turn = turn_id.clone();
                    self.saw_turn_completed = false;
                    self.turn_failure = None;
                }
            }
            NativeEvent::TurnCompleted { turn_id } => {
                self.observation.rounds = self.observation.rounds.saturating_add(1);
                if self.terminal_matches(turn_id.as_deref()) {
                    self.saw_turn_completed = true;
                    self.observation.state = STATE_RUNNING.into();
                }
            }
            NativeEvent::TurnFailed { turn_id, message } => {
                if self.terminal_matches(turn_id.as_deref()) {
                    self.turn_failure = Some(message.clone());
                }
            }
            NativeEvent::Error { message } => {
                self.failure = Some(message.clone());
            }
            NativeEvent::Item { phase, item } => {
                if self.saw_thread && self.observation.state == STATE_STARTED {
                    self.observation.state = STATE_RUNNING.into();
                }
                if *phase == ItemPhase::Completed {
                    if item.kind == "agent_message" {
                        self.observation.messages = self.observation.messages.saturating_add(1);
                    } else if let Some(operation) = tool_operation_kind(&item.kind) {
                        self.count_tool_operation(operation);
                    }
                }
            }
            NativeEvent::Unknown { .. } => {}
        }
        self.observation.updated_ms = now_ms();
        self.observation != before
    }

    /// A terminal event finishes the current turn only. Matching ids are
    /// required when both sides name a turn; an id-less legacy event belongs
    /// to whatever turn is current after the latest `turn.started`.
    fn terminal_matches(&self, turn_id: Option<&str>) -> bool {
        match (self.accepted_turn.as_deref(), turn_id) {
            (Some(accepted), Some(id)) => accepted == id,
            _ => true,
        }
    }

    pub(crate) fn count_malformed(&mut self) -> bool {
        self.observation.malformed += 1;
        self.observation.updated_ms = now_ms();
        true
    }

    /// Counts one completed tool item in both the aggregate counter and the
    /// per-operation breakdown, so the breakdown always partitions the
    /// aggregate.
    fn count_tool_operation(&mut self, operation: &'static str) {
        self.observation.tool_calls = self.observation.tool_calls.saturating_add(1);
        let count = self
            .observation
            .tool_operation_counts
            .entry(operation.to_owned())
            .or_default();
        *count = count.saturating_add(1);
    }

    pub(crate) fn count_oversize(&mut self) {
        self.oversize += 1;
        self.observation.malformed += 1;
    }

    /// Records that the observer itself failed and terminated the run: the
    /// state is a named failure with the real cause, never a success.
    pub(crate) fn observer_failed(&mut self, cause: String) {
        self.session_failed(cause);
    }

    /// Records that this run never reached a conversation: startup failed
    /// closed with the named cause, so no session identity and no exit code
    /// exist and nothing here reports progress.
    pub(crate) fn session_failed(&mut self, cause: String) {
        self.observation.state = STATE_FAILED.into();
        self.observation.exit_code = None;
        self.observation.cause = Some(cause);
        self.observation.updated_ms = now_ms();
    }

    /// Records that the only frontend exited during active work. Session
    /// identity, the result locator and partial files stay. This is an
    /// unsuccessful outcome, never a successful exit.
    pub(crate) fn session_interrupted(&mut self, cause: String) {
        self.observation.state = STATE_INTERRUPTED.into();
        self.observation.exit_code = None;
        self.observation.cause = Some(cause);
        self.observation.updated_ms = now_ms();
    }

    /// Applies one control-backed record: the receipt state the record
    /// establishes (when it establishes one), the activity of a completed
    /// item, and the exact native thread identity. Returns true when the
    /// recorded observation changed and must be persisted again.
    pub(crate) fn apply_control(
        &mut self,
        state: Option<&str>,
        completed_item: Option<&str>,
        thread: &str,
    ) -> bool {
        let before = self.observation.clone();
        self.observation.events += 1;
        if !thread.is_empty() {
            self.saw_thread = true;
            self.observation.session = Some(thread.to_owned());
        }
        if let Some(state) = state {
            // The control driver commits this state once per completed turn and
            // stamps it on that turn's own event, so one observation is one
            // completed interaction round.
            if state == STATE_COMPLETED {
                self.observation.rounds = self.observation.rounds.saturating_add(1);
            }
            self.observation.state = state.to_owned();
        }
        if let Some(kind) = completed_item {
            if kind == "agentMessage" {
                self.observation.messages = self.observation.messages.saturating_add(1);
            } else if let Some(operation) = tool_operation_kind(kind) {
                self.count_tool_operation(operation);
            }
        }
        self.observation.updated_ms = now_ms();
        self.observation != before
    }

    /// Records the terminal outcome of a control-backed run and returns the
    /// exit code the host must return.
    ///
    /// The state is the one the control driver established from the thread's
    /// own records, and the outcome follows the same rules as [`Self::finish`]:
    /// a completed turn with a nonempty final message is the only success, an
    /// empty or missing message is an output defect, and a failed, interrupted
    /// or deviating run is never reported as a completed one.
    pub(crate) fn finish_control(&mut self, outcome: &ControlOutcome<'_>) -> i32 {
        end_clock(&mut self.observation);
        let (state, cause, exit) = match (outcome.state, outcome.final_message) {
            (STATE_COMPLETED, Some(FinalMessage::Present)) => (STATE_COMPLETED, None, 0),
            (STATE_COMPLETED, Some(FinalMessage::Empty)) => (
                STATE_DEFECT,
                Some(
                    "the final message is empty; an empty completion is an output defect, not evidence of model, authentication or quota unavailability"
                        .to_owned(),
                ),
                EXIT_DEFECT,
            ),
            (STATE_COMPLETED, _) => (
                STATE_DEFECT,
                Some(
                    outcome.cause.map(str::to_owned).unwrap_or_else(|| {
                        "the completed turn carries no final assistant message in the thread items"
                            .to_owned()
                    }),
                ),
                EXIT_DEFECT,
            ),
            (STATE_INTERRUPTED, _) => (
                STATE_INTERRUPTED,
                Some(
                    outcome
                        .cause
                        .unwrap_or(
                            "the native turn was interrupted; the run has no completed result",
                        )
                        .to_owned(),
                ),
                EXIT_FAILED,
            ),
            (STATE_DEFECT, _) => (
                STATE_DEFECT,
                Some(
                    outcome
                        .cause
                        .unwrap_or("a protocol deviation left the run's state unknown")
                        .to_owned(),
                ),
                EXIT_DEFECT,
            ),
            _ => (
                STATE_FAILED,
                Some(
                    outcome
                        .cause
                        .unwrap_or("the conversation ended without a completed turn")
                        .to_owned(),
                ),
                EXIT_FAILED,
            ),
        };
        self.observation.state = state.into();
        self.observation.cause = cause;
        self.observation.exit_code = Some(exit);
        self.observation.updated_ms = now_ms();
        exit
    }

    /// Derives the terminal state from the stream and the child's exit code
    /// and final-message file. Returns the exit code the host must return.
    pub(crate) fn finish(&mut self, exit_code: i32, result: Option<&Path>) -> i32 {
        end_clock(&mut self.observation);
        let mut cause = None;
        if exit_code != 0 {
            self.observation.state = STATE_FAILED.into();
            cause = Some(if self.saw_thread {
                format!("the launcher exited with code {exit_code}")
            } else {
                format!(
                    "the launcher exited with code {exit_code} before any native event; native start was never observed"
                )
            });
            if let Some(failure) = &self.failure {
                cause = Some(format!("{failure}; {}", cause.unwrap_or_default()));
            }
        } else if let Some(failure) = self.turn_failure.clone().or_else(|| self.failure.clone()) {
            self.observation.state = STATE_FAILED.into();
            cause = Some(failure);
        } else if !self.saw_thread {
            self.observation.state = STATE_FAILED.into();
            cause = Some(
                "the event stream ended without native thread identity (thread.started); the session never started"
                    .into(),
            );
        } else if !self.saw_turn_completed {
            self.observation.state = STATE_FAILED.into();
            cause = Some(
                "the event stream ended without turn.completed; the run was interrupted or the stream was truncated"
                    .into(),
            );
        } else {
            match final_message(result) {
                FinalMessage::Present => {
                    self.observation.state = STATE_COMPLETED.into();
                }
                FinalMessage::Empty => {
                    self.observation.state = STATE_DEFECT.into();
                    cause = Some(
                        "the final message is empty; an empty completion is an output defect, not evidence of model, authentication or quota unavailability"
                            .into(),
                    );
                }
                FinalMessage::Missing => {
                    self.observation.state = STATE_DEFECT.into();
                    cause = Some(
                        "turn.completed arrived but the CLI wrote no final-message file (--output-last-message)"
                            .into(),
                    );
                }
            }
        }
        if self.oversize > 0 {
            let note = format!(
                "{} oversize event lines were dropped from the stream",
                self.oversize
            );
            cause = Some(match cause {
                Some(cause) => format!("{cause}; {note}"),
                None => note,
            });
        }
        self.observation.exit_code = Some(exit_code);
        self.observation.cause = cause;
        self.observation.updated_ms = now_ms();
        if exit_code != 0 {
            return exit_code;
        }
        match self.observation.state.as_str() {
            STATE_COMPLETED => 0,
            STATE_DEFECT => EXIT_DEFECT,
            _ => EXIT_FAILED,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalMessage {
    Present,
    Empty,
    Missing,
}

/// The terminal outcome of one control-backed run, as its own driver
/// established it: the lifecycle state mapped onto the receipt vocabulary, the
/// final-message classification of the thread's own items (present only when a
/// turn completed) and the first recorded failure or protocol deviation.
pub(crate) struct ControlOutcome<'a> {
    pub state: &'a str,
    pub final_message: Option<FinalMessage>,
    pub cause: Option<&'a str>,
}

/// Writes the final message a control-backed run returned to its recorded
/// result locator, so `executor watch`, the receipt review and the resume
/// remedy read this run exactly as they read a JSONL-backed one. The write is
/// staged against a distinct temp name, so a concurrent reader never observes
/// a partial message and a failed write leaves the previous file untouched.
pub(crate) fn write_final_message(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, text)?;
    match fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// Classifies the final-message file without reading more than a bounded
/// prefix: presence, emptiness and the actual returned text matter here.
pub(crate) fn final_message(path: Option<&Path>) -> FinalMessage {
    let Some(path) = path else {
        return FinalMessage::Missing;
    };
    match fs::metadata(path) {
        Ok(metadata) if metadata.len() == 0 => FinalMessage::Empty,
        Ok(_) => match read_bounded(path, MAX_RESULT_READ) {
            Ok(bytes) if String::from_utf8_lossy(&bytes).trim().is_empty() => FinalMessage::Empty,
            Ok(_) => FinalMessage::Present,
            Err(_) => FinalMessage::Missing,
        },
        Err(_) => FinalMessage::Missing,
    }
}

/// Reads at most `limit` bytes of a file.
pub(crate) fn read_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    io::Read::take(&mut file, limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The host process identity of this run. Recorded before the launcher starts
/// so a watcher can distinguish a live run from an interrupted host without
/// trusting a bare pid.
pub(crate) fn host_identity() -> io::Result<HostIdentity> {
    let program = std::env::current_exe()?;
    let user = harness_core::process_service::current_user()?;
    let identity = ServiceProcess::observe(std::process::id(), &program, 0, &user)?.identity();
    Ok(HostIdentity {
        pid: identity.pid,
        created: identity.creation_time,
        program,
    })
}

/// True when the exact recorded host process is gone: its image is missing or
/// its pid/creation-time identity no longer runs. Unverifiable identity is
/// treated as still running, so a watcher waits instead of guessing.
pub(crate) fn host_ended(host: &HostIdentity) -> bool {
    if !host.program.is_file() {
        return true;
    }
    let Ok(user) = harness_core::process_service::current_user() else {
        return false;
    };
    match ServiceProcess::inspect(
        ProcessIdentity {
            pid: host.pid,
            creation_time: host.created,
        },
        &host.program,
        &user,
    ) {
        Ok(Some(_)) => false,
        Ok(None) => true,
        Err(_) => false,
    }
}

/// Messaging records an unresolved required reply on the dispatch receipt.
/// A native turn that ends while one remains is not a completed run and must
/// not be closed as an empty-output defect. Absence of both records is no hold.
///
/// `replyRequests` retains the run when `status` is `unresolved`, it is not a
/// notification (`kind` of `notify`), and `requiresReply` is not false.
/// `leadMessages` is what `lead message` writes: a `reply-request` that was not
/// refused and not resolved is the same hold. A notification does not hold.
/// This reader keeps the base lifecycle from treating the hold as completion,
/// and it is the one reader behind the watch action-required result. The
/// control driver keeps an equivalent reader because that file is also
/// compiled alone by its fixture tests.
pub(crate) fn unresolved_reply_hold(receipt: &Value) -> bool {
    !unresolved_reply_references(receipt, 1).0.is_empty()
}

/// One unresolved reply request as an observation surface reports it: which
/// hold record carries it, the opaque reference that `executor message
/// --reply-to` consumes, and the recorded sender and run it belongs to. Every
/// text field is bounded, so a hand-edited receipt cannot grow model-visible
/// output, and a missing field is named as unrecorded instead of invented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplyReference {
    /// `replyRequests` or `leadMessages`: the record holding the request.
    pub record: &'static str,
    pub id: String,
    pub kind: String,
    pub status: String,
    /// Native thread of the lead being asked, when the record carries one.
    pub lead_thread: Option<String>,
    /// Native session of the executor run the request was recorded against.
    pub session: Option<String>,
    /// When the request's own record names a spill, the absolute harness-owned
    /// file holding its complete payload and that payload's exact size, so the
    /// watch surface reports a spilled delivery as one.
    pub payload_path: Option<String>,
    pub payload_bytes: Option<u64>,
}

/// The unresolved reply requests of a receipt in record order, bounded by
/// `limit`. The second value counts the references left out of the list, so a
/// bounded list never looks like the whole record. A notification, a refused
/// send and a resolved reply are not requests.
pub(crate) fn unresolved_reply_references(
    receipt: &Value,
    limit: usize,
) -> (Vec<ReplyReference>, usize) {
    let mut found = Vec::new();
    if let Some(requests) = receipt.get("replyRequests").and_then(Value::as_array) {
        for request in requests {
            let holds = request.get("status").and_then(Value::as_str) == Some("unresolved")
                && request.get("kind").and_then(Value::as_str) != Some("notify")
                && request.get("requiresReply").and_then(Value::as_bool) != Some(false);
            if holds {
                found.push(reply_reference("replyRequests", request));
            }
        }
    }
    if let Some(messages) = receipt.get("leadMessages").and_then(Value::as_array) {
        for message in messages {
            let holds = message.get("kind").and_then(Value::as_str) == Some("reply-request")
                && message.get("requiresReply").and_then(Value::as_bool) != Some(false)
                && !matches!(
                    message.get("status").and_then(Value::as_str),
                    Some("resolved" | "error" | "refused")
                );
            if holds {
                found.push(reply_reference("leadMessages", message));
            }
        }
    }
    let omitted = found.len().saturating_sub(limit);
    found.truncate(limit);
    (found, omitted)
}

/// Bounded view of one holding record: the reply reference and the recorded
/// identity, with untrustworthy text clipped and nothing invented.
fn reply_reference(record: &'static str, entry: &Value) -> ReplyReference {
    let text = |name: &str| {
        entry
            .get(name)
            .and_then(Value::as_str)
            .map(|value| excerpt(value, MAX_REFERENCE_TEXT))
    };
    ReplyReference {
        record,
        id: text("id").unwrap_or_else(|| "unknown".into()),
        kind: text("kind").unwrap_or_else(|| "reply-request".into()),
        status: text("status").unwrap_or_else(|| "unrecorded".into()),
        lead_thread: text("leadThreadId"),
        session: text("session"),
        payload_path: text("payloadPath"),
        payload_bytes: entry.get("payloadBytes").and_then(Value::as_u64),
    }
}

/// Records one input that arrived after closure began. The same id is recorded
/// once. Delivered entries are not rewritten. The existing `messages` field is
/// the report; this does not add a second queue. The control driver records
/// the live path; this copy keeps the receipt contract testable here.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn record_undelivered_input(receipt: &Path, id: &str, detail: &str) -> io::Result<()> {
    if id.is_empty() {
        return Ok(());
    }
    with_receipt_lock(receipt, || {
        let mut value = read_receipt_value(receipt)?;
        let messages = value
            .as_object_mut()
            .ok_or_else(|| io::Error::other("dispatch receipt is not an object"))?
            .entry("messages")
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(messages) = messages.as_array_mut() else {
            return Err(io::Error::other(
                "dispatch receipt messages field is not an array",
            ));
        };
        if messages
            .iter()
            .any(|message| message.get("id").and_then(Value::as_str) == Some(id))
        {
            return Ok(());
        }
        messages.push(json!({
            "schema": 1,
            "id": id,
            "status": "undelivered",
            "undeliveredMs": now_ms(),
            "detail": detail,
        }));
        write_receipt(receipt, &value)
    })
}

/// Updates only the `observation` field of the receipt, preserving every
/// other dispatch input. The read/modify/write runs under the kit-local
/// receipt lock and the replacement is atomic, so the console dispatcher's
/// `window` write and the host's lifecycle writes cannot lose each other.
pub(crate) fn update_receipt(receipt: &Path, observation: &RunObservation) -> io::Result<()> {
    with_receipt_lock(receipt, || {
        let mut value = read_receipt_value(receipt)?;
        value["observation"] = serde_json::to_value(observation).map_err(io::Error::other)?;
        write_receipt(receipt, &value)
    })
}

/// Replaces one top-level receipt field under the same lock, preserving the
/// rest of the document (including a lifecycle the host wrote meanwhile).
pub(crate) fn update_receipt_field(receipt: &Path, name: &str, field: Value) -> io::Result<()> {
    with_receipt_lock(receipt, || {
        let mut value = read_receipt_value(receipt)?;
        value[name] = field;
        write_receipt(receipt, &value)
    })
}

/// Records a cleanup failure beside the assignment outcome. The observation
/// state and exit code are left untouched.
pub(crate) fn record_cleanup(receipt: &Path, record: &CleanupRecord) -> io::Result<()> {
    let field = serde_json::to_value(record).map_err(|error| {
        io::Error::other(format!(
            "cleanup failure record is not serializable: {error}"
        ))
    })?;
    update_receipt_field(receipt, "cleanup", field)
}

/// Writes a complete receipt document under the same lock: the dispatcher's
/// initial write replaces the record of any previous run without racing a
/// still-finishing host.
pub(crate) fn write_receipt_document(receipt: &Path, value: &Value) -> io::Result<()> {
    with_receipt_lock(receipt, || write_receipt(receipt, value))
}

/// Commits one stop outcome under the receipt lock. The lifecycle is re-read at
/// commit time, so a stop racing natural completion reports the completed
/// result instead of recording a stop that did not happen; queued messages are
/// marked undelivered; an exit code that was never observed stays unknown; and
/// the stop itself releases, resets and completes nothing.
pub(crate) fn commit_stop(
    receipt: &Path,
    stop: &mut StopRecord,
    transition: StopTransition,
) -> io::Result<StopCommit> {
    with_receipt_lock(receipt, || {
        let mut value = read_receipt_value(receipt)?;
        let mut recorded = RunObservation::from_receipt(&value);
        // The race with natural completion is resolved here, at commit time: a
        // run that completed while the stop was verifying keeps its recorded
        // result, and the stop reports that the completion won.
        if matches!(stop.outcome.as_str(), STOP_STOPPED | STOP_PARTIAL)
            && let Some(run) = &recorded
            && run.state == STATE_COMPLETED
        {
            stop.outcome = STOP_ALREADY_COMPLETED.into();
            stop.survivors.clear();
            stop.detail = format!(
                "the run completed while stop was verifying, so its recorded result stands and no stop was recorded; {}",
                stop.detail
            );
        }
        stop.undelivered = mark_pending_messages(&mut value, stop.completed_ms);
        if let Some(run) = &mut recorded {
            match stop.outcome.as_str() {
                STOP_STOPPED => run.state = STATE_STOPPED.into(),
                STOP_PARTIAL => run.state = STATE_PARTIAL_STOP.into(),
                _ => {}
            }
            if transition == StopTransition::Unobserved && run.state != STATE_COMPLETED {
                run.state = STATE_INTERRUPTED.into();
                let cause = format!("{}; the exact exit code is unknown", stop.detail);
                run.cause = Some(merge_cause(run.cause.take(), &cause));
                run.exit_code = None;
            }
            if matches!(stop.outcome.as_str(), STOP_STOPPED | STOP_PARTIAL) {
                run.cause = Some(merge_cause(run.cause.take(), &stop.detail));
            }
            run.updated_ms = now_ms();
            value["observation"] = serde_json::to_value(&*run).map_err(io::Error::other)?;
        }
        stop.exit_code = recorded.as_ref().and_then(|run| run.exit_code);
        value["stop"] = serde_json::to_value(&*stop).map_err(io::Error::other)?;
        write_receipt(receipt, &value)?;
        Ok(StopCommit {
            outcome: stop.outcome.clone(),
            state: recorded.map(|run| run.state).unwrap_or_default(),
            exit_code: stop.exit_code,
        })
    })
}

/// Commits frontend loss unless a terminal result is already retained.
///
/// The receipt is re-read under its lock, so a completion recorded while the
/// frontend was closing keeps that result. Frontend exit never manufactures
/// success. Returns true when the retained result was left unchanged.
pub(crate) fn commit_frontend_loss(
    receipt: &Path,
    tracker: &mut RunTracker,
    cause: String,
) -> io::Result<bool> {
    with_receipt_lock(receipt, || {
        let mut value = read_receipt_value(receipt)?;
        if let Some(retained) = RunObservation::from_receipt(&value)
            && retained_terminal_result(&retained)
        {
            tracker.observation = retained;
            return Ok(true);
        }
        tracker.session_interrupted(cause);
        value["observation"] =
            serde_json::to_value(&tracker.observation).map_err(io::Error::other)?;
        write_receipt(receipt, &value)?;
        Ok(false)
    })
}

/// A settled terminal outcome. A completed run counts only after its exit code
/// was recorded, so a bare in-progress state cannot be promoted by frontend exit.
fn retained_terminal_result(run: &RunObservation) -> bool {
    match run.state.as_str() {
        STATE_COMPLETED => run.exit_code == Some(0),
        STATE_FAILED | STATE_DEFECT | STATE_INTERRUPTED | STATE_STOPPED | STATE_PARTIAL_STOP => {
            run.exit_code.is_some()
        }
        _ => false,
    }
}

/// Adds one cause sentence without repeating text a repeated stop already
/// recorded, so a repeated request cannot grow the record without limit.
fn merge_cause(previous: Option<String>, next: &str) -> String {
    match previous {
        Some(previous) if previous.contains(next) => previous,
        Some(previous) if !previous.is_empty() => format!("{previous}; {next}"),
        _ => next.to_owned(),
    }
}

/// Stops further delivery of queued messages to a run and records their
/// undelivered state honestly: entries the message path queued without a
/// confirmed delivery become `undelivered`, while delivered and errored
/// entries are never rewritten.
fn mark_pending_messages(value: &mut Value, stop_ms: u64) -> Vec<String> {
    let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) else {
        return Vec::new();
    };
    let mut marked = Vec::new();
    for message in messages {
        let status = message.get("status").and_then(Value::as_str);
        if !matches!(status, Some("queued") | Some("pending")) {
            continue;
        }
        let id = message
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("unnamed")
            .to_owned();
        message["status"] = Value::String("undelivered".into());
        message["undeliveredMs"] = Value::from(stop_ms);
        marked.push(id);
    }
    marked
}

fn read_receipt_value(receipt: &Path) -> io::Result<Value> {
    let bytes = fs::read(receipt)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn write_receipt(receipt: &Path, value: &Value) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    // A fresh temp name per writer keeps a stale leftover from a killed writer
    // from being replaced under an unrelated rename.
    let temp = receipt.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, bytes)?;
    fs::rename(&temp, receipt)
}

/// Serializes one receipt's writers with the kit's stable OS file lock: the
/// console dispatcher's window write and the host's lifecycle writes preserve
/// each other's fields, a killed writer's lock is released by the OS, and the
/// wait is bounded instead of a stale-file heuristic.
fn with_receipt_lock<T>(receipt: &Path, action: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let path = receipt.with_extension("lock");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let deadline = Deadline::after(LOCK_WAIT)?;
    let _lock =
        ExclusiveFileLock::acquire(&path, deadline, &Cancellation::default()).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "receipt lock {} was not acquired within {}s: {error}",
                    path.display(),
                    LOCK_WAIT.as_secs()
                ),
            )
        })?;
    action()
}

/// Appends one raw event line to the bounded detail stream. Returns false once
/// the stream is full; the caller records the truncation once.
pub(crate) fn append_detail(path: &Path, line: &str) -> io::Result<bool> {
    let current = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if current >= MAX_STREAM_BYTES {
        return Ok(false);
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let room = (MAX_STREAM_BYTES - current) as usize;
    let bytes = line.as_bytes();
    let take = room.min(bytes.len());
    file.write_all(&bytes[..take])?;
    if take == bytes.len() {
        file.write_all(b"\n")?;
        return Ok(true);
    }
    file.write_all(b"\n")?;
    Ok(false)
}

const MAX_ACTIVITY_MARKS: usize = 256;

/// Samples the attempt window before the child produces events. A failed
/// sample leaves the window unknown; it is not a zero-length attempt.
pub(crate) fn begin_clock(observation: &mut RunObservation) {
    let Ok((frequency, sample)) = harness_core::heavy_command_trace::sample_clock() else {
        observation.activity_gap = true;
        return;
    };
    let Some(boot) = harness_core::heavy_command_trace::boot_filetime() else {
        observation.activity_gap = true;
        return;
    };
    observation.clock = Some(ObservationClock {
        frequency,
        boot,
        started_qpc: sample.qpc,
        ended_qpc: None,
        started_span: sample.sample_span_ticks,
        ended_span: None,
        started_filetime: Some(sample.filetime),
        ended_filetime: None,
    });
}

pub(crate) fn end_clock(observation: &mut RunObservation) {
    let Some(clock) = observation.clock.as_mut() else {
        observation.activity_gap = true;
        return;
    };
    let Ok((frequency, sample)) = harness_core::heavy_command_trace::sample_clock() else {
        observation.activity_gap = true;
        return;
    };
    if frequency != clock.frequency {
        observation.activity_gap = true;
        return;
    }
    clock.ended_qpc = Some(sample.qpc);
    clock.ended_span = sample.sample_span_ticks;
    clock.ended_filetime = Some(sample.filetime);
}

/// Retains one compact boundary. A full raw detail file does not stop this.
pub(crate) fn note_activity(observation: &mut RunObservation, id: &str, kind: &str, phase: &str) {
    note_sourced_activity(observation, id, kind, phase, None, None);
}

/// Records one activity boundary. Producer milliseconds are kept only when the
/// notification carried them; receipt QPC is always the delivery sample.
pub(crate) fn note_sourced_activity(
    observation: &mut RunObservation,
    id: &str,
    kind: &str,
    phase: &str,
    started_at_ms: Option<i64>,
    completed_at_ms: Option<i64>,
) {
    if observation.activity.len() >= MAX_ACTIVITY_MARKS {
        observation.activity_truncated = true;
        return;
    }
    let Ok(id) = harness_core::heavy_command_trace::validate_token(id) else {
        observation.activity_gap = true;
        return;
    };
    let Ok(kind) = harness_core::heavy_command_trace::validate_token(kind) else {
        observation.activity_gap = true;
        return;
    };
    let Ok(phase) = harness_core::heavy_command_trace::validate_token(phase) else {
        observation.activity_gap = true;
        return;
    };
    let Ok((frequency, sample)) = harness_core::heavy_command_trace::sample_clock() else {
        observation.activity_gap = true;
        return;
    };
    let Some(boot) = harness_core::heavy_command_trace::boot_filetime() else {
        observation.activity_gap = true;
        return;
    };
    if observation
        .clock
        .as_ref()
        .is_some_and(|clock| clock.frequency != frequency || clock.boot != boot)
    {
        observation.activity_gap = true;
        return;
    }
    observation.activity.push(ActivityMark {
        id,
        kind,
        phase,
        qpc: sample.qpc,
        frequency,
        boot,
        span_ticks: sample.sample_span_ticks,
        bound: receipt_bound(),
        started_at_ms,
        completed_at_ms,
    });
    observation.updated_ms = now_ms();
}

fn note_parsed_activity(observation: &mut RunObservation, event: &NativeEvent) {
    let NativeEvent::Item { phase, item } = event else {
        return;
    };
    let Some(id) = item.id.as_deref() else {
        observation.activity_gap = true;
        return;
    };
    let phase = match phase {
        ItemPhase::Started => "started",
        ItemPhase::Updated => "updated",
        ItemPhase::Completed => "completed",
    };
    note_activity(observation, id, &item.kind, phase);
}

/// Transient spool of the owned child's raw stdout. The child writes here
/// while the host tails it; it is removed when the run ends, and the retained
/// bounded detail file is written only by the host.
pub(crate) fn spool_path(receipt: &Path) -> PathBuf {
    receipt.with_extension("running.jsonl")
}

/// Bounded tail of the owned child's stderr log for the visible surface.
pub(crate) fn stderr_tail(path: &Path, limit: usize) -> Option<String> {
    let bytes = read_bounded(path, 64 * 1024).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let start = bytes.len().saturating_sub(limit);
    Some(String::from_utf8_lossy(&bytes[start..]).into_owned())
}

/// Runs one observed CLI child under an owned job: the launcher tree is
/// contained from birth, so an abnormal host death reaps it while an ordinary
/// exit preserves the CLI's own background members (the launcher's policy).
///
/// Setup that matters for honest observation - host identity, the spool, the
/// bounded detail file and the initial recorded state - happens before the
/// child exists; any later read, render or persistence failure terminates and
/// drains the owned tree and fails the host, so a failing observer can never
/// report successful execution.
pub(crate) fn run_observed(
    mut command: CommandSpec,
    receipt: &Path,
    tracker: &mut RunTracker,
    header: &str,
    stderr_log: Option<&Path>,
) -> io::Result<i32> {
    let result = tracker
        .observation
        .result
        .clone()
        .ok_or_else(|| io::Error::other("observed run has no recorded result file"))?;
    let detail = tracker.observation.detail.clone();
    tracker.observation.host = Some(host_identity()?);
    tracker.observation.updated_ms = now_ms();
    begin_clock(&mut tracker.observation);
    let spool = spool_path(receipt);
    if let Some(parent) = spool.parent() {
        fs::create_dir_all(parent)?;
    }
    let spool_file = fs::File::create(&spool).map_err(|error| {
        io::Error::other(format!(
            "cannot create the event spool {} before the model starts: {error}",
            spool.display()
        ))
    })?;
    // A stale final message from an earlier run must never read as this run's
    // result, and the retained detail stream is per run.
    let _ = fs::remove_file(&result);
    if let Some(detail) = &detail {
        if let Some(parent) = detail.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::File::create(detail).map_err(|error| {
            io::Error::other(format!(
                "cannot create the bounded detail file {} before the model starts: {error}",
                detail.display()
            ))
        })?;
    }
    update_receipt(receipt, &tracker.observation).map_err(|error| {
        io::Error::other(format!(
            "the initial observation record could not be written before the model starts: {error}"
        ))
    })?;
    print!("{header}");
    io::stdout().flush()?;

    command.stdout = Some(spool_file);
    let mut cache_monitor =
        cache::Monitor::new(receipt, &harness_core::native_launcher::codex_home()?, None)?;
    let mut cache_loss = None;
    let job = Job::new(Limits::default())?;
    let child = job.spawn(&command).map_err(|error| {
        io::Error::other(format!(
            "the owned launcher tree did not start: {error}; detail: {}",
            spool.display()
        ))
    })?;
    let mut tail = SpoolTail::new(fs::File::open(&spool)?);
    let mut failure = None;
    {
        let mut sink = TailSink {
            receipt,
            detail: detail.as_deref(),
            tracker: &mut *tracker,
            stdout: io::stdout(),
            truncation_noted: false,
        };
        loop {
            match tail.read_available() {
                Ok(read) => {
                    if let Err(error) = sink.drain(&mut tail) {
                        failure = Some(error);
                        break;
                    }
                    if !matches!(
                        sink.tracker.observation.state.as_str(),
                        STATE_COMPLETED | STATE_FAILED | STATE_DEFECT | STATE_INTERRUPTED
                    ) && let Some(monitor) = &mut cache_monitor
                    {
                        match monitor.poll(sink.tracker.observation.session.as_deref(), read > 0) {
                            Ok(Some(loss)) => {
                                cache_loss = Some(loss);
                                break;
                            }
                            Ok(None) => {}
                            Err(error) => {
                                failure = Some(error);
                                break;
                            }
                        }
                    }
                    if read == 0 {
                        match child.wait_for_exit(Duration::from_millis(0)) {
                            Ok(true) => break,
                            Ok(false) => std::thread::sleep(TAIL_POLL),
                            Err(error) => {
                                failure = Some(error);
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if failure.is_none() && cache_loss.is_none() {
            loop {
                match tail.read_available() {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Err(error) = sink.drain(&mut tail) {
                            failure = Some(error);
                            break;
                        }
                    }
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
        }
        if failure.is_none()
            && cache_loss.is_none()
            && let Some(line) = tail.take_line(true)
            && let Err(error) = sink.line(line)
        {
            failure = Some(error);
        }
    }
    if let Some(loss) = cache_loss {
        let result = stop_cache_run(receipt, tracker, job, loss);
        tail.discard_remaining();
        let _ = fs::remove_file(&spool);
        return result;
    }
    if let Some(error) = failure {
        // The observer failed: stop and reap the owned tree, drain what the
        // child already produced, record the real cause and fail the host.
        let cleanup = job.terminate(1, JOB_CLEANUP);
        tail.discard_remaining();
        let _ = fs::remove_file(&spool);
        let cause = format!(
            "executor observation failed: {error}; the owned launcher tree was terminated ({})",
            match &cleanup {
                Ok(snapshot) => match snapshot.active_processes {
                    0 => "no process remained".to_owned(),
                    remaining => format!("{remaining} processes remained"),
                },
                Err(error) => format!("cleanup error: {error}"),
            }
        );
        let mut stdout = io::stdout();
        let _ = writeln!(stdout, "result: failed: {cause}");
        let _ = writeln!(
            stdout,
            "detail: {} stderr log: {}",
            locator(detail.as_deref()),
            locator(stderr_log)
        );
        let _ = stdout.flush();
        tracker.observer_failed(cause);
        let record = update_receipt(receipt, &tracker.observation);
        return Err(io::Error::new(
            error.kind(),
            format!(
                "executor observation failed: {error}; the owned launcher tree was terminated; detail: {}",
                match record {
                    Ok(()) => format!(
                        "{}; stderr log: {}",
                        locator(detail.as_deref()),
                        locator(stderr_log)
                    ),
                    Err(record_error) => format!(
                        "the failure record could not be written ({record_error}); stderr log: {}",
                        locator(stderr_log)
                    ),
                }
            ),
        ));
    }

    let code = child
        .exit_code()?
        .ok_or_else(|| io::Error::other("owned launcher still running after its spool closed"))?;
    let exit = tracker.finish(code as i32, Some(result.as_path()));
    // The terminal record is written before the summary is printed: a visible
    // output failure below must not lose the true final state.
    let record = update_receipt(receipt, &tracker.observation);
    let _ = fs::remove_file(&spool);
    let observation = &tracker.observation;
    let mut stdout = io::stdout();
    match observation.state.as_str() {
        STATE_COMPLETED => {
            writeln!(
                stdout,
                "result: completed (events={} messages={} tool calls={} session={})",
                observation.events,
                observation.messages,
                observation.tool_calls,
                observation.session.as_deref().unwrap_or("unrecorded")
            )?;
            writeln!(stdout, "result message: {}", result.display())?;
        }
        state => {
            writeln!(
                stdout,
                "result: {state}: {} (exit {})",
                observation.cause.as_deref().unwrap_or("no cause recorded"),
                observation.exit_code.unwrap_or_default()
            )?;
            if let Some(tail) = stderr_log.and_then(|path| stderr_tail(path, MAX_STDERR_TAIL)) {
                writeln!(stdout, "launcher stderr (bounded tail):")?;
                writeln!(stdout, "{tail}")?;
            }
            writeln!(
                stdout,
                "detail: {} stderr log: {}",
                detail
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "unavailable".into()),
                locator(stderr_log)
            )?;
            if observation.state == STATE_DEFECT {
                writeln!(
                    stdout,
                    "remedy: the session is not running; continue or correct it with `codex exec resume SESSION_ID` and treat the empty result as an executor output defect"
                )?;
            }
        }
    }
    // Mirror the launcher's own policy: an ordinary session exit preserves the
    // CLI's remaining background members, while an abnormal host death still
    // reaps the tree through kill-on-close.
    if let Err(error) = job.wait_session_root(&child) {
        eprintln!(
            "codex-harness: executor job preservation note: {error}; the tree stays contained"
        );
    }
    stdout.flush()?;
    if let Err(error) = record {
        return Err(io::Error::other(format!(
            "the completed run could not be recorded: {error}; stderr log: {}",
            locator(stderr_log)
        )));
    }
    Ok(exit)
}

fn locator(path: Option<&Path>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "unavailable".into())
}

/// Drains and records the child's event spool while it runs.
struct TailSink<'a> {
    receipt: &'a Path,
    detail: Option<&'a Path>,
    tracker: &'a mut RunTracker,
    stdout: io::Stdout,
    truncation_noted: bool,
}

impl TailSink<'_> {
    fn drain(&mut self, tail: &mut SpoolTail) -> io::Result<()> {
        while let Some(line) = tail.take_line(false) {
            self.line(line)?;
        }
        Ok(())
    }

    fn line(&mut self, line: Line) -> io::Result<()> {
        match line {
            Line::Oversize(note) => {
                self.tracker.count_oversize();
                writeln!(self.stdout, "unparsed event: {note}")?;
                update_receipt(self.receipt, &self.tracker.observation)
            }
            Line::Text(line) => {
                if line.trim().is_empty() {
                    return Ok(());
                }
                if let Some(detail) = self.detail {
                    match append_detail(detail, &line) {
                        Ok(true) => {}
                        Ok(false) if !self.truncation_noted => {
                            self.truncation_noted = true;
                            self.tracker.observation.detail_truncated = true;
                            writeln!(
                                self.stdout,
                                "note: raw event detail reached the {} byte bound; the detail file stops here while the readable surface continues",
                                MAX_STREAM_BYTES
                            )?;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            return Err(io::Error::other(format!(
                                "the bounded detail file {} is not writable: {error}",
                                detail.display()
                            )));
                        }
                    }
                }
                match parse_event(&line) {
                    Ok(event) => {
                        let changed = self.tracker.apply(&event);
                        note_parsed_activity(&mut self.tracker.observation, &event);
                        render_event(&event, &mut self.stdout)?;
                        if changed {
                            update_receipt(self.receipt, &self.tracker.observation)?;
                        }
                        Ok(())
                    }
                    Err(note) => {
                        self.tracker.count_malformed();
                        writeln!(self.stdout, "unparsed event: {note}")?;
                        update_receipt(self.receipt, &self.tracker.observation)
                    }
                }
            }
        }
    }
}

/// One complete event line: `Oversize` means the line exceeded the accepted
/// event size and the rest of it was dropped.
enum Line {
    Text(String),
    Oversize(String),
}

/// Incremental reader of the child's growing event spool: bounded per line, so
/// a pathological single line cannot grow host memory or the retained detail.
struct SpoolTail {
    file: fs::File,
    buffer: Vec<u8>,
    oversize: bool,
}

impl SpoolTail {
    fn new(file: fs::File) -> Self {
        Self {
            file,
            buffer: Vec::new(),
            oversize: false,
        }
    }

    /// Reads whatever the child has produced since the last call; zero means
    /// the current end of the spool was reached.
    fn read_available(&mut self) -> io::Result<usize> {
        let mut chunk = [0u8; 16 * 1024];
        let read = self.file.read(&mut chunk)?;
        self.buffer.extend_from_slice(&chunk[..read]);
        Ok(read)
    }

    /// Drains whatever remains after the owned tree was stopped, bounded, so
    /// the spool is not removed while a writer could still hold data.
    fn discard_remaining(&mut self) {
        let mut chunk = [0u8; 16 * 1024];
        let mut discarded = 0usize;
        while discarded < MAX_STREAM_BYTES as usize {
            match self.file.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    discarded += read;
                    self.buffer.clear();
                    self.oversize = false;
                }
            }
        }
    }

    /// Takes the next complete line. With `eof` set, a trailing unterminated
    /// line is returned as a partial line so it is parsed (and reported as
    /// truncated) instead of being silently dropped.
    fn take_line(&mut self, eof: bool) -> Option<Line> {
        if let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let oversize = self.oversize;
            let mut line: Vec<u8> = self.buffer.drain(..=position).collect();
            line.pop();
            self.oversize = false;
            return Some(classify_line(line, oversize));
        }
        if self.oversize || self.buffer.len() > MAX_EVENT_LINE {
            // Drop the rest of an over-long line; its newline ends it.
            self.oversize = true;
            self.buffer.clear();
        }
        if !eof || self.buffer.is_empty() {
            return None;
        }
        let oversize = self.oversize;
        let line = std::mem::take(&mut self.buffer);
        self.oversize = false;
        Some(classify_line(line, oversize))
    }
}

fn classify_line(line: Vec<u8>, oversize: bool) -> Line {
    if oversize || line.len() > MAX_EVENT_LINE {
        Line::Oversize(format!(
            "event line exceeded {MAX_EVENT_LINE} bytes and was dropped"
        ))
    } else {
        Line::Text(String::from_utf8_lossy(&line).into_owned())
    }
}

/// Bounded one-line excerpt used in human-readable records.
pub(crate) fn excerpt(text: &str, limit: usize) -> String {
    excerpt_total(text, limit, text.len() as u64)
}

/// Bounded one-line excerpt whose announced total is the underlying record's
/// full size even when the read that produced `text` was itself bounded.
fn excerpt_total(text: &str, limit: usize, total: u64) -> String {
    let text = text.trim();
    if text.len() <= limit && total <= text.len() as u64 {
        return text.to_owned();
    }
    let mut end = limit.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… ({} bytes total, truncated)", &text[..end], total)
}

/// Bounded, model-free delivery-improvement evidence.
///
/// The collector reads the existing private records of one originating-lead run
/// generation: the spawned run's dispatch receipt, the lead and executor
/// control conversations while they are addressable, the bounded raw detail
/// tail, and the recorded addressed-message exchanges in both directions. It
/// turns authorized visible messages and tool outcomes into a bounded batch of
/// new events for the improvement observer and keeps its cursor, active batch
/// and last acknowledged outcome in the caller's own receipt under the
/// top-level `evidence` key. It makes no model call, starts no conversation,
/// asks no status and never inspects opaque reasoning state; an unchanged pass
/// is an explicit no-op.
///
/// Cursor semantics: a retained board finding or an explicit no-finding result
/// advances the cursor over exactly the identities the acknowledged batch
/// carried. An unknown outcome releases the batch without advancing the
/// cursor, so the same evidence is offered again instead of being skipped.
/// Only one batch is in flight at a time; arrivals during analysis stay
/// unacknowledged and are offered by the next claim. Events beyond the batch
/// bound wait behind the reported pending count. Missing, capped or unreadable
/// sources are reported as coverage limits; delivery metadata alone is never
/// presented as message content.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod evidence {
    use super::super::{control, executor_message};
    use super::{
        COVERAGE_UNAVAILABLE, ItemPhase, ItemSummary, MAX_STREAM_BYTES, NativeEvent,
        RunObservation, STATE_UNOBSERVED, event_turn_id, excerpt, excerpt_total, now_ms,
        parse_event, read_bounded, read_receipt_value, tool_operation_kind, with_receipt_lock,
        write_receipt,
    };
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs, io,
        io::{Read, Seek, SeekFrom},
        path::Path,
        time::Duration,
    };

    /// Version of the caller-owned `evidence` state document.
    pub(crate) const EVIDENCE_SCHEMA: u32 = 1;
    /// Hard bound on the events one claimed batch carries.
    pub(crate) const EVIDENCE_MAX_BATCH: usize = 64;
    /// Bound on remembered acknowledged event identities. It exceeds the
    /// widest bounded evidence window, so an acknowledged event that is still
    /// visible is never offered again; identities whose evidence aged out of
    /// every window are pruned instead of being carried forever.
    const EVIDENCE_CURSOR_LIMIT: usize = 4096;
    /// Bound on visible items scanned per conversation, newest kept.
    const EVIDENCE_SCAN_LIMIT: usize = 512;
    /// Bound on coverage notes one claim reports.
    const EVIDENCE_COVERAGE_LIMIT: usize = 64;
    /// Bound on retained text of one visible message.
    const EVIDENCE_TEXT_BYTES: usize = 4000;
    /// Bound on one recorded detail string.
    const EVIDENCE_DETAIL_BYTES: usize = 600;
    /// Bound on one read of a spilled or pointed-to payload.
    const EVIDENCE_PAYLOAD_READ: u64 = 64 * 1024;
    /// Bound on the raw detail tail read.
    const EVIDENCE_TAIL_READ: u64 = 256 * 1024;

    /// One collection or acknowledgment pass over one originating-lead run
    /// generation.
    pub(crate) struct EvidenceRequest<'a> {
        /// Kit home owning the pool records (`harness/executor-pool`).
        pub home: &'a Path,
        /// Originating-lead run generation; it names the spawned run to observe
        /// and scopes every event identity.
        pub run_generation: &'a str,
        /// Any private JSON receipt the consumer owns; the collector keeps the
        /// `evidence` state under that receipt's top-level key.
        pub state_receipt: &'a Path,
        /// Bound on one control-conversation attach and read.
        pub bound: Duration,
        /// Requested events per batch, clamped to [`EVIDENCE_MAX_BATCH`].
        pub max_events: usize,
    }

    /// Result of one model-free claim.
    #[derive(Debug)]
    pub(crate) enum EvidenceClaim {
        /// One bounded batch of new evidence. No further batch can be claimed
        /// until this one is acknowledged.
        Ready(EvidenceBatch),
        /// No new eligible evidence; no model turn is implied.
        NoNewEvidence {
            contexts: Vec<EvidenceContext>,
            coverage: Vec<EvidenceCoverage>,
        },
        /// A batch is already in flight.
        BatchInFlight { batch_id: String, created_ms: u64 },
    }

    /// One bounded batch of new events for the improvement observer.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub(crate) struct EvidenceBatch {
        pub batch_id: String,
        pub run_generation: String,
        pub created_ms: u64,
        /// Events beyond this batch's bound; the next claim carries them.
        pub pending: u64,
        pub events: Vec<EvidenceEvent>,
        pub contexts: Vec<EvidenceContext>,
        pub coverage: Vec<EvidenceCoverage>,
    }

    /// Recorded assignment and lifecycle context of one observed run.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub(crate) struct EvidenceContext {
        pub slot: u64,
        pub owner: String,
        pub checkout: String,
        pub worktree: String,
        pub bd: String,
        pub assignment_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub assignment: Option<String>,
        pub state: String,
        pub observation_coverage: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub session: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub exit_code: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub cause: Option<String>,
        pub updated_ms: u64,
        pub detail_truncated: bool,
        pub activity_truncated: bool,
        pub activity_gap: bool,
    }

    /// One piece of new evidence. Its identity is the native item or message
    /// id plus the run generation, slot and phase it was recorded in;
    /// `reasoning` items are never collected.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub(crate) struct EvidenceEvent {
        /// `message-to-executor`, `message-to-lead`, `executor-item`,
        /// `lead-item`, `run` or `gap`.
        pub source: String,
        pub slot: u64,
        /// Native item, message or lifecycle id.
        pub id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub turn: Option<String>,
        /// `message`, `tool`, `status` or `gap`.
        pub kind: String,
        /// Recorded status or lifecycle phase.
        pub phase: String,
        /// Command identity or tool name, when the event is a tool outcome.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub name: Option<String>,
        /// Retained visible text, bounded; `None` is reported through coverage
        /// and never filled with delivery metadata.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub exit_code: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub detail: Option<String>,
        /// Bounded locator of the retained content when it was read from a
        /// recorded file (a spilled payload); `None` when the content was
        /// inline.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub locator: Option<String>,
    }

    /// One honest coverage limit of a claim.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub(crate) struct EvidenceCoverage {
        pub source: String,
        pub reason: String,
    }

    /// The consumer's acknowledgment of the consumed batch.
    #[derive(Debug, Clone)]
    pub(crate) enum EvidenceAck {
        /// A board finding was retained for this batch.
        Finding { reference: String },
        /// Explicit no-finding acknowledgment; a valid outcome.
        NoFinding,
        /// The outcome is unknown; the cursor must not advance.
        Unknown { reason: String },
    }

    /// Result of one acknowledgment pass.
    #[derive(Debug)]
    pub(crate) enum EvidenceAckOutcome {
        /// The cursor advanced over exactly the acknowledged batch.
        Advanced { acked: usize, no_finding: bool },
        /// Unknown outcome: the batch was released and the cursor kept, so the
        /// same evidence is offered again.
        Preserved { reason: String },
        /// No batch is active, or the named batch is not the active one.
        Refused { reason: String },
    }

    /// The caller-owned `evidence` state document.
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", default)]
    struct EvidenceState {
        schema: u32,
        /// Claims made so far; the next batch id derives from it.
        batches: u64,
        /// Acknowledged event identities, oldest first, bounded.
        cursor: Vec<String>,
        /// Set when the oldest acknowledgments were evicted at the cursor
        /// bound; some evidence may be delivered again.
        cursor_evicted: bool,
        /// The originating-lead run generation this state observes; a claim
        /// for another generation resets the cursor instead of applying it to
        /// another run.
        #[serde(default)]
        generation: String,
        /// Identities released by an unknown outcome; they are re-offered
        /// against the next gathered evidence instead of being advanced.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        released: Vec<String>,
        active: Option<ActiveBatch>,
        last_ack: Option<LastAck>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ActiveBatch {
        batch_id: String,
        created_ms: u64,
        /// Exactly the identities this batch carried; the acknowledgment
        /// advances over these and never over later arrivals.
        ids: Vec<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct LastAck {
        batch_id: String,
        outcome: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
        ms: u64,
    }

    fn read_document(path: &Path) -> io::Result<Value> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "the evidence state receipt {} is not JSON: {error}; refusing to advance unfinished evidence",
                        path.display()
                    ),
                )
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(json!({})),
            Err(error) => Err(io::Error::new(
                error.kind(),
                format!(
                    "the evidence state receipt {} is unreadable: {error}",
                    path.display()
                ),
            )),
        }
    }

    fn state_from_document(document: &Value) -> io::Result<EvidenceState> {
        match document.get("evidence") {
            None | Some(Value::Null) => Ok(EvidenceState::default()),
            Some(evidence) => serde_json::from_value(evidence.clone()).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "the recorded evidence state is malformed: {error}; refusing to advance unfinished evidence"
                    ),
                )
            }),
        }
    }

    fn read_state(path: &Path) -> io::Result<EvidenceState> {
        state_from_document(&read_document(path)?)
    }

    // ------------------------------------------------------------ claim state

    /// The identity one event is acknowledged by: its source, slot, native
    /// item, message or lifecycle id, and the phase it was recorded in. The
    /// state and cursor are scoped to one run generation, so the same native id
    /// in another generation is a different identity.
    fn identity(event: &EvidenceEvent) -> String {
        format!(
            "{}|{}|{}|{}",
            event.source, event.slot, event.id, event.phase
        )
    }

    fn refuse_newer_schema(state: &EvidenceState) -> io::Result<()> {
        if state.schema > EVIDENCE_SCHEMA {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "the recorded evidence state uses schema {} while this collector writes {EVIDENCE_SCHEMA}; refusing to advance unfinished evidence",
                    state.schema
                ),
            ));
        }
        Ok(())
    }

    /// Merges the caller-owned `evidence` state under its top-level key,
    /// preserving every other field of the receipt. The caller already holds
    /// the receipt lock: claim and acknowledgment read, decide and write in
    /// one locked section, so a transition can never be applied to a state a
    /// concurrent writer has since replaced.
    fn persist_state_under_lock(path: &Path, state: &EvidenceState) -> io::Result<()> {
        let field = serde_json::to_value(state).map_err(io::Error::other)?;
        let mut document = match read_receipt_value(path) {
            Ok(document) => document,
            Err(error) if error.kind() == io::ErrorKind::NotFound => json!({}),
            Err(error) => return Err(error),
        };
        if !document.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "the evidence state receipt {} is not a JSON object; refusing to overwrite it",
                    path.display()
                ),
            ));
        }
        document["evidence"] = field;
        write_receipt(path, &document)
    }

    fn advance_cursor(state: &mut EvidenceState, ids: &[String]) -> usize {
        for id in ids {
            if state.cursor.iter().any(|known| known == id) {
                continue;
            }
            state.cursor.push(id.clone());
        }
        while state.cursor.len() > EVIDENCE_CURSOR_LIMIT {
            state.cursor.remove(0);
            state.cursor_evicted = true;
        }
        ids.len()
    }

    /// Applies the run scoping every state transition shares: a state written
    /// for another generation is reset instead of carrying a cursor, an
    /// active batch or a last acknowledgment across runs. Returns whether the
    /// reset changed the state and must be persisted.
    fn prepare_generation(
        state: &mut EvidenceState,
        run_generation: &str,
        notes: &mut CoverageNotes,
        reset_noted: &mut bool,
    ) -> bool {
        let mut changed = false;
        if state.schema == 0 {
            state.schema = EVIDENCE_SCHEMA;
            changed = true;
        }
        if state.generation != run_generation {
            if !state.generation.is_empty() && !*reset_noted {
                notes.note(
                    "state",
                    format!(
                        "the recorded evidence state observed run generation {} while this claim observes {}; its cursor, active batch and last acknowledgment were reset for the new run instead of being applied to another run",
                        state.generation, run_generation
                    ),
                );
                *reset_noted = true;
            }
            *state = EvidenceState {
                schema: EVIDENCE_SCHEMA,
                generation: run_generation.to_owned(),
                ..EvidenceState::default()
            };
            changed = true;
        }
        changed
    }

    /// The source name one retained identity belongs to: the `source` segment
    /// of the `source|slot|id|phase` identity written by [`identity`].
    fn identity_source(id: &str) -> &str {
        id.split_once('|').map(|(source, _)| source).unwrap_or(id)
    }

    /// Claims one bounded batch of new evidence for one run generation.
    ///
    /// An unchanged pass reports [`EvidenceClaim::NoNewEvidence`] with the
    /// current contexts and coverage and never implies a model turn. A pass
    /// while a batch is in flight reports its identity instead of gathering,
    /// so exactly one analysis batch is authoritative at a time; arrivals
    /// during that analysis are gathered by the next claim.
    ///
    /// The state transition is atomic in the receipt's own lock: the state is
    /// read and scoped in a short locked phase, the native records are
    /// gathered outside it (a slow conversation or file read never blocks a
    /// concurrent receipt write), and the final phase re-reads the state
    /// under the lock before deciding. Two concurrent claims can therefore
    /// never both start a batch, and a stale read can never overwrite a
    /// transition a concurrent writer installed meanwhile.
    pub(crate) fn claim(request: &EvidenceRequest<'_>) -> io::Result<EvidenceClaim> {
        let mut notes = CoverageNotes::default();
        let mut reset_noted = false;
        let under_way = with_receipt_lock(
            request.state_receipt,
            || -> io::Result<Option<EvidenceClaim>> {
                let document = read_document(request.state_receipt)?;
                let mut state = state_from_document(&document)?;
                refuse_newer_schema(&state)?;
                prepare_generation(
                    &mut state,
                    request.run_generation,
                    &mut notes,
                    &mut reset_noted,
                );
                Ok(state
                    .active
                    .as_ref()
                    .map(|active| EvidenceClaim::BatchInFlight {
                        batch_id: active.batch_id.clone(),
                        created_ms: active.created_ms,
                    }))
            },
        )?;
        if let Some(under_way) = under_way {
            return Ok(under_way);
        }
        let gathered = gather(request, &mut notes);
        with_receipt_lock(request.state_receipt, || -> io::Result<EvidenceClaim> {
            let document = read_document(request.state_receipt)?;
            let mut state = state_from_document(&document)?;
            refuse_newer_schema(&state)?;
            let state_dirty = prepare_generation(
                &mut state,
                request.run_generation,
                &mut notes,
                &mut reset_noted,
            );
            if let Some(active) = &state.active {
                return Ok(EvidenceClaim::BatchInFlight {
                    batch_id: active.batch_id.clone(),
                    created_ms: active.created_ms,
                });
            }
            if state.cursor_evicted {
                notes.note(
                    "retention",
                    format!(
                        "the acknowledged-identity cursor reached its {EVIDENCE_CURSOR_LIMIT}-entry bound and older identities were evicted; evidence older than the retained window can be offered again instead of staying trusted as consumed"
                    ),
                );
            }
            let visible: BTreeSet<String> = gathered.events.iter().map(identity).collect();
            // The cursor keeps an acknowledged identity while it is still
            // visible this pass and also while its source could not be read:
            // a missing read is not permanent eviction, so a temporarily
            // unreadable source cannot cause its evidence to be offered
            // again. Only an identity absent from a source whose reads
            // completed this pass has aged out of every bounded window that
            // can present it, so it is dropped instead of carried forever.
            let acknowledged = state.cursor.len();
            let mut retained_gap = 0usize;
            let mut retained_gap_example: Option<String> = None;
            state.cursor.retain(|id| {
                if visible.contains(id) {
                    return true;
                }
                if gathered.covered.contains(identity_source(id)) {
                    return false;
                }
                retained_gap += 1;
                if retained_gap_example.is_none() {
                    retained_gap_example = Some(id.clone());
                }
                true
            });
            let cursor_pruned = state.cursor.len() != acknowledged;
            if retained_gap > 0 {
                notes.note(
                    "retention",
                    format!(
                        "{retained_gap} acknowledged identities could not be read from their source this pass (for example {}); a missing read never evicts acknowledged evidence, so they stay retained instead of being offered again",
                        retained_gap_example.as_deref().unwrap_or("")
                    ),
                );
            }
            // A released identity is satisfied once this pass presents it
            // again; it stays recoverable while its source is unreadable and
            // is only dropped when a completed read shows it aged out of
            // every bounded window that can present it.
            let released = state.released.len();
            let mut released_gap = 0usize;
            let mut released_gap_example: Option<String> = None;
            state.released.retain(|id| {
                if visible.contains(id) {
                    return false;
                }
                if gathered.covered.contains(identity_source(id)) {
                    return false;
                }
                released_gap += 1;
                if released_gap_example.is_none() {
                    released_gap_example = Some(id.clone());
                }
                true
            });
            let released_changed = state.released.len() != released;
            if released_gap > 0 {
                notes.note(
                    "replay",
                    format!(
                        "{released_gap} identities released by an unknown outcome are not readable this pass (for example {}); they are neither advanced nor silently dropped and are re-offered once their source is readable again",
                        released_gap_example.as_deref().unwrap_or("")
                    ),
                );
            }
            let cursor: BTreeSet<&String> = state.cursor.iter().collect();
            let mut seen = BTreeSet::new();
            let mut events: Vec<(String, EvidenceEvent)> = Vec::new();
            for event in gathered.events {
                let id = identity(&event);
                if cursor.contains(&id) || !seen.insert(id.clone()) {
                    continue;
                }
                events.push((id, event));
            }
            let coverage = notes.finish();
            if events.is_empty() {
                if state_dirty || cursor_pruned || released_changed {
                    persist_state_under_lock(request.state_receipt, &state)?;
                }
                return Ok(EvidenceClaim::NoNewEvidence {
                    contexts: gathered.contexts,
                    coverage,
                });
            }
            let limit = request.max_events.clamp(1, EVIDENCE_MAX_BATCH);
            let pending = events.len().saturating_sub(limit) as u64;
            events.truncate(limit);
            let ids: Vec<String> = events.iter().map(|(id, _)| id.clone()).collect();
            let chosen: Vec<EvidenceEvent> = events.into_iter().map(|(_, event)| event).collect();
            state.batches += 1;
            let batch_id = format!("batch-{}", state.batches);
            let created_ms = now_ms();
            state.active = Some(ActiveBatch {
                batch_id: batch_id.clone(),
                created_ms,
                ids,
            });
            persist_state_under_lock(request.state_receipt, &state)?;
            Ok(EvidenceClaim::Ready(EvidenceBatch {
                batch_id,
                run_generation: request.run_generation.to_owned(),
                created_ms,
                pending,
                events: chosen,
                contexts: gathered.contexts,
                coverage,
            }))
        })
    }

    /// Acknowledges the active batch. A retained finding or an explicit
    /// no-finding result advances the cursor over exactly the identities the
    /// batch carried; an unknown outcome releases the batch without advancing
    /// the cursor, so the same evidence is offered again instead of skipped.
    ///
    /// The whole read-validate-write sequence runs in the receipt's own
    /// locked section, so an acknowledgment that raced a newer claim, a
    /// generation reset or another acknowledgment is decided against the
    /// state that is actually recorded instead of overwriting it from a
    /// stale read. Nothing here performs a native conversation read, so the
    /// lock is held only around local file work.
    pub(crate) fn acknowledge(
        request: &EvidenceRequest<'_>,
        batch_id: &str,
        ack: &EvidenceAck,
    ) -> io::Result<EvidenceAckOutcome> {
        with_receipt_lock(
            request.state_receipt,
            || -> io::Result<EvidenceAckOutcome> {
                let document = read_document(request.state_receipt)?;
                let mut state = state_from_document(&document)?;
                refuse_newer_schema(&state)?;
                if state.generation != request.run_generation {
                    let recorded = if state.generation.is_empty() {
                        "no recorded run".to_owned()
                    } else {
                        state.generation.clone()
                    };
                    return Ok(EvidenceAckOutcome::Refused {
                        reason: format!(
                            "the recorded evidence state observes run generation {recorded} instead of {}; nothing was advanced",
                            request.run_generation
                        ),
                    });
                }
                let Some(active) = state.active.as_ref() else {
                    return Ok(EvidenceAckOutcome::Refused {
                        reason: "no evidence batch is active; nothing was advanced".to_owned(),
                    });
                };
                if active.batch_id != batch_id {
                    return Ok(EvidenceAckOutcome::Refused {
                        reason: format!(
                            "the active evidence batch is {} instead of {batch_id}; nothing was advanced",
                            active.batch_id
                        ),
                    });
                }
                let active = state.active.take().expect("the active batch was checked");
                let now = now_ms();
                let outcome = match ack {
                    EvidenceAck::Finding { reference } if reference.trim().is_empty() => {
                        // Nothing was persisted, so the taken batch is simply
                        // dropped with the refused transition.
                        return Ok(EvidenceAckOutcome::Refused {
                        reason: "a retained finding must name its board record; nothing was advanced"
                            .to_owned(),
                    });
                    }
                    EvidenceAck::Finding { reference } => {
                        let acked = advance_cursor(&mut state, &active.ids);
                        state.last_ack = Some(LastAck {
                            batch_id: active.batch_id.clone(),
                            outcome: "finding".to_owned(),
                            reference: Some(excerpt(reference, EVIDENCE_DETAIL_BYTES)),
                            ms: now,
                        });
                        EvidenceAckOutcome::Advanced {
                            acked,
                            no_finding: false,
                        }
                    }
                    EvidenceAck::NoFinding => {
                        let acked = advance_cursor(&mut state, &active.ids);
                        state.last_ack = Some(LastAck {
                            batch_id: active.batch_id.clone(),
                            outcome: "no-finding".to_owned(),
                            reference: None,
                            ms: now,
                        });
                        EvidenceAckOutcome::Advanced {
                            acked,
                            no_finding: true,
                        }
                    }
                    EvidenceAck::Unknown { reason } => {
                        state.released = active.ids.clone();
                        state.last_ack = Some(LastAck {
                            batch_id: active.batch_id.clone(),
                            outcome: "unknown".to_owned(),
                            reference: Some(excerpt(reason, EVIDENCE_DETAIL_BYTES)),
                            ms: now,
                        });
                        EvidenceAckOutcome::Preserved {
                            reason: reason.clone(),
                        }
                    }
                };
                persist_state_under_lock(request.state_receipt, &state)?;
                Ok(outcome)
            },
        )
    }

    /// Bounded coverage notes of one pass: the list never grows past
    /// [`EVIDENCE_COVERAGE_LIMIT`]; the rest is counted in one final note.
    #[derive(Default)]
    struct CoverageNotes {
        notes: Vec<EvidenceCoverage>,
        dropped: u64,
    }

    impl CoverageNotes {
        fn note(&mut self, source: &str, reason: String) {
            if self.notes.len() < EVIDENCE_COVERAGE_LIMIT {
                self.notes.push(EvidenceCoverage {
                    source: source.to_owned(),
                    reason,
                });
            } else {
                self.dropped += 1;
            }
        }

        fn finish(mut self) -> Vec<EvidenceCoverage> {
            if self.dropped > 0 {
                let summary = EvidenceCoverage {
                    source: "coverage".to_owned(),
                    reason: format!(
                        "{} further coverage limits were omitted at the {EVIDENCE_COVERAGE_LIMIT}-note bound",
                        self.dropped
                    ),
                };
                if self.notes.len() == EVIDENCE_COVERAGE_LIMIT {
                    *self.notes.last_mut().expect("a bounded list is non-empty") = summary;
                } else {
                    self.notes.push(summary);
                }
            }
            self.notes
        }
    }

    /// Everything one collection pass gathered for a claim.
    struct Gathered {
        events: Vec<EvidenceEvent>,
        contexts: Vec<EvidenceContext>,
        /// Source names whose bounded reads completed this pass. An identity
        /// absent from a covered source has aged out of every window that can
        /// present it; a source missing here had a failed or skipped read
        /// this pass, so absence proves nothing and must never evict an
        /// acknowledged or released identity.
        covered: BTreeSet<String>,
    }

    /// One raw-detail read: the bounded events it could present and whether
    /// every read it attempted over the retained tail succeeded. A failed
    /// metadata, open or read step leaves `readable` false, so the pass proves
    /// nothing about an identity's absence and retained identities stay
    /// instead of being evicted by a temporary loss. A capped, partial or
    /// malformed tail is still a completed read of the retained window: it is
    /// reported through the coverage notes and the stable gap event and never
    /// presented as progress.
    struct DetailRead {
        events: Vec<EvidenceEvent>,
        readable: bool,
    }

    // ------------------------------------------------------------ gathering

    /// One control conversation as a read pass found it: the retained visible
    /// text of its user messages by client identity and its newest bounded
    /// visible items.
    struct ConversationRead {
        user_texts: BTreeMap<String, String>,
        events: Vec<EvidenceEvent>,
    }

    /// Everything one conversation read needs beyond the coverage notes.
    struct ConversationSpec<'a> {
        endpoint: &'a Path,
        slot: u64,
        /// Coverage source of this conversation.
        side: &'a str,
        /// Event source of this conversation's items.
        source: &'a str,
        /// Addressed identities whose turns a lead read keeps; the executor
        /// read keeps every turn.
        correlation: &'a BTreeSet<String>,
        /// Message identities already recorded as deliveries; the executor
        /// read does not present their delivered inputs a second time.
        represented: &'a BTreeSet<String>,
        bound: Duration,
        /// Thread identity the endpoint record must name, when the records
        /// under observation name one.
        expected_thread: Option<&'a str>,
        /// `executor` or `lead`, used in coverage reasons.
        label: &'a str,
    }

    /// One claim's collection pass over one run generation: the spawned run its
    /// pool records name, the control conversations beside its receipt and its
    /// retained raw detail stream. Every failure to read evidence is a coverage
    /// note on this pass, never a silent success. A search, receipt or
    /// conversation that could not be read covers nothing, so the identities
    /// it would have carried are never treated as aged out.
    fn gather(request: &EvidenceRequest<'_>, notes: &mut CoverageNotes) -> Gathered {
        let mut gathered = Gathered {
            events: Vec::new(),
            contexts: Vec::new(),
            covered: BTreeSet::new(),
        };
        let receipts =
            match executor_message::matching_receipts(request.home, request.run_generation) {
                Ok(receipts) => receipts,
                Err(error) => {
                    notes.note(
                        "receipt",
                        format!(
                            "the pool records could not be searched for run generation {}: {error}",
                            request.run_generation
                        ),
                    );
                    return gathered;
                }
            };
        match receipts.as_slice() {
            [] => notes.note(
                "receipt",
                format!(
                    "no readable dispatched-run record names generation {} under {}; the run may not have been spawned by this home yet, or its receipt is unreadable",
                    request.run_generation,
                    request.home.display()
                ),
            ),
            [receipt] => gather_receipt(request, receipt, notes, &mut gathered),
            many => notes.note(
                "receipt",
                format!(
                    "{} dispatched runs record generation {}; exactly one run is expected, so none of them was observed until that is resolved",
                    many.len(),
                    request.run_generation
                ),
            ),
        }
        gathered
    }

    /// The recorded message identities of one delivery array.
    fn message_ids(entries: &[Value]) -> BTreeSet<String> {
        entries
            .iter()
            .filter_map(|entry| entry["id"].as_str())
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect()
    }
    /// Reads one spawned run's records: its assignment context, its run
    /// lifecycle event, both recorded message directions, the executor and
    /// lead control conversations and the raw detail tail. Every readable
    /// source is recorded as covered for this pass; a source whose read
    /// failed or was skipped is not, so the retention rule in `claim` cannot
    /// mistake its missing identities for aged-out evidence.
    fn gather_receipt(
        request: &EvidenceRequest<'_>,
        receipt: &Path,
        notes: &mut CoverageNotes,
        gathered: &mut Gathered,
    ) {
        let document = match read_receipt_value(receipt) {
            Ok(document) => document,
            Err(error) => {
                notes.note(
                    "receipt",
                    format!(
                        "the dispatched-run record {} is not readable: {error}",
                        receipt.display()
                    ),
                );
                return;
            }
        };
        if document["originatingLead"]["runGeneration"].as_str() != Some(request.run_generation) {
            notes.note(
                "receipt",
                format!(
                    "the dispatched-run record {} no longer names generation {}; it changed between the search and this read and none of it was observed",
                    receipt.display(),
                    request.run_generation
                ),
            );
            return;
        }
        let slot = document["slot"]["index"].as_u64().unwrap_or(0);
        let observation = observation_of(&document, notes);
        gathered
            .contexts
            .push(context_of(&document, slot, &observation));
        gathered.events.push(run_event(slot, &observation));
        gathered.covered.insert("run".to_owned());

        let messages_are_an_array = document["messages"].is_array();
        let lead_messages_are_an_array = document["leadMessages"].is_array();
        let messages = document["messages"].as_array().cloned().unwrap_or_default();
        let lead_messages = document["leadMessages"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let represented = message_ids(&messages);
        let lead_ids = message_ids(&lead_messages);
        let parent = receipt.parent().map(Path::to_path_buf).unwrap_or_default();

        // A run whose coverage is unavailable or which never reached an
        // observed state has no recorded conversation to read; probing its
        // address would report a transport failure instead of the recorded
        // fact, so the recorded unavailability is reported as itself.
        let conversations_unavailable =
            observation.coverage == COVERAGE_UNAVAILABLE || observation.state == STATE_UNOBSERVED;
        let executor = if conversations_unavailable {
            notes.note(
                "executor-conversation",
                conversation_unavailable_reason(&observation, "executor"),
            );
            None
        } else {
            let executor_thread = observation.recorded_session().map(str::to_owned);
            read_conversation(
                &ConversationSpec {
                    endpoint: &parent.join(format!("endpoint-{slot}.json")),
                    slot,
                    side: "executor-conversation",
                    source: "executor-item",
                    correlation: &BTreeSet::new(),
                    represented: &represented,
                    bound: request.bound,
                    expected_thread: executor_thread.as_deref(),
                    label: "executor",
                },
                notes,
            )
        };
        let executor_readable = executor.is_some();
        if let Some(read) = &executor {
            gathered.events.extend(read.events.iter().cloned());
        }
        gathered.events.extend(record_message_events(
            slot,
            &messages,
            executor.as_ref(),
            notes,
        ));

        let lead_thread = field_text(&document, "/originatingLead/threadId");
        let lead = if conversations_unavailable {
            None
        } else {
            read_conversation(
                &ConversationSpec {
                    endpoint: &parent.join(format!("lead-endpoint-{slot}.json")),
                    slot,
                    side: "lead-conversation",
                    source: "lead-item",
                    correlation: &lead_ids,
                    represented: &BTreeSet::new(),
                    bound: request.bound,
                    expected_thread: lead_thread.as_deref(),
                    label: "lead",
                },
                notes,
            )
        };
        let lead_readable = lead.is_some();
        if let Some(read) = &lead {
            gathered.events.extend(read.events.iter().cloned());
        }
        gathered.events.extend(record_lead_message_events(
            slot,
            &lead_messages,
            lead.as_ref(),
            notes,
        ));

        let detail_readable = match observation.detail.as_deref() {
            Some(detail) => {
                let read = collect_detail(detail, slot, &observation, notes);
                let readable = read.readable;
                gathered.events.extend(read.events);
                readable
            }
            None => {
                notes.note(
                    "detail",
                    "the recorded observation carries no raw detail stream; visible tool and message content beyond the recorded conversations is unavailable".to_owned(),
                );
                false
            }
        };

        // An identity is only evictable when every read that can present it
        // completed this pass: the executor conversation and the detail tail
        // both present `executor-item`, and both must have been read for its
        // absence to mean the identity aged out.
        if executor_readable && detail_readable {
            gathered.covered.insert("executor-item".to_owned());
        }
        if lead_readable {
            gathered.covered.insert("lead-item".to_owned());
        }
        if messages_are_an_array {
            gathered.covered.insert("message-to-executor".to_owned());
        }
        if lead_messages_are_an_array {
            gathered.covered.insert("message-to-lead".to_owned());
        }
        if detail_readable {
            gathered.covered.insert("gap".to_owned());
        }
    }

    /// Why one conversation read was skipped instead of probing the endpoint.
    fn conversation_unavailable_reason(observation: &RunObservation, label: &str) -> String {
        match observation.coverage.as_str() {
            COVERAGE_UNAVAILABLE => format!(
                "the run records observation coverage {} ({}); its {label} conversation is reported unavailable instead of being probed as a live run",
                observation.coverage,
                observation
                    .reason
                    .as_deref()
                    .unwrap_or("no reason recorded")
            ),
            _ => format!(
                "the run never reached an observed state (recorded state {}); its {label} conversation is reported unavailable instead of being probed",
                observation.state
            ),
        }
    }
    /// The run lifecycle recorded in the receipt, or an explicit unavailable
    /// observation carrying the reason. A legacy or malformed record is never
    /// guessed into a state.
    fn observation_of(document: &Value, notes: &mut CoverageNotes) -> RunObservation {
        if let Some(observation) = RunObservation::from_receipt(document) {
            return observation;
        }
        let reason = match document.get("observation") {
            Some(value) if !value.is_null() => {
                "the recorded run observation is malformed or incomplete"
            }
            _ => "the receipt records no run observation (a tui or legacy dispatch)",
        };
        notes.note(
            "run",
            format!(
                "{reason}; the run lifecycle, message visibility and tool outcomes are covered as unavailable instead of being guessed"
            ),
        );
        RunObservation::unavailable(reason)
    }

    /// The recorded assignment and lifecycle context of the observed run.
    fn context_of(document: &Value, slot: u64, observation: &RunObservation) -> EvidenceContext {
        let assignment = field_text(document, "/control/originalAssignment")
            .or_else(|| field_text(document, "/control/assignment"));
        EvidenceContext {
            slot,
            owner: field_text(document, "/slot/owner").unwrap_or_default(),
            checkout: field_text(document, "/slot/source").unwrap_or_default(),
            worktree: field_text(document, "/slot/path").unwrap_or_default(),
            bd: field_text(document, "/bd").unwrap_or_default(),
            assignment_id: field_text(document, "/assignmentId").unwrap_or_default(),
            assignment,
            state: observation.state.clone(),
            observation_coverage: observation.coverage.clone(),
            session: observation.recorded_session().map(str::to_owned),
            exit_code: observation.exit_code,
            cause: observation
                .cause
                .as_deref()
                .filter(|cause| !cause.is_empty())
                .map(|cause| excerpt(cause, EVIDENCE_DETAIL_BYTES)),
            updated_ms: observation.updated_ms,
            detail_truncated: observation.detail_truncated,
            activity_truncated: observation.activity_truncated,
            activity_gap: observation.activity_gap,
        }
    }

    /// One lifecycle event of the observed run. Its phase is the recorded
    /// state, so a later state is new evidence and an unchanged state is not.
    fn run_event(slot: u64, observation: &RunObservation) -> EvidenceEvent {
        let detail = format!(
            "coverage {}; state {}; events {}; messages {}; tool calls {}; rounds {}; malformed {}{}{}{}",
            observation.coverage,
            observation.state,
            observation.events,
            observation.messages,
            observation.tool_calls,
            observation.rounds,
            observation.malformed,
            if observation.detail_truncated {
                "; detail truncated"
            } else {
                ""
            },
            if observation.activity_truncated {
                "; activity truncated"
            } else {
                ""
            },
            if observation.activity_gap {
                "; activity gap"
            } else {
                ""
            },
        );
        EvidenceEvent {
            source: "run".to_owned(),
            slot,
            id: "run".to_owned(),
            turn: None,
            kind: "status".to_owned(),
            phase: observation.state.clone(),
            name: None,
            text: observation
                .cause
                .as_deref()
                .filter(|cause| !cause.is_empty())
                .map(|cause| excerpt(cause, EVIDENCE_DETAIL_BYTES)),
            exit_code: observation.exit_code.map(i64::from),
            detail: Some(detail),
            locator: observation
                .detail
                .as_ref()
                .map(|detail| detail.display().to_string()),
        }
    }
    /// Attaches to one recorded control conversation and reads its visible
    /// state. Every failure names itself in the coverage notes and returns
    /// `None`: an unreadable conversation never becomes empty evidence that
    /// looks like silence.
    fn read_conversation(
        spec: &ConversationSpec<'_>,
        notes: &mut CoverageNotes,
    ) -> Option<ConversationRead> {
        let endpoint = match control::Endpoint::read(spec.endpoint) {
            Ok(endpoint) => endpoint,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let extra = if spec.label == "lead" {
                    "; an ordinary main session exposes no managed lead endpoint, so this direction is reported unavailable here instead of being guessed"
                } else {
                    ""
                };
                notes.note(
                    spec.side,
                    format!(
                        "the run records no {} control endpoint at {}; its conversation is unavailable for this pass{extra}",
                        spec.label,
                        spec.endpoint.display()
                    ),
                );
                return None;
            }
            Err(error) => {
                notes.note(
                    spec.side,
                    format!(
                        "the recorded {} control endpoint {} is not usable: {error}; its conversation is unavailable for this pass",
                        spec.label,
                        spec.endpoint.display()
                    ),
                );
                return None;
            }
        };
        if let Some(expected) = spec.expected_thread
            && endpoint.thread_id.as_deref() != Some(expected)
        {
            notes.note(
                spec.side,
                format!(
                    "the recorded {} control endpoint names thread {} instead of the recorded thread {expected}; refusing to read another conversation",
                    spec.label,
                    endpoint.thread_id.as_deref().unwrap_or("(no thread)")
                ),
            );
            return None;
        }
        let mut conversation = match control::Conversation::attach(&endpoint, spec.bound) {
            Ok(conversation) => conversation,
            Err(error) => {
                notes.note(
                    spec.side,
                    format!(
                        "the recorded {} control endpoint {} did not accept a read-only attach: {error}; its conversation is unavailable for this pass",
                        spec.label,
                        spec.endpoint.display()
                    ),
                );
                return None;
            }
        };
        let thread = match conversation.thread_state() {
            Ok(thread) => thread,
            Err(error) => {
                notes.note(
                    spec.side,
                    format!(
                        "the recorded {} conversation state could not be read: {error}; its conversation is unavailable for this pass",
                        spec.label
                    ),
                );
                return None;
            }
        };
        let mut unmapped = 0u64;
        let (events, unscanned, user_texts) = collect_thread_items(
            &thread,
            spec.slot,
            spec.source,
            spec.correlation,
            spec.represented,
            &mut unmapped,
        );
        if unmapped > 0 {
            notes.note(
                spec.side,
                format!(
                    "{unmapped} visible {} item(s) carried no usable native identity or type and were not presented",
                    spec.label
                ),
            );
        }
        if unscanned > 0 {
            notes.note(
                spec.side,
                format!(
                    "{unscanned} older visible {} item(s) precede the newest {EVIDENCE_SCAN_LIMIT}-item window and were not scanned",
                    spec.label
                ),
            );
        }
        Some(ConversationRead { user_texts, events })
    }
    /// The visible items of one conversation's thread state, newest bounded:
    /// `(events, older-unscanned count, user-message text by client identity)`.
    /// `reasoning` items are never collected. On the lead side only the turns
    /// that answer one of the addressed lead messages are kept, and the
    /// envelope framing of a delivered lead message is stripped so the record
    /// carries the payload, not the transport; on the executor side the
    /// delivered inputs already recorded as messages are not presented a
    /// second time as items.
    fn collect_thread_items(
        thread: &Value,
        slot: u64,
        source: &str,
        correlation: &BTreeSet<String>,
        represented: &BTreeSet<String>,
        unmapped: &mut u64,
    ) -> (Vec<EvidenceEvent>, u64, BTreeMap<String, String>) {
        let mut events = Vec::new();
        let mut user_texts = BTreeMap::new();
        let lead_side = source == "lead-item";
        for turn in thread["turns"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let turn_id = turn["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
            let Some(items) = turn["items"].as_array() else {
                continue;
            };
            if lead_side
                && !items.iter().any(|item| {
                    item["type"].as_str() == Some("userMessage")
                        && control::item_client_id(item).is_some_and(|id| correlation.contains(id))
                })
            {
                continue;
            }
            for item in items {
                let Some(item_type) = item["type"].as_str() else {
                    *unmapped += 1;
                    continue;
                };
                if item_type == "reasoning" {
                    continue;
                }
                if item_type == "userMessage" || item_type == "user_message" {
                    if let Some(client) = control::item_client_id(item) {
                        let literal = control::user_message_text(item);
                        let stored = if lead_side {
                            literal
                                .split_once(executor_message::PAYLOAD_MARK)
                                .map(|(_, payload)| payload)
                                .unwrap_or(&literal)
                                .to_owned()
                        } else {
                            literal
                        };
                        if !stored.is_empty() {
                            user_texts
                                .entry(client.to_owned())
                                .or_insert_with(|| excerpt(&stored, EVIDENCE_TEXT_BYTES));
                        }
                        if lead_side || represented.contains(client) {
                            continue;
                        }
                    } else if lead_side {
                        continue;
                    }
                }
                match item_event(source, slot, item, turn_id.clone(), None) {
                    Some(event) => events.push(event),
                    None => *unmapped += 1,
                }
            }
        }
        let unscanned = events.len().saturating_sub(EVIDENCE_SCAN_LIMIT) as u64;
        if unscanned > 0 {
            events.drain(..unscanned as usize);
        }
        (events, unscanned, user_texts)
    }
    /// One visible conversation item as an event. Its identity is its native
    /// item (or client) id with the phase it was recorded in; `reasoning` is
    /// never collected, and unknown item types stay unknown instead of being
    /// presented as something they are not.
    fn item_event(
        source: &str,
        slot: u64,
        item: &Value,
        turn: Option<String>,
        phase: Option<&str>,
    ) -> Option<EvidenceEvent> {
        let item_type = item["type"].as_str().filter(|kind| !kind.is_empty())?;
        if item_type == "reasoning" {
            return None;
        }
        let id = item["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .or_else(|| control::item_client_id(item).filter(|id| !id.is_empty()))?
            .to_owned();
        let mut event = EvidenceEvent {
            source: source.to_owned(),
            slot,
            id,
            turn,
            kind: "item".to_owned(),
            phase: match phase {
                Some(phase) => phase.to_owned(),
                None => item["status"]
                    .as_str()
                    .filter(|status| !status.is_empty())
                    .unwrap_or("completed")
                    .to_owned(),
            },
            name: None,
            text: None,
            exit_code: None,
            detail: None,
            locator: None,
        };
        match tool_operation_kind(item_type) {
            Some(operation) => {
                event.kind = "tool".to_owned();
                match operation {
                    "commandExecution" => {
                        event.name = item["command"]
                            .as_str()
                            .filter(|command| !command.is_empty())
                            .map(|command| excerpt(command, EVIDENCE_DETAIL_BYTES));
                        event.exit_code = item["exitCode"]
                            .as_i64()
                            .or_else(|| item["exit_code"].as_i64());
                    }
                    "mcpToolCall" => {
                        let server = item["server"].as_str().filter(|name| !name.is_empty());
                        let tool = item["tool"].as_str().filter(|name| !name.is_empty());
                        event.name = match (server, tool) {
                            (Some(server), Some(tool)) => Some(format!("{server}.{tool}")),
                            (Some(server), None) => Some(server.to_owned()),
                            (None, Some(tool)) => Some(tool.to_owned()),
                            (None, None) => None,
                        };
                    }
                    "webSearch" => {
                        event.name = Some(operation.to_owned());
                        event.text = item["query"]
                            .as_str()
                            .filter(|query| !query.is_empty())
                            .map(|query| excerpt(query, EVIDENCE_DETAIL_BYTES));
                    }
                    _ => {
                        let changes = item["changes"]
                            .as_array()
                            .map(Vec::len)
                            .or_else(|| item["changes"].as_u64().map(|count| count as usize));
                        event.name = changes.map(|count| format!("{count} file(s)"));
                    }
                }
            }
            None => match item_type {
                "agentMessage" | "agent_message" => {
                    event.kind = "message".to_owned();
                    event.text = bounded_item_text(item, "text");
                }
                "userMessage" | "user_message" => {
                    event.kind = "message".to_owned();
                    let literal = control::user_message_text(item);
                    if !literal.is_empty() {
                        event.text = Some(excerpt(&literal, EVIDENCE_TEXT_BYTES));
                    }
                }
                _ => {
                    event.text = bounded_item_text(item, "text");
                }
            },
        }
        Some(event)
    }

    /// One bounded visible-text field of an item.
    fn bounded_item_text(item: &Value, field: &str) -> Option<String> {
        item[field]
            .as_str()
            .filter(|text| !text.is_empty())
            .map(|text| excerpt(text, EVIDENCE_TEXT_BYTES))
    }
    /// The delivered inputs one `messages` array records, in their recorded
    /// status. Text comes from the executor conversation that actually
    /// carries an inline input or from the recorded spill file when the
    /// delivery was spilled; delivery metadata is never presented as message
    /// content.
    fn record_message_events(
        slot: u64,
        messages: &[Value],
        read: Option<&ConversationRead>,
        notes: &mut CoverageNotes,
    ) -> Vec<EvidenceEvent> {
        let mut events = Vec::new();
        let mut unnamed = 0u64;
        let mut missing = 0u64;
        let mut capped = 0u64;
        for entry in messages {
            let Some(id) = entry["id"].as_str().filter(|id| !id.is_empty()) else {
                unnamed += 1;
                continue;
            };
            let (text, locator) = message_text(entry, read, &mut capped);
            if text.is_none() {
                missing += 1;
            }
            events.push(message_event(
                "message-to-executor",
                slot,
                entry,
                id,
                text,
                locator,
            ));
        }
        if unnamed > 0 {
            notes.note(
                "message-to-executor",
                format!(
                    "{unnamed} recorded delivery entries carry no message identity and were not presented"
                ),
            );
        }
        if missing > 0 {
            notes.note(
                "message-to-executor",
                format!(
                    "{missing} recorded message(s) name no readable content; the delivery metadata is not presented as message content"
                ),
            );
        }
        if capped > 0 {
            notes.note(
                "message-to-executor",
                format!(
                    "{capped} spilled message payload(s) exceed the {EVIDENCE_PAYLOAD_READ}-byte read bound; their retained text is a bounded excerpt"
                ),
            );
        }
        events
    }

    /// The lead messages one `leadMessages` array records, in their recorded
    /// status. The content digest of a record is never presented as content.
    fn record_lead_message_events(
        slot: u64,
        messages: &[Value],
        read: Option<&ConversationRead>,
        notes: &mut CoverageNotes,
    ) -> Vec<EvidenceEvent> {
        let mut events = Vec::new();
        let mut unnamed = 0u64;
        let mut missing = 0u64;
        let mut capped = 0u64;
        for entry in messages {
            let Some(id) = entry["id"].as_str().filter(|id| !id.is_empty()) else {
                unnamed += 1;
                continue;
            };
            let (text, locator) = lead_message_text(entry, read, &mut capped);
            if text.is_none() {
                missing += 1;
            }
            events.push(message_event(
                "message-to-lead",
                slot,
                entry,
                id,
                text,
                locator,
            ));
        }
        if unnamed > 0 {
            notes.note(
                "message-to-lead",
                format!(
                    "{unnamed} recorded lead delivery entries carry no message identity and were not presented"
                ),
            );
        }
        if missing > 0 {
            notes.note(
                "message-to-lead",
                format!(
                    "{missing} recorded lead message(s) name no readable content; the delivery metadata is not presented as message content"
                ),
            );
        }
        if capped > 0 {
            notes.note(
                "message-to-lead",
                format!(
                    "{capped} spilled lead payload(s) exceed the {EVIDENCE_PAYLOAD_READ}-byte read bound; their retained text is a bounded excerpt"
                ),
            );
        }
        events
    }

    /// One recorded message direction as an event. Its phase is the recorded
    /// delivery status, so queued, delivered, unknown and undelivered stay
    /// distinct, and its kind names the lead message kind when the record has
    /// one.
    fn message_event(
        source: &str,
        slot: u64,
        entry: &Value,
        id: &str,
        text: Option<String>,
        locator: Option<String>,
    ) -> EvidenceEvent {
        EvidenceEvent {
            source: source.to_owned(),
            slot,
            id: id.to_owned(),
            turn: entry["turnId"]
                .as_str()
                .filter(|turn| !turn.is_empty())
                .map(str::to_owned),
            kind: "message".to_owned(),
            phase: entry["status"]
                .as_str()
                .filter(|status| !status.is_empty())
                .unwrap_or("recorded")
                .to_owned(),
            name: entry["kind"]
                .as_str()
                .filter(|kind| !kind.is_empty())
                .map(str::to_owned),
            text,
            exit_code: None,
            detail: optional_detail(entry),
            locator,
        }
    }

    /// The retained text of one recorded message to the executor: the spill
    /// file when the delivery was spilled, otherwise the executor
    /// conversation's own input item.
    fn message_text(
        entry: &Value,
        read: Option<&ConversationRead>,
        capped: &mut u64,
    ) -> (Option<String>, Option<String>) {
        if let Some(path) = entry["payloadPath"]
            .as_str()
            .filter(|path| !path.is_empty())
        {
            let text = read_payload(Path::new(path), capped);
            return (text, Some(path.to_owned()));
        }
        let id = entry["id"].as_str().unwrap_or_default();
        let text = read
            .and_then(|read| read.user_texts.get(id))
            .map(|text| excerpt(text, EVIDENCE_TEXT_BYTES));
        (text, None)
    }

    /// The retained text of one recorded message to the lead: the spill file
    /// when the delivery was spilled, otherwise the payload the lead
    /// conversation recorded after its envelope framing.
    fn lead_message_text(
        entry: &Value,
        read: Option<&ConversationRead>,
        capped: &mut u64,
    ) -> (Option<String>, Option<String>) {
        if let Some(path) = entry["payloadPath"]
            .as_str()
            .filter(|path| !path.is_empty())
        {
            let text = read_payload(Path::new(path), capped);
            return (text, Some(path.to_owned()));
        }
        let id = entry["id"].as_str().unwrap_or_default();
        let text = read
            .and_then(|read| read.user_texts.get(id))
            .map(|text| excerpt(text, EVIDENCE_TEXT_BYTES));
        (text, None)
    }

    /// One recorded payload file, bounded. `capped` counts files longer than
    /// the read bound so the coverage says the retained text is an excerpt;
    /// the excerpt names the file's real size, never the read bound.
    fn read_payload(path: &Path, capped: &mut u64) -> Option<String> {
        let bytes = read_bounded(path, EVIDENCE_PAYLOAD_READ).ok()?;
        let length = fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(bytes.len() as u64);
        if length > EVIDENCE_PAYLOAD_READ {
            *capped += 1;
        }
        Some(excerpt_total(
            &String::from_utf8_lossy(&bytes),
            EVIDENCE_TEXT_BYTES,
            length,
        ))
    }

    /// One recorded detail string, bounded.
    fn optional_detail(entry: &Value) -> Option<String> {
        entry["detail"]
            .as_str()
            .filter(|detail| !detail.is_empty())
            .map(|detail| excerpt(detail, EVIDENCE_DETAIL_BYTES))
    }

    /// One non-empty string field of a recorded document.
    fn field_text(value: &Value, pointer: &str) -> Option<String> {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    }

    /// One parsed exec-route item in the control route's camelCase spelling,
    /// so both routes pass through one item mapper.
    fn summary_value(item: &ItemSummary) -> Value {
        json!({
            "id": item.id,
            "type": item.kind,
            "status": item.status,
            "text": item.text,
            "command": item.command,
            "exitCode": item.exit_code,
            "server": item.server,
            "tool": item.tool,
            "query": item.query,
            "changes": item.changes,
        })
    }
    /// Reads the raw detail stream's tail into bounded visible item events.
    /// Malformed and partially written records are counted, never presented
    /// as progress; a stream at its retention bound emits one stable gap
    /// event whose text states that a capped or stale tail proves no idle or
    /// stalled executor - newer visible work may simply be beyond the cap.
    /// `readable` is false only when a read step failed, so absence from a
    /// temporarily unreadable stream never evicts retained identities; a
    /// capped, partial or malformed tail is still a completed read of the
    /// retained window and is reported through the coverage notes.
    fn collect_detail(
        detail: &Path,
        slot: u64,
        observation: &RunObservation,
        notes: &mut CoverageNotes,
    ) -> DetailRead {
        let mut events = Vec::new();
        let length = match fs::metadata(detail) {
            Ok(metadata) => metadata.len(),
            Err(error) => {
                notes.note(
                    "detail",
                    format!(
                        "the recorded raw detail stream {} is not readable: {error}; its visible content is unavailable for this pass",
                        detail.display()
                    ),
                );
                return DetailRead {
                    events,
                    readable: false,
                };
            }
        };
        let truncated = observation.detail_truncated || length >= MAX_STREAM_BYTES;
        let start = length.saturating_sub(EVIDENCE_TAIL_READ);
        let mut file = match fs::File::open(detail) {
            Ok(file) => file,
            Err(error) => {
                notes.note(
                    "detail",
                    format!(
                        "the recorded raw detail stream {} could not be opened: {error}; its visible content is unavailable for this pass",
                        detail.display()
                    ),
                );
                return DetailRead {
                    events,
                    readable: false,
                };
            }
        };
        if start > 0 {
            let _ = file.seek(SeekFrom::Start(start));
        }
        let mut bytes = Vec::new();
        if let Err(error) = (&mut file).take(EVIDENCE_TAIL_READ).read_to_end(&mut bytes) {
            notes.note(
                "detail",
                format!(
                    "the retained tail of {} could not be read: {error}",
                    detail.display()
                ),
            );
            return DetailRead {
                events,
                readable: false,
            };
        }
        let mut tail = String::from_utf8_lossy(&bytes).into_owned();
        let mut partial = 0u64;
        if start > 0 {
            // The window begins mid-record unless the file fit the read
            // bound; the leading fragment belongs to an older record.
            match tail.find('\n') {
                Some(at) => tail = tail[at + 1..].to_owned(),
                None => {
                    tail.clear();
                    partial += 1;
                }
            }
        }
        if !tail.ends_with('\n') {
            // A record still being written is not complete yet. It is read
            // again by a later pass instead of being presented now.
            let keep = tail.rfind('\n').map(|at| at + 1).unwrap_or(0);
            tail.truncate(keep);
            partial += 1;
        }
        let mut malformed = 0u64;
        let mut current_turn: Option<String> = None;
        for line in tail.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                malformed += 1;
                continue;
            };
            // The control route appends whole notifications; an item
            // notification is the same visible evidence the exec route
            // appends as a bare JSONL event.
            if let Some(method) = value["method"].as_str() {
                if let Some(item) = value
                    .pointer("/params/item")
                    .filter(|item| item.is_object())
                {
                    let phase = match item["status"].as_str().filter(|status| !status.is_empty()) {
                        Some(status) => status.to_owned(),
                        None => match method.rsplit('/').next() {
                            Some("started" | "updated") => "in-progress".to_owned(),
                            _ => "completed".to_owned(),
                        },
                    };
                    if let Some(event) = item_event(
                        "executor-item",
                        slot,
                        item,
                        field_text(&value, "/params/turnId"),
                        Some(&phase),
                    ) {
                        events.push(event);
                    }
                }
                continue;
            }
            match parse_event(line) {
                Ok(NativeEvent::TurnStarted { turn_id }) => {
                    if turn_id.is_some() {
                        current_turn = turn_id;
                    }
                }
                Ok(NativeEvent::Item { phase, item }) => {
                    let phase = match phase {
                        ItemPhase::Started | ItemPhase::Updated => "in-progress",
                        ItemPhase::Completed => "completed",
                    };
                    let turn = event_turn_id(&value).or_else(|| current_turn.clone());
                    if let Some(event) = item_event(
                        "executor-item",
                        slot,
                        &summary_value(&item),
                        turn,
                        Some(phase),
                    ) {
                        events.push(event);
                    }
                }
                Ok(_) => {}
                Err(_) => malformed += 1,
            }
        }
        let unscanned = events.len().saturating_sub(EVIDENCE_SCAN_LIMIT) as u64;
        if unscanned > 0 {
            events.drain(..unscanned as usize);
        }
        if malformed + partial > 0 {
            notes.note(
                "detail",
                format!(
                    "{} raw detail record(s) were malformed or partial at the retention tail and were not presented as progress; the retained stream may hold a record cut at its {MAX_STREAM_BYTES}-byte bound",
                    malformed + partial
                ),
            );
        }
        if unscanned > 0 {
            notes.note(
                "detail",
                format!(
                    "{unscanned} older raw detail record(s) precede the newest {EVIDENCE_SCAN_LIMIT}-record window and were not scanned; their visible items stay readable from the conversations"
                ),
            );
        }
        if truncated {
            events.push(truncated_gap_event(detail, slot, length, observation));
        }
        DetailRead {
            events,
            readable: true,
        }
    }

    /// The one gap event a capped detail stream emits. Its identity is stable,
    /// so it is acknowledged once while the cap persists, and its text states
    /// plainly that a capped or stale tail proves no idleness.
    fn truncated_gap_event(
        detail: &Path,
        slot: u64,
        length: u64,
        observation: &RunObservation,
    ) -> EvidenceEvent {
        EvidenceEvent {
            source: "gap".to_owned(),
            slot,
            id: "detail-truncated".to_owned(),
            turn: None,
            kind: "gap".to_owned(),
            phase: "truncated".to_owned(),
            name: None,
            text: Some(format!(
                "the retained raw detail stream stops at its {MAX_STREAM_BYTES}-byte bound and its tail can hold a record cut by the cap; newer visible activity may exist beyond it, and a capped or stale detail tail is never evidence that the executor is idle or stalled{}",
                if observation.activity_truncated {
                    "; the compact activity marks are capped at their own bound too"
                } else {
                    ""
                }
            )),
            exit_code: None,
            detail: Some(format!(
                "retained detail length {length} bytes{}",
                if observation.detail_truncated {
                    ", recorded by the run as truncated"
                } else {
                    ""
                }
            )),
            locator: Some(detail.display().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn observation() -> RunObservation {
        RunObservation::accepted(
            PathBuf::from(r"C:\state\message-1.txt"),
            PathBuf::from(r"C:\state\stream-1.jsonl"),
        )
    }

    #[test]
    fn compact_activity_survives_a_full_detail_file() {
        let root = tempfile::tempdir().unwrap();
        let detail = root.path().join("stream.jsonl");
        fs::write(&detail, vec![b'x'; MAX_STREAM_BYTES as usize]).unwrap();
        assert!(!append_detail(&detail, "later").unwrap());
        let mut observation = observation();
        observation.detail_truncated = true;
        begin_clock(&mut observation);
        note_activity(
            &mut observation,
            "heavy-cmd-1",
            "commandExecution",
            "started",
        );
        note_activity(
            &mut observation,
            "heavy-cmd-1",
            "commandExecution",
            "completed",
        );
        end_clock(&mut observation);
        assert!(observation.detail_truncated);
        assert_eq!(observation.activity.len(), 2, "{:?}", observation.activity);
        assert!(
            observation
                .clock
                .as_ref()
                .is_some_and(|clock| clock.ended_qpc.is_some())
        );
        assert!(!observation.activity_truncated);
    }

    #[test]
    fn activity_receipt_is_not_the_work_instant() {
        let mut observation =
            RunObservation::accepted(PathBuf::from("result.txt"), PathBuf::from("detail.jsonl"));
        let before = harness_core::heavy_command_trace::qpc_now().expect("qpc");
        std::thread::sleep(Duration::from_millis(30));
        note_activity(&mut observation, "message-1", "agentMessage", "completed");
        let mark = &observation.activity[0];
        assert_eq!(mark.bound, "receipt");
        assert!(
            mark.qpc >= before,
            "the mark must be the receipt sample, not an earlier work instant"
        );
    }

    #[test]
    fn thread_identity_and_state_come_from_the_event_stream() {
        let mut tracker = RunTracker::new(observation());
        assert_eq!(tracker.observation.state, STATE_ACCEPTED);
        let started =
            parse_event(r#"{"type":"thread.started","thread_id":"01a0-session"}"#).unwrap();
        assert!(tracker.apply(&started));
        assert_eq!(tracker.observation.session.as_deref(), Some("01a0-session"));
        assert_eq!(tracker.observation.state, STATE_STARTED);
        let item = parse_event(
            r#"{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"done"}}"#,
        )
        .unwrap();
        tracker.apply(&item);
        assert_eq!(tracker.observation.state, STATE_RUNNING);
        assert_eq!(tracker.observation.messages, 1);
        let tool = parse_event(
            r#"{"type":"item.completed","item":{"id":"i2","type":"command_execution","command":"cargo test","aggregated_output":"ok","exit_code":0,"status":"completed"}}"#,
        )
        .unwrap();
        tracker.apply(&tool);
        assert_eq!(tracker.observation.tool_calls, 1);
        let turn = parse_event(
            r#"{"type":"turn.completed","usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":2}}"#,
        )
        .unwrap();
        tracker.apply(&turn);
        assert_eq!(tracker.observation.events, 4);
    }

    /// The aggregate tool counter and its per-operation breakdown count the
    /// same completed items, exec item spellings normalize to one operation
    /// vocabulary, and every completed turn counts once as a round.
    #[test]
    fn tool_counters_partition_by_operation_and_completed_turns_are_rounds() {
        let mut tracker = RunTracker::new(observation());
        for line in [
            r#"{"type":"thread.started","thread_id":"s-1"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"id":"c1","type":"command_execution"}}"#,
            r#"{"type":"item.completed","item":{"id":"c2","type":"command_execution"}}"#,
            r#"{"type":"item.completed","item":{"id":"w1","type":"web_search"}}"#,
            r#"{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"done"}}"#,
            r#"{"type":"turn.completed"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"id":"f1","type":"file_change"}}"#,
            r#"{"type":"turn.completed"}"#,
        ] {
            tracker.apply(&parse_event(line).unwrap());
        }
        assert_eq!(tracker.observation.messages, 1);
        assert_eq!(tracker.observation.tool_calls, 4);
        assert_eq!(tracker.observation.rounds, 2);
        let counts = &tracker.observation.tool_operation_counts;
        assert_eq!(counts.get("commandExecution"), Some(&2));
        assert_eq!(counts.get("webSearch"), Some(&1));
        assert_eq!(counts.get("fileChange"), Some(&1));
        assert_eq!(counts.get("mcpToolCall"), None);
        assert_eq!(
            counts.values().sum::<u64>(),
            tracker.observation.tool_calls,
            "the per-operation counters must partition the aggregate"
        );
    }

    /// The control route records the same per-operation counters under its own
    /// item spellings, and a committed completed turn counts once as a round
    /// while `running` establishes no round.
    #[test]
    fn control_items_record_operation_counters_and_completed_rounds() {
        let mut tracker = RunTracker::new(observation());
        tracker.apply_control(Some("running"), None, "thread-1");
        assert_eq!(tracker.observation.rounds, 0);
        for kind in [
            "agentMessage",
            "commandExecution",
            "mcpToolCall",
            "webSearch",
            "fileChange",
        ] {
            tracker.apply_control(Some("running"), Some(kind), "thread-1");
        }
        assert_eq!(tracker.observation.messages, 1);
        assert_eq!(tracker.observation.tool_calls, 4);
        let counts = &tracker.observation.tool_operation_counts;
        for operation in ["commandExecution", "mcpToolCall", "webSearch", "fileChange"] {
            assert_eq!(counts.get(operation), Some(&1), "{operation}");
        }
        tracker.apply_control(Some("completed"), None, "thread-1");
        assert_eq!(tracker.observation.rounds, 1);
        assert_eq!(tracker.observation.state, STATE_COMPLETED);
    }

    #[test]
    fn malformed_lines_and_unknown_events_never_fake_progress() {
        assert!(parse_event("not json").is_err());
        assert!(parse_event(r#"{"type":123}"#).is_err());
        assert!(parse_event(r#"{"type":"thread.started"}"#).is_err());
        assert!(parse_event(r#"{"type":"item.completed","item":{}}"#).is_err());
        let mut tracker = RunTracker::new(observation());
        assert!(tracker.count_malformed());
        assert_eq!(tracker.observation.malformed, 1);
        assert_eq!(tracker.observation.state, STATE_ACCEPTED);
        let unknown = parse_event(r#"{"type":"future.thing","payload":{}}"#).unwrap();
        assert_eq!(
            unknown,
            NativeEvent::Unknown {
                kind: "future.thing".into()
            }
        );
        tracker.apply(&unknown);
        assert_eq!(tracker.observation.state, STATE_ACCEPTED);
    }

    #[test]
    fn render_covers_messages_tools_and_lifecycle_without_reasoning() {
        let mut output = Vec::new();
        for line in [
            r#"{"type":"thread.started","thread_id":"s-1"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"id":"r","type":"reasoning","text":"secret chain of thought"}}"#,
            r#"{"type":"item.completed","item":{"id":"m","type":"agent_message","text":"done: all checks passed"}}"#,
            r#"{"type":"item.completed","item":{"id":"c","type":"command_execution","command":"cargo test -p x","aggregated_output":"noise","exit_code":0,"status":"completed"}}"#,
            r#"{"type":"item.completed","item":{"id":"f","type":"file_change","changes":[{"path":"a.rs","kind":"update"}],"status":"completed"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0}}"#,
        ] {
            render_event(&parse_event(line).unwrap(), &mut output).unwrap();
        }
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("native start (session s-1)"), "{text}");
        assert!(text.contains("assistant:"), "{text}");
        assert!(text.contains("done: all checks passed"), "{text}");
        assert!(text.contains("tool: exec cargo test -p x"), "{text}");
        assert!(text.contains("exit 0"), "{text}");
        assert!(text.contains("files: 1 changed"), "{text}");
        assert!(text.contains("turn completed"), "{text}");
        assert!(
            !text.contains("secret chain of thought"),
            "reasoning state is not part of the readable surface: {text}"
        );
    }

    #[test]
    fn finish_distinguishes_completion_failure_defect_and_start_error() {
        let root = tempfile::tempdir().unwrap();
        let result = root.path().join("message.txt");
        let mut tracker = RunTracker::new(observation());
        tracker.apply(&parse_event(r#"{"type":"thread.started","thread_id":"s"}"#).unwrap());
        tracker.apply(&parse_event(r#"{"type":"turn.completed"}"#).unwrap());
        fs::write(&result, "done").unwrap();
        assert_eq!(tracker.finish(0, Some(result.as_path())), 0);
        assert_eq!(tracker.observation.state, STATE_COMPLETED);

        fs::write(&result, "   \n").unwrap();
        let mut tracker = RunTracker::new(observation());
        tracker.apply(&parse_event(r#"{"type":"thread.started","thread_id":"s"}"#).unwrap());
        tracker.apply(&parse_event(r#"{"type":"turn.completed"}"#).unwrap());
        assert_eq!(tracker.finish(0, Some(result.as_path())), EXIT_DEFECT);
        assert_eq!(tracker.observation.state, STATE_DEFECT);
        assert!(
            tracker
                .observation
                .cause
                .as_deref()
                .unwrap()
                .contains("output defect"),
            "{:?}",
            tracker.observation.cause
        );

        fs::remove_file(&result).unwrap();
        let mut tracker = RunTracker::new(observation());
        tracker.apply(&parse_event(r#"{"type":"thread.started","thread_id":"s"}"#).unwrap());
        tracker.apply(&parse_event(r#"{"type":"turn.completed"}"#).unwrap());
        assert_eq!(tracker.finish(0, Some(result.as_path())), EXIT_DEFECT);
        assert!(
            tracker
                .observation
                .cause
                .as_deref()
                .unwrap()
                .contains("--output-last-message"),
            "{:?}",
            tracker.observation.cause
        );

        // A start error never produced identity; the child's own code wins.
        let mut tracker = RunTracker::new(observation());
        assert_eq!(tracker.finish(19, Some(result.as_path())), 19);
        assert_eq!(tracker.observation.state, STATE_FAILED);
        assert!(
            tracker
                .observation
                .cause
                .as_deref()
                .unwrap()
                .contains("native start was never observed"),
            "{:?}",
            tracker.observation.cause
        );

        // A truncated stream after native start is a failure, not completion.
        let mut tracker = RunTracker::new(observation());
        tracker.apply(&parse_event(r#"{"type":"thread.started","thread_id":"s"}"#).unwrap());
        assert_eq!(tracker.finish(0, Some(result.as_path())), EXIT_FAILED);
        assert!(
            tracker
                .observation
                .cause
                .as_deref()
                .unwrap()
                .contains("without turn.completed"),
            "{:?}",
            tracker.observation.cause
        );

        // A turn failure event is named even when the process exits 0.
        let mut tracker = RunTracker::new(observation());
        tracker.apply(&parse_event(r#"{"type":"thread.started","thread_id":"s"}"#).unwrap());
        tracker.apply(
            &parse_event(r#"{"type":"turn.failed","error":{"message":"quota exhausted"}}"#)
                .unwrap(),
        );
        assert_eq!(tracker.finish(0, Some(result.as_path())), EXIT_FAILED);
        assert_eq!(
            tracker.observation.cause.as_deref(),
            Some("quota exhausted")
        );
    }

    #[test]
    fn a_stale_turn_completion_does_not_finish_a_newer_turn() {
        let root = tempfile::tempdir().unwrap();
        let result = root.path().join("message.txt");
        fs::write(&result, "done").unwrap();

        let mut tracker = RunTracker::new(observation());
        tracker.apply(&parse_event(r#"{"type":"thread.started","thread_id":"s"}"#).unwrap());
        tracker.apply(&parse_event(r#"{"type":"turn.started","turn_id":"old"}"#).unwrap());
        tracker.apply(&parse_event(r#"{"type":"turn.completed","turn_id":"old"}"#).unwrap());
        tracker.apply(&parse_event(r#"{"type":"turn.started","turn_id":"new"}"#).unwrap());
        assert_ne!(
            tracker.finish(0, Some(result.as_path())),
            0,
            "a resumed turn must not inherit the predecessor completion"
        );

        let mut tracker = RunTracker::new(observation());
        tracker.apply(&parse_event(r#"{"type":"thread.started","thread_id":"s"}"#).unwrap());
        tracker.apply(&parse_event(r#"{"type":"turn.started","turn_id":"new"}"#).unwrap());
        tracker.apply(&parse_event(r#"{"type":"turn.completed","turn_id":"old"}"#).unwrap());
        assert_ne!(tracker.finish(0, Some(result.as_path())), 0);
        tracker.apply(&parse_event(r#"{"type":"turn.completed","turn_id":"new"}"#).unwrap());
        assert_eq!(tracker.finish(0, Some(result.as_path())), 0);

        let mut tracker = RunTracker::new(observation());
        tracker.apply(&parse_event(r#"{"type":"thread.started","thread_id":"s"}"#).unwrap());
        tracker.apply(
            &parse_event(
                r#"{"type":"turn.failed","turn_id":"old","error":{"message":"predecessor failed"}}"#,
            )
            .unwrap(),
        );
        tracker.apply(&parse_event(r#"{"type":"turn.started","turn_id":"new"}"#).unwrap());
        tracker.apply(&parse_event(r#"{"type":"turn.completed","turn_id":"new"}"#).unwrap());
        assert_eq!(tracker.finish(0, Some(result.as_path())), 0);
    }

    #[test]
    fn an_unresolved_reply_hold_is_not_completion_and_late_input_is_undelivered_once() {
        let open = json!({"replyRequests": [{"id": "q1", "status": "unresolved"}]});
        assert!(unresolved_reply_hold(&open));
        let notice =
            json!({"replyRequests": [{"id": "n1", "status": "unresolved", "kind": "notify"}]});
        assert!(!unresolved_reply_hold(&notice));
        let resolved = json!({"replyRequests": [{"id": "q1", "status": "resolved"}]});
        assert!(!unresolved_reply_hold(&resolved));
        assert!(!unresolved_reply_hold(&json!({})));
        let asked = json!({"leadMessages": [{
            "id": "lead-asked",
            "kind": "reply-request",
            "status": "delivered"
        }]});
        assert!(unresolved_reply_hold(&asked));
        let queued = json!({"leadMessages": [{
            "id": "lead-queued",
            "kind": "reply-request",
            "status": "queued"
        }]});
        assert!(unresolved_reply_hold(&queued));
        let uncertain = json!({"leadMessages": [{
            "id": "lead-uncertain",
            "kind": "reply-request",
            "status": "indeterminate"
        }]});
        assert!(unresolved_reply_hold(&uncertain));
        let lead_notice = json!({"leadMessages": [{
            "id": "lead-notice",
            "kind": "notification",
            "status": "delivered"
        }]});
        assert!(!unresolved_reply_hold(&lead_notice));
        let refused = json!({"leadMessages": [{
            "id": "lead-refused",
            "kind": "reply-request",
            "status": "error"
        }]});
        assert!(!unresolved_reply_hold(&refused));
        let answered = json!({"leadMessages": [{
            "id": "lead-answered",
            "kind": "reply-request",
            "status": "resolved"
        }]});
        assert!(!unresolved_reply_hold(&answered));
        let one_left = json!({"leadMessages": [
            {"id": "lead-answered", "kind": "reply-request", "status": "resolved"},
            {"id": "lead-open", "kind": "reply-request", "status": "delivered"}
        ]});
        assert!(unresolved_reply_hold(&one_left));

        let root = tempfile::tempdir().unwrap();
        let receipt = root.path().join("spawn-1.json");
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({"schema": 1})).unwrap(),
        )
        .unwrap();
        record_undelivered_input(&receipt, "input-1", "after closure").unwrap();
        record_undelivered_input(&receipt, "input-1", "repeat").unwrap();
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["messages"].as_array().unwrap().len(), 1);
        assert_eq!(value["messages"][0]["status"], "undelivered");
        assert_eq!(value["schema"], 1);
    }

    #[test]
    fn unresolved_reply_references_are_bounded_and_name_the_reply() {
        let receipt = json!({
            "replyRequests": [
                {"id": "open", "status": "unresolved", "requiresReply": true},
                {"id": "quiet", "status": "unresolved", "requiresReply": false},
                {"id": "notice", "status": "unresolved", "kind": "notify"},
                {"id": "answered", "status": "resolved", "requiresReply": true}
            ],
            "leadMessages": [
                {"id": "lead-1", "kind": "reply-request", "status": "delivered",
                 "leadThreadId": "01a0-lead", "session": "01a0-session"},
                {"id": "lead-2", "kind": "reply-request", "status": "resolved"},
                {"id": "lead-note", "kind": "notification", "status": "delivered"}
            ]
        });
        let (references, omitted) = unresolved_reply_references(&receipt, 4);
        assert_eq!(omitted, 0);
        let ids: Vec<&str> = references
            .iter()
            .map(|reference| reference.id.as_str())
            .collect();
        assert_eq!(ids, ["open", "lead-1"]);
        assert_eq!(references[0].record, "replyRequests");
        assert_eq!(references[1].record, "leadMessages");
        assert_eq!(references[1].lead_thread.as_deref(), Some("01a0-lead"));
        assert_eq!(references[1].session.as_deref(), Some("01a0-session"));
        assert!(unresolved_reply_hold(&receipt));

        let (bounded, omitted) = unresolved_reply_references(&receipt, 1);
        assert_eq!(bounded.len(), 1);
        assert_eq!(omitted, 1);
        assert!(
            unresolved_reply_hold(&receipt),
            "the hold is the request record, not the bounded list"
        );

        let long = json!({"replyRequests": [{
            "id": "x".repeat(4096),
            "status": "unresolved"
        }]});
        let (references, _) = unresolved_reply_references(&long, 1);
        assert!(
            references[0].id.len() < 512,
            "a hand-edited receipt must not grow model-visible output"
        );
    }

    #[test]
    fn receipt_updates_preserve_other_fields_and_legacy_receipts_read_empty() {
        let root = tempfile::tempdir().unwrap();
        let receipt = root.path().join("spawn-1.json");
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(
                &json!({"schema":1,"launcher":r"C:\x\codex.exe","slot":null}),
            )
            .unwrap(),
        )
        .unwrap();
        let mut recorded = observation();
        recorded.session = Some("s-1".into());
        update_receipt(&receipt, &recorded).unwrap();
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["launcher"], r"C:\x\codex.exe");
        let parsed = RunObservation::from_receipt(&value).expect("recorded observation");
        assert_eq!(parsed.session.as_deref(), Some("s-1"));
        assert_eq!(parsed.coverage, COVERAGE_NATIVE);
        update_receipt_field(&receipt, "window", json!({"columns": 80})).unwrap();
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["window"]["columns"], 80);
        assert_eq!(value["observation"]["session"], "s-1");
        assert!(RunObservation::from_receipt(&json!({"schema":1})).is_none());
        assert!(RunObservation::from_receipt(&json!({"observation": null})).is_none());
        // TUI coverage stays explicitly unavailable, with its reason.
        let tui = RunObservation::unavailable("tui mode records no native stream");
        assert_eq!(tui.state, STATE_UNOBSERVED);
        assert_eq!(tui.recorded_session(), None);
        // A resume attempt keeps the identity it carried over when it never
        // observed one itself.
        let mut carried = observation();
        carried.previous_session = Some("old-session".into());
        assert_eq!(carried.recorded_session(), Some("old-session"));
    }

    #[test]
    fn oversize_and_partial_lines_are_bounded() {
        let root = tempfile::tempdir().unwrap();
        let oversized = root.path().join("oversized.jsonl");
        fs::write(
            &oversized,
            [vec![b'a'; MAX_EVENT_LINE + 10], b"\nok\n".to_vec()].concat(),
        )
        .unwrap();
        let mut tail = SpoolTail::new(fs::File::open(&oversized).unwrap());
        drain_spool(&mut tail);
        match tail.take_line(false).expect("oversize line") {
            Line::Oversize(note) => assert!(note.contains("exceeded"), "{note}"),
            _ => panic!("oversize line must be dropped"),
        }
        // The line after the dropped one is still parsed normally.
        match tail.take_line(false).expect("following line") {
            Line::Text(line) => assert_eq!(line, "ok"),
            _ => panic!("the following line must survive"),
        }
        // A truncated final line (no newline) is still returned for parsing,
        // and the parser reports it as unparsed rather than completion.
        let partial = root.path().join("partial.jsonl");
        fs::write(&partial, br#"{"type":"thread.sta"#).unwrap();
        let mut tail = SpoolTail::new(fs::File::open(&partial).unwrap());
        drain_spool(&mut tail);
        match tail.take_line(true).expect("partial line") {
            Line::Text(line) => assert!(parse_event(&line).is_err()),
            _ => panic!("partial line must be returned"),
        }
    }

    /// Reads the whole spool the way the host tail loop does, in bounded
    /// increments.
    fn drain_spool(tail: &mut SpoolTail) {
        let mut reads = 0;
        while tail.read_available().unwrap() > 0 {
            reads += 1;
            assert!(reads < 10_000, "spool never reached its end");
        }
    }

    #[test]
    fn receipt_writers_preserve_concurrent_fields() {
        let root = tempfile::tempdir().unwrap();
        let receipt = root.path().join("spawn-1.json");
        fs::write(
            &receipt,
            serde_json::to_vec(&json!({"schema":1,"slot":null})).unwrap(),
        )
        .unwrap();
        let mut observation = observation();
        observation.session = Some("01a0c719-f4d4-7880-a9d2-1a96ee0f23f4".into());
        let writers: Vec<_> = (0..2)
            .map(|writer| {
                let receipt = receipt.clone();
                let observation = observation.clone();
                std::thread::spawn(move || {
                    for round in 0..150 {
                        if writer == 0 {
                            let mut step = observation.clone();
                            step.events = round;
                            update_receipt(&receipt, &step).unwrap();
                        } else {
                            update_receipt_field(&receipt, "window", json!({"round": round}))
                                .unwrap();
                        }
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        // The last observation write survives together with the console's
        // window field: neither writer erased the other's field.
        assert_eq!(value["window"]["round"], 149, "{value}");
        assert_eq!(value["observation"]["events"], 149, "{value}");
        assert_eq!(
            value["observation"]["session"],
            "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4"
        );
        // The stable lock file stays on disk unlocked; a killed writer's lock
        // is released by the OS, so no stale-file takeover is needed.
        let lock_path = receipt.with_extension("lock");
        assert!(lock_path.exists());
        assert!(
            ExclusiveFileLock::try_acquire(&lock_path)
                .unwrap()
                .is_some(),
            "the writer lock is released when its holder finishes"
        );
        assert!(
            fs::read_dir(root.path())
                .unwrap()
                .flatten()
                .all(|entry| !entry.file_name().to_string_lossy().ends_with(".tmp")),
            "no temp file survives a completed write"
        );
    }

    #[test]
    fn a_receipt_writer_lock_is_waited_out_not_stolen() {
        let root = tempfile::tempdir().unwrap();
        let receipt = root.path().join("spawn-1.json");
        fs::write(&receipt, serde_json::to_vec(&json!({"schema":1})).unwrap()).unwrap();
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let release = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let holder = {
            let receipt = receipt.clone();
            let release = release.clone();
            std::thread::spawn(move || {
                with_receipt_lock(&receipt, || {
                    held_tx.send(()).unwrap();
                    while !release.load(std::sync::atomic::Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    let mut value = read_receipt_value(&receipt)?;
                    value["window"] = json!({"columns": 80});
                    write_receipt(&receipt, &value)
                })
                .unwrap();
            })
        };
        held_rx.recv().unwrap();
        let waiter = {
            let receipt = receipt.clone();
            std::thread::spawn(move || {
                update_receipt_field(&receipt, "window", json!({"columns": 120}))
            })
        };
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !waiter.is_finished(),
            "a live writer lock must not be stolen"
        );
        release.store(true, std::sync::atomic::Ordering::Relaxed);
        holder.join().unwrap();
        waiter.join().unwrap().unwrap();
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["window"]["columns"], 120, "{value}");
    }

    /// A dispatch receipt whose recorded lifecycle is `state`, with the one
    /// queued and one delivered message a stop must classify.
    fn stop_receipt(root: &Path, state: &str) -> PathBuf {
        let receipt = root.join("spawn-1.json");
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "slot": {"index": 1, "owner": "exec-ds-7", "path": r"C:\pool\proj-wt1", "base": "abc"},
                "messages": [
                    {"id": "m-queued", "status": "queued"},
                    {"id": "m-delivered", "status": "delivered"}
                ],
                "observation": {
                    "schema": 1,
                    "coverage": "native",
                    "state": state,
                    "session": "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4",
                    "result": r"C:\state\message-1.txt",
                    "detail": r"C:\state\stream-1.jsonl",
                    "host": {"pid": 4242, "created": 99, "program": r"C:\kit\codex-harness.exe"}
                }
            }))
            .unwrap(),
        )
        .unwrap();
        receipt
    }

    fn stop_record(outcome: &str) -> StopRecord {
        StopRecord {
            schema: STOP_SCHEMA,
            outcome: outcome.into(),
            requested_ms: 1_000,
            completed_ms: 0,
            duration_ms: 0,
            detail: "the recorded host was terminated and its identity verified".into(),
            host: Some(HostIdentity {
                pid: 4242,
                created: 99,
                program: PathBuf::from(r"C:\kit\codex-harness.exe"),
            }),
            interrupt: None,
            ended: 1,
            survivors: Vec::new(),
            surface: Some("the recorded tab closed".into()),
            undelivered: Vec::new(),
            repeats: 0,
            repeated_ms: None,
            exit_code: None,
        }
    }

    #[test]
    fn stop_records_its_outcome_without_releasing_or_fabricating_an_exit_code() {
        let root = tempfile::tempdir().unwrap();
        let receipt = stop_receipt(root.path(), STATE_RUNNING);
        let mut stop = stop_record(STOP_STOPPED);
        stop.completed_ms = 1_250;
        stop.duration_ms = 250;
        let commit = commit_stop(&receipt, &mut stop, StopTransition::Stopped).unwrap();
        assert_eq!(commit.outcome, STOP_STOPPED);
        assert_eq!(commit.state, STATE_STOPPED);
        assert_eq!(stop.undelivered, vec!["m-queued".to_owned()]);
        assert_eq!(
            stop.exit_code, None,
            "an unobserved exit code stays unknown"
        );
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["observation"]["state"], STATE_STOPPED, "{value}");
        assert_eq!(value["stop"]["outcome"], STOP_STOPPED);
        assert_eq!(value["stop"]["durationMs"], 250);
        assert!(value["stop"]["exitCode"].is_null(), "{value}");
        assert!(value["observation"]["exitCode"].is_null(), "{value}");
        assert_eq!(value["messages"][0]["status"], "undelivered");
        assert_eq!(value["messages"][0]["undeliveredMs"], 1_250);
        assert_eq!(value["messages"][1]["status"], "delivered");
        // The recorded run is never released, reset or completed by a stop.
        assert_eq!(value["slot"]["owner"], "exec-ds-7", "{value}");
        assert!(value["slot"]["disposition"].is_null(), "{value}");
        assert_eq!(
            value["observation"]["session"],
            "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4"
        );
        assert_eq!(value["observation"]["result"], r"C:\state\message-1.txt");
        // A partial stop records its named survivor and is not success.
        let mut partial = stop_record(STOP_PARTIAL);
        partial.survivors.push(StopSurvivor {
            kind: "process".into(),
            pid: Some(5150),
            created: Some(77),
            image: Some(PathBuf::from(r"C:\kit\child.exe")),
            surface: None,
            cause: "the recorded process did not end within the bound".into(),
            next_action: "terminate pid 5150 yourself".into(),
        });
        let commit = commit_stop(&receipt, &mut partial, StopTransition::Partial).unwrap();
        assert_eq!(commit.outcome, STOP_PARTIAL);
        assert_eq!(commit.state, STATE_PARTIAL_STOP);
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["observation"]["state"], STATE_PARTIAL_STOP, "{value}");
        assert_eq!(value["stop"]["survivors"][0]["pid"], 5150);
        assert_eq!(
            value["stop"]["survivors"][0]["nextAction"],
            "terminate pid 5150 yourself"
        );
        // An error keeps the recorded lifecycle and only reports the refusal.
        let mut refusal = stop_record(STOP_ERROR);
        refusal.detail = "no live process matches the recorded host identity".into();
        let commit = commit_stop(&receipt, &mut refusal, StopTransition::Keep).unwrap();
        assert_eq!(commit.outcome, STOP_ERROR);
        assert_eq!(commit.state, STATE_PARTIAL_STOP);
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["stop"]["outcome"], STOP_ERROR);
        // A stop that found the host already gone records the unobserved end
        // with the honest unknown exit code and never claims a stop.
        let receipt = stop_receipt(root.path(), STATE_RUNNING);
        let mut unobserved = stop_record(STOP_ERROR);
        unobserved.detail = "no live process matches the recorded host identity".into();
        commit_stop(&receipt, &mut unobserved, StopTransition::Unobserved).unwrap();
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["observation"]["state"], STATE_INTERRUPTED, "{value}");
        assert!(value["observation"]["exitCode"].is_null(), "{value}");
        assert!(
            value["observation"]["cause"]
                .as_str()
                .is_some_and(|cause| cause.contains("unknown")),
            "{value}"
        );
        assert_ne!(value["stop"]["outcome"], STOP_STOPPED);
    }

    #[test]
    fn cleanup_failure_record_does_not_rewrite_the_assignment_outcome() {
        let root = tempfile::tempdir().unwrap();
        let receipt = root.path().join("spawn-1.json");
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "slot": {"owner": "exec-ds-7"},
                "observation": {
                    "schema": 1,
                    "coverage": "native",
                    "state": STATE_DEFECT,
                    "exitCode": EXIT_DEFECT,
                    "session": "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4",
                    "result": r"C:\state\message-1.txt"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        record_cleanup(
            &receipt,
            &CleanupRecord {
                schema: CLEANUP_SCHEMA,
                closed: false,
                survivors: vec![CleanupSurvivor {
                    kind: "frontend".into(),
                    pid: 77,
                    created: 88,
                    image: PathBuf::from(r"C:\kit\harness-frontend-double.exe"),
                    cause: "surviving owned frontend pid 77 was still running".into(),
                    next_action: CLEANUP_RECOVERY.into(),
                }],
                recovery: CLEANUP_RECOVERY.into(),
            },
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["observation"]["state"], STATE_DEFECT, "{value}");
        assert_eq!(value["observation"]["exitCode"], EXIT_DEFECT, "{value}");
        assert_eq!(
            value["observation"]["session"],
            "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4"
        );
        assert_eq!(value["slot"]["owner"], "exec-ds-7");
        assert_eq!(value["cleanup"]["closed"], false, "{value}");
        assert_eq!(value["cleanup"]["survivors"][0]["pid"], 77);
        assert!(
            value["cleanup"]["recovery"]
                .as_str()
                .unwrap()
                .contains("does not change the recorded assignment outcome"),
            "{value}"
        );
    }

    #[test]
    fn a_stop_racing_natural_completion_reports_the_completed_result() {
        let root = tempfile::tempdir().unwrap();
        let receipt = stop_receipt(root.path(), STATE_RUNNING);
        // The stop read the receipt while the run was still working; the host
        // completed it before the stop outcome was committed.
        let mut completed: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        completed["observation"]["state"] = json!(STATE_COMPLETED);
        completed["observation"]["exitCode"] = json!(0);
        fs::write(&receipt, serde_json::to_vec_pretty(&completed).unwrap()).unwrap();
        let mut stop = stop_record(STOP_STOPPED);
        stop.survivors.push(StopSurvivor {
            kind: "process".into(),
            pid: Some(1),
            created: Some(2),
            image: None,
            surface: None,
            cause: "not confirmed".into(),
            next_action: "inspect".into(),
        });
        let commit = commit_stop(&receipt, &mut stop, StopTransition::Stopped).unwrap();
        assert_eq!(commit.outcome, STOP_ALREADY_COMPLETED);
        assert_eq!(commit.state, STATE_COMPLETED);
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["observation"]["state"], STATE_COMPLETED, "{value}");
        assert_eq!(value["observation"]["exitCode"], 0, "{value}");
        assert_eq!(value["stop"]["outcome"], STOP_ALREADY_COMPLETED);
        assert_eq!(
            value["stop"]["survivors"],
            json!([]),
            "a completion race records no stop survivors: {value}"
        );
        assert!(
            value["stop"]["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("completed while stop was verifying")),
            "{value}"
        );
        // The recorded result stays readable for the lead.
        assert_eq!(value["observation"]["result"], r"C:\state\message-1.txt");
    }

    #[test]
    fn frontend_loss_keeps_a_retained_completion_and_otherwise_interrupts() {
        let root = tempfile::tempdir().unwrap();
        let result = root.path().join("message-1.txt");
        fs::write(&result, "kept result").unwrap();
        let receipt = root.path().join("spawn-1.json");
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "slot": {"owner": "exec-ds-7"},
                "observation": {
                    "schema": 1,
                    "coverage": "native",
                    "state": STATE_COMPLETED,
                    "exitCode": 0,
                    "session": "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4",
                    "result": &result,
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let mut tracker = RunTracker::new(RunObservation::accepted(
            result.clone(),
            root.path().join("stream.jsonl"),
        ));
        tracker.observation.session = Some("should-not-replace".into());
        tracker.observation.state = STATE_RUNNING.into();
        let kept = commit_frontend_loss(&receipt, &mut tracker, "frontend exited".into()).unwrap();
        assert!(kept);
        assert_eq!(tracker.observation.state, STATE_COMPLETED);
        assert_eq!(tracker.observation.exit_code, Some(0));
        assert_eq!(
            tracker.observation.session.as_deref(),
            Some("01a0c719-f4d4-7880-a9d2-1a96ee0f23f4")
        );
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["observation"]["state"], STATE_COMPLETED, "{value}");
        assert_eq!(value["observation"]["exitCode"], 0, "{value}");
        assert_eq!(value["slot"]["owner"], "exec-ds-7");

        let running = root.path().join("spawn-2.json");
        fs::write(
            &running,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "observation": {
                    "schema": 1,
                    "coverage": "native",
                    "state": STATE_RUNNING,
                    "session": "session-kept",
                    "result": &result,
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let recorded = RunObservation::from_receipt(
            &serde_json::from_slice::<Value>(&fs::read(&running).unwrap()).unwrap(),
        )
        .unwrap();
        let mut tracker = RunTracker::new(recorded);
        let kept = commit_frontend_loss(
            &running,
            &mut tracker,
            "the owned native frontend exited".into(),
        )
        .unwrap();
        assert!(!kept);
        assert_eq!(tracker.observation.state, STATE_INTERRUPTED);
        assert_eq!(tracker.observation.session.as_deref(), Some("session-kept"));
        assert!(tracker.observation.exit_code.is_none());
        let value: Value = serde_json::from_slice(&fs::read(&running).unwrap()).unwrap();
        assert_eq!(value["observation"]["state"], STATE_INTERRUPTED, "{value}");
        assert_eq!(value["observation"]["session"], "session-kept");
        assert!(
            value["observation"]["cause"]
                .as_str()
                .is_some_and(|cause| cause.contains("frontend exited")),
            "{value}"
        );
    }

    #[test]
    fn a_repeated_stop_keeps_the_first_stop_and_counts_the_repeat() {
        let root = tempfile::tempdir().unwrap();
        let receipt = stop_receipt(root.path(), STATE_RUNNING);
        let mut first = stop_record(STOP_STOPPED);
        first.completed_ms = 2_000;
        first.duration_ms = 750;
        commit_stop(&receipt, &mut first, StopTransition::Stopped).unwrap();
        let recorded: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(recorded["stop"]["repeats"], 0);
        // The repeated request reports the recorded state and counts itself.
        let mut repeat: StopRecord = serde_json::from_value(recorded["stop"].clone()).unwrap();
        repeat.repeats += 1;
        repeat.repeated_ms = Some(9_000);
        let commit = commit_stop(&receipt, &mut repeat, StopTransition::Keep).unwrap();
        assert_eq!(commit.outcome, STOP_STOPPED);
        assert_eq!(commit.state, STATE_STOPPED);
        let repeated: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(repeated["stop"]["outcome"], STOP_STOPPED);
        assert_eq!(repeated["stop"]["repeats"], 1);
        assert_eq!(repeated["stop"]["repeatedMs"], 9_000);
        assert_eq!(repeated["stop"]["completedMs"], 2_000);
        assert_eq!(repeated["stop"]["durationMs"], 750);
        assert_eq!(repeated["observation"]["state"], STATE_STOPPED);
    }

    // ---------------------------------------------------------- evidence ---
    //
    // The model-free evidence collector: one bounded batch at a time over one
    // originating-lead run generation, reading the records that already exist
    // (dispatch receipt, control conversations, spilled payloads, raw detail
    // tail) and never a model or an opaque stream.

    #[path = "../../tests/fixtures/control_endpoint.rs"]
    mod control_endpoint;

    use super::super::{executor_input, executor_message};

    fn evidence_token(letter: char) -> String {
        letter.to_string().repeat(64)
    }

    fn evidence_request<'a>(
        home: &'a Path,
        generation: &'a str,
        state: &'a Path,
    ) -> evidence::EvidenceRequest<'a> {
        evidence::EvidenceRequest {
            home,
            run_generation: generation,
            state_receipt: state,
            bound: Duration::from_secs(5),
            max_events: 64,
        }
    }

    fn evidence_seed_run(
        home: &Path,
        generation: &str,
        slot: u64,
        observation: &RunObservation,
    ) -> PathBuf {
        let directory = home.join("harness/executor-pool/checkout-1");
        fs::create_dir_all(&directory).unwrap();
        let receipt = directory.join(format!("spawn-{slot}.json"));
        write_receipt_document(
            &receipt,
            &json!({
                "schema": 1,
                "slot": {
                    "index": slot,
                    "owner": "fixture-owner",
                    "source": "C:/fixture/source",
                    "path": "C:/fixture/worktree",
                },
                "bd": "codex-harness-8me.1.2",
                "assignmentId": "fixture-task",
                "control": {"assignment": "record the evidence foundation"},
                "originatingLead": {"runGeneration": generation, "threadId": "thread-lead"},
            }),
        )
        .unwrap();
        update_receipt(&receipt, observation).unwrap();
        receipt
    }

    fn evidence_running(detail: &Path) -> RunObservation {
        let mut observation =
            RunObservation::accepted(PathBuf::from("C:/fixture/result.txt"), detail.to_path_buf());
        observation.state = STATE_RUNNING.to_owned();
        observation.session = Some("thread-exec".to_owned());
        observation
    }

    fn evidence_plant_endpoint(home: &Path, name: &str, port: u16, token: &str, thread: &str) {
        let directory = home.join("harness/executor-pool/checkout-1");
        fs::write(directory.join(format!("{name}.token")), token).unwrap();
        fs::write(
            directory.join(name),
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "port": port,
                "token": token,
                "threadId": thread,
                "process": {
                    "pid": std::process::id(),
                    "creationTime": 1,
                    "program": "harness-control-fixture",
                },
            }))
            .unwrap(),
        )
        .unwrap();
    }

    /// Both recorded conversations answering with an empty thread. The two
    /// servers must outlive the claim they serve.
    fn evidence_quiet_conversations(
        home: &Path,
    ) -> (control_endpoint::Server, control_endpoint::Server) {
        let executor = control_endpoint::Server::start(control_endpoint::Bearer::File(
            home.join("harness/executor-pool/checkout-1/endpoint-1.json.token"),
        ));
        executor.answer(
            "thread/read",
            control_endpoint::Answer::Result(json!({"thread": {"id": "thread-exec", "turns": []}})),
        );
        evidence_plant_endpoint(
            home,
            "endpoint-1.json",
            executor.port,
            &evidence_token('e'),
            "thread-exec",
        );
        let lead = control_endpoint::Server::start(control_endpoint::Bearer::File(
            home.join("harness/executor-pool/checkout-1/lead-endpoint-1.json.token"),
        ));
        lead.answer(
            "thread/read",
            control_endpoint::Answer::Result(json!({"thread": {"id": "thread-lead", "turns": []}})),
        );
        evidence_plant_endpoint(
            home,
            "lead-endpoint-1.json",
            lead.port,
            &evidence_token('l'),
            "thread-lead",
        );
        (executor, lead)
    }

    fn evidence_detail(records: &[String]) -> String {
        let mut text = records.join("\n");
        text.push('\n');
        text
    }

    fn evidence_started_line() -> String {
        r#"{"type":"thread.started","thread_id":"thread-exec"}"#.to_owned()
    }

    fn evidence_message_line(id: &str, text: &str) -> String {
        json!({
            "type": "item.completed",
            "item": {"id": id, "type": "agent_message", "text": text},
        })
        .to_string()
    }

    fn evidence_ready(home: &Path, generation: &str) -> evidence::EvidenceBatch {
        let state = home.join("state.json");
        let request = evidence_request(home, generation, &state);
        match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::Ready(batch) => batch,
            other => panic!("expected new evidence for generation {generation}: {other:?}"),
        }
    }

    fn evidence_acknowledge(
        home: &Path,
        generation: &str,
        batch_id: &str,
        ack: &evidence::EvidenceAck,
    ) -> evidence::EvidenceAckOutcome {
        let state = home.join("state.json");
        let request = evidence_request(home, generation, &state);
        evidence::acknowledge(&request, batch_id, ack).unwrap()
    }

    fn evidence_event<'a>(
        batch: &'a evidence::EvidenceBatch,
        id: &str,
    ) -> &'a evidence::EvidenceEvent {
        batch
            .events
            .iter()
            .find(|event| event.id == id)
            .unwrap_or_else(|| panic!("no evidence event {id} in {:?}", batch.events))
    }

    fn evidence_read_state(home: &Path) -> Value {
        serde_json::from_slice(&fs::read(home.join("state.json")).unwrap()).unwrap()
    }
    #[test]
    fn evidence_unchanged_pass_is_a_no_op() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[evidence_message_line("a1", "first visible message")]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        evidence_seed_run(home.path(), "gen-quiet", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());

        let batch = evidence_ready(home.path(), "gen-quiet");
        assert!(
            batch
                .events
                .iter()
                .any(|event| event.source == "run" && event.phase == STATE_RUNNING)
        );
        evidence_event(&batch, "a1");
        let outcome = evidence_acknowledge(
            home.path(),
            "gen-quiet",
            &batch.batch_id,
            &evidence::EvidenceAck::NoFinding,
        );
        assert!(matches!(
            outcome,
            evidence::EvidenceAckOutcome::Advanced {
                no_finding: true,
                ..
            }
        ));

        let state = home.path().join("state.json");
        let request = evidence_request(home.path(), "gen-quiet", &state);
        match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::NoNewEvidence { contexts, coverage } => {
                assert_eq!(contexts.len(), 1);
                assert!(
                    coverage.is_empty(),
                    "an unchanged readable pass adds no coverage limit: {coverage:?}"
                );
            }
            other => panic!("an unchanged pass must not imply another model turn: {other:?}"),
        }
        // Silence stays silent: no active batch is left behind for an acknowledgment.
        let refused =
            evidence::acknowledge(&request, &batch.batch_id, &evidence::EvidenceAck::NoFinding)
                .unwrap();
        assert!(matches!(
            refused,
            evidence::EvidenceAckOutcome::Refused { .. }
        ));
    }

    #[test]
    fn evidence_bounded_batches_retain_arrivals_and_hold_one_active_batch() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[
                evidence_message_line("a1", "first"),
                evidence_message_line("a2", "second"),
                evidence_message_line("a3", "third"),
            ]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        evidence_seed_run(home.path(), "gen-bounded", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());

        let state = home.path().join("state.json");
        let mut request = evidence_request(home.path(), "gen-bounded", &state);
        request.max_events = 2;
        let first = match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::Ready(batch) => batch,
            other => panic!("expected a bounded batch: {other:?}"),
        };
        assert_eq!(first.events.len(), 2);
        assert_eq!(first.pending, 2);
        let first_ids: Vec<&str> = first.events.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(first_ids, ["run", "a1"]);

        match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::BatchInFlight {
                batch_id,
                created_ms,
            } => {
                assert_eq!(
                    batch_id, first.batch_id,
                    "the active batch stays authoritative"
                );
                assert_eq!(
                    created_ms, first.created_ms,
                    "the in-flight report carries the active batch creation instant"
                );
            }
            other => panic!("a second claim must not start a parallel batch: {other:?}"),
        }

        // Evidence arriving while the batch is being analyzed is retained.
        fs::write(
            &detail,
            evidence_detail(&[
                evidence_message_line("a1", "first"),
                evidence_message_line("a2", "second"),
                evidence_message_line("a3", "third"),
                evidence_message_line("a4", "arrived during analysis"),
            ]),
        )
        .unwrap();

        let outcome = evidence::acknowledge(
            &request,
            &first.batch_id,
            &evidence::EvidenceAck::Finding {
                reference: "codex-harness-8me.1.2".to_owned(),
            },
        )
        .unwrap();
        assert!(matches!(
            outcome,
            evidence::EvidenceAckOutcome::Advanced {
                acked: 2,
                no_finding: false
            }
        ));

        request.max_events = 64;
        let second = match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::Ready(batch) => batch,
            other => panic!("the continuation must carry the retained events: {other:?}"),
        };
        let ids: Vec<&str> = second
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect();
        assert_eq!(ids, ["a2", "a3", "a4"]);
        assert_eq!(second.pending, 0);
        assert_eq!(second.run_generation, "gen-bounded");
    }
    #[test]
    fn evidence_unknown_outcome_preserves_and_replays() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[evidence_message_line("a1", "only visible work")]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        evidence_seed_run(home.path(), "gen-replay", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());

        let first = evidence_ready(home.path(), "gen-replay");
        let outcome = evidence_acknowledge(
            home.path(),
            "gen-replay",
            &first.batch_id,
            &evidence::EvidenceAck::Unknown {
                reason: "the analysis failed before it retained a finding".to_owned(),
            },
        );
        let preserved_reason = match outcome {
            evidence::EvidenceAckOutcome::Preserved { reason } => reason,
            other => panic!("an unknown outcome must preserve the batch: {other:?}"),
        };
        assert_eq!(
            preserved_reason, "the analysis failed before it retained a finding",
            "the preserved outcome reports the unknown analysis reason"
        );
        assert_eq!(
            evidence_read_state(home.path())["evidence"]["lastAck"]["outcome"],
            "unknown"
        );

        let second = evidence_ready(home.path(), "gen-replay");
        let identity = |batch: &evidence::EvidenceBatch| -> Vec<(String, String, String)> {
            batch
                .events
                .iter()
                .map(|event| (event.source.clone(), event.id.clone(), event.phase.clone()))
                .collect()
        };
        assert_eq!(
            identity(&first),
            identity(&second),
            "an unknown outcome replays the same evidence instead of skipping it"
        );

        let outcome = evidence_acknowledge(
            home.path(),
            "gen-replay",
            &second.batch_id,
            &evidence::EvidenceAck::Finding {
                reference: "codex-harness-8me.1.2".to_owned(),
            },
        );
        assert!(matches!(
            outcome,
            evidence::EvidenceAckOutcome::Advanced { acked: 2, .. }
        ));
        assert_eq!(
            evidence_read_state(home.path())["evidence"]["lastAck"]["outcome"],
            "finding"
        );
        let state = home.path().join("state.json");
        let request = evidence_request(home.path(), "gen-replay", &state);
        assert!(matches!(
            evidence::claim(&request).unwrap(),
            evidence::EvidenceClaim::NoNewEvidence { .. }
        ));
    }

    #[test]
    fn evidence_ack_refusals_keep_the_batch_active() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[evidence_message_line("a1", "work")]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        evidence_seed_run(home.path(), "gen-refuse", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());

        let batch = evidence_ready(home.path(), "gen-refuse");
        let state = home.path().join("state.json");
        let request = evidence_request(home.path(), "gen-refuse", &state);

        let outcome =
            evidence::acknowledge(&request, "batch-99", &evidence::EvidenceAck::NoFinding).unwrap();
        assert!(matches!(
            outcome,
            evidence::EvidenceAckOutcome::Refused { .. }
        ));
        match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::BatchInFlight { batch_id, .. } => {
                assert_eq!(batch_id, batch.batch_id)
            }
            other => panic!("a refused acknowledgment must not release the batch: {other:?}"),
        }

        let outcome = evidence::acknowledge(
            &request,
            &batch.batch_id,
            &evidence::EvidenceAck::Finding {
                reference: "   ".to_owned(),
            },
        )
        .unwrap();
        assert!(matches!(
            outcome,
            evidence::EvidenceAckOutcome::Refused { .. }
        ));
        match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::BatchInFlight { .. } => {}
            other => panic!("an unnamed finding must not release the batch: {other:?}"),
        }

        assert!(matches!(
            evidence::acknowledge(&request, &batch.batch_id, &evidence::EvidenceAck::NoFinding)
                .unwrap(),
            evidence::EvidenceAckOutcome::Advanced { .. }
        ));
        assert!(matches!(
            evidence::acknowledge(&request, &batch.batch_id, &evidence::EvidenceAck::NoFinding)
                .unwrap(),
            evidence::EvidenceAckOutcome::Refused { .. }
        ));
    }

    #[test]
    fn evidence_cursor_eviction_is_reported() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[evidence_message_line("a1", "work")]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        evidence_seed_run(home.path(), "gen-retention", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());
        let state = home.path().join("state.json");
        fs::write(
            &state,
            serde_json::to_vec_pretty(&json!({"evidence": {
                "schema": 1,
                "generation": "gen-retention",
                "cursorEvicted": true,
            }}))
            .unwrap(),
        )
        .unwrap();

        let request = evidence_request(home.path(), "gen-retention", &state);
        let batch = match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::Ready(batch) => batch,
            other => panic!("expected a batch: {other:?}"),
        };
        let note = batch
            .coverage
            .iter()
            .find(|note| note.source == "retention")
            .unwrap_or_else(|| {
                panic!(
                    "an evicted acknowledgment cursor must be reported: {:?}",
                    batch.coverage
                )
            });
        assert!(
            note.reason.contains("evicted") && note.reason.contains("offered again"),
            "{}",
            note.reason
        );
    }
    #[test]
    fn evidence_capped_detail_tail_is_a_gap_not_an_idle_executor() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        let mut text =
            evidence_detail(&[evidence_message_line("a1", "last complete visible item")]);
        // The exact codex-harness-8me.1.1 shape: the retained stream ends in a
        // record cut by the 4 MiB cap while newer visible work exists beyond it.
        text.push_str("{\"type\":\"item.completed\",\"item\":{\"id\":\"cut-1\",\"type\":\"agent_");
        fs::write(&detail, text).unwrap();
        let mut observation = evidence_running(&detail);
        observation.detail_truncated = true;
        observation.activity_truncated = true;
        evidence_seed_run(home.path(), "gen-capped", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());

        let batch = evidence_ready(home.path(), "gen-capped");
        evidence_event(&batch, "a1");
        assert!(
            batch.events.iter().all(|event| event.id != "cut-1"),
            "a record cut at the retention tail is not presented as work: {:?}",
            batch.events
        );

        let gap = evidence_event(&batch, "detail-truncated");
        assert_eq!(gap.source, "gap");
        assert_eq!(gap.kind, "gap");
        assert_eq!(gap.phase, "truncated");
        let gap_text = gap.text.as_deref().unwrap();
        assert!(
            gap_text.contains("never evidence that the executor is idle or stalled"),
            "a capped tail must never imply idleness: {gap_text}"
        );
        assert!(gap_text.contains("activity marks are capped"), "{gap_text}");
        assert_eq!(
            gap.locator.as_deref(),
            Some(detail.to_string_lossy().as_ref())
        );
        assert!(
            batch
                .coverage
                .iter()
                .any(|note| note.source == "detail" && note.reason.contains("malformed or partial")),
            "the partial tail record is counted honestly: {:?}",
            batch.coverage
        );

        let outcome = evidence_acknowledge(
            home.path(),
            "gen-capped",
            &batch.batch_id,
            &evidence::EvidenceAck::NoFinding,
        );
        assert!(matches!(
            outcome,
            evidence::EvidenceAckOutcome::Advanced { .. }
        ));
        let state = home.path().join("state.json");
        let request = evidence_request(home.path(), "gen-capped", &state);
        match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::NoNewEvidence { .. } => {}
            other => panic!("the stable gap identity must not repeat as new evidence: {other:?}"),
        }
    }

    #[test]
    fn evidence_unavailable_runs_report_explicit_coverage() {
        let home = tempfile::tempdir().unwrap();
        let observation =
            RunObservation::unavailable("the run is a tui dispatch with no recorded stream");
        let receipt = evidence_seed_run(home.path(), "gen-unavailable", 1, &observation);
        update_receipt_field(
            &receipt,
            "messages",
            json!([
                {"schema": 1, "id": "msg-1", "status": "queued",
                 "detail": "accepted for a turn; not shown in the conversation yet"},
                {"schema": 1, "id": "msg-2", "status": "indeterminate",
                 "detail": "the input may or may not have been applied"},
                {"schema": 1, "status": "delivered"},
            ]),
        )
        .unwrap();
        update_receipt_field(
            &receipt,
            "leadMessages",
            json!([
                {"schema": 1, "id": "lead-1", "kind": "finding", "status": "pending"},
            ]),
        )
        .unwrap();

        let batch = evidence_ready(home.path(), "gen-unavailable");
        let context = &batch.contexts[0];
        assert_eq!(context.observation_coverage, COVERAGE_UNAVAILABLE);
        assert_eq!(context.state, STATE_UNOBSERVED);
        assert_eq!(context.session, None);
        assert!(
            batch
                .coverage
                .iter()
                .any(|note| note.source == "executor-conversation"
                    && note.reason.contains("unavailable instead of being probed")),
            "an unobservable run is reported as its recorded fact: {:?}",
            batch.coverage
        );
        assert!(
            batch
                .coverage
                .iter()
                .any(|note| note.source == "detail" && note.reason.contains("no raw detail stream")),
            "{:?}",
            batch.coverage
        );
        assert!(
            batch
                .coverage
                .iter()
                .any(|note| note.source == "message-to-executor"
                    && note.reason.contains("not presented as message content")),
            "{:?}",
            batch.coverage
        );
        assert!(
            batch
                .coverage
                .iter()
                .any(|note| note.source == "message-to-executor"
                    && note.reason.contains("carry no message identity")),
            "{:?}",
            batch.coverage
        );

        let queued = evidence_event(&batch, "msg-1");
        assert_eq!(queued.phase, "queued");
        assert_eq!(queued.text, None);
        assert!(
            queued
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("accepted for a turn"))
        );
        let indeterminate = evidence_event(&batch, "msg-2");
        assert_eq!(indeterminate.phase, "indeterminate");
        assert_eq!(indeterminate.text, None);
        let lead = evidence_event(&batch, "lead-1");
        assert_eq!(lead.phase, "pending");
        assert_eq!(lead.name.as_deref(), Some("finding"));
        assert_eq!(lead.text, None);
        let run = evidence_event(&batch, "run");
        assert_eq!(run.phase, STATE_UNOBSERVED);
        assert_eq!(run.source, "run");
    }

    #[test]
    fn evidence_lead_absence_names_the_ordinary_main_limit() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[evidence_message_line("a1", "visible work")]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        let receipt = evidence_seed_run(home.path(), "gen-leadless", 1, &observation);
        update_receipt_field(
            &receipt,
            "leadMessages",
            json!([
                {"schema": 1, "id": "lead-1", "kind": "request", "status": "delivered"},
            ]),
        )
        .unwrap();

        let executor = control_endpoint::Server::start(control_endpoint::Bearer::File(
            home.path()
                .join("harness/executor-pool/checkout-1/endpoint-1.json.token"),
        ));
        executor.answer(
            "thread/read",
            control_endpoint::Answer::Result(json!({"thread": {"id": "thread-exec", "turns": []}})),
        );
        evidence_plant_endpoint(
            home.path(),
            "endpoint-1.json",
            executor.port,
            &evidence_token('e'),
            "thread-exec",
        );

        let batch = evidence_ready(home.path(), "gen-leadless");
        assert!(
            batch
                .coverage
                .iter()
                .any(|note| note.source == "lead-conversation"
                    && note
                        .reason
                        .contains("an ordinary main session exposes no managed lead endpoint")),
            "the ordinary-main limitation is reported as itself: {:?}",
            batch.coverage
        );
        let lead = evidence_event(&batch, "lead-1");
        assert_eq!(lead.phase, "delivered");
        assert_eq!(
            lead.text, None,
            "an unreadable direction is reported, not guessed"
        );
        assert!(
            batch
                .coverage
                .iter()
                .any(|note| note.source == "message-to-lead"
                    && note.reason.contains("not presented as message content")),
            "{:?}",
            batch.coverage
        );
    }
    #[test]
    fn evidence_bidirectional_text_is_read_from_persistence() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(&detail, evidence_detail(&[evidence_started_line()])).unwrap();
        let observation = evidence_running(&detail);
        let receipt = evidence_seed_run(home.path(), "gen-text", 1, &observation);

        let executor = control_endpoint::Server::start(control_endpoint::Bearer::File(
            home.path()
                .join("harness/executor-pool/checkout-1/endpoint-1.json.token"),
        ));
        executor.answer(
            "thread/read",
            control_endpoint::Answer::Result(json!({"thread": {
                "id": "thread-exec",
                "turns": [{
                    "id": "turn-1",
                    "items": [
                        {"id": "exec-input-1", "type": "userMessage", "clientId": "msg-inline",
                         "text": "inline instruction for the executor"},
                        {"id": "exec-note-1", "type": "agentMessage",
                         "text": "visible executor answer"}
                    ]
                }]
            }})),
        );
        evidence_plant_endpoint(
            home.path(),
            "endpoint-1.json",
            executor.port,
            &evidence_token('e'),
            "thread-exec",
        );

        let lead = control_endpoint::Server::start(control_endpoint::Bearer::File(
            home.path()
                .join("harness/executor-pool/checkout-1/lead-endpoint-1.json.token"),
        ));
        let lead_payload = "inline lead payload";
        let framed = format!(
            "header from the dispatcher{}{lead_payload}",
            executor_message::PAYLOAD_MARK
        );
        lead.answer(
            "thread/read",
            control_endpoint::Answer::Result(json!({"thread": {
                "id": "thread-lead",
                "turns": [{
                    "id": "turn-lead-1",
                    "items": [
                        {"id": "lead-input-1", "type": "userMessage", "clientId": "lead-inline",
                         "text": framed}
                    ]
                }]
            }})),
        );
        evidence_plant_endpoint(
            home.path(),
            "lead-endpoint-1.json",
            lead.port,
            &evidence_token('l'),
            "thread-lead",
        );

        // The real writers persist the deliveries: an inline-sized message and
        // a payload above the inline bound that is spilled to its harness file.
        let inline = executor_input::compose(
            home.path(),
            "msg-inline",
            "inline instruction for the executor",
            |body| body.to_owned(),
        )
        .unwrap();
        assert!(inline.spill.is_none());
        let large = "s".repeat(300_000);
        let spilled =
            executor_input::compose(home.path(), "msg-spill", &large, |body| body.to_owned())
                .unwrap();
        let spill = spilled
            .spill
            .clone()
            .expect("above the inline bound the payload is persisted");
        assert_eq!(fs::read_to_string(&spill.path).unwrap(), large);
        let lead_large = "L".repeat(300_000);
        let lead_spilled =
            executor_input::compose(home.path(), "lead-spill", &lead_large, |body| {
                format!("header{}{body}", executor_message::PAYLOAD_MARK)
            })
            .unwrap();
        let lead_spill = lead_spilled
            .spill
            .clone()
            .expect("the framed lead message spills too");

        let mut spill_entry = json!({"schema": 1, "id": "msg-spill", "status": "delivered"});
        spilled.record(&mut spill_entry);
        let mut lead_spill_entry =
            json!({"schema": 1, "id": "lead-spill", "kind": "finding", "status": "delivered"});
        lead_spilled.record(&mut lead_spill_entry);
        update_receipt_field(
            &receipt,
            "messages",
            json!([
                {"schema": 1, "id": "msg-inline", "status": "delivered", "turnId": "turn-1"},
                spill_entry,
            ]),
        )
        .unwrap();
        update_receipt_field(
            &receipt,
            "leadMessages",
            json!([
                {"schema": 1, "id": "lead-inline", "kind": "request", "status": "delivered"},
                lead_spill_entry,
            ]),
        )
        .unwrap();

        let batch = evidence_ready(home.path(), "gen-text");
        let inline_event = evidence_event(&batch, "msg-inline");
        assert_eq!(inline_event.source, "message-to-executor");
        assert_eq!(inline_event.phase, "delivered");
        assert_eq!(
            inline_event.text.as_deref(),
            Some("inline instruction for the executor"),
            "the inline text comes from the conversation, not from metadata"
        );
        assert_eq!(inline_event.locator, None);
        assert_eq!(
            batch
                .events
                .iter()
                .filter(|event| event.id == "msg-inline")
                .count(),
            1,
            "a delivered input is not presented twice: {:?}",
            batch.events
        );

        let spill_event = evidence_event(&batch, "msg-spill");
        let text = spill_event
            .text
            .clone()
            .expect("the spilled payload is read back from its file");
        assert!(text.starts_with(&"s".repeat(3_999)), "{text}");
        assert!(
            text.ends_with(&format!("({} bytes total, truncated)", large.len())),
            "the bounded text names the real file size: {text}"
        );
        assert_eq!(
            spill_event.locator.as_deref(),
            Some(spill.path.to_string_lossy().as_ref())
        );

        let lead_event = evidence_event(&batch, "lead-inline");
        assert_eq!(lead_event.source, "message-to-lead");
        assert_eq!(lead_event.name.as_deref(), Some("request"));
        assert_eq!(
            lead_event.text.as_deref(),
            Some(lead_payload),
            "the transport frame is stripped from the recorded payload"
        );
        let lead_spill_event = evidence_event(&batch, "lead-spill");
        assert_eq!(lead_spill_event.name.as_deref(), Some("finding"));
        let text = lead_spill_event
            .text
            .clone()
            .expect("the spilled lead payload is read back from its file");
        assert!(text.starts_with(&"L".repeat(3_999)), "{text}");
        assert!(text.ends_with(&format!("({} bytes total, truncated)", lead_large.len())));
        assert_eq!(
            lead_spill_event.locator.as_deref(),
            Some(lead_spill.path.to_string_lossy().as_ref())
        );

        let note = evidence_event(&batch, "exec-note-1");
        assert_eq!(note.kind, "message");
        assert_eq!(note.text.as_deref(), Some("visible executor answer"));
    }
    #[test]
    fn evidence_lead_tool_outcome_keeps_the_command_and_exit_code() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(&detail, evidence_detail(&[evidence_started_line()])).unwrap();
        let observation = evidence_running(&detail);
        let receipt = evidence_seed_run(home.path(), "gen-tool", 1, &observation);

        let executor = control_endpoint::Server::start(control_endpoint::Bearer::File(
            home.path()
                .join("harness/executor-pool/checkout-1/endpoint-1.json.token"),
        ));
        executor.answer(
            "thread/read",
            control_endpoint::Answer::Result(json!({"thread": {"id": "thread-exec", "turns": []}})),
        );
        evidence_plant_endpoint(
            home.path(),
            "endpoint-1.json",
            executor.port,
            &evidence_token('e'),
            "thread-exec",
        );

        let lead = control_endpoint::Server::start(control_endpoint::Bearer::File(
            home.path()
                .join("harness/executor-pool/checkout-1/lead-endpoint-1.json.token"),
        ));
        lead.answer(
            "thread/read",
            control_endpoint::Answer::Result(json!({"thread": {
                "id": "thread-lead",
                "turns": [{
                    "id": "turn-lead-1",
                    "items": [
                        {"id": "lead-input-1", "type": "userMessage", "clientId": "lead-1",
                         "text": "please fix the pool"},
                        {"id": "lead-tool-1", "type": "commandExecution",
                         "command": "cargo test --test executor_message",
                         "exitCode": 101, "status": "completed"},
                        {"id": "lead-reason-1", "type": "reasoning", "text": "opaque reasoning"}
                    ]
                }]
            }})),
        );
        evidence_plant_endpoint(
            home.path(),
            "lead-endpoint-1.json",
            lead.port,
            &evidence_token('l'),
            "thread-lead",
        );
        update_receipt_field(
            &receipt,
            "leadMessages",
            json!([
                {"schema": 1, "id": "lead-1", "kind": "finding", "status": "delivered"},
            ]),
        )
        .unwrap();

        let batch = evidence_ready(home.path(), "gen-tool");
        let tool = evidence_event(&batch, "lead-tool-1");
        assert_eq!(tool.source, "lead-item");
        assert_eq!(tool.kind, "tool");
        assert_eq!(
            tool.name.as_deref(),
            Some("cargo test --test executor_message")
        );
        assert_eq!(tool.exit_code, Some(101));
        assert_eq!(tool.phase, "completed");
        assert_eq!(tool.turn.as_deref(), Some("turn-lead-1"));
        assert!(
            batch.events.iter().all(|event| event.id != "lead-reason-1"),
            "reasoning is never collected: {:?}",
            batch.events
        );
        let message = evidence_event(&batch, "lead-1");
        assert_eq!(message.text.as_deref(), Some("please fix the pool"));
    }

    #[test]
    fn evidence_both_item_spellings_and_reasoning_are_normalized() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[
                r#"{"type":"item.completed","item":{"id":"snake-1","type":"command_execution","command":"cargo fmt","exit_code":0,"status":"completed"}}"#.to_owned(),
                r#"{"method":"item/completed","params":{"threadId":"thread-exec","turnId":"t-camel","item":{"id":"camel-1","type":"commandExecution","command":"cargo clippy","exitCode":7,"status":"completed"}}}"#.to_owned(),
                r#"{"type":"item.completed","item":{"id":"reason-1","type":"reasoning","text":"private chain"}}"#.to_owned(),
                r#"{"method":"item/completed","params":{"threadId":"thread-exec","item":{"id":"reason-2","type":"reasoning","text":"private chain too"}}}"#.to_owned(),
                r#"{"type":"item.completed","item":{"id":"camel-agent-1","type":"agent_message","text":"exec route message"}}"#.to_owned(),
                r#"{"method":"item/completed","params":{"threadId":"thread-exec","item":{"id":"camel-agent-2","type":"agentMessage","text":"control route message"}}}"#.to_owned(),
            ]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        evidence_seed_run(home.path(), "gen-spellings", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());

        let batch = evidence_ready(home.path(), "gen-spellings");
        let snake = evidence_event(&batch, "snake-1");
        assert_eq!(snake.kind, "tool");
        assert_eq!(snake.name.as_deref(), Some("cargo fmt"));
        assert_eq!(snake.exit_code, Some(0));
        assert_eq!(snake.phase, "completed");

        let camel = evidence_event(&batch, "camel-1");
        assert_eq!(camel.kind, "tool");
        assert_eq!(camel.name.as_deref(), Some("cargo clippy"));
        assert_eq!(camel.exit_code, Some(7));
        assert_eq!(camel.phase, "completed");
        assert_eq!(camel.turn.as_deref(), Some("t-camel"));

        let exec_message = evidence_event(&batch, "camel-agent-1");
        assert_eq!(exec_message.kind, "message");
        assert_eq!(exec_message.text.as_deref(), Some("exec route message"));
        let control_message = evidence_event(&batch, "camel-agent-2");
        assert_eq!(control_message.kind, "message");
        assert_eq!(
            control_message.text.as_deref(),
            Some("control route message")
        );

        for event in &batch.events {
            assert!(
                !matches!(event.id.as_str(), "reason-1" | "reason-2"),
                "reasoning is never collected: {:?}",
                batch.events
            );
            assert!(
                event
                    .text
                    .as_deref()
                    .is_none_or(|text| !text.contains("private chain")),
                "reasoning text never enters evidence: {:?}",
                batch.events
            );
        }
    }
    #[test]
    fn evidence_assignment_context_comes_from_the_recorded_receipt() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(&detail, evidence_detail(&[evidence_started_line()])).unwrap();
        let observation = evidence_running(&detail);
        let receipt = evidence_seed_run(home.path(), "gen-context", 1, &observation);
        update_receipt_field(
            &receipt,
            "control",
            json!({
                "originalAssignment": "record the evidence foundation",
                "assignment": "a later narrowed assignment",
            }),
        )
        .unwrap();
        let _conversations = evidence_quiet_conversations(home.path());

        let batch = evidence_ready(home.path(), "gen-context");
        let context = &batch.contexts[0];
        assert_eq!(context.slot, 1);
        assert_eq!(context.owner, "fixture-owner");
        assert_eq!(context.checkout, "C:/fixture/source");
        assert_eq!(context.worktree, "C:/fixture/worktree");
        assert_eq!(context.bd, "codex-harness-8me.1.2");
        assert_eq!(context.assignment_id, "fixture-task");
        assert_eq!(
            context.assignment.as_deref(),
            Some("record the evidence foundation"),
            "the recorded original assignment is the context"
        );
        assert_eq!(context.observation_coverage, COVERAGE_NATIVE);
        assert_eq!(context.state, STATE_RUNNING);
        assert_eq!(context.session.as_deref(), Some("thread-exec"));
        assert!(context.updated_ms > 0);

        // Without the original, the current recorded assignment is the record.
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(&detail, evidence_detail(&[evidence_started_line()])).unwrap();
        let observation = evidence_running(&detail);
        let receipt = evidence_seed_run(home.path(), "gen-context-b", 1, &observation);
        update_receipt_field(
            &receipt,
            "control",
            json!({"assignment": "a later narrowed assignment"}),
        )
        .unwrap();
        let _conversations = evidence_quiet_conversations(home.path());
        let batch = evidence_ready(home.path(), "gen-context-b");
        assert_eq!(
            batch.contexts[0].assignment.as_deref(),
            Some("a later narrowed assignment")
        );
    }

    #[test]
    fn evidence_run_generation_change_resets_instead_of_crossing_runs() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[evidence_message_line("a1", "work in the first generation")]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        let receipt = evidence_seed_run(home.path(), "gen-a", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());

        let state = home.path().join("state.json");
        let request_a = evidence_request(home.path(), "gen-a", &state);
        let batch = match evidence::claim(&request_a).unwrap() {
            evidence::EvidenceClaim::Ready(batch) => batch,
            other => panic!("expected a batch for the first generation: {other:?}"),
        };
        assert!(matches!(
            evidence::acknowledge(
                &request_a,
                &batch.batch_id,
                &evidence::EvidenceAck::Finding {
                    reference: "codex-harness-8me.1.2".to_owned()
                }
            )
            .unwrap(),
            evidence::EvidenceAckOutcome::Advanced { .. }
        ));

        // The run is re-launched under one new generation; the pool record
        // changes and the old cursor must not cross into the new run.
        update_receipt_field(
            &receipt,
            "originatingLead",
            json!({"runGeneration": "gen-b", "threadId": "thread-lead"}),
        )
        .unwrap();
        let request_b = evidence_request(home.path(), "gen-b", &state);
        let batch_b = match evidence::claim(&request_b).unwrap() {
            evidence::EvidenceClaim::Ready(batch) => batch,
            other => panic!("the new generation must offer its own evidence: {other:?}"),
        };
        assert!(
            batch_b
                .coverage
                .iter()
                .any(|note| note.source == "state" && note.reason.contains("reset for the new run")),
            "the reset is reported: {:?}",
            batch_b.coverage
        );
        assert!(evidence_event(&batch_b, "a1").phase == "completed");
        assert!(
            batch_b.events.iter().any(|event| event.source == "run"),
            "the run lifecycle is offered again under its new generation"
        );
        let refused = evidence::acknowledge(
            &request_a,
            &batch_b.batch_id,
            &evidence::EvidenceAck::NoFinding,
        )
        .unwrap();
        match refused {
            evidence::EvidenceAckOutcome::Refused { reason } => {
                assert!(reason.contains("instead of"), "{reason}");
            }
            other => panic!("an acknowledgment from another run must be refused: {other:?}"),
        }
        assert!(matches!(
            evidence::acknowledge(
                &request_b,
                &batch_b.batch_id,
                &evidence::EvidenceAck::NoFinding
            )
            .unwrap(),
            evidence::EvidenceAckOutcome::Advanced { .. }
        ));
    }

    #[test]
    fn evidence_missing_or_malformed_records_never_advance_silently() {
        // No pool records yet: honest coverage, no events, no model turn.
        let home = tempfile::tempdir().unwrap();
        let state = home.path().join("state.json");
        let request = evidence_request(home.path(), "gen-missing", &state);
        match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::NoNewEvidence { contexts, coverage } => {
                assert!(contexts.is_empty());
                assert_eq!(coverage.len(), 1, "{coverage:?}");
                assert_eq!(coverage[0].source, "receipt");
                assert!(
                    coverage[0]
                        .reason
                        .contains("no readable dispatched-run record names generation gen-missing"),
                    "{}",
                    coverage[0].reason
                );
            }
            other => panic!("missing records must not fabricate evidence: {other:?}"),
        }

        // A malformed state receipt refuses instead of starting over.
        let malformed = home.path().join("malformed-state.json");
        fs::write(&malformed, "{not json").unwrap();
        let error =
            evidence::claim(&evidence_request(home.path(), "gen-missing", &malformed)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            error
                .to_string()
                .contains("refusing to advance unfinished evidence"),
            "{error}"
        );

        // A newer schema is refused for the same reason.
        let newer = home.path().join("newer-state.json");
        fs::write(
            &newer,
            serde_json::to_vec(&json!({"evidence": {"schema": 99, "generation": "gen-missing"}}))
                .unwrap(),
        )
        .unwrap();
        let error =
            evidence::claim(&evidence_request(home.path(), "gen-missing", &newer)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("refusing to advance unfinished evidence"),
            "{error}"
        );
    }

    #[test]
    fn evidence_concurrent_claims_never_start_two_batches() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[evidence_message_line("a1", "only visible work")]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        evidence_seed_run(home.path(), "gen-race", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());

        let state = home.path().join("state.json");
        let request = evidence_request(home.path(), "gen-race", &state);
        // Holding the receipt lock stops both claims from persisting until
        // both have read the initial state, so a claim that decides outside
        // the lock lets each of them start its own batch.
        let claims = std::thread::scope(|scope| {
            let handles = with_receipt_lock(&state, || {
                let first = scope.spawn(|| evidence::claim(&request));
                let second = scope.spawn(|| evidence::claim(&request));
                std::thread::sleep(Duration::from_millis(800));
                Ok::<_, io::Error>((first, second))
            })
            .unwrap();
            [
                handles.0.join().unwrap().unwrap(),
                handles.1.join().unwrap().unwrap(),
            ]
        });
        let ready: Vec<&evidence::EvidenceBatch> = claims
            .iter()
            .filter_map(|claim| match claim {
                evidence::EvidenceClaim::Ready(batch) => Some(batch),
                _ => None,
            })
            .collect();
        let in_flight: Vec<&String> = claims
            .iter()
            .filter_map(|claim| match claim {
                evidence::EvidenceClaim::BatchInFlight { batch_id, .. } => Some(batch_id),
                _ => None,
            })
            .collect();
        assert_eq!(
            ready.len(),
            1,
            "two concurrent claims must yield exactly one batch: {claims:?}"
        );
        assert_eq!(
            in_flight.len(),
            1,
            "the concurrent claim that lost must report the active batch: {claims:?}"
        );
        assert_eq!(
            in_flight[0], &ready[0].batch_id,
            "the losing claim names the batch that won"
        );
        let recorded = evidence_read_state(home.path());
        assert_eq!(recorded["evidence"]["batches"], 1, "{recorded}");
        assert_eq!(
            recorded["evidence"]["active"]["batchId"],
            ready[0].batch_id.as_str()
        );
    }

    #[test]
    fn evidence_acknowledgment_cannot_overwrite_a_newer_batch() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[evidence_message_line("a1", "only visible work")]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        evidence_seed_run(home.path(), "gen-stale", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());

        let first = evidence_ready(home.path(), "gen-stale");
        let state = home.path().join("state.json");
        let request = evidence_request(home.path(), "gen-stale", &state);
        let finding = evidence::EvidenceAck::Finding {
            reference: "codex-harness-8me.1.2".to_owned(),
        };
        // The acknowledgment starts while the receipt lock is held, and a
        // newer generation-identical batch replaces the active one before it
        // can persist. A stale read must be refused against the recorded
        // state instead of overwriting the newer batch.
        let outcome = std::thread::scope(|scope| {
            let acknowledge = with_receipt_lock(&state, || {
                let acknowledge =
                    scope.spawn(|| evidence::acknowledge(&request, &first.batch_id, &finding));
                std::thread::sleep(Duration::from_millis(500));
                let mut document = read_receipt_value(&state).unwrap();
                document["evidence"]["batches"] = json!(2);
                document["evidence"]["active"] = json!({
                    "batchId": "batch-2",
                    "createdMs": 42,
                    "ids": ["run|1|run|running"],
                });
                write_receipt(&state, &document).unwrap();
                Ok::<_, io::Error>(acknowledge)
            })
            .unwrap();
            acknowledge.join().unwrap().unwrap()
        });
        let evidence::EvidenceAckOutcome::Refused { reason } = outcome else {
            panic!(
                "a stale acknowledgment must be refused instead of overwriting a newer batch: {outcome:?}"
            );
        };
        assert!(reason.contains("batch-2"), "{reason}");
        let recorded = evidence_read_state(home.path());
        assert_eq!(
            recorded["evidence"]["batches"], 2,
            "the newer claim's state survives: {recorded}"
        );
        assert_eq!(recorded["evidence"]["active"]["batchId"], "batch-2");
        assert_eq!(recorded["evidence"]["cursor"], json!([]));
        assert!(
            recorded["evidence"]["lastAck"].is_null(),
            "the refused acknowledgment retains nothing: {recorded}"
        );
    }

    #[test]
    fn evidence_temporary_unreadable_source_never_replays_acknowledged_evidence() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[evidence_message_line("a1", "only visible work")]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        evidence_seed_run(home.path(), "gen-gap", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());
        let state = home.path().join("state.json");
        // An unrelated field of the same receipt must survive every merge.
        fs::write(
            &state,
            serde_json::to_vec(&json!({"sibling": {"kept": true}})).unwrap(),
        )
        .unwrap();

        let first = evidence_ready(home.path(), "gen-gap");
        evidence_event(&first, "a1");
        let outcome = evidence_acknowledge(
            home.path(),
            "gen-gap",
            &first.batch_id,
            &evidence::EvidenceAck::NoFinding,
        );
        assert!(matches!(
            outcome,
            evidence::EvidenceAckOutcome::Advanced { .. }
        ));

        // The detail stream becomes temporarily unreadable - rotation, a
        // rename or an access failure - while its acknowledged evidence
        // still exists. The pass may not evict it or offer it as new.
        let hidden = home.path().join("detail.jsonl.hidden");
        fs::rename(&detail, &hidden).unwrap();
        let request = evidence_request(home.path(), "gen-gap", &state);
        match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::NoNewEvidence { .. } => {}
            other => panic!(
                "an unreadable source must not present its acknowledged evidence as new: {other:?}"
            ),
        }
        let recorded = evidence_read_state(home.path());
        assert_eq!(
            recorded["sibling"]["kept"], true,
            "unrelated receipt fields survive the state merge: {recorded}"
        );
        let cursor: Vec<&str> = recorded["evidence"]["cursor"]
            .as_array()
            .map(|ids| ids.iter().filter_map(|id| id.as_str()).collect())
            .unwrap_or_default();
        assert!(
            cursor.iter().any(|id| id.ends_with("|a1|completed")),
            "an acknowledged identity stays retained while its source is unreadable: {recorded}"
        );

        // Restoring the source must not replay what is still acknowledged.
        fs::rename(&hidden, &detail).unwrap();
        match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::NoNewEvidence { .. } => {}
            other => panic!(
                "restoring a temporarily unreadable source must not replay acknowledged evidence: {other:?}"
            ),
        }
    }

    #[test]
    fn evidence_unknown_release_is_not_cleared_while_its_source_is_unreadable() {
        let home = tempfile::tempdir().unwrap();
        let detail = home.path().join("detail.jsonl");
        fs::write(
            &detail,
            evidence_detail(&[evidence_message_line("a1", "only visible work")]),
        )
        .unwrap();
        let observation = evidence_running(&detail);
        evidence_seed_run(home.path(), "gen-release", 1, &observation);
        let _conversations = evidence_quiet_conversations(home.path());

        let first = evidence_ready(home.path(), "gen-release");
        evidence_event(&first, "a1");
        let outcome = evidence_acknowledge(
            home.path(),
            "gen-release",
            &first.batch_id,
            &evidence::EvidenceAck::Unknown {
                reason: "the analysis failed before it retained a finding".to_owned(),
            },
        );
        assert!(matches!(
            outcome,
            evidence::EvidenceAckOutcome::Preserved { .. }
        ));

        // The only source of a1 becomes unreadable while its release is
        // pending; the pass must keep the release recoverable instead of
        // clearing it because the identity happened to be invisible.
        let hidden = home.path().join("detail.jsonl.hidden");
        fs::rename(&detail, &hidden).unwrap();
        let state = home.path().join("state.json");
        let request = evidence_request(home.path(), "gen-release", &state);
        let second = match evidence::claim(&request).unwrap() {
            evidence::EvidenceClaim::Ready(batch) => batch,
            other => panic!("the still-readable run event is new evidence: {other:?}"),
        };
        assert_eq!(
            second
                .events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["run"]
        );
        let recorded = evidence_read_state(home.path());
        let released: Vec<&str> = recorded["evidence"]["released"]
            .as_array()
            .map(|ids| ids.iter().filter_map(|id| id.as_str()).collect())
            .unwrap_or_default();
        assert!(
            released.iter().any(|id| id.ends_with("|a1|completed")),
            "an unknown-released identity stays recoverable while its source is unreadable: {recorded}"
        );
        assert!(
            second.coverage.iter().any(|note| note.source == "replay"),
            "the pending release is reported honestly: {:?}",
            second.coverage
        );

        // Acknowledge the re-offered run event, restore the source and the
        // released evidence is offered again instead of skipped.
        let outcome = evidence_acknowledge(
            home.path(),
            "gen-release",
            &second.batch_id,
            &evidence::EvidenceAck::NoFinding,
        );
        assert!(matches!(
            outcome,
            evidence::EvidenceAckOutcome::Advanced { .. }
        ));
        fs::rename(&hidden, &detail).unwrap();
        let third = evidence_ready(home.path(), "gen-release");
        evidence_event(&third, "a1");
        let recorded = evidence_read_state(home.path());
        let released: Vec<&str> = recorded["evidence"]["released"]
            .as_array()
            .map(|ids| ids.iter().filter_map(|id| id.as_str()).collect())
            .unwrap_or_default();
        assert!(
            !released.iter().any(|id| id.ends_with("|a1|completed")),
            "the release is satisfied once its evidence is offered again: {recorded}"
        );
    }
}
