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
            format!("Lead action required: review preserved partial work, then start a NEW session for the original assignment in the SAME worktree with `codex-harness executor restart --source {} --codex-home {} --slot {slot} --owner {} --session {}`. Do not use spawn/release/reset or resume the expensive history; if this recurs, investigate before another restart", quote(source), quote(&home.to_string_lossy()), quote(owner), quote(session))
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
            malformed: 0,
            cause: None,
            host: None,
            result: Some(result),
            detail: Some(detail),
            updated_ms: now_ms(),
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
            malformed: 0,
            cause: None,
            host: None,
            result: None,
            detail: None,
            updated_ms: now_ms(),
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
                    match item.kind.as_str() {
                        "agent_message" => self.observation.messages += 1,
                        "command_execution" | "mcp_tool_call" | "web_search" | "file_change" => {
                            self.observation.tool_calls += 1
                        }
                        _ => {}
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
            self.observation.state = state.to_owned();
        }
        if let Some(kind) = completed_item {
            match kind {
                "agentMessage" => self.observation.messages += 1,
                "commandExecution" | "mcpToolCall" | "webSearch" | "fileChange" => {
                    self.observation.tool_calls += 1
                }
                _ => {}
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
        cache::Monitor::new(receipt, &harness_core::native_launcher::codex_home()?)?;
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
    let text = text.trim();
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… ({} bytes total, truncated)", &text[..end], text.len())
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
}
