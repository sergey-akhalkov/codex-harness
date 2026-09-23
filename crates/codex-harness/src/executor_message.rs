//! Addressed delivery of one literal message into a live pooled executor
//! conversation.
//!
//! The command addresses one run through the accepted checkout, the kit home,
//! the slot, the owner and, when given, the exact recorded session; verifies the
//! recorded slot binding, the live lease, the recorded host identity, the
//! app-server child and the run's own state; and then delivers the literal text,
//! from `--text` or from a UTF-8 `--file`, with no shell evaluation and its real
//! line breaks, into that run's recorded conversation through
//! `endpoint-<index>.json`.
//!
//! It never starts a conversation: an active turn is steered with `turn/steer`
//! at the nearest supported point (the tool call in flight is not interrupted),
//! and an idle thread receives a new turn on its own thread. Model, provider,
//! reasoning effort, conversation context and completed work are unchanged.
//!
//! Classification is honest by construction:
//!
//! - `delivered` requires observed evidence from the conversation itself: a
//!   `userMessage` item of the addressed thread carrying the recorded client
//!   message id or the exact delivered text.
//! - `queued` is the acceptance answer of the recorded turn when that evidence
//!   was not observed within the bounded window; it is never reported as the
//!   executor having applied the correction.
//! - `error` is the native endpoint's own refusal; nothing was delivered.
//! - an unanswered request is `indeterminate`: the input may or may not have
//!   been applied, so it is reported with its recorded request identity and
//!   never as delivered.
//!
//! Every attempt is recorded with its content identity in the receipt's
//! `messages` field (the field the stop path marks undelivered), so a repeat of
//! the same text cannot deliver the same context twice: a delivered or queued
//! text is reported instead of sent again, an indeterminate attempt refuses the
//! repeat and names the next action, and only a definite error allows another
//! attempt. A local file write is never presented as model delivery.
//!
//! A completed, stopped, interrupted or unavailable run gets its actual state
//! with the exact-session `executor resume` remedy, and a surface without a
//! recorded control endpoint (tui mode, legacy receipts) is reported as
//! unsupported with the same remedy. Nothing here resets, cleans, releases or
//! completes a run.

use super::control::{
    self, BoundIdentity, ControlPaths, Conversation, Endpoint, Reply, active_turn,
};
use super::observation::{
    self, HostIdentity, RunObservation, STATE_COMPLETED, STATE_DEFECT, STATE_FAILED,
    STATE_INTERRUPTED, STATE_PARTIAL_STOP, STATE_STOPPED,
};
use super::{
    SessionLease, invalid, lease_path, option_text, read_lease, receipt_binding, receipt_path,
    required,
};
use harness_core::orchestration_config;
use harness_core::process::{Cancellation, Deadline, ExclusiveFileLock, ProcessIdentity};
use harness_core::process_service::{self, ServiceProcess};
use harness_core::task_succession;
use harness_core::task_worktree;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

/// Bound on connecting to the recorded endpoint and on one control request.
const CONTROL_BOUND: Duration = Duration::from_secs(20);
/// How long the delivered input is watched for in the conversation's own items
/// before the acceptance answer is reported as queued instead of delivered.
const EVIDENCE_WINDOW: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(200);
/// One lead message is bounded, and what is sent is always this literal text:
/// a file is a correction, not a task re-send.
const MAX_TEXT: u64 = 256 * 1024;
/// Bound on recorded message attempts kept in the receipt.
const MAX_ATTEMPTS: usize = 64;
/// Bound on delivery rounds: a rejected steer means the active turn changed, so
/// the turn is re-read and the input is submitted once more.
const DELIVERY_ROUNDS: usize = 3;
/// How long the receipt's own lock may be awaited for one attempt record.
const LOCK_WAIT: Duration = Duration::from_secs(10);
const SCHEMA: u32 = 1;

const STATUS_PENDING: &str = "pending";
const STATUS_QUEUED: &str = "queued";
const STATUS_DELIVERED: &str = "delivered";
const STATUS_ERROR: &str = "error";
const STATUS_INDETERMINATE: &str = "indeterminate";

struct Request {
    source: PathBuf,
    codex_home: PathBuf,
    slot: u32,
    owner: String,
    /// The exact session this invocation addresses: the `--session` value once
    /// it is verified against the receipt, otherwise the recorded one.
    session: Option<String>,
    text: String,
}

impl Request {
    fn parse(args: &[OsString]) -> io::Result<Self> {
        let mut source = None;
        let mut codex_home = None;
        let mut slot = None;
        let mut owner = None;
        let mut session = None;
        let mut text = None;
        let mut file = None;
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let key = arg
                .to_str()
                .ok_or_else(|| invalid("invalid native executor options"))?;
            let value = iter
                .next()
                .ok_or_else(|| invalid("invalid native executor options"))?;
            match key {
                "--source" => source = Some(PathBuf::from(value)),
                "--codex-home" => codex_home = Some(PathBuf::from(value)),
                "--slot" => slot = Some(option_text(value)?),
                "--owner" => owner = Some(option_text(value)?),
                "--session" => session = Some(option_text(value)?),
                "--text" => text = Some(option_text(value)?),
                "--file" => file = Some(PathBuf::from(value)),
                _ => return Err(invalid("invalid native executor options")),
            }
        }
        let owner = owner
            .filter(|owner| !owner.trim().is_empty())
            .ok_or_else(|| invalid("--owner ID is required"))?;
        let text = match (text, file) {
            (Some(_), Some(_)) => {
                return Err(invalid(
                    "give either --text TEXT or --file FILE, not both: one message is one input",
                ));
            }
            (None, None) => {
                return Err(invalid(
                    "a message needs its literal content: --text TEXT or --file FILE",
                ));
            }
            (Some(text), None) => text,
            (None, Some(file)) => read_message_file(&file)?,
        };
        if text.is_empty() {
            return Err(invalid(
                "the message is empty; nothing would be delivered to the conversation",
            ));
        }
        Ok(Self {
            source: required(source, "--source")?,
            codex_home: required(codex_home, "--codex-home")?,
            slot: slot
                .ok_or_else(|| invalid("--slot is required"))?
                .parse()
                .map_err(|_| invalid("--slot must be a positive pool slot index"))?,
            owner,
            session,
            text,
        })
    }
}

