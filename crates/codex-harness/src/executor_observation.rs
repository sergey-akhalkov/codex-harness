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

use harness_core::process::ProcessIdentity;
use harness_core::process_service::ServiceProcess;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs, io,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

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

pub(crate) const STATE_ACCEPTED: &str = "dispatch-accepted";
pub(crate) const STATE_STARTED: &str = "native-start";
pub(crate) const STATE_RUNNING: &str = "running";
pub(crate) const STATE_COMPLETED: &str = "completed";
pub(crate) const STATE_FAILED: &str = "failed";
pub(crate) const STATE_DEFECT: &str = "defect";
pub(crate) const STATE_INTERRUPTED: &str = "interrupted";
pub(crate) const STATE_UNOBSERVED: &str = "unobserved";

pub(crate) const COVERAGE_NATIVE: &str = "native";
pub(crate) const COVERAGE_UNAVAILABLE: &str = "unavailable";

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

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// One parsed event of the native stream.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NativeEvent {
    ThreadStarted { thread_id: String },
    TurnStarted,
    TurnCompleted,
    TurnFailed { message: String },
    Item { phase: ItemPhase, item: ItemSummary },
    Error { message: String },
    Unknown { kind: String },
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
        "turn.started" => NativeEvent::TurnStarted,
        "turn.completed" => NativeEvent::TurnCompleted,
        "turn.failed" => NativeEvent::TurnFailed {
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
        NativeEvent::TurnStarted => Some("state: turn started".to_owned()),
        NativeEvent::TurnCompleted => Some("state: turn completed".to_owned()),
        NativeEvent::TurnFailed { message } => Some(format!("turn failed: {message}")),
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
    saw_turn_completed: bool,
    failure: Option<String>,
    oversize: u64,
}

