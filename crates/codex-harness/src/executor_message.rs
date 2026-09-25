//! Addressed delivery of one literal message into a live pooled executor
//! conversation.
//!
//! The command addresses one run through the accepted checkout, the kit home,
//! the slot, the owner and, when given, the exact recorded session. A lead
//! reply instead supplies only `--reply-to MESSAGE_ID` and `--text` or a UTF-8
//! `--file`. That id is resolved from the compact lifecycle lookup `lead
//! message` writes; the lookup is a locator, not sender identity. Either form
//! verifies the recorded slot binding, the live lease, the recorded host
//! identity, the app-server child and the run's own state, then delivers the
//! literal text, with no shell evaluation and its real line breaks, into that
//! run's recorded conversation through `endpoint-<index>.json`.
//!
//! Reply resolution fails closed before any send: an unknown or retired id,
//! another lead, a reused slot or generation, and a contradictory explicit
//! address are refused. A fully addressed message without `--reply-to` keeps
//! its current behavior. Reply addressing does not use the lead's working
//! directory and does not start a listener.
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
    /// The lead-message id this invocation answers, when it is a correlated reply.
    reply_to: Option<String>,
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
        let mut reply_to = None;
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
                "--reply-to" => reply_to = Some(option_text(value)?),
                _ => return Err(invalid("invalid native executor options")),
            }
        }
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
        let slot = match slot {
            Some(value) => Some(
                value
                    .parse()
                    .map_err(|_| invalid("--slot must be a positive pool slot index"))?,
            ),
            None => None,
        };
        let owner = match owner {
            Some(owner) if !owner.trim().is_empty() => Some(owner),
            Some(_) => return Err(invalid("--owner ID is required")),
            None => None,
        };
        if let Some(id) = reply_to {
            return resolve_reply(
                &id,
                &ExplicitAddress {
                    source,
                    codex_home,
                    slot,
                    owner,
                    session,
                },
                text,
            );
        }
        Ok(Self {
            source: required(source, "--source")?,
            codex_home: required(codex_home, "--codex-home")?,
            slot: slot.ok_or_else(|| invalid("--slot is required"))?,
            owner: owner.ok_or_else(|| invalid("--owner ID is required"))?,
            session,
            text,
            reply_to: None,
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

/// Marks the one answered lead-message request resolved. Other requests stay
/// as recorded. A message that is not a correlated reply changes nothing.
/// Queued, refused, and indeterminate attempts do not call this: only observed
/// delivery clears that request's hold.
fn resolve_observed_reply(request: &Request, receipt: &Path) -> io::Result<()> {
    let Some(id) = request.reply_to.as_deref() else {
        return Ok(());
    };
    let current = read_receipt_value(receipt)?;
    let Some(existing) = current.get("leadMessages").and_then(Value::as_array) else {
        return Err(invalid(&format!(
            "delivered reply {id} was observed, but the receipt has no leadMessages record to resolve"
        )));
    };
    let mut messages = existing.clone();
    let Some(entry) = messages
        .iter_mut()
        .find(|entry| entry.get("id").and_then(Value::as_str) == Some(id))
    else {
        return Err(invalid(&format!(
            "delivered reply {id} was observed, but that request is not on the receipt"
        )));
    };
    if entry.get("kind").and_then(Value::as_str) != Some(KIND_REQUEST) {
        return Err(invalid(&format!(
            "delivered reply {id} was observed, but the record is not a reply-request"
        )));
    }
    entry["status"] = Value::String("resolved".into());
    observation::update_receipt_field(receipt, "leadMessages", Value::Array(messages))
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
    if status == STATUS_DELIVERED {
        resolve_observed_reply(request, receipt)?;
    }
    writeln!(out, "receipt: {}", receipt.display())?;
    Ok(0)
}

fn report_delivered(
    request: &Request,
    receipt: &Path,
    evidence: &InputEvidence,
    started: Instant,
) -> io::Result<i32> {
    resolve_observed_reply(request, receipt)?;
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

/// Usage for `codex-harness lead message`. No recipient, slot, session,
/// checkout, or lead address is accepted.
pub(crate) const LEAD_USAGE: &str = "\
codex-harness lead message (--text TEXT | --file FILE) [--notify]
  Send one literal payload to the originating lead of this spawned run.
  The caller supplies no recipient, slot, session, checkout, or lead address.
  The default kind is reply-request. --notify does not request a reply.
";

const PAYLOAD_MARK: &str = "\n---payload---\n";
const KIND_REQUEST: &str = "reply-request";
const KIND_NOTICE: &str = "notification";
const UNAVAILABLE: &str = "unavailable";

struct LeadInput {
    text: Option<String>,
    file: Option<PathBuf>,
    notify: bool,
}

struct SpawnedRun {
    receipt: PathBuf,
    lead_thread: String,
    generation: String,
    owner: String,
    session: String,
    checkout: String,
    worktree: String,
    slot: u32,
    assignment: String,
    bd: String,
}

/// Sends one payload from a verified member of a live spawned run to that
/// run's recorded originating lead. Authority is the run-generation marker,
/// the live dispatcher identity, and membership in the recorded host's process
/// lineage. A copied marker, cwd, label, or most recent session is not
/// authority. Delivery uses the existing app-server owner: `turn/steer` on an
/// active turn and `turn/start` on an idle thread. No listener is started.
pub(crate) fn lead_message(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!("{LEAD_USAGE}");
        return Ok(0);
    }
    let input = parse_lead_input(args)?;
    let run = resolve_spawned_run()?;
    let endpoint = verified_lead_endpoint(&run)?;
    let payload = lead_payload(&input)?;
    let kind = if input.notify {
        KIND_NOTICE
    } else {
        KIND_REQUEST
    };
    let nonce = recorded_lead_messages(&run.receipt)?.len() as u64 + 1;
    let id = lead_message_id(&run.generation, kind, nonce, &payload);
    let reply = if input.notify {
        "not-requested".to_owned()
    } else {
        format!("codex-harness executor message --reply-to {id} --text TEXT")
    };
    let header = json!({
        "id": id,
        "kind": kind,
        "owner": run.owner,
        "runGeneration": run.generation,
        "session": run.session,
        "checkout": run.checkout,
        "worktree": run.worktree,
        "slot": run.slot,
        "leadThreadId": run.lead_thread,
        "assignment": run.assignment,
        "bd": run.bd,
        "reply": reply,
    });
    let header = serde_json::to_string(&header).map_err(|error| {
        invalid(&format!(
            "lead message header could not be encoded: {error}; nothing was sent"
        ))
    })?;
    let envelope = format!("{header}{PAYLOAD_MARK}{payload}");
    if envelope.len() as u64 > MAX_TEXT {
        return Err(invalid(&format!(
            "lead message exceeds the {MAX_TEXT}-byte bound; nothing was sent"
        )));
    }
    deliver_to_recorded_lead(&run, &endpoint, &id, kind, &envelope, &payload)
}

fn parse_lead_input(args: &[OsString]) -> io::Result<LeadInput> {
    let mut text = None;
    let mut file = None;
    let mut notify = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let key = arg
            .to_str()
            .ok_or_else(|| lead_override("an option was not Unicode"))?;
        match key {
            "--notify" => notify = true,
            "--text" => {
                let value = iter
                    .next()
                    .ok_or_else(|| invalid("--text needs its literal TEXT; nothing was sent"))?;
                text = Some(option_text(value)?);
            }
            "--file" => {
                let value = iter
                    .next()
                    .ok_or_else(|| invalid("--file needs its FILE; nothing was sent"))?;
                file = Some(PathBuf::from(value));
            }
            "--help" => {
                return Err(lead_override(
                    "--help cannot be combined with another argument",
                ));
            }
            _ => return Err(lead_override(key)),
        }
    }
    match (&text, &file) {
        (Some(_), Some(_)) => {
            return Err(invalid(
                "give either --text TEXT or --file FILE, not both; nothing was sent",
            ));
        }
        (None, None) => {
            return Err(invalid(
                "a lead message needs its literal content: --text TEXT or --file FILE; nothing was sent",
            ));
        }
        _ => {}
    }
    Ok(LeadInput { text, file, notify })
}

fn lead_override(what: &str) -> io::Error {
    invalid(&format!(
        "lead message accepts only --text, --file, or --notify; a recipient, slot, session, checkout, or lead override is not accepted ({what}); refusing before any send"
    ))
}

fn lead_payload(input: &LeadInput) -> io::Result<String> {
    let text = match (&input.text, &input.file) {
        (Some(text), None) => text.clone(),
        (None, Some(file)) => read_message_file(file)?,
        _ => {
            return Err(invalid(
                "a lead message needs exactly one of --text or --file; nothing was sent",
            ));
        }
    };
    if text.is_empty() {
        return Err(invalid(
            "the message is empty; nothing would be delivered; nothing was sent",
        ));
    }
    if text.len() as u64 > MAX_TEXT {
        return Err(invalid(&format!(
            "lead message exceeds the {MAX_TEXT}-byte bound; nothing was sent"
        )));
    }
    Ok(text)
}

fn resolve_spawned_run() -> io::Result<SpawnedRun> {
    let marker = std::env::var(control::EXECUTOR_RUN_ENV).map_err(|_| {
        invalid(
            "not a member of a live spawned executor run: HARNESS_EXECUTOR_RUN is missing; cwd, a label, and the most recent session are not authority; refusing before any send",
        )
    })?;
    let marker = marker.trim();
    if marker.is_empty() {
        return Err(invalid(
            "not a member of a live spawned executor run: HARNESS_EXECUTOR_RUN is blank; a copied marker is not authority; refusing before any send",
        ));
    }
    let home = std::env::var_os("CODEX_HOME").ok_or_else(|| {
        invalid(
            "spawned run context is missing: CODEX_HOME is not set, so the run marker cannot name a recorded lead; refusing before any send",
        )
    })?;
    let matches = matching_receipts(Path::new(&home), marker)?;
    let receipt = match matches.as_slice() {
        [one] => one.clone(),
        [] => {
            return Err(invalid(
                "no live spawned run records this run generation; a copied or stale marker is not authority; refusing before any send",
            ));
        }
        _ => {
            return Err(invalid(
                "this run generation matches more than one spawned run; refusing before any send",
            ));
        }
    };
    let value = read_receipt_value(&receipt)?;
    let lead: control::OriginatingLead = serde_json::from_value(value["originatingLead"].clone())
        .map_err(|error| {
        invalid(&format!(
            "recorded originating lead is not usable: {error}; refusing before any send"
        ))
    })?;
    if lead.run_generation != marker {
        return Err(invalid(
            "recorded run generation does not match the caller marker; refusing before any send",
        ));
    }
    require_live(
        lead.dispatcher.pid,
        lead.dispatcher.creation_time,
        &lead.dispatcher.program,
        "stale sender: the recorded dispatcher is not the live dispatching process",
    )?;
    let host: observation::HostIdentity =
        serde_json::from_value(value["observation"]["host"].clone()).map_err(|error| {
            invalid(&format!(
                "the spawned run records no host identity: {error}; refusing before any send"
            ))
        })?;
    require_live(
        host.pid,
        host.created,
        &host.program,
        "stale sender: the recorded host is not live",
    )?;
    if !caller_in_lineage(host.pid) {
        return Err(invalid(
            "caller is not in the spawned run's process lineage; a copied marker, sibling, or unrelated process is not a member of this run; refusing before any send",
        ));
    }
    let slot = value["slot"]["index"]
        .as_u64()
        .ok_or_else(|| invalid("the spawned run records no slot; refusing before any send"))?;
    let slot = u32::try_from(slot).map_err(|_| {
        invalid("the spawned run records a slot that is not an index; refusing before any send")
    })?;
    Ok(SpawnedRun {
        receipt,
        lead_thread: lead.thread_id,
        generation: lead.run_generation,
        owner: registered_text(&value["slot"]["owner"]),
        session: registered_text(&value["observation"]["session"]),
        checkout: registered_text(&value["slot"]["source"]),
        worktree: registered_text(&value["slot"]["path"]),
        slot,
        assignment: registered_text(&value["assignmentId"]),
        bd: registered_text(&value["bd"]),
    })
}

fn registered_text(value: &Value) -> String {
    value
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .unwrap_or(UNAVAILABLE)
        .to_owned()
}

fn matching_receipts(home: &Path, marker: &str) -> io::Result<Vec<PathBuf>> {
    let root = home.join("harness/executor-pool");
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for checkout in fs::read_dir(&root)? {
        let checkout = checkout?;
        if !checkout.file_type()?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(checkout.path())? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("spawn-") && name.ends_with(".json")) {
                continue;
            }
            let Ok(value) = read_receipt_value(&entry.path()) else {
                continue;
            };
            if value["originatingLead"]["runGeneration"].as_str() == Some(marker) {
                found.push(entry.path());
                if found.len() > 8 {
                    return Err(invalid(
                        "this run generation matches more than one spawned run; refusing before any send",
                    ));
                }
            }
        }
    }
    Ok(found)
}