/// The literal UTF-8 content of `--file`: whatever the file holds, with its
/// real line breaks. A file that is not UTF-8 text is refused instead of being
/// lossily rewritten; a leading byte-order mark is not content.
fn read_message_file(path: &Path) -> io::Result<String> {
    let bytes = observation::read_bounded(path, MAX_TEXT + 1)
        .map_err(|error| invalid(&format!("message file {}: {error}", path.display())))?;
    if bytes.len() as u64 > MAX_TEXT {
        return Err(invalid(&format!(
            "message file {} exceeds the {MAX_TEXT}-byte bound of one message",
            path.display()
        )));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        invalid(&format!(
            "message file {} is not UTF-8 text",
            path.display()
        ))
    })?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}

pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    let mut request = Request::parse(args)?;
    let started = Instant::now();
    // Address one recorded run exactly as `executor stop` does: the slot must be
    // bound to the addressed owner, and an explicit --session must be the
    // session the receipt recorded.
    let config = orchestration_config::load(&request.source)?;
    let layout = task_worktree::pool(&request.source, config.max_concurrent_executors)?;
    layout.slot(request.slot)?;
    let record =
        task_worktree::load_slot_record(&request.codex_home, &request.source, request.slot)?
            .ok_or_else(|| {
                invalid(&format!(
                    "slot {} has no recorded session binding; message addresses a dispatched run, so dispatch or resume that session first",
                    request.slot
                ))
            })?;
    match record.owner.as_deref() {
        Some(owner) if owner == request.owner => {}
        recorded => {
            return Err(invalid(&format!(
                "slot {} is bound to session {} instead of {}; message that owner's run or release the slot explicitly",
                request.slot,
                recorded.unwrap_or("no session"),
                request.owner
            )));
        }
    }
    let receipt = receipt_path(&request.codex_home, &request.source, request.slot)?;
    let bytes = fs::read(&receipt).map_err(|error| {
        invalid(&format!(
            "executor message needs the dispatch receipt {} to name the exact run: {error}; dispatch `codex-harness executor spawn` first",
            receipt.display()
        ))
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        invalid(&format!(
            "dispatch receipt {} is not JSON: {error}",
            receipt.display()
        ))
    })?;
    if let Some(binding) = receipt_binding(&value)?
        && (binding.index != request.slot || binding.owner != request.owner)
    {
        return Err(invalid(&format!(
            "the recorded run in slot {} belongs to owner {} instead of {}; message that run or release the slot explicitly",
            binding.index, binding.owner, request.owner
        )));
    }
    let recorded_run = RunObservation::from_receipt(&value);
    let recorded_session = recorded_run
        .as_ref()
        .and_then(RunObservation::recorded_session)
        .map(str::to_owned);
    if let Some(session) = &request.session {
        let exact = task_succession::exact_session_id(session)?;
        match recorded_session.as_deref() {
            Some(recorded) if recorded == exact => request.session = Some(exact.to_owned()),
            Some(recorded) => {
                return Err(invalid(&format!(
                    "the recorded run in slot {} observed session {recorded} instead of {exact}; message the exact recorded run or address its recorded session",
                    request.slot
                )));
            }
            None => {
                return Err(invalid(&format!(
                    "the recorded run in slot {} names no exact session, so --session {exact} cannot be verified against it; message without --session to address the recorded run by its recorded identity",
                    request.slot
                )));
            }
        }
    } else {
        request.session = recorded_session.clone();
    }
    let remedy = resume_remedy(&request);
    let state = recorded_run
        .as_ref()
        .map(|run| run.state.clone())
        .unwrap_or_default();
    // A run that already ended keeps its result and its conversation: the
    // message never revives it and never starts another one.
    if let Some(run) = &recorded_run
        && matches!(
            run.state.as_str(),
            STATE_COMPLETED
                | STATE_STOPPED
                | STATE_PARTIAL_STOP
                | STATE_FAILED
                | STATE_DEFECT
                | STATE_INTERRUPTED
        )
    {
        return report_ended(&request, &receipt, run, &remedy, started);
    }
    // The address of the live conversation: the control endpoint record the
    // host wrote beside the receipt. A surface without one has no addressed
    // input channel, and saying so is the honest result.
    if value["control"].is_null() {
        return report_unaddressable(
            &request,
            &receipt,
            format!(
                "this run records no control-backed conversation (tui mode, or a receipt written before the control route), so it has no addressed input channel; recorded state {state}"
            ),
            &remedy,
            started,
        );
    }
    let paths = ControlPaths::for_slot(&request.codex_home, &request.source, request.slot)?;
    let endpoint = match Endpoint::read(&paths.endpoint) {
        Ok(endpoint) => endpoint,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return report_unaddressable(
                &request,
                &receipt,
                format!(
                    "the run has not recorded its control endpoint yet (state {state}), so its conversation is not addressable; it may still be starting - observe it with `codex-harness executor watch --source {} --codex-home {} --slot {}` and message again once its conversation is recorded",
                    request.source.display(),
                    request.codex_home.display(),
                    request.slot
                ),
                &remedy,
                started,
            );
        }
        Err(error) => {
            return Err(invalid(&format!(
                "the recorded control endpoint {} is not usable: {error}; nothing was delivered",
                paths.endpoint.display()
            )));
        }
    };
    let Some(thread_id) = endpoint.thread_id.clone() else {
        return report_unaddressable(
            &request,
            &receipt,
            format!(
                "the recorded control endpoint names no thread (state {state}), so the run never started a conversation to address"
            ),
            &remedy,
            started,
        );
    };
    // The live run: the recorded host identity, the lease that must agree with
    // it, and the app-server child that serves the conversation.
    let lease = read_lease(&lease_path(
        &request.codex_home,
        &request.source,
        request.slot,
    )?)
    .filter(|lease| lease.owner == request.owner && lease.index == request.slot);
    let receipt_host = recorded_run.as_ref().and_then(|run| run.host.clone());
    if let (Some(receipt_host), Some(lease)) = (&receipt_host, &lease)
        && (lease.pid != receipt_host.pid
            || lease.created != receipt_host.created
            || lease.program != receipt_host.program)
    {
        return Err(invalid(&format!(
            "the recorded host identity disagrees between the dispatch receipt (pid {}) and the lease (pid {}); nothing was delivered because the address no longer names one exact run; next action: inspect the slot and the receipt, then continue the exact session with `{remedy}`",
            receipt_host.pid, lease.pid
        )));
    }
    let host = receipt_host.or_else(|| lease.as_ref().map(lease_identity));
    let Some(host) = host else {
        return report_unaddressable(
            &request,
            &receipt,
            format!(
                "the receipt records no exact host process identity for slot {} ({}), so the live run cannot be verified; nothing was sent",
                request.slot,
                if recorded_run.is_none() {
                    "a legacy receipt without observation coverage"
                } else {
                    "no host was observed"
                }
            ),
            &remedy,
            started,
        );
    };
    let user = process_service::current_user()?;
    match ServiceProcess::inspect(
        ProcessIdentity {
            pid: host.pid,
            creation_time: host.created,
        },
        &host.program,
        &user,
    ) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return report_unaddressable(
                &request,
                &receipt,
                format!(
                    "the recorded host process (pid {}, {}) is not running; the run ended without a terminal record and its last recorded state is {state}, so nothing was sent",
                    host.pid,
                    host.program.display()
                ),
                &remedy,
                started,
            );
        }
        Err(error) => {
            return Err(invalid(&format!(
                "the recorded host process (pid {}, {}) could not be verified: {error}; nothing was delivered",
                host.pid,
                host.program.display()
            )));
        }
    }
    let Some(child) = endpoint.process.clone() else {
        return Err(invalid(
            "the recorded control endpoint names no app-server child, so the address cannot be verified; nothing was delivered",
        ));
    };
    match ServiceProcess::inspect(
        ProcessIdentity {
            pid: child.pid,
            creation_time: child.creation_time,
        },
        &child.program,
        &user,
    ) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return report_unaddressable(
                &request,
                &receipt,
                format!(
                    "the app-server child (pid {}, {}) recorded for this conversation is not running; the conversation can no longer be addressed and nothing was sent - the host is expected to record the run's failure, so observe the run before continuing the exact session",
                    child.pid,
                    child.program.display()
                ),
                &remedy,
                started,
            );
        }
        Err(error) => {
            return Err(invalid(&format!(
                "the recorded app-server child (pid {}, {}) could not be verified: {error}; nothing was delivered",
                child.pid,
                child.program.display()
            )));
        }
    }
    // The live conversation itself: connected through its recorded endpoint,
    // read back so the input goes to the addressed slot's own thread.
    let mut conversation = Conversation::attach(&endpoint, CONTROL_BOUND).map_err(|error| {
        invalid(&format!(
            "the recorded control endpoint on port {} did not accept an addressed connection: {error}; nothing was delivered",
            endpoint.port()
        ))
    })?;
    let thread = conversation.thread_state()?;
    verify_live_thread(&value, &thread, &request)?;
    let mut stdout = io::stdout();
    // What this content already established: a delivered or queued text is
    // never submitted again, an indeterminate attempt must be resolved by
    // evidence before anything repeats it, and a definite error is retryable.
    let client_message_id = content_id(&thread_id, &request.text);
    let previous = read_attempts(&value)
        .into_iter()
        .find(|attempt| attempt["id"].as_str() == Some(client_message_id.as_str()));
    let previous_status = previous
        .as_ref()
        .map(|attempt| recorded_status(attempt).to_owned())
        .unwrap_or_default();
    if previous_status == STATUS_DELIVERED {
        return report_repeat(
            &request,
            &receipt,
            previous
                .as_ref()
                .expect("a recorded status has its attempt"),
            STATUS_DELIVERED,
            started,
        );
    }
    if previous_status == STATUS_QUEUED || previous_status == STATUS_PENDING {
        let evidence = observe_input(
            &mut conversation,
            &client_message_id,
            &request.text,
            Instant::now() + EVIDENCE_WINDOW,
        );
        let attempt = previous
            .as_ref()
            .expect("a recorded status has its attempt");
        if let Some(evidence) = evidence {
            record_attempt(&receipt, delivered_entry(attempt, &evidence))?;
            return report_delivered(&request, &receipt, &evidence, started);
        }
        return match previous_status.as_str() {
            STATUS_QUEUED => report_repeat(&request, &receipt, attempt, STATUS_QUEUED, started),
            _ => report_indeterminate(
                &request,
                &receipt,
                attempt,
                "an earlier attempt with this exact content stopped before its outcome was recorded, and the conversation's own items do not show it",
                &remedy,
                started,
            ),
        };
    }
    if previous_status == STATUS_INDETERMINATE {
        let evidence = observe_input(
            &mut conversation,
            &client_message_id,
            &request.text,
            Instant::now() + EVIDENCE_WINDOW,
        );
        let attempt = previous
            .as_ref()
            .expect("a recorded status has its attempt");
        if let Some(evidence) = evidence {
            record_attempt(&receipt, delivered_entry(attempt, &evidence))?;
            return report_delivered(&request, &receipt, &evidence, started);
        }
        return report_indeterminate(
            &request,
            &receipt,
            attempt,
            "an earlier attempt with this exact content was recorded as indeterminate, and the conversation's own items do not show it",
            &remedy,
            started,
        );
    }
    let previous_attempts = previous
        .as_ref()
        .and_then(|attempt| attempt["attempts"].as_u64())
        .unwrap_or(0);
    let mut sent = 0u64;
    let mut expected_turn: Option<String> = None;
    let mut outcome: Option<Delivery> = None;
    while sent < DELIVERY_ROUNDS as u64 {
        let active = active_turn(&thread);
        expected_turn = active.clone();
        let (method, params) =
            request_params(&thread_id, &client_message_id, &request.text, &active);
        sent += 1;
        // The attempt is recorded before the request, so an invocation that is
        // interrupted mid-request leaves its content identity, method and turn
        // behind instead of an invitation to deliver the same text again.
        record_attempt(
            &receipt,
            pending_entry(
                &client_message_id,
                method,
                active.as_deref(),
                &request.text,
                previous_attempts + sent,
            ),
        )?;
        let (turn, rejection, unanswered) = match conversation.request(method, params) {
            Ok(Reply::Result(result)) => {
                let turn = answer_turn(&result);
                if turn.is_empty() {
                    // An acceptance without a turn identity leaves the input's
                    // fate to the conversation's own items, exactly like an
                    // answer that never arrived.
                    (
                        None,
                        None,
                        Some(format!(
                            "{method} answered without a turn identity, so which turn holds the input is unknown"
                        )),
                    )
                } else {
                    (Some(turn), None, None)
                }
            }
            Ok(Reply::Rejected(error)) => (
                None,
                Some(observation::excerpt(&error.to_string(), 400)),
                None,
            ),
            // No answer arrived, so whether the input was applied is unknown
            // until the conversation's own items say so.
            Ok(Reply::Unanswered) => (
                None,
                None,
                Some(format!(
                    "{method} was not answered within {}s, so whether the input reached the conversation is unknown",
                    CONTROL_BOUND.as_secs()
                )),
            ),
            Err(error) => (
                None,
                None,
                Some(format!(
                    "{method} failed on the recorded control endpoint: {}",
                    observation::excerpt(&error.to_string(), 400)
                )),
            ),
        };
        // The answer is transport acceptance; only the conversation's own items
        // establish delivery. A refusal is read once - it applied nothing - while
        // an acceptance or a request without an answer is watched for the whole
        // bounded window.
        let window = if rejection.is_some() {
            Duration::ZERO
        } else {
            EVIDENCE_WINDOW
        };
        let evidence = observe_input(
            &mut conversation,
            &client_message_id,
            &request.text,
            Instant::now() + window,
        );
        if let Some(evidence) = evidence {
            outcome = Some(Delivery::Delivered {
                method: method.to_owned(),
                turn,
                evidence,
            });
            break;
        }
        if let Some(cause) = unanswered {
            outcome = Some(Delivery::Indeterminate {
                method: method.to_owned(),
                turn: active.clone(),
                cause,
            });
            break;
        }
        match rejection {
            // A refusal applied nothing, but the turn may have changed under
            // the request: the thread is read again and the input is submitted
            // once more to its current turn.
            Some(cause) if sent < DELIVERY_ROUNDS as u64 => match conversation.thread_state() {
                Ok(refreshed) => {
                    verify_live_thread(&value, &refreshed, &request)?;
                    writeln!(
                        stdout,
                        "note: {method} was refused ({cause}); the conversation still runs at the addressed slot, so the input is submitted again to its current turn"
                    )?;
                }
                Err(error) => {
                    outcome = Some(Delivery::Error {
                        method: method.to_owned(),
                        turn: active.clone(),
                        cause: format!(
                            "{method} was refused ({cause}) and the conversation could not be re-read: {error}"
                        ),
                    });
                    break;
                }
            },
            Some(cause) => {
                outcome = Some(Delivery::Error {
                    method: method.to_owned(),
                    turn: active.clone(),
                    cause: format!("{method} was refused after {sent} rounds: {cause}"),
                });
                break;
            }
            None => {
                outcome = Some(Delivery::Queued {
                    method: method.to_owned(),
                    turn,
                });
                break;
            }
        }
    }
    let Some(outcome) = outcome else {
        return Err(io::Error::other(
            "the delivery rounds ended without establishing an outcome; nothing is reported as delivered",
        ));
    };
    let attempts = previous_attempts + sent;
    match &outcome {
        Delivery::Delivered { evidence, .. } => {
            let entry = delivered_entry_of(
                &client_message_id,
                &outcome,
                evidence,
                &request.text,
                attempts,
                expected_turn.as_deref(),
            );
            record_attempt(&receipt, entry)?;
            report_delivered(&request, &receipt, evidence, started)
        }
        Delivery::Queued { .. } | Delivery::Error { .. } | Delivery::Indeterminate { .. } => {
            record_attempt(
                &receipt,
                outcome_entry(
                    &client_message_id,
                    &outcome,
                    &request.text,
                    attempts,
                    expected_turn.as_deref(),
                ),
            )?;
            match &outcome {
                Delivery::Queued { method, turn } => {
                    report_queued(&request, &receipt, method, turn.as_deref(), started)
                }
                Delivery::Error {
                    method,
                    turn,
                    cause,
                } => report_error(&request, &receipt, method, turn.as_deref(), cause, started),
                Delivery::Indeterminate {
                    method,
                    turn,
                    cause,
                } => {
                    let attempt = json!({
                        "id": client_message_id,
                        "status": STATUS_INDETERMINATE,
                        "method": method,
                        "turnId": turn,
                    });
                    report_indeterminate(&request, &receipt, &attempt, cause, &remedy, started)
                }
                Delivery::Delivered { .. } => unreachable!(),
            }
        }
    }
}