impl RunTracker {
    pub(crate) fn new(mut observation: RunObservation) -> Self {
        observation.state = STATE_ACCEPTED.into();
        observation.updated_ms = now_ms();
        Self {
            observation,
            saw_thread: false,
            saw_turn_completed: false,
            failure: None,
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
            NativeEvent::TurnStarted => {
                if self.saw_thread {
                    self.observation.state = STATE_RUNNING.into();
                }
            }
            NativeEvent::TurnCompleted => {
                self.saw_turn_completed = true;
                self.observation.state = STATE_RUNNING.into();
            }
            NativeEvent::TurnFailed { message } => {
                self.failure = Some(message.clone());
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

    pub(crate) fn count_malformed(&mut self) -> bool {
        self.observation.malformed += 1;
        self.observation.updated_ms = now_ms();
        true
    }

    pub(crate) fn count_oversize(&mut self) {
        self.oversize += 1;
        self.observation.malformed += 1;
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
        } else if let Some(failure) = &self.failure {
            self.observation.state = STATE_FAILED.into();
            cause = Some(failure.clone());
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

pub(crate) enum FinalMessage {
    Present,
    Empty,
    Missing,
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

/// Updates only the `observation` field of the receipt, preserving every
/// other dispatch input and updating atomically through a sibling temp file.
pub(crate) fn update_receipt(receipt: &Path, observation: &RunObservation) -> io::Result<()> {
    let bytes = fs::read(receipt)?;
    let mut value: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    value["observation"] = serde_json::to_value(observation).map_err(io::Error::other)?;
    write_receipt(receipt, &value)
}

/// Replaces one top-level receipt field, preserving the rest.
pub(crate) fn update_receipt_field(receipt: &Path, name: &str, field: Value) -> io::Result<()> {
    let bytes = fs::read(receipt)?;
    let mut value: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    value[name] = field;
    write_receipt(receipt, &value)
}

fn write_receipt(receipt: &Path, value: &Value) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let temp = receipt.with_extension("tmp");
    fs::write(&temp, bytes)?;
    fs::rename(&temp, receipt)
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

/// Runs one observed CLI child: pipes the JSONL stream, renders it readably
/// onto this process's own visible surface, keeps the receipt's recorded
/// lifecycle current, and returns the exit code the host must propagate.
pub(crate) fn run_observed(
    command: &mut Command,
    receipt: &Path,
    tracker: &mut RunTracker,
    header: &str,
) -> io::Result<i32> {
    let result = tracker
        .observation
        .result
        .clone()
        .ok_or_else(|| io::Error::other("observed run has no recorded result file"))?;
    let detail = tracker.observation.detail.clone();
    if let Err(error) = record_host(tracker) {
        eprintln!("codex-harness: executor host identity unavailable: {error}");
    }
    // A stale final message from an earlier run must never read as this run's
    // result, and the detail stream is per run.
    let _ = fs::remove_file(&result);
    if let Some(detail) = &detail {
        if let Some(parent) = detail.parent() {
            fs::create_dir_all(parent)?;
        }
        let _ = fs::write(detail, b"");
    }
    persist(
        receipt,
        tracker,
        "the run continues without its kit-local record",
    );
    print!("{header}");
    io::stdout().flush()?;
    command
        .stdout(std::process::Stdio::piped())
        .stdin(std::process::Stdio::inherit());
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("observed run stdout is not piped"))?;
    let mut reader = std::io::BufReader::new(stdout);
    let mut truncation_noted = false;
    let mut stdout = io::stdout();
    loop {
        match next_line(&mut reader, MAX_EVENT_LINE)? {
            Line::Eof => break,
            Line::Oversize(note) => {
                tracker.count_oversize();
                writeln!(stdout, "unparsed event: {note}")?;
                persist(receipt, tracker, "an unparsed event was not recorded");
            }
            Line::Text(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                if let Some(detail) = &detail {
                    match append_detail(detail, &line) {
                        Ok(true) => {}
                        Ok(false) if !truncation_noted => {
                            truncation_noted = true;
                            writeln!(
                                stdout,
                                "note: raw event detail reached the {} byte bound; the detail file stops here while the readable surface continues",
                                MAX_STREAM_BYTES
                            )?;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            eprintln!("codex-harness: executor detail log unavailable: {error}")
                        }
                    }
                }
                match parse_event(&line) {
                    Ok(event) => {
                        let changed = tracker.apply(&event);
                        render_event(&event, &mut stdout)?;
                        if changed {
                            persist(receipt, tracker, "a parsed event was not recorded");
                        }
                    }
                    Err(note) => {
                        tracker.count_malformed();
                        writeln!(stdout, "unparsed event: {note}")?;
                        persist(receipt, tracker, "an unparsed event was not recorded");
                    }
                }
            }
        }
    }
    let code = child
        .wait()?
        .code()
        .ok_or_else(|| io::Error::other("executor launcher terminated without an exit code"))?;
    let exit = tracker.finish(code, Some(result.as_path()));
    let observation = &tracker.observation;
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
            writeln!(
                stdout,
                "detail: {}",
                detail
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "unavailable".into())
            )?;
            if observation.state == STATE_DEFECT {
                writeln!(
                    stdout,
                    "remedy: the session is not running; continue or correct it with `codex exec resume SESSION_ID` and treat the empty result as an executor output defect"
                )?;
            }
        }
    }
    persist(receipt, tracker, "the final state was not recorded");
    stdout.flush()?;
    Ok(exit)
}

fn record_host(tracker: &mut RunTracker) -> io::Result<()> {
    tracker.observation.host = Some(host_identity()?);
    tracker.observation.updated_ms = now_ms();
    Ok(())
}

fn persist(receipt: &Path, tracker: &RunTracker, note: &str) {
    if let Err(error) = update_receipt(receipt, &tracker.observation) {
        eprintln!("codex-harness: executor observation record unavailable: {error}; {note}");
    }
}

/// One line from a bounded reader: `Oversize` means the line exceeded the
/// accepted event size and the rest of it was dropped.
enum Line {
    Text(String),
    Oversize(String),
    Eof,
}

fn next_line<R: io::Read>(reader: &mut io::BufReader<R>, limit: usize) -> io::Result<Line> {
    let mut collected: Vec<u8> = Vec::new();
    let mut oversize = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if collected.is_empty() {
                return Ok(Line::Eof);
            }
            break;
        }
        match available.iter().position(|byte| *byte == b'\n') {
            Some(position) => {
                if !oversize {
                    collected.extend_from_slice(&available[..position]);
                }
                reader.consume(position + 1);
                break;
            }
            None => {
                let length = available.len();
                if collected.len() + length > limit {
                    oversize = true;
                }
                if !oversize {
                    collected.extend_from_slice(available);
                }
                reader.consume(length);
            }
        }
    }
    if oversize {
        return Ok(Line::Oversize(format!(
            "event line exceeded {limit} bytes and was dropped"
        )));
    }
    Ok(Line::Text(String::from_utf8_lossy(&collected).into_owned()))
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
        let mut observation = observation();
        observation.session = Some("s-1".into());
        update_receipt(&receipt, &observation).unwrap();
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
        let mut reader = io::BufReader::new(io::Cursor::new(vec![b'a'; MAX_EVENT_LINE + 10]));
        match next_line(&mut reader, MAX_EVENT_LINE).unwrap() {
            Line::Oversize(note) => assert!(note.contains("exceeded"), "{note}"),
            _ => panic!("oversize line must be dropped"),
        }
        // A truncated final line (no newline) is still returned for parsing,
        // and the parser reports it as unparsed rather than completion.
        let mut reader = io::BufReader::new(io::Cursor::new(br#"{"type":"thread.sta"#.to_vec()));
        match next_line(&mut reader, MAX_EVENT_LINE).unwrap() {
            Line::Text(line) => assert!(parse_event(&line).is_err()),
            _ => panic!("partial line must be returned"),
        }
    }
}