fn read_receipt_value(path: &Path) -> io::Result<Value> {
    let bytes = fs::read(path).map_err(|error| {
        invalid(&format!(
            "spawn receipt {} is unreadable: {error}; refusing before any send",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        invalid(&format!(
            "spawn receipt {} is not JSON: {error}; refusing before any send",
            path.display()
        ))
    })
}

fn require_live(pid: u32, created: u64, program: &Path, reason: &str) -> io::Result<()> {
    if pid == 0 || created == 0 || !program.is_file() {
        return Err(invalid(&format!("{reason}; refusing before any send")));
    }
    let user = process_service::current_user().map_err(|error| {
        invalid(&format!(
            "{reason}: the caller account is unavailable: {error}; refusing before any send"
        ))
    })?;
    match ServiceProcess::inspect(
        ProcessIdentity {
            pid,
            creation_time: created,
        },
        program,
        &user,
    ) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(invalid(&format!("{reason}; refusing before any send"))),
        Err(error) => Err(invalid(&format!(
            "{reason}: {error}; refusing before any send"
        ))),
    }
}

fn caller_in_lineage(host_pid: u32) -> bool {
    ancestor_pids().contains(&host_pid)
}

fn ancestor_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    let mut current = std::process::id();
    for _ in 0..64 {
        if current == 0 || pids.contains(&current) {
            break;
        }
        pids.push(current);
        let Some(parent) = parent_process_id(current) else {
            break;
        };
        current = parent;
    }
    pids
}