/// One delivery outcome of this invocation, which is also the recorded status
/// of its attempt.
enum Delivery {
    /// The input was observed in the conversation's own items.
    Delivered {
        method: String,
        turn: Option<String>,
        evidence: InputEvidence,
    },
    /// Accepted by the recorded turn without observed evidence.
    Queued {
        method: String,
        turn: Option<String>,
    },
    /// The native endpoint refused the request; nothing was delivered.
    Error {
        method: String,
        turn: Option<String>,
        cause: String,
    },
    /// The request may or may not have been applied.
    Indeterminate {
        method: String,
        turn: Option<String>,
        cause: String,
    },
}

/// What the conversation's own record showed about a delivered input.
#[derive(Debug, Clone)]
struct InputEvidence {
    turn: String,
    item: String,
    /// `clientId` when the item carried the recorded client message id; `text`
    /// when only the exact delivered text matched.
    by: &'static str,
    observed_ms: u64,
}

impl InputEvidence {
    fn detail(&self) -> String {
        match self.by {
            "clientId" => format!(
                "observed as userMessage {} of turn {}, correlated by the recorded client message id",
                self.item, self.turn
            ),
            _ => format!(
                "observed as userMessage {} of turn {}, carrying the exact delivered text",
                self.item, self.turn
            ),
        }
    }
}

