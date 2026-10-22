//! Bounded automatic goal recovery for the ordinary hosted conversation.
//!
//! This owner is deliberately small: the native goal remains authoritative,
//! the recovery module decides eligibility and timing, and the frontend relay
//! supplies the ordering boundary between a user mutation and our dispatch.
//! It never resends a prompt, starts a turn, recreates a thread, changes a
//! budget or writes native state directly.

use crate::executor_cli::{
    control::{Conversation, Reply},
    note_log,
    warning::{FrontendGate, QueueOutcome, WarningQueue},
};
use harness_core::{
    task_failure::{FailureCause, TurnFailure},
    task_recovery::{Decision, Dispatch, Recovery},
};
use serde_json::{Value, json};
use std::{
    fs,
    io,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const RECONCILE_WINDOW: Duration = Duration::from_secs(3);
const JITTER_CEILING_MS: u64 = 400;

/// One session's recovery state. It lives in the host process that already
/// owns the app-server connection and the frontend relay.
pub(crate) struct RecoveryHost {
    thread: String,
    gate: FrontendGate,
    warnings: WarningQueue,
    checkpoint: PathBuf,
    log: PathBuf,
    recovery: Recovery,
    /// Gate revision observed when the current attempt was scheduled.
    scheduled_revision: u64,
    /// A request that may have reached Codex without an acknowledgement.
    uncertain: Option<Dispatch>,
}

impl RecoveryHost {
    pub(crate) fn new(
        thread: String,
        gate: FrontendGate,
        warnings: WarningQueue,
        checkpoint: PathBuf,
        log: PathBuf,
    ) -> Self {
        gate.bind(&thread);
        let host = Self {
            thread,
            gate,
            warnings,
            checkpoint,
            log,
            recovery: Recovery::new(Default::default()),
            scheduled_revision: 0,
            uncertain: None,
        };
        // A checkpoint from an earlier owner is evidence, not authority. It is
        // never replayed: a fresh host starts idle and reconciles from native
        // state only when a new failure arrives.
        host.save("idle", None);
        host
    }

    /// Feed one native control notification. Returns an I/O error only when a
    /// required native goal read failed; the caller reports it as an
    /// observation failure and cancels pending recovery.
    pub(crate) fn on_event(
        &mut self,
        conversation: &mut Conversation,
        raw: &Value,
    ) -> io::Result<()> {
        let Some(method) = raw["method"].as_str() else {
            return Ok(());
        };
        if raw["params"]["threadId"].as_str() != Some(self.thread.as_str()) {
            return Ok(());
        }
        // The relay bumps this revision while it forwards a user mutation.
        // Checking it on every observation also covers an interruption during
        // an in-flight attempt, not only the wait before dispatch.
        if self.uncertain.is_none()
            && (self.recovery.is_waiting() || self.recovery.is_in_flight())
            && self.gate.revision() != self.scheduled_revision
        {
            self.cancel("a user action");
            return Ok(());
        }
        match method {
            "turn/completed" => {
                let failed = raw["params"]["turn"]["status"] == "failed";
                if !failed {
                    let decision = self.recovery.on_completed_turn(&self.thread);
                    self.apply(decision);
                    return Ok(());
                }
                let Some(failure) = TurnFailure::from_event(raw) else {
                    return Ok(());
                };
                if failure.cause != FailureCause::Overloaded {
                    self.cancel("an unrelated turn error");
                    return Ok(());
                }
                let goal = conversation.thread_goal(&self.thread)?;
                let decision = self.recovery.on_failed_turn(
                    &self.thread,
                    &failure.turn_id,
                    failure.cause,
                    &goal,
                    Instant::now(),
                    self.jitter(),
                );
                self.apply(decision);
            }
            "thread/goal/updated" => {
                let decision = self
                    .recovery
                    .on_goal(&self.thread, &raw["params"]["goal"]);
                self.apply(decision);
            }
            "thread/goal/cleared" => {
                let decision = self.recovery.on_goal(&self.thread, &Value::Null);
                self.apply(decision);
            }
            _ => {}
        }
        Ok(())
    }

    /// Dispatch one due attempt while holding the relay's ordering gate.
    pub(crate) fn tick(&mut self, conversation: &mut Conversation) {
        if let Some(dispatch) = self.uncertain.take() {
            self.reconcile(conversation, dispatch);
            return;
        }
        let Some(dispatch) = self.recovery.due(Instant::now()) else {
            return;
        };
        // This guard is the ordering boundary: the relay holds it while it
        // forwards every frontend request. A user pause admitted before this
        // point has already bumped the revision; a later one keeps native
        // semantics and cancels further attempts.
        let gate = self.gate.lock();
        if gate.revision() != self.scheduled_revision {
            drop(gate);
            self.cancel("a user action");
            return;
        }
        let goal = match conversation.thread_goal(&self.thread) {
            Ok(goal) => goal,
            Err(error) => {
                drop(gate);
                self.observation_failed(&error.to_string());
                return;
            }
        };
        if !eligible(&goal, &dispatch) {
            drop(gate);
            let decision = self.recovery.on_goal(&self.thread, &goal);
            self.apply(decision);
            return;
        }
        let reply = conversation.request(
            "thread/goal/set",
            json!({
                "threadId": self.thread,
                "status": "active",
                "origin": "automatic",
            }),
        );
        drop(gate);
        match reply {
            Ok(Reply::Result(result)) => {
                let goal = &result["goal"];
                if eligible_identity(goal, &dispatch) && goal["status"] == "active" {
                    self.notify(format!(
                        "Automatic recovery attempt {}/{} was accepted; waiting for the native turn.",
                        dispatch.attempt,
                        harness_core::task_recovery::MAX_ATTEMPTS
                    ));
                    self.save("inFlight", Some(&dispatch));
                } else if goal["status"] == "blocked" {
                    let decision = self.recovery.blocked(
                        &dispatch,
                        Instant::now(),
                        self.jitter(),
                    );
                    self.apply(decision);
                } else {
                    let decision = self.recovery.on_goal(&self.thread, goal);
                    self.apply(decision);
                }
            }
            Ok(Reply::Rejected(_)) => {
                let decision =
                    self.recovery
                        .rejected(&dispatch, Instant::now(), self.jitter());
                self.apply(decision);
            }
            // The request may have reached Codex. Never send it again to find
            // out; reconcile the goal before any later action.
            Ok(Reply::Unanswered) | Err(_) => {
                self.uncertain = Some(dispatch);
                self.notify(
                    "Automatic recovery could not confirm its resume acknowledgement; checking the native goal before any further action."
                        .to_owned(),
                );
                self.save("uncertain", self.uncertain.as_ref());
            }
        }
    }

    /// The frontend or owner is leaving. Cancel pending work before the
    /// process tree closes; this never reopens the session.
    pub(crate) fn shutdown(&mut self) {
        self.cancel("the session closed");
        self.uncertain = None;
        self.save("closed", None);
    }

    /// A required observation failed. Preserve the goal and report a manual
    /// action instead of pretending the resume succeeded or retrying blindly.
    pub(crate) fn observation_failed(&mut self, error: &str) {
        self.cancel("an observation failure");
        self.uncertain = None;
        self.save("observationFailed", None);
        note_log(
            &self.log,
            &format!("ordinary recovery: observation failed: {error}"),
        );
        self.notify(format!(
            "Automatic recovery stopped because the native goal could not be observed ({error}). The goal remains as native reports it; use /goal resume to continue manually."
        ));
    }

    fn reconcile(&mut self, conversation: &mut Conversation, dispatch: Dispatch) {
        let until = Instant::now() + RECONCILE_WINDOW;
        loop {
            match conversation.thread_goal(&self.thread) {
                Ok(goal) => {
                    if eligible_identity(&goal, &dispatch) && goal["status"] == "active" {
                        self.notify(
                            "The unconfirmed recovery resume reached the native goal; no duplicate was sent."
                                .to_owned(),
                        );
                        self.save("inFlight", Some(&dispatch));
                        return;
                    }
                    // A still-blocked goal means the unconfirmed request did
                    // not change state. Stop rather than resend an ambiguous
                    // attempt; the user keeps an explicit manual action.
                    let decision = self.recovery.cancel("an unconfirmed resume");
                    self.apply(decision);
                    self.notify(
                        "Automatic recovery stopped after an unconfirmed acknowledgement; the goal is still blocked. Use /goal resume to continue."
                            .to_owned(),
                    );
                    self.save("unconfirmed", Some(&dispatch));
                    return;
                }
                Err(error) if Instant::now() < until => {
                    note_log(
                        &self.log,
                        &format!("ordinary recovery: reconciliation read failed: {error}"),
                    );
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => {
                    self.observation_failed(&error.to_string());
                    return;
                }
            }
        }
    }

    fn cancel(&mut self, reason: &'static str) {
        if self.uncertain.is_some()
            || self.recovery.is_waiting()
            || self.recovery.is_in_flight()
        {
            let decision = self.recovery.cancel(reason);
            self.apply(decision);
        }
    }

    fn apply(&mut self, decision: Decision) {
        match decision {
            Decision::Ignore => {}
            Decision::Scheduled(scheduled) => {
                self.scheduled_revision = self.gate.revision();
                self.notify(format!(
                    "Codex is temporarily overloaded. Automatic recovery attempt {}/{} is scheduled in {:.1}s; pause or type to cancel.",
                    scheduled.attempt,
                    harness_core::task_recovery::MAX_ATTEMPTS,
                    scheduled.delay.as_secs_f64()
                ));
                self.save("waiting", None);
            }
            Decision::Exhausted { attempts } => {
                self.notify(format!(
                    "Automatic recovery stopped after {attempts} attempts. The goal is still blocked; run /goal resume to continue."
                ));
                self.save("exhausted", None);
            }
            Decision::Cancelled { reason } => {
                self.notify(format!(
                    "Automatic recovery cancelled ({reason}); the goal keeps its native state."
                ));
                self.save("cancelled", None);
            }
        }
    }

    fn notify(&self, message: String) {
        if !matches!(self.warnings.queue(&message), QueueOutcome::Queued) {
            note_log(
                &self.log,
                &format!("ordinary recovery notice not shown: {message}"),
            );
        }
    }

    fn save(&self, state: &str, dispatch: Option<&Dispatch>) {
        let value = json!({
            "schema": 1,
            "threadId": self.thread,
            "status": state,
            "attempt": self.recovery.attempts_used(),
            "dispatch": dispatch.map(|dispatch| json!({
                "threadId": dispatch.goal.thread_id,
                "createdAt": dispatch.goal.created_at,
                "attempt": dispatch.attempt,
            })),
            "savedAtUnixMs": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_millis())
                .unwrap_or_default(),
        });
        let Ok(bytes) = serde_json::to_vec_pretty(&value) else {
            return;
        };
        let temporary = self.checkpoint.with_extension("json.tmp");
        if fs::write(&temporary, bytes).is_ok() {
            let _ = fs::rename(&temporary, &self.checkpoint);
        }
    }

    fn jitter(&self) -> Duration {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.subsec_nanos() as u64)
            .unwrap_or_default();
        let seed = self
            .thread
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
            });
        Duration::from_millis((now ^ seed) % JITTER_CEILING_MS)
    }
}

/// The native goal is still the same blocked goal the episode bound.
fn eligible(goal: &Value, dispatch: &Dispatch) -> bool {
    eligible_identity(goal, dispatch) && goal["status"] == "blocked"
}

fn eligible_identity(goal: &Value, dispatch: &Dispatch) -> bool {
    goal["threadId"].as_str() == Some(dispatch.goal.thread_id.as_str())
        && goal["createdAt"].as_i64() == Some(dispatch.goal.created_at)
}