fn parent_process_id(pid: u32) -> Option<u32> {
    use std::mem::size_of;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    #[repr(C)]
    struct ProcessBasicInformation {
        exit_status: i32,
        peb_base_address: *mut std::ffi::c_void,
        affinity_mask: usize,
        base_priority: i32,
        unique_process_id: usize,
        inherited_from_unique_process_id: usize,
    }
    type NtQuery = unsafe extern "system" fn(
        windows_sys::Win32::Foundation::HANDLE,
        u32,
        *mut std::ffi::c_void,
        u32,
        *mut u32,
    ) -> i32;
    let ntdll = unsafe { GetModuleHandleW(windows_sys::core::w!("ntdll.dll")) };
    if ntdll.is_null() {
        return None;
    }
    let symbol = unsafe { GetProcAddress(ntdll, c"NtQueryInformationProcess".as_ptr().cast()) }?;
    let query =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, NtQuery>(symbol) };
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut basic = ProcessBasicInformation {
        exit_status: 0,
        peb_base_address: std::ptr::null_mut(),
        affinity_mask: 0,
        base_priority: 0,
        unique_process_id: 0,
        inherited_from_unique_process_id: 0,
    };
    let mut used = 0u32;
    let status = unsafe {
        query(
            handle,
            0,
            (&mut basic as *mut ProcessBasicInformation).cast(),
            size_of::<ProcessBasicInformation>() as u32,
            &mut used,
        )
    };
    unsafe { CloseHandle(handle) };
    if status != 0 {
        return None;
    }
    u32::try_from(basic.inherited_from_unique_process_id).ok()
}