/// The content identity of one message within one conversation: the same
/// literal text addressed to the same thread is one identity, so a repeat of it
/// is a repeat and not a second delivery. The identity is a digest, so the
/// receipt never carries the message text.
fn content_id(thread_id: &str, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(thread_id.as_bytes());
    hasher.update([0u8]);
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("msg-{hex}")
}

/// The request of one delivery round: `turn/steer` with the active turn as its
/// precondition when a turn is running (it is applied at the nearest supported
/// point and does not interrupt it), otherwise `turn/start` on the same thread.
/// The recorded client message id travels with the input, which is what the
/// conversation's own item can be correlated by.
fn request_params(
    thread_id: &str,
    client_message_id: &str,
    text: &str,
    active: &Option<String>,
) -> (&'static str, Value) {
    let input = json!([{"type": "text", "text": text}]);
    match active {
        Some(turn) => (
            "turn/steer",
            json!({
                "threadId": thread_id,
                "expectedTurnId": turn,
                "input": input,
                "clientUserMessageId": client_message_id,
            }),
        ),
        None => (
            "turn/start",
            json!({
                "threadId": thread_id,
                "input": input,
                "clientUserMessageId": client_message_id,
            }),
        ),
    }
}

/// The turn an accepted request named: `turn/steer` answers with the active turn
/// id, `turn/start` with the turn it started.
fn answer_turn(result: &Value) -> String {
    result["turnId"]
        .as_str()
        .or_else(|| result["turn"]["id"].as_str())
        .unwrap_or_default()
        .to_owned()
}

