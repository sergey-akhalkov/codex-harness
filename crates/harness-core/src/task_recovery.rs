//! Bounded, deterministic recovery decisions for a blocked native goal.
//!
//! This module performs no I/O and never mutates goal JSON. The caller feeds
//! structured failures and goal reads, then forwards a due dispatch through
//! its existing single control owner.

use crate::task_failure::FailureCause;
use serde_json::Value;
use std::time::{Duration, Instant};

pub const MAX_ATTEMPTS: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// A zero budget is accepted and immediately reports exhaustion.
    pub max_attempts: u8,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            max_attempts: MAX_ATTEMPTS,
            base_delay: Duration::from_secs(4),
            max_delay: Duration::from_secs(60),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Goal {
    pub thread_id: String,
    pub created_at: i64,
}

impl Goal {
    /// Parses a native goal object without interpreting its status.
    pub fn parse(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let thread_id = object.get("threadId")?.as_str()?;
        if thread_id.is_empty() {
            return None;
        }
        let created_at = object.get("createdAt")?.as_i64()?;
        Some(Self {
            thread_id: thread_id.to_owned(),
            created_at,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserAction {
    Pause,
    Resume,
    Replace,
    Clear,
    Interrupt,
    NewInput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scheduled {
    pub attempt: u8,
    pub due: Instant,
    pub delay: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dispatch {
    pub goal: Goal,
    pub attempt: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// This observation establishes no change (duplicate, older turn, unrelated thread, etc.).
    Ignore,
    /// A recovery attempt is now scheduled for `Scheduled.due`.
    Scheduled(Scheduled),
    /// The episode used its last attempt; no further automatic resume is allowed.
    Exhausted { attempts: u8 },
    /// The episode is no longer authorized; `reason` is a short static label.
    Cancelled { reason: &'static str },
}

enum Phase {
    Waiting(Scheduled),
    InFlight(u8),
    Uncertain,
    Exhausted,
}

struct Episode {
    goal: Goal,
    attempts: u8,
    phase: Phase,
}

pub struct Recovery {
    policy: Policy,
    episode: Option<Episode>,
    // Native turn IDs are UUIDv7, so canonical text order is creation order.
    last_turn_id: Option<String>,
}

impl Recovery {
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            episode: None,
            last_turn_id: None,
        }
    }

    /// Feed one terminal failed turn. `goal` is the native `thread/goal/get`
    /// result (null or a goal object).
    pub fn on_failed_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        cause: FailureCause,
        goal: &Value,
        now: Instant,
        jitter: Duration,
    ) -> Decision {
        let Some(parsed) = Goal::parse(goal) else {
            return Decision::Ignore;
        };
        if cause != FailureCause::Overloaded
            || parsed.thread_id != thread_id
            || goal.get("status").and_then(Value::as_str) != Some("blocked")
        {
            return Decision::Ignore;
        }
        if self
            .last_turn_id
            .as_deref()
            .is_some_and(|last| turn_id <= last)
        {
            return Decision::Ignore;
        }
        if self
            .episode
            .as_ref()
            .is_some_and(|episode| episode.goal.thread_id != thread_id)
        {
            return Decision::Ignore;
        }

        let same_episode = self
            .episode
            .as_ref()
            .is_some_and(|episode| episode.goal == parsed);
        if !same_episode {
            self.episode = None;
        }
        self.last_turn_id = Some(turn_id.to_owned());

        if same_episode {
            let episode = self.episode.as_ref().expect("checked above");
            match &episode.phase {
                Phase::Waiting(_) => return Decision::Ignore,
                Phase::InFlight(_) | Phase::Uncertain => {
                    return self.schedule_next(now, jitter);
                }
                Phase::Exhausted => {
                    return Decision::Exhausted {
                        attempts: episode.attempts,
                    };
                }
            }
        }

        self.episode = Some(Episode {
            goal: parsed,
            attempts: 0,
            phase: Phase::Exhausted,
        });
        self.schedule_next(now, jitter)
    }

    /// A user/frontend mutation observed by the owning relay.
    pub fn on_user_action(&mut self, thread_id: &str, action: UserAction) -> Decision {
        if !self
            .episode
            .as_ref()
            .is_some_and(|episode| episode.goal.thread_id == thread_id)
        {
            return Decision::Ignore;
        }
        let reason = match action {
            UserAction::Pause => "user pause",
            UserAction::Resume => "user resume",
            UserAction::Replace => "goal replaced",
            UserAction::Clear => "goal cleared",
            UserAction::Interrupt => "user interrupt",
            UserAction::NewInput => "new input",
        };
        self.cancel(reason)
    }

    /// A native goal update/clear notification, or a goal read used for reconciliation.
    pub fn on_goal(&mut self, thread_id: &str, goal: &Value) -> Decision {
        let Some(episode) = self.episode.as_ref() else {
            return Decision::Ignore;
        };
        if episode.goal.thread_id != thread_id {
            return Decision::Ignore;
        }
        let reason = if goal.is_null() {
            "goal cleared"
        } else {
            match Goal::parse(goal) {
                None => "goal invalid",
                Some(parsed) if parsed != episode.goal => "goal changed",
                Some(_) => match goal.get("status").and_then(Value::as_str) {
                    Some("blocked") => return Decision::Ignore,
                    Some("active") => "goal active",
                    Some("paused") => "goal paused",
                    Some("complete") => "goal complete",
                    Some("usageLimited") => "usage limit",
                    Some("budgetLimited") => "budget limit",
                    _ => "goal invalid",
                },
            }
        };
        self.cancel(reason)
    }

    /// A turn for the bound thread completed successfully. Resets the episode.
    pub fn on_completed_turn(&mut self, thread_id: &str) -> Decision {
        if self
            .episode
            .as_ref()
            .is_some_and(|episode| episode.goal.thread_id == thread_id)
        {
            self.episode = None;
        }
        Decision::Ignore
    }

    /// Returns the dispatch that is due, exactly once.
    pub fn due(&mut self, now: Instant) -> Option<Dispatch> {
        let episode = self.episode.as_mut()?;
        let Phase::Waiting(scheduled) = &episode.phase else {
            return None;
        };
        if now < scheduled.due {
            return None;
        }
        let scheduled = scheduled.clone();
        episode.attempts = scheduled.attempt;
        episode.phase = Phase::InFlight(scheduled.attempt);
        Some(Dispatch {
            goal: episode.goal.clone(),
            attempt: scheduled.attempt,
        })
    }

    /// The dispatch's control request was answered with a native error.
    pub fn rejected(&mut self, dispatch: &Dispatch, now: Instant, jitter: Duration) -> Decision {
        if !self.authorizes(dispatch) {
            return Decision::Ignore;
        }
        self.schedule_next(now, jitter)
    }

    /// The dispatch was acknowledged and the same goal is still blocked.
    pub fn blocked(&mut self, dispatch: &Dispatch, now: Instant, jitter: Duration) -> Decision {
        if !self.authorizes(dispatch) {
            return Decision::Ignore;
        }
        self.schedule_next(now, jitter)
    }

    /// The dispatch's answer was lost. Reconcile before doing anything else.
    pub fn uncertain(&mut self) -> Decision {
        let Some(episode) = self.episode.as_mut() else {
            return Decision::Ignore;
        };
        episode.phase = Phase::Uncertain;
        Decision::Cancelled {
            reason: "uncertain",
        }
    }

    /// Explicit cancellation from the session owner (frontend/owner exit).
    pub fn cancel(&mut self, reason: &'static str) -> Decision {
        if self.episode.take().is_some() {
            Decision::Cancelled { reason }
        } else {
            Decision::Ignore
        }
    }

    pub fn bound_goal(&self) -> Option<&Goal> {
        self.episode.as_ref().map(|episode| &episode.goal)
    }

    pub fn attempts_used(&self) -> u8 {
        self.episode.as_ref().map_or(0, |episode| episode.attempts)
    }

    pub fn is_waiting(&self) -> bool {
        self.episode
            .as_ref()
            .is_some_and(|episode| matches!(&episode.phase, Phase::Waiting(_)))
    }

    pub fn is_in_flight(&self) -> bool {
        self.episode
            .as_ref()
            .is_some_and(|episode| matches!(&episode.phase, Phase::InFlight(_)))
    }

    fn authorizes(&self, dispatch: &Dispatch) -> bool {
        self.episode.as_ref().is_some_and(|episode| {
            episode.goal == dispatch.goal
                && matches!(&episode.phase, Phase::InFlight(attempt) if *attempt == dispatch.attempt)
        })
    }

    fn schedule_next(&mut self, now: Instant, jitter: Duration) -> Decision {
        let policy = self.policy;
        let Some(episode) = self.episode.as_mut() else {
            return Decision::Ignore;
        };
        if policy.max_attempts == 0 || episode.attempts >= policy.max_attempts {
            episode.phase = Phase::Exhausted;
            return Decision::Exhausted {
                attempts: episode.attempts,
            };
        }

        let attempt = episode.attempts + 1;
        let delay = policy.delay(attempt, jitter);
        let Some(due) = now.checked_add(delay) else {
            episode.phase = Phase::Exhausted;
            return Decision::Exhausted {
                attempts: episode.attempts,
            };
        };
        let scheduled = Scheduled {
            attempt,
            due,
            delay,
        };
        episode.phase = Phase::Waiting(scheduled.clone());
        Decision::Scheduled(scheduled)
    }
}

impl Policy {
    fn delay(self, attempt: u8, jitter: Duration) -> Duration {
        let exponential = self.exponential(attempt);
        exponential.saturating_add(jitter.min(exponential / 4))
    }

    fn exponential(self, attempt: u8) -> Duration {
        let mut delay = self.base_delay.min(self.max_delay);
        for _ in 1..attempt {
            delay = delay.saturating_mul(2).min(self.max_delay);
        }
        delay
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Decision, Dispatch, Duration, FailureCause, Goal, Instant, MAX_ATTEMPTS, Policy, Recovery,
        Scheduled, UserAction,
    };
    use serde_json::{Value, json};
    use std::collections::BTreeSet;

    const THREAD: &str = "thread-1";
    const CREATED: i64 = 100;

    fn goal(created_at: i64, status: &str) -> Value {
        json!({
            "threadId": THREAD,
            "createdAt": created_at,
            "status": status,
            "objective": "finish the owned work",
            "usage": 42,
            "budget": 100,
        })
    }

    fn blocked() -> Value {
        goal(CREATED, "blocked")
    }

    fn failed(
        recovery: &mut Recovery,
        turn: &str,
        goal: &Value,
        now: Instant,
        jitter: Duration,
    ) -> Decision {
        recovery.on_failed_turn(THREAD, turn, FailureCause::Overloaded, goal, now, jitter)
    }

    fn scheduled(decision: Decision) -> Scheduled {
        match decision {
            Decision::Scheduled(scheduled) => scheduled,
            other => panic!("expected a schedule, got {other:?}"),
        }
    }

    fn cancelled(decision: Decision) -> &'static str {
        match decision {
            Decision::Cancelled { reason } => reason,
            other => panic!("expected cancellation, got {other:?}"),
        }
    }

    fn exhausted(decision: Decision) -> u8 {
        match decision {
            Decision::Exhausted { attempts } => attempts,
            other => panic!("expected exhaustion, got {other:?}"),
        }
    }

    fn after(now: Instant, delay: Duration) -> Instant {
        now.checked_add(delay).expect("test deadline")
    }

    #[test]
    fn parses_only_native_goal_identity() {
        assert_eq!(
            Goal::parse(&blocked()),
            Some(Goal {
                thread_id: THREAD.to_owned(),
                created_at: CREATED,
            })
        );
        assert_eq!(
            Goal::parse(&json!({"threadId": THREAD, "createdAt": CREATED})),
            Some(Goal {
                thread_id: THREAD.to_owned(),
                created_at: CREATED,
            })
        );
        for malformed in [
            json!(null),
            json!({}),
            json!({"threadId": "", "createdAt": CREATED}),
            json!({"threadId": THREAD}),
            json!({"threadId": THREAD, "createdAt": 1.5}),
            json!({"threadId": THREAD, "createdAt": "100"}),
        ] {
            assert_eq!(Goal::parse(&malformed), None, "{malformed}");
        }
    }

    #[test]
    fn first_overload_schedules_base_plus_jitter_before_reserving() {
        let policy = Policy::default();
        assert_eq!(policy.max_attempts, MAX_ATTEMPTS);
        assert_eq!(policy.base_delay, Duration::from_secs(4));
        assert_eq!(policy.max_delay, Duration::from_secs(60));

        let mut recovery = Recovery::new(policy);
        let now = Instant::now();
        let jitter = Duration::from_millis(500);
        let schedule = scheduled(failed(&mut recovery, "turn-1", &blocked(), now, jitter));
        let expected_delay = Duration::from_millis(4_500);
        assert_eq!(schedule.attempt, 1);
        assert_eq!(schedule.delay, expected_delay);
        assert_eq!(schedule.due, after(now, expected_delay));
        assert_eq!(recovery.attempts_used(), 0);
        assert!(recovery.is_waiting());
        assert!(!recovery.is_in_flight());
        assert_eq!(
            recovery.bound_goal(),
            Some(&Goal {
                thread_id: THREAD.to_owned(),
                created_at: CREATED,
            })
        );

        let just_before = schedule
            .due
            .checked_sub(Duration::from_nanos(1))
            .expect("test deadline");
        assert_eq!(recovery.due(just_before), None);
        assert_eq!(recovery.attempts_used(), 0);
        assert_eq!(
            recovery.due(schedule.due),
            Some(Dispatch {
                goal: Goal {
                    thread_id: THREAD.to_owned(),
                    created_at: CREATED,
                },
                attempt: 1,
            })
        );
        assert_eq!(recovery.attempts_used(), 1);
        assert!(!recovery.is_waiting());
        assert!(recovery.is_in_flight());
        assert_eq!(recovery.due(schedule.due), None);
    }

    #[test]
    fn duplicate_turns_and_events_do_not_reschedule_or_replenish() {
        let mut recovery = Recovery::new(Policy::default());
        let now = Instant::now();
        let first = scheduled(failed(
            &mut recovery,
            "turn-1",
            &blocked(),
            now,
            Duration::ZERO,
        ));

        assert_eq!(
            failed(
                &mut recovery,
                "turn-1",
                &blocked(),
                after(now, Duration::from_secs(2)),
                Duration::from_secs(30),
            ),
            Decision::Ignore
        );
        let dispatch = recovery.due(first.due).expect("first dispatch");
        assert_eq!(recovery.attempts_used(), 1);

        let second = scheduled(recovery.blocked(&dispatch, first.due, Duration::ZERO));
        assert_eq!(second.attempt, 2);
        assert_eq!(
            failed(
                &mut recovery,
                "turn-1",
                &blocked(),
                after(second.due, Duration::from_secs(1)),
                Duration::ZERO,
            ),
            Decision::Ignore
        );
        assert_eq!(
            recovery.blocked(&dispatch, second.due, Duration::ZERO),
            Decision::Ignore
        );
        assert_eq!(recovery.attempts_used(), 1);
        assert!(recovery.is_waiting());
        assert_eq!(
            recovery.due(second.due).expect("second dispatch").attempt,
            2
        );
    }

    #[test]
    fn five_attempts_exhaust_exactly_without_a_sixth_dispatch() {
        let mut recovery = Recovery::new(Policy::default());
        let mut now = Instant::now();
        let mut schedule = scheduled(failed(
            &mut recovery,
            "turn-1",
            &blocked(),
            now,
            Duration::ZERO,
        ));

        for attempt in 1..=MAX_ATTEMPTS {
            assert_eq!(schedule.attempt, attempt);
            assert_eq!(recovery.attempts_used(), attempt - 1);
            now = schedule.due;
            let dispatch = recovery.due(now).expect("attempt dispatch");
            assert_eq!(dispatch.attempt, attempt);
            assert_eq!(recovery.attempts_used(), attempt);
            assert_eq!(recovery.due(now), None);

            let decision = recovery.blocked(&dispatch, now, Duration::ZERO);
            if attempt == MAX_ATTEMPTS {
                assert_eq!(exhausted(decision), MAX_ATTEMPTS);
            } else {
                schedule = scheduled(decision);
            }
        }

        assert_eq!(recovery.attempts_used(), MAX_ATTEMPTS);
        assert!(!recovery.is_waiting());
        assert!(!recovery.is_in_flight());
        assert_eq!(recovery.due(after(now, Duration::from_secs(3_600))), None);
        assert_eq!(
            exhausted(failed(
                &mut recovery,
                "turn-6",
                &blocked(),
                after(now, Duration::from_secs(1)),
                Duration::ZERO,
            )),
            MAX_ATTEMPTS
        );
        assert_eq!(recovery.due(after(now, Duration::from_secs(3_600))), None);
    }

    #[test]
    fn completed_turn_resets_to_a_full_later_episode() {
        let mut recovery = Recovery::new(Policy::default());
        let now = Instant::now();
        let first = scheduled(failed(
            &mut recovery,
            "turn-1",
            &blocked(),
            now,
            Duration::ZERO,
        ));
        let dispatch = recovery.due(first.due).expect("first dispatch");
        recovery.blocked(&dispatch, first.due, Duration::ZERO);
        assert_eq!(recovery.attempts_used(), 1);

        assert_eq!(
            recovery.on_completed_turn("another-thread"),
            Decision::Ignore
        );
        assert_eq!(recovery.attempts_used(), 1);
        assert!(recovery.is_waiting());

        assert_eq!(recovery.on_completed_turn(THREAD), Decision::Ignore);
        assert_eq!(recovery.bound_goal(), None);
        assert_eq!(recovery.attempts_used(), 0);
        assert!(!recovery.is_waiting());
        assert!(!recovery.is_in_flight());
        assert_eq!(recovery.due(after(now, Duration::from_secs(3_600))), None);

        let later = scheduled(failed(
            &mut recovery,
            "turn-2",
            &blocked(),
            after(now, Duration::from_secs(10)),
            Duration::ZERO,
        ));
        assert_eq!(later.attempt, 1);
        assert_eq!(later.delay, Duration::from_secs(4));
    }

    #[test]
    fn older_turns_are_ignored_and_a_newer_failure_after_dispatch_advances() {
        let mut recovery = Recovery::new(Policy::default());
        let now = Instant::now();
        let first = scheduled(failed(
            &mut recovery,
            "turn-2",
            &blocked(),
            now,
            Duration::ZERO,
        ));
        assert_eq!(
            failed(&mut recovery, "turn-1", &blocked(), now, Duration::ZERO),
            Decision::Ignore
        );
        assert!(recovery.is_waiting());

        let first_dispatch = recovery.due(first.due).expect("first dispatch");
        assert_eq!(
            failed(
                &mut recovery,
                "turn-2",
                &blocked(),
                first.due,
                Duration::ZERO
            ),
            Decision::Ignore
        );
        assert_eq!(
            failed(
                &mut recovery,
                "turn-1",
                &blocked(),
                first.due,
                Duration::ZERO
            ),
            Decision::Ignore
        );
        assert!(recovery.is_in_flight());

        let second = scheduled(failed(
            &mut recovery,
            "turn-3",
            &blocked(),
            first.due,
            Duration::ZERO,
        ));
        assert_eq!(second.attempt, 2);
        assert_eq!(recovery.attempts_used(), 1);
        assert_eq!(
            recovery.blocked(&first_dispatch, second.due, Duration::ZERO),
            Decision::Ignore
        );
        assert!(recovery.is_waiting());
        assert_eq!(
            recovery.due(second.due).expect("second dispatch").attempt,
            2
        );
        assert_eq!(recovery.attempts_used(), 2);
    }

    #[test]
    fn ineligible_causes_and_nonblocked_goals_are_ignored() {
        let causes = [
            FailureCause::Quota,
            FailureCause::Throttle,
            FailureCause::LocalBudget,
            FailureCause::Authentication,
            FailureCause::ContextLimit,
            FailureCause::Policy,
            FailureCause::Transport,
            FailureCause::RetryExhausted,
            FailureCause::Service,
            FailureCause::IncompleteOutput,
            FailureCause::Unknown,
        ];
        let mut recovery = Recovery::new(Policy::default());
        let now = Instant::now();
        for (index, cause) in causes.into_iter().enumerate() {
            assert_eq!(
                recovery.on_failed_turn(
                    THREAD,
                    &format!("cause-{index}"),
                    cause,
                    &blocked(),
                    now,
                    Duration::ZERO,
                ),
                Decision::Ignore,
                "{cause:?}"
            );
        }
        for status in [
            "active",
            "paused",
            "complete",
            "usageLimited",
            "budgetLimited",
            "running",
        ] {
            assert_eq!(
                failed(
                    &mut recovery,
                    &format!("status-{status}"),
                    &goal(CREATED, status),
                    now,
                    Duration::ZERO,
                ),
                Decision::Ignore,
                "{status}"
            );
        }
        for goal in [
            Value::Null,
            json!({}),
            json!({"threadId": "other-thread", "createdAt": CREATED, "status": "blocked"}),
            json!({"threadId": THREAD, "createdAt": CREATED}),
        ] {
            assert_eq!(
                failed(
                    &mut recovery,
                    "malformed-or-foreign",
                    &goal,
                    now,
                    Duration::ZERO,
                ),
                Decision::Ignore,
                "{goal}"
            );
        }
        assert_eq!(recovery.bound_goal(), None);
        assert_eq!(recovery.due(now), None);
    }

    #[test]
    fn goal_identity_change_cancels_and_a_fresh_blocked_goal_rebinds() {
        let mut recovery = Recovery::new(Policy::default());
        let now = Instant::now();
        let first = scheduled(failed(
            &mut recovery,
            "turn-1",
            &goal(100, "blocked"),
            now,
            Duration::ZERO,
        ));
        let dispatch = recovery.due(first.due).expect("first dispatch");
        assert_eq!(recovery.attempts_used(), 1);

        assert_eq!(
            cancelled(recovery.on_goal(THREAD, &goal(101, "blocked"))),
            "goal changed"
        );
        assert_eq!(recovery.bound_goal(), None);
        assert_eq!(recovery.attempts_used(), 0);
        assert!(!recovery.is_in_flight());
        assert_eq!(recovery.due(after(now, Duration::from_secs(3_600))), None);
        assert_eq!(
            recovery.blocked(&dispatch, now, Duration::ZERO),
            Decision::Ignore
        );

        let fresh_goal = goal(101, "blocked");
        let fresh = scheduled(failed(
            &mut recovery,
            "turn-2",
            &fresh_goal,
            now,
            Duration::ZERO,
        ));
        assert_eq!(fresh.attempt, 1);
        assert_eq!(
            recovery.bound_goal(),
            Some(&Goal {
                thread_id: THREAD.to_owned(),
                created_at: 101,
            })
        );
    }

    #[test]
    fn every_user_action_cancels_a_pending_episode() {
        let now = Instant::now();
        for action in [
            UserAction::Pause,
            UserAction::Resume,
            UserAction::Replace,
            UserAction::Clear,
            UserAction::Interrupt,
            UserAction::NewInput,
        ] {
            let mut recovery = Recovery::new(Policy::default());
            let first = scheduled(failed(
                &mut recovery,
                "turn-1",
                &blocked(),
                now,
                Duration::ZERO,
            ));
            let dispatch = recovery.due(first.due).expect("first dispatch");
            assert_eq!(recovery.attempts_used(), 1);

            assert!(
                matches!(
                    recovery.on_user_action("another-thread", action),
                    Decision::Ignore
                ),
                "{action:?}"
            );
            assert_eq!(recovery.attempts_used(), 1);
            let decision = recovery.on_user_action(THREAD, action);
            assert!(
                matches!(decision, Decision::Cancelled { .. }),
                "{action:?}: {decision:?}"
            );
            assert_eq!(recovery.bound_goal(), None);
            assert_eq!(recovery.attempts_used(), 0);
            assert!(!recovery.is_waiting());
            assert!(!recovery.is_in_flight());
            assert_eq!(recovery.due(after(now, Duration::from_secs(3_600))), None);
            assert_eq!(
                recovery.blocked(&dispatch, now, Duration::ZERO),
                Decision::Ignore
            );
        }
    }

    #[test]
    fn goal_status_changes_cancel_with_distinct_reasons_and_active_rearms() {
        let now = Instant::now();
        let cases = [
            ("active", "goal active"),
            ("paused", "goal paused"),
            ("complete", "goal complete"),
            ("usageLimited", "usage limit"),
            ("budgetLimited", "budget limit"),
        ];
        let mut reasons = BTreeSet::new();

        for (status, expected_reason) in cases {
            let mut recovery = Recovery::new(Policy::default());
            let first = scheduled(failed(
                &mut recovery,
                "turn-1",
                &blocked(),
                now,
                Duration::ZERO,
            ));
            recovery.due(first.due).expect("first dispatch");
            assert_eq!(recovery.attempts_used(), 1);

            let reason = cancelled(recovery.on_goal(THREAD, &goal(CREATED, status)));
            assert_eq!(reason, expected_reason);
            assert!(reasons.insert(reason), "duplicate reason {reason}");
            assert_eq!(recovery.attempts_used(), 0);
            assert_eq!(recovery.bound_goal(), None);
            assert_eq!(recovery.due(after(now, Duration::from_secs(3_600))), None);

            let later = scheduled(failed(
                &mut recovery,
                &format!("turn-after-{status}"),
                &goal(CREATED, "blocked"),
                now,
                Duration::ZERO,
            ));
            assert_eq!(later.attempt, 1, "{status}");
        }

        let mut recovery = Recovery::new(Policy::default());
        let waiting = scheduled(failed(
            &mut recovery,
            "turn-same",
            &blocked(),
            now,
            Duration::ZERO,
        ));
        assert_eq!(recovery.on_goal(THREAD, &blocked()), Decision::Ignore);
        assert!(recovery.is_waiting());
        assert_eq!(
            recovery
                .due(waiting.due)
                .expect("preserved dispatch")
                .attempt,
            1
        );
    }

    #[test]
    fn null_malformed_and_foreign_goal_observations_follow_bound_authority() {
        let now = Instant::now();

        let mut recovery = Recovery::new(Policy::default());
        let waiting = scheduled(failed(
            &mut recovery,
            "turn-1",
            &blocked(),
            now,
            Duration::ZERO,
        ));
        assert_eq!(
            cancelled(recovery.on_goal(THREAD, &Value::Null)),
            "goal cleared"
        );
        assert_eq!(recovery.bound_goal(), None);
        assert_eq!(recovery.due(waiting.due), None);

        let mut recovery = Recovery::new(Policy::default());
        scheduled(failed(
            &mut recovery,
            "turn-2",
            &blocked(),
            now,
            Duration::ZERO,
        ));
        assert_eq!(
            cancelled(recovery.on_goal(THREAD, &json!({"threadId": THREAD, "status": "blocked"}),)),
            "goal invalid"
        );
        assert_eq!(recovery.bound_goal(), None);

        let mut recovery = Recovery::new(Policy::default());
        let waiting = scheduled(failed(
            &mut recovery,
            "turn-3",
            &blocked(),
            now,
            Duration::ZERO,
        ));
        assert_eq!(
            recovery.on_goal("another-thread", &Value::Null),
            Decision::Ignore
        );
        assert!(recovery.is_waiting());
        assert_eq!(
            recovery
                .due(waiting.due)
                .expect("preserved dispatch")
                .attempt,
            1
        );
    }

    #[test]
    fn uncertain_preserves_attempts_and_blocks_until_a_new_failure() {
        let mut recovery = Recovery::new(Policy::default());
        let now = Instant::now();
        let first = scheduled(failed(
            &mut recovery,
            "turn-1",
            &blocked(),
            now,
            Duration::ZERO,
        ));
        let first_dispatch = recovery.due(first.due).expect("first dispatch");
        assert_eq!(recovery.attempts_used(), 1);

        assert_eq!(cancelled(recovery.uncertain()), "uncertain");
        assert_eq!(recovery.attempts_used(), 1);
        assert!(!recovery.is_waiting());
        assert!(!recovery.is_in_flight());
        assert_eq!(recovery.due(after(now, Duration::from_secs(3_600))), None);
        assert_eq!(
            recovery.rejected(&first_dispatch, now, Duration::ZERO),
            Decision::Ignore
        );
        assert_eq!(
            recovery.blocked(&first_dispatch, now, Duration::ZERO),
            Decision::Ignore
        );
        assert_eq!(recovery.due(after(now, Duration::from_secs(3_600))), None);

        let second = scheduled(failed(
            &mut recovery,
            "turn-2",
            &blocked(),
            now,
            Duration::ZERO,
        ));
        assert_eq!(second.attempt, 2);
        assert_eq!(recovery.attempts_used(), 1);
        assert_eq!(
            recovery.due(second.due).expect("second dispatch").attempt,
            2
        );
        assert_eq!(recovery.attempts_used(), 2);
    }

    #[test]
    fn jitter_is_bounded_and_waits_strictly_increase() {
        let mut recovery = Recovery::new(Policy::default());
        let jitter = Duration::from_secs(100);
        let now = Instant::now();
        let mut schedule = scheduled(failed(&mut recovery, "turn-1", &blocked(), now, jitter));
        let exponential = [
            Duration::from_secs(4),
            Duration::from_secs(8),
            Duration::from_secs(16),
            Duration::from_secs(32),
            Duration::from_secs(60),
        ];
        let mut delays = Vec::new();

        for (index, exponential_delay) in exponential.into_iter().enumerate() {
            assert_eq!(schedule.attempt, index as u8 + 1);
            assert!(
                schedule.delay <= exponential_delay + schedule.delay / 4,
                "{schedule:?}"
            );
            let accepted_jitter = schedule.delay - exponential_delay;
            assert!(accepted_jitter <= exponential_delay / 4);
            assert!(accepted_jitter <= schedule.delay / 4);
            delays.push(schedule.delay);

            let dispatch = recovery.due(schedule.due).expect("jitter attempt dispatch");
            let decision = recovery.blocked(&dispatch, schedule.due, jitter);
            if index + 1 == MAX_ATTEMPTS as usize {
                assert_eq!(exhausted(decision), MAX_ATTEMPTS);
            } else {
                schedule = scheduled(decision);
            }
        }

        assert!(
            delays.windows(2).all(|pair| pair[0] < pair[1]),
            "{delays:?}"
        );
    }

    #[test]
    fn rejected_attempts_schedule_remaining_then_exhaust() {
        let policy = Policy {
            max_attempts: 2,
            ..Policy::default()
        };
        let mut recovery = Recovery::new(policy);
        let now = Instant::now();
        let first = scheduled(failed(
            &mut recovery,
            "turn-1",
            &blocked(),
            now,
            Duration::ZERO,
        ));
        let first_dispatch = recovery.due(first.due).expect("first dispatch");

        let second = scheduled(recovery.rejected(&first_dispatch, first.due, Duration::ZERO));
        assert_eq!(second.attempt, 2);
        let second_dispatch = recovery.due(second.due).expect("second dispatch");
        assert_eq!(recovery.attempts_used(), 2);
        assert_eq!(
            exhausted(recovery.rejected(&second_dispatch, second.due, Duration::ZERO)),
            2
        );
        assert_eq!(recovery.due(after(now, Duration::from_secs(3_600))), None);
        assert_eq!(
            recovery.rejected(&second_dispatch, second.due, Duration::ZERO),
            Decision::Ignore
        );
    }

    #[test]
    fn one_and_zero_attempt_policies_report_exhaustion() {
        let now = Instant::now();
        let mut one = Recovery::new(Policy {
            max_attempts: 1,
            ..Policy::default()
        });
        let first = scheduled(failed(&mut one, "turn-1", &blocked(), now, Duration::ZERO));
        let dispatch = one.due(first.due).expect("only dispatch");
        assert_eq!(
            one.blocked(&dispatch, first.due, Duration::ZERO),
            Decision::Exhausted { attempts: 1 }
        );
        assert_eq!(one.due(after(now, Duration::from_secs(3_600))), None);

        let mut zero = Recovery::new(Policy {
            max_attempts: 0,
            ..Policy::default()
        });
        assert_eq!(
            failed(&mut zero, "turn-1", &blocked(), now, Duration::ZERO),
            Decision::Exhausted { attempts: 0 }
        );
        assert_eq!(zero.attempts_used(), 0);
        assert!(!zero.is_waiting());
        assert_eq!(zero.due(after(now, Duration::from_secs(3_600))), None);
    }
}