fn verified_lead_endpoint(run: &SpawnedRun) -> io::Result<Endpoint> {
    let path = run
        .receipt
        .parent()
        .ok_or_else(|| fresh_launch(&run.lead_thread, "the receipt has no directory"))?
        .join(format!("lead-endpoint-{}.json", run.slot));
    let endpoint = match Endpoint::read(&path) {
        Ok(endpoint) => endpoint,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(fresh_launch(
                &run.lead_thread,
                "no lead-endpoint record exists beside the spawn receipt",
            ));
        }
        Err(error) => {
            return Err(invalid(&format!(
                "recorded lead endpoint {} is not usable: {error}; nothing was sent and no daemon, listener, or second conversation was started",
                path.display()
            )));
        }
    };
    if endpoint.thread_id.as_deref() != Some(run.lead_thread.as_str()) {
        return Err(invalid(&format!(
            "recorded lead endpoint names a different thread than originating lead {}; refusing before any send",
            run.lead_thread
        )));
    }
    let Some(process) = endpoint.process.as_ref() else {
        return Err(fresh_launch(
            &run.lead_thread,
            "the endpoint record names no process",
        ));
    };
    require_live(
        process.pid,
        process.creation_time,
        &process.program,
        "no verified lead endpoint is recorded: its process is not live",
    )
    .map_err(|_| {
        fresh_launch(
            &run.lead_thread,
            "the recorded endpoint process is not live",
        )
    })?;
    Ok(endpoint)
}