/// Watches the conversation's own items for the input this invocation
/// submitted, within the bounded window.
fn observe_input(
    conversation: &mut Conversation,
    client_message_id: &str,
    text: &str,
    until: Instant,
) -> Option<InputEvidence> {
    loop {
        match conversation.thread_state() {
            Ok(thread) => {
                if let Some(evidence) = input_evidence(&thread, client_message_id, text) {
                    return Some(evidence);
                }
            }
            // A conversation that cannot be read cannot show the input; saying
            // so is honest, and waiting longer would only delay the report.
            Err(_) => return None,
        }
        if Instant::now() >= until {
            return None;
        }
        thread::sleep(POLL);
    }
}

/// The conversation's own record of one input: exact correlation by the
/// recorded client message id first, then the exact delivered text.
fn input_evidence(thread: &Value, client_message_id: &str, text: &str) -> Option<InputEvidence> {
    let turns = thread["turns"].as_array()?;
    let items = |matching: &dyn Fn(&Value, &str) -> bool| -> Option<InputEvidence> {
        for turn in turns.iter().rev() {
            let turn_id = turn["id"].as_str().unwrap_or_default().to_owned();
            let Some(items) = turn["items"].as_array() else {
                continue;
            };
            for item in items.iter().rev() {
                if item["type"] != "userMessage" {
                    continue;
                }
                if matching(item, &turn_id) {
                    return Some(InputEvidence {
                        turn: turn_id.clone(),
                        item: item["id"].as_str().unwrap_or_default().to_owned(),
                        by: "clientId",
                        observed_ms: now_ms(),
                    });
                }
            }
        }
        None
    };
    if let Some(evidence) =
        items(&|item: &Value, _: &str| item["clientId"].as_str() == Some(client_message_id))
    {
        return Some(evidence);
    }
    items(&|item: &Value, _: &str| control::user_message_text(item) == text).map(|mut evidence| {
        evidence.by = "text";
        evidence
    })
}

/// Verifies the live conversation is the addressed run before anything is
/// delivered: the recorded session identity is the live thread, it runs in the
/// addressed slot, and it carries the profile binding the receipt recorded.
fn verify_live_thread(receipt: &Value, thread: &Value, request: &Request) -> io::Result<()> {
    let recorded_session = RunObservation::from_receipt(receipt)
        .and_then(|run| run.recorded_session().map(str::to_owned));
    if let (Some(recorded), Some(live)) = (recorded_session.as_deref(), thread["id"].as_str())
        && recorded != live
    {
        return Err(invalid(&format!(
            "the live conversation reports session {live} instead of the recorded {recorded}; nothing was delivered because the address no longer names the recorded run"
        )));
    }
    let slot = slot_of(receipt);
    if let Some(cwd) = thread["cwd"].as_str()
        && !same_directory(Path::new(cwd), &slot)
    {
        return Err(invalid(&format!(
            "the live conversation runs at {cwd} instead of the addressed slot {}; nothing was delivered",
            slot.display()
        )));
    }
    let identity = &receipt["control"]["identity"];
    if !identity.is_null()
        && let Ok(bound) = serde_json::from_value::<BoundIdentity>(identity.clone())
        && let Err(mismatch) = bound.verify(thread)
    {
        return Err(invalid(&format!(
            "the live conversation does not carry the profile binding slot {} recorded: {mismatch}; nothing was delivered into a conversation routed differently from the address",
            request.slot
        )));
    }
    Ok(())
}

/// The slot path the receipt's own slot binding records.
fn slot_of(receipt: &Value) -> PathBuf {
    receipt["slot"]["path"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// True when both paths name the same directory: canonicalization resolves the
/// different spellings of one path, and comparison is case-insensitive because
/// that is the Windows path rule.
fn same_directory(a: &Path, b: &Path) -> bool {
    if b.as_os_str().is_empty() {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy()),
        _ => a
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches(['\\', '/'])),
    }
}

fn lease_identity(lease: &SessionLease) -> HostIdentity {
    HostIdentity {
        pid: lease.pid,
        created: lease.created,
        program: lease.program.clone(),
    }
}

/// The recorded attempts of the receipt, oldest first.
fn read_attempts(receipt: &Value) -> Vec<Value> {
    receipt["messages"]
        .as_array()
        .map(|messages| messages.to_vec())
        .unwrap_or_default()
}

fn recorded_status(attempt: &Value) -> &str {
    attempt["status"].as_str().unwrap_or_default()
}

fn pending_entry(
    id: &str,
    method: &str,
    expected_turn: Option<&str>,
    text: &str,
    attempts: u64,
) -> Value {
    json!({
        "schema": SCHEMA,
        "id": id,
        "clientMessageId": id,
        "status": STATUS_PENDING,
        "method": method,
        "expectedTurnId": expected_turn,
        "turnId": Value::Null,
        "evidence": Value::Null,
        "attempts": attempts,
        "bytes": text.len(),
        "lines": text.lines().count().max(1),
        "requestedMs": now_ms(),
        "recordedMs": now_ms(),
        "deliveredMs": Value::Null,
        "detail": "the input is being submitted to the conversation and its own items are being read",
    })
}

/// The committed record of one content, upgraded to delivered by evidence a
/// later invocation observed. The attempt count it already carries stands: this
/// invocation sent nothing.
fn delivered_entry(previous: &Value, evidence: &InputEvidence) -> Value {
    let mut entry = previous.clone();
    entry["status"] = Value::String(STATUS_DELIVERED.into());
    entry["turnId"] = Value::String(evidence.turn.clone());
    entry["evidence"] = Value::String(evidence.detail());
    entry["deliveredMs"] = Value::from(evidence.observed_ms);
    entry["recordedMs"] = Value::from(now_ms());
    entry["detail"] = Value::String(format!(
        "the input is in the conversation's own items ({}); whether the executor applied it is the executor's own report",
        evidence.detail()
    ));
    entry
}

fn delivered_entry_of(
    id: &str,
    outcome: &Delivery,
    evidence: &InputEvidence,
    text: &str,
    attempts: u64,
    expected_turn: Option<&str>,
) -> Value {
    let mut entry = outcome_entry(id, outcome, text, attempts, expected_turn);
    entry["status"] = Value::String(STATUS_DELIVERED.into());
    // The turn the conversation's own record places the input in is the turn
    // this attempt is known to have reached.
    entry["turnId"] = Value::String(evidence.turn.clone());
    entry["evidence"] = Value::String(evidence.detail());
    entry["deliveredMs"] = Value::from(evidence.observed_ms);
    entry["recordedMs"] = Value::from(now_ms());
    entry["detail"] = Value::String(format!(
        "the input is in the conversation's own items ({}); whether the executor applied it is the executor's own report",
        evidence.detail()
    ));
    entry
}