fn fresh_launch(thread: &str, reason: &str) -> io::Error {
    invalid(&format!(
        "no verified lead endpoint is recorded for originating lead thread {thread}: {reason}. Ordinary leads expose no delivery endpoint. This command does not start a daemon, listener, or second conversation. Next action: fresh-launch the lead session so this same thread records a verified app-server endpoint, then dispatch the executor again. Nothing was sent"
    ))
}

fn lead_message_id(generation: &str, kind: &str, nonce: u64, payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(generation.as_bytes());
    hasher.update([0]);
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(nonce.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(payload.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("lead-{hex}")
}

fn recorded_lead_messages(receipt: &Path) -> io::Result<Vec<Value>> {
    let value = read_receipt_value(receipt)?;
    Ok(value["leadMessages"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

fn deliver_to_recorded_lead(
    run: &SpawnedRun,
    endpoint: &Endpoint,
    id: &str,
    kind: &str,
    envelope: &str,
    payload: &str,
) -> io::Result<i32> {
    let mut conversation = Conversation::attach(endpoint, CONTROL_BOUND).map_err(|error| {
        invalid(&format!(
            "the recorded lead endpoint did not accept a connection: {error}; nothing was sent and no second conversation was started"
        ))
    })?;
    let mut thread = conversation.thread_state().map_err(|error| {
        invalid(&format!(
            "the recorded lead thread could not be read: {error}; nothing was sent"
        ))
    })?;
    if thread["id"].as_str() != Some(run.lead_thread.as_str()) {
        return Err(invalid(&format!(
            "the live lead conversation is not originating thread {}; nothing was sent",
            run.lead_thread
        )));
    }
    let mut sent_method = String::new();
    let mut outcome = None;
    for _ in 0..DELIVERY_ROUNDS {
        let active = active_turn(&thread);
        let (method, params) = request_params(&run.lead_thread, id, envelope, &active);
        sent_method = method.to_owned();
        match conversation.request(method, params) {
            Ok(control::Reply::Result(result)) => {
                let evidence = observe_input(
                    &mut conversation,
                    id,
                    envelope,
                    Instant::now() + EVIDENCE_WINDOW,
                );
                outcome = Some(if evidence.is_some() {
                    "delivered"
                } else {
                    let _ = answer_turn(&result);
                    "queued"
                });
                break;
            }
            Ok(control::Reply::Rejected(_)) => match conversation.thread_state() {
                Ok(refreshed) if refreshed["id"].as_str() == Some(run.lead_thread.as_str()) => {
                    thread = refreshed;
                }
                _ => {
                    outcome = Some("error");
                    break;
                }
            },
            Ok(control::Reply::Unanswered) | Err(_) => {
                outcome = Some("indeterminate");
                break;
            }
        }
    }
    let status = outcome.unwrap_or("error");
    record_lead_message(
        &run.receipt,
        id,
        kind,
        &sent_method,
        status,
        &run.lead_thread,
    )?;
    let mut out = io::stdout();
    writeln!(out, "lead message: {status}")?;
    writeln!(out, "id: {id}")?;
    writeln!(out, "kind: {kind}")?;
    writeln!(out, "owner: {}", run.owner)?;
    writeln!(out, "runGeneration: {}", run.generation)?;
    writeln!(out, "session: {}", run.session)?;
    writeln!(out, "checkout: {}", run.checkout)?;
    writeln!(out, "worktree: {}", run.worktree)?;
    writeln!(out, "slot: {}", run.slot)?;
    writeln!(out, "leadThreadId: {}", run.lead_thread)?;
    writeln!(out, "assignment: {}", run.assignment)?;
    writeln!(out, "bd: {}", run.bd)?;
    writeln!(out, "method: {sent_method}")?;
    writeln!(out, "payloadBytes: {}", payload.len())?;
    if kind == KIND_REQUEST {
        writeln!(
            out,
            "reply: codex-harness executor message --reply-to {id} --text TEXT"
        )?;
    } else {
        writeln!(out, "reply: not-requested")?;
    }
    Ok(if status == "error" || status == "indeterminate" {
        1
    } else {
        0
    })
}

fn record_lead_message(
    receipt: &Path,
    id: &str,
    kind: &str,
    method: &str,
    status: &str,
    lead_thread: &str,
) -> io::Result<()> {
    let current = read_receipt_value(receipt)?;
    let generation = current["originatingLead"]["runGeneration"]
        .as_str()
        .unwrap_or("unavailable");
    let session = current["observation"]["session"]
        .as_str()
        .unwrap_or("unavailable");
    let owner = current["slot"]["owner"].as_str().unwrap_or("unavailable");
    let slot = current["slot"]["index"].as_u64().unwrap_or(0);
    let mut messages = recorded_lead_messages(receipt)?;
    messages.push(json!({
        "id": id,
        "kind": kind,
        "method": method,
        "status": status,
        "leadThreadId": lead_thread,
        "runGeneration": generation,
        "session": session,
        "slot": slot,
        "owner": owner,
    }));
    observation::update_receipt_field(receipt, "leadMessages", Value::Array(messages))?;
    if kind == KIND_REQUEST {
        write_reply_index(receipt, id)?;
    }
    Ok(())
}

/// Address fields supplied beside `--reply-to`. A present field that does not
/// name the resolved run is a contradiction and is refused before delivery.
struct ExplicitAddress {
    source: Option<PathBuf>,
    codex_home: Option<PathBuf>,
    slot: Option<u32>,
    owner: Option<String>,
    session: Option<String>,
}

fn valid_reply_id(id: &str) -> bool {
    let Some(hex) = id.strip_prefix("lead-") else {
        return false;
    };
    hex.len() == 24
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn unknown_reply(id: &str) -> io::Error {
    invalid(&format!(
        "unknown message id {id}; refusing before any send"
    ))
}

fn retired_reply(id: &str) -> io::Error {
    invalid(&format!(
        "retired message id {id}; refusing before any send"
    ))
}

fn reply_index_path(home: &Path, id: &str) -> io::Result<PathBuf> {
    if !valid_reply_id(id) {
        return Err(unknown_reply(id));
    }
    Ok(home
        .join("harness")
        .join("executor-pool")
        .join("message-index")
        .join(format!("{id}.json")))
}

/// Locator only: the receipt path is re-read and is not sender authority.
fn write_reply_index(receipt: &Path, id: &str) -> io::Result<()> {
    let home = std::env::var_os("CODEX_HOME").ok_or_else(|| {
        invalid(
            "reply reference could not be recorded: CODEX_HOME is not set; refusing to publish an unresolvable id",
        )
    })?;
    let Some(receipt) = receipt.to_str() else {
        return Err(invalid(
            "reply reference could not be recorded: the receipt path is not Unicode",
        ));
    };
    if Path::new(receipt).is_relative() {
        return Err(invalid(
            "reply reference could not be recorded: the receipt path is not absolute",
        ));
    }
    let path = reply_index_path(Path::new(&home), id)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let body = json!({
        "schema": 1,
        "id": id,
        "receipt": receipt,
    });
    let bytes = serde_json::to_vec_pretty(&body)
        .map_err(|error| invalid(&format!("reply reference could not be recorded: {error}")))?;
    fs::write(&path, bytes)
        .map_err(|error| invalid(&format!("reply reference could not be recorded: {error}")))
}

fn reply_home() -> io::Result<PathBuf> {
    let home = std::env::var_os("CODEX_HOME").ok_or_else(|| {
        invalid(
            "reply-to cannot resolve the message record: CODEX_HOME is not set; refusing before any send",
        )
    })?;
    if home.is_empty() {
        return Err(invalid(
            "reply-to cannot resolve the message record: CODEX_HOME is blank; refusing before any send",
        ));
    }
    Ok(PathBuf::from(home))
}

fn receipt_in_pool(home: &Path, receipt: &Path) -> bool {
    if receipt.is_relative() {
        return false;
    }
    let root = home.join("harness").join("executor-pool");
    match (fs::canonicalize(&root), fs::canonicalize(receipt)) {
        (Ok(root), Ok(receipt)) => receipt.starts_with(root),
        _ => false,
    }
}

fn reply_run_ended(state: &str) -> bool {
    matches!(
        state,
        STATE_COMPLETED
            | STATE_STOPPED
            | STATE_PARTIAL_STOP
            | STATE_FAILED
            | STATE_DEFECT
            | STATE_INTERRUPTED
    )
}

fn verify_calling_lead(lead: &control::OriginatingLead) -> io::Result<()> {
    let thread = match std::env::var("CODEX_THREAD_ID") {
        Ok(value) => value,
        Err(_) => {
            return Err(invalid(
                "another lead cannot use this reply reference: CODEX_THREAD_ID is missing; refusing before any send",
            ));
        }
    };
    if thread.trim() != lead.thread_id {
        return Err(invalid(
            "another lead cannot use this reply reference; refusing before any send",
        ));
    }
    require_live(
        lead.dispatcher.pid,
        lead.dispatcher.creation_time,
        &lead.dispatcher.program,
        "the originating lead is not the live dispatching process",
    )?;
    if !caller_in_lineage(lead.dispatcher.pid) {
        return Err(invalid(
            "another lead cannot use this reply reference; refusing before any send",
        ));
    }
    Ok(())
}

fn addresses_differ(given: &Path, recorded: &Path) -> bool {
    !recorded.as_os_str().is_empty() && !same_directory(given, recorded)
}

fn reject_contradictory(explicit: &ExplicitAddress, request: &Request) -> io::Result<()> {
    let source = explicit
        .source
        .as_deref()
        .is_some_and(|path| addresses_differ(path, &request.source));
    let home = explicit
        .codex_home
        .as_deref()
        .is_some_and(|path| addresses_differ(path, &request.codex_home));
    let slot = explicit.slot.is_some_and(|slot| slot != request.slot);
    let owner = explicit
        .owner
        .as_deref()
        .is_some_and(|owner| owner != request.owner);
    let session = explicit
        .session
        .as_deref()
        .is_some_and(|session| Some(session) != request.session.as_deref());
    if source || home || slot || owner || session {
        return Err(invalid(
            "contradictory explicit address mixed with --reply-to; refusing before any send",
        ));
    }
    Ok(())
}

/// Resolves one lead reply to the executor run that sent it, then returns the
/// addressed request the existing steer/start path already delivers. Nothing
/// here connects to an endpoint.
fn resolve_reply(id: &str, explicit: &ExplicitAddress, text: String) -> io::Result<Request> {
    if !valid_reply_id(id) {
        return Err(unknown_reply(id));
    }
    let home = reply_home()?;
    let index = reply_index_path(&home, id)?;
    if !index.is_file() {
        return Err(unknown_reply(id));
    }
    let pointer = fs::read(&index).map_err(|_| retired_reply(id))?;
    let pointer: Value = serde_json::from_slice(&pointer).map_err(|_| retired_reply(id))?;
    if pointer["schema"].as_u64() != Some(1)
        || pointer["id"].as_str() != Some(id)
        || pointer["retired"].as_bool() == Some(true)
    {
        return Err(retired_reply(id));
    }
    let Some(receipt_path) = pointer["receipt"].as_str() else {
        return Err(retired_reply(id));
    };
    let receipt_path = PathBuf::from(receipt_path);
    if !receipt_path.is_file() || !receipt_in_pool(&home, &receipt_path) {
        return Err(retired_reply(id));
    }
    let value = read_receipt_value(&receipt_path).map_err(|_| retired_reply(id))?;
    let Some(entry) = value["leadMessages"].as_array().and_then(|messages| {
        messages
            .iter()
            .find(|entry| entry["id"].as_str() == Some(id))
    }) else {
        return Err(retired_reply(id));
    };
    if entry["kind"].as_str() != Some(KIND_REQUEST) {
        return Err(retired_reply(id));
    }
    let lead: control::OriginatingLead =
        serde_json::from_value(value["originatingLead"].clone()).map_err(|_| retired_reply(id))?;
    if entry["runGeneration"].as_str() != Some(lead.run_generation.as_str())
        || entry["leadThreadId"].as_str() != Some(lead.thread_id.as_str())
    {
        return Err(invalid(&format!(
            "retired message id {id}: sender generation was reused; refusing before any send"
        )));
    }
    let state = value["observation"]["state"].as_str().unwrap_or("");
    if reply_run_ended(state) {
        return Err(retired_reply(id));
    }
    verify_calling_lead(&lead)?;
    let source = PathBuf::from(
        value["slot"]["source"]
            .as_str()
            .ok_or_else(|| retired_reply(id))?,
    );
    let worktree = PathBuf::from(value["slot"]["path"].as_str().unwrap_or(""));
    let slot = u32::try_from(
        value["slot"]["index"]
            .as_u64()
            .ok_or_else(|| retired_reply(id))?,
    )
    .map_err(|_| retired_reply(id))?;
    let owner = value["slot"]["owner"]
        .as_str()
        .ok_or_else(|| retired_reply(id))?
        .to_owned();
    let session = value["observation"]["session"]
        .as_str()
        .ok_or_else(|| retired_reply(id))?
        .to_owned();
    if entry["session"].as_str() != Some(session.as_str())
        || entry["owner"].as_str() != Some(owner.as_str())
        || entry["slot"].as_u64() != Some(u64::from(slot))
        || worktree.as_os_str().is_empty()
    {
        return Err(invalid(&format!(
            "reused slot {slot} or generation cannot receive reply {id}; refusing before any send"
        )));
    }
    let record = task_worktree::load_slot_record(&home, &source, slot).map_err(|error| {
        invalid(&format!(
            "reused slot {slot} cannot receive reply {id}: {error}; refusing before any send"
        ))
    })?;
    let Some(record) = record else {
        return Err(invalid(&format!(
            "reused slot {slot} cannot receive reply {id}; refusing before any send"
        )));
    };
    if record.index != slot
        || record.owner.as_deref() != Some(owner.as_str())
        || addresses_differ(&record.source, &source)
        || addresses_differ(&record.path, &worktree)
    {
        return Err(invalid(&format!(
            "reused slot {slot} cannot receive reply {id}; refusing before any send"
        )));
    }
    let request = Request {
        source,
        codex_home: home,
        slot,
        owner,
        session: Some(session),
        text,
        reply_to: Some(id.to_owned()),
    };
    reject_contradictory(explicit, &request)?;
    Ok(request)
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