fn outcome_entry(
    id: &str,
    outcome: &Delivery,
    text: &str,
    attempts: u64,
    expected_turn: Option<&str>,
) -> Value {
    let (status, method, turn, cause) = match outcome {
        Delivery::Delivered { method, turn, .. } => (STATUS_DELIVERED, method, turn, ""),
        Delivery::Queued { method, turn } => (STATUS_QUEUED, method, turn, ""),
        Delivery::Error {
            method,
            turn,
            cause,
        } => (STATUS_ERROR, method, turn, cause.as_str()),
        Delivery::Indeterminate {
            method,
            turn,
            cause,
        } => (STATUS_INDETERMINATE, method, turn, cause.as_str()),
    };
    let detail = match status {
        STATUS_QUEUED => format!(
            "accepted by {method} for turn {}; the conversation's own items do not show the input yet",
            turn.as_deref().unwrap_or("unrecorded")
        ),
        STATUS_ERROR => format!("{method} was refused: {cause}; nothing was delivered"),
        STATUS_INDETERMINATE => format!(
            "{cause}; the input may or may not have been applied, so it is not reported as delivered"
        ),
        _ => String::new(),
    };
    json!({
        "schema": SCHEMA,
        "id": id,
        "clientMessageId": id,
        "status": status,
        "method": method,
        "expectedTurnId": expected_turn,
        "turnId": turn,
        "evidence": Value::Null,
        "attempts": attempts,
        "bytes": text.len(),
        "lines": text.lines().count().max(1),
        "requestedMs": now_ms(),
        "recordedMs": now_ms(),
        "deliveredMs": Value::Null,
        "detail": detail,
    })
}

/// Records one attempt under the receipt's own lock, merged by content
/// identity: the lock is the one the observation owner's receipt writes use, so
/// a stop marking queued messages undelivered is never overwritten by a
/// concurrent delivery record.
fn record_attempt(receipt: &Path, entry: Value) -> io::Result<()> {
    let lock = receipt.with_extension("lock");
    if let Some(parent) = lock.parent() {
        fs::create_dir_all(parent)?;
    }
    let deadline = Deadline::after(LOCK_WAIT)?;
    let _lock =
        ExclusiveFileLock::acquire(&lock, deadline, &Cancellation::default()).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("receipt lock {} was not acquired: {error}", lock.display()),
            )
        })?;
    let mut document: Value = serde_json::from_slice(&fs::read(receipt)?)?;
    let id = entry["id"].as_str().unwrap_or_default().to_owned();
    let mut messages = read_attempts(&document);
    match messages
        .iter()
        .position(|message| message["id"].as_str() == Some(id.as_str()))
    {
        Some(index) => {
            // The committed record of the same content keeps the highest
            // attempt count and the request time its own pending write recorded,
            // so the outcome entry describes the attempt that was sent.
            let mut merged = entry.clone();
            if let (Some(previous), Some(combined)) = (
                messages[index].get("attempts").and_then(Value::as_u64),
                entry.get("attempts").and_then(Value::as_u64),
            ) && previous > combined
            {
                merged["attempts"] = Value::from(previous);
            }
            if recorded_status(&messages[index]) == STATUS_PENDING
                && recorded_status(&merged) != STATUS_PENDING
                && let Some(requested) = messages[index]["requestedMs"].as_u64()
            {
                merged["requestedMs"] = Value::from(requested);
            }
            messages[index] = merged;
        }
        None => messages.push(entry),
    }
    while messages.len() > MAX_ATTEMPTS {
        // A failed attempt is retryable, so it is the oldest disposable record;
        // a delivered, queued or indeterminate one is what prevents a repeat of
        // the same content, so it is dropped only when nothing else can be.
        match messages
            .iter()
            .position(|message| recorded_status(message) == STATUS_ERROR)
        {
            Some(index) => {
                messages.remove(index);
            }
            None => {
                messages.remove(0);
            }
        }
    }
    document["messages"] = Value::Array(messages);
    let bytes = serde_json::to_vec_pretty(&document)?;
    // A fresh temp name per writer keeps a stale leftover from a killed writer
    // from being replaced under an unrelated rename.
    let temp = receipt.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, bytes)?;
    fs::rename(&temp, receipt)?;
    Ok(())
}

fn resume_remedy(request: &Request) -> String {
    format!(
        "codex-harness executor resume --source {} --codex-home {} --slot {} --owner {}{}",
        request.source.display(),
        request.codex_home.display(),
        request.slot,
        request.owner,
        match &request.session {
            Some(session) => format!(" --session {session}"),
            None => String::new(),
        }
    )
}

fn now_ms() -> u64 {
    observation::now_ms()
}

/// Nothing was sent because the addressed run's lifecycle already ended: its
/// state and result stand, and the remedy is the exact-session continuation.
fn report_ended(
    request: &Request,
    receipt: &Path,
    run: &RunObservation,
    remedy: &str,
    started: Instant,
) -> io::Result<i32> {
    let mut out = io::stdout();
    writeln!(
        out,
        "executor message: slot {} owner {} session {}: not delivered ({}) in {}ms",
        request.slot,
        request.owner,
        request.session.as_deref().unwrap_or("unrecorded"),
        run.state,
        started.elapsed().as_millis()
    )?;
    writeln!(
        out,
        "state: {}{}",
        run.state,
        match run.cause.as_deref() {
            Some(cause) => format!(" ({cause})"),
            None => String::new(),
        }
    )?;
    if let Some(result) = &run.result {
        writeln!(out, "result: {}", result.display())?;
    }
    writeln!(
        out,
        "nothing was sent: a finished run keeps its own conversation and result, and message starts no new one"
    )?;
    writeln!(out, "receipt: {} state: {}", receipt.display(), run.state)?;
    writeln!(out, "continue: {remedy}")?;
    Ok(2)
}

/// Nothing was sent because the live conversation cannot be addressed right
/// now: no control endpoint, no thread, no verified host or child. What was
/// observed is reported verbatim with the continuation remedy.
fn report_unaddressable(
    request: &Request,
    receipt: &Path,
    cause: String,
    remedy: &str,
    started: Instant,
) -> io::Result<i32> {
    let mut out = io::stdout();
    writeln!(
        out,
        "executor message: slot {} owner {} session {}: not delivered (unavailable) in {}ms",
        request.slot,
        request.owner,
        request.session.as_deref().unwrap_or("unrecorded"),
        started.elapsed().as_millis()
    )?;
    writeln!(out, "unavailable: {cause}")?;
    writeln!(
        out,
        "nothing was sent: no local write is presented as model delivery, and no new conversation is started"
    )?;
    writeln!(out, "receipt: {}", receipt.display())?;
    writeln!(out, "continue: {remedy}")?;
    Ok(2)
}

/// A previously recorded delivery stands: the same content is not sent again.
fn report_repeat(
    request: &Request,
    receipt: &Path,
    previous: &Value,
    status: &str,
    started: Instant,
) -> io::Result<i32> {
    let mut out = io::stdout();
    writeln!(
        out,
        "executor message: slot {} owner {} session {}: already {status} in {}ms",
        request.slot,
        request.owner,
        request.session.as_deref().unwrap_or("unrecorded"),
        started.elapsed().as_millis()
    )?;
    writeln!(
        out,
        "recorded: identity {}, status {status}, method {}, turn {}, attempts {}",
        previous["id"].as_str().unwrap_or("unrecorded"),
        previous["method"].as_str().unwrap_or("unrecorded"),
        previous["turnId"].as_str().unwrap_or("unrecorded"),
        previous["attempts"].as_u64().unwrap_or(0)
    )?;
    if let Some(detail) = previous["detail"].as_str() {
        writeln!(out, "detail: {detail}")?;
    }
    writeln!(
        out,
        "nothing was sent again: the same literal text addressed to the same conversation is one input, so a repeat cannot deliver it twice"
    )?;
    writeln!(out, "receipt: {}", receipt.display())?;
    Ok(0)
}

fn report_delivered(
    request: &Request,
    receipt: &Path,
    evidence: &InputEvidence,
    started: Instant,
) -> io::Result<i32> {
    let mut out = io::stdout();
    writeln!(
        out,
        "executor message: slot {} owner {} session {}: delivered in {}ms",
        request.slot,
        request.owner,
        request.session.as_deref().unwrap_or("unrecorded"),
        started.elapsed().as_millis()
    )?;
    writeln!(out, "delivery: {}", evidence.detail())?;
    writeln!(
        out,
        "the input is in the conversation; whether the executor already applied it is the executor's own report, not this command's"
    )?;
    writeln!(out, "receipt: {}", receipt.display())?;
    Ok(0)
}

fn report_queued(
    request: &Request,
    receipt: &Path,
    method: &str,
    turn: Option<&str>,
    started: Instant,
) -> io::Result<i32> {
    let mut out = io::stdout();
    writeln!(
        out,
        "executor message: slot {} owner {} session {}: queued in {}ms",
        request.slot,
        request.owner,
        request.session.as_deref().unwrap_or("unrecorded"),
        started.elapsed().as_millis()
    )?;
    writeln!(
        out,
        "queued: {method} accepted the input for turn {}; the conversation's own items do not show it yet, so it is in the conversation and its application to the model is not yet observed",
        turn.unwrap_or("unrecorded")
    )?;
    writeln!(
        out,
        "the tool call in flight was not interrupted: the input is applied at the nearest supported point"
    )?;
    writeln!(out, "receipt: {}", receipt.display())?;
    Ok(0)
}

fn report_error(
    request: &Request,
    receipt: &Path,
    method: &str,
    turn: Option<&str>,
    cause: &str,
    started: Instant,
) -> io::Result<i32> {
    let mut out = io::stdout();
    writeln!(
        out,
        "executor message: slot {} owner {} session {}: error in {}ms",
        request.slot,
        request.owner,
        request.session.as_deref().unwrap_or("unrecorded"),
        started.elapsed().as_millis()
    )?;
    writeln!(
        out,
        "error: {method} for turn {} was refused by the native endpoint: {cause}; nothing was delivered",
        turn.unwrap_or("unrecorded")
    )?;
    writeln!(
        out,
        "the recorded attempt is retryable: a refusal applied nothing, so the same text may be sent again"
    )?;
    writeln!(out, "receipt: {}", receipt.display())?;
    Ok(1)
}

fn report_indeterminate(
    request: &Request,
    receipt: &Path,
    previous: &Value,
    cause: &str,
    remedy: &str,
    started: Instant,
) -> io::Result<i32> {
    let mut out = io::stdout();
    writeln!(
        out,
        "executor message: slot {} owner {} session {}: indeterminate in {}ms",
        request.slot,
        request.owner,
        request.session.as_deref().unwrap_or("unrecorded"),
        started.elapsed().as_millis()
    )?;
    writeln!(out, "indeterminate: {cause}")?;
    writeln!(
        out,
        "attempt identity: {} (method {}, turn {})",
        previous["id"].as_str().unwrap_or("unrecorded"),
        previous["method"].as_str().unwrap_or("unrecorded"),
        previous["turnId"].as_str().unwrap_or("unrecorded")
    )?;
    writeln!(
        out,
        "nothing is reported as delivered: this attempt may or may not have applied the input"
    )?;
    writeln!(
        out,
        "repeat: the same literal text addressed to this conversation stays refused while the attempt is indeterminate, so it cannot be delivered twice"
    )?;
    writeln!(
        out,
        "next action: inspect the conversation (the run's own tab, or `codex-harness executor watch --source {} --codex-home {} --slot {}`); if the input is genuinely absent, send the correction again with a distinguishing first line, or continue the exact session with `{remedy}`",
        request.source.display(),
        request.codex_home.display(),
        request.slot
    )?;
    writeln!(out, "receipt: {}", receipt.display())?;
    Ok(1)
}
