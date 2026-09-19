//! Budget-honest pacing from scoped account observations.
//!
//! Pacing shapes *new* work only: assignment admission, supported concurrency,
//! reasoning effort and feedback cadence. A healthy executor already running
//! keeps its slot, its model and its instruction; the plan never contains an
//! action against it. Every deviation from the configured limits carries a
//! visible reason and the observation it was derived from, expires with that
//! observation, and can be withdrawn explicitly, so pacing is inspectable and
//! reversible. There is no background scheduler: the lead recomputes the plan
//! at a decision boundary from fresh observations.
use crate::scoped_observations::{AccountObservation, account_view};
use std::collections::BTreeSet;
use std::io;

/// Percentage of a known account window above which new work is paced.
pub const PRESSURE_PERCENT: u8 = 70;
/// Percentage at which new work waits for the window to reset.
pub const CRITICAL_PERCENT: u8 = 90;
/// Spread between tasks that become eligible at the same account reset.
pub const BURST_STAGGER_SECONDS: u64 = 120;
/// Backoff for a deferred task whose account reset time is unknown.
pub const UNKNOWN_RESET_BACKOFF_SECONDS: u64 = 300;
/// Effort ceiling applied while an account window is critical.
pub const REDUCED_EFFORT: &str = "low";
/// Ordered reasoning levels the kit recognizes; unknown values pass through.
pub const EFFORT_LEVELS: [&str; 5] = ["minimal", "low", "medium", "high", "xhigh"];

const DECISION_PREFIX: &str = "pacing-decision v1";
const REVOKE_PREFIX: &str = "pacing-revoke v1";

/// Configured limits the plan starts from. Pacing lowers them; unknown
/// telemetry never raises them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacingLimits {
    pub max_concurrent_executors: u32,
    pub feedback_batch_limit: u32,
}

impl PacingLimits {
    pub fn configured(
        max_concurrent_executors: u32,
        feedback_batch_limit: u32,
    ) -> io::Result<Self> {
        if max_concurrent_executors == 0 || max_concurrent_executors > 32 {
            return Err(invalid(
                "max_concurrent_executors must be a positive integer at most 32",
            ));
        }
        if feedback_batch_limit == 0 || feedback_batch_limit > 32 {
            return Err(invalid(
                "feedback_batch_limit must be a positive integer at most 32",
            ));
        }
        Ok(Self {
            max_concurrent_executors,
            feedback_batch_limit,
        })
    }
}

/// A new assignment waiting for admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAssignment {
    pub id: String,
    pub scope: String,
    pub requested_effort: Option<String>,
}

/// A running, healthy executor. It is never preempted by pacing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveExecutor {
    pub task: String,
    pub scope: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PacingKnob {
    NewAssignments,
    Concurrency,
    Effort,
    FeedbackCadence,
}

impl PacingKnob {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NewAssignments => "new-assignments",
            Self::Concurrency => "concurrency",
            Self::Effort => "effort",
            Self::FeedbackCadence => "feedback-cadence",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "new-assignments" => Some(Self::NewAssignments),
            "concurrency" => Some(Self::Concurrency),
            "effort" => Some(Self::Effort),
            "feedback-cadence" => Some(Self::FeedbackCadence),
            _ => None,
        }
    }
}

/// One inspectable pacing change with the observation it came from and the
/// moment it stops applying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacingDecision {
    pub scope: String,
    pub knob: PacingKnob,
    pub from: String,
    pub to: String,
    pub reason: String,
    pub basis: String,
    pub expires_at: Option<u64>,
}

impl PacingDecision {
    pub fn id(&self) -> String {
        format!("{}:{}", self.scope, self.knob.as_str())
    }

    pub fn to_comment(&self) -> String {
        format!(
            "{DECISION_PREFIX} id={} scope={} knob={} from={} to={} expires_at={} reason={} basis={}",
            self.id(),
            self.scope,
            self.knob.as_str(),
            self.from,
            self.to,
            self.expires_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_owned()),
            self.reason,
            self.basis
        )
    }

    fn parse(comment: &str) -> Option<Self> {
        let rest = comment.strip_prefix(DECISION_PREFIX)?.trim();
        let head = rest.split(" reason=").next()?;
        let tail = rest.split(" reason=").nth(1)?;
        let (reason, basis) = tail.split_once(" basis=").unwrap_or((tail, ""));
        let mut scope = None;
        let mut knob = None;
        let mut from = None;
        let mut to = None;
        let mut expires_at = None;
        for part in head.split_whitespace() {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            match key {
                "scope" => scope = Some(value.to_owned()),
                "knob" => knob = PacingKnob::parse(value),
                "from" => from = Some(value.to_owned()),
                "to" => to = Some(value.to_owned()),
                "expires_at" => {
                    expires_at = match value {
                        "none" => None,
                        _ => Some(value.parse().ok()?),
                    }
                }
                _ => {}
            }
        }
        Some(Self {
            scope: scope?,
            knob: knob?,
            from: from?,
            to: to?,
            reason: reason.to_owned(),
            basis: basis.to_owned(),
            expires_at,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pressure {
    /// A fresh reading below the pressure band.
    Calm,
    /// A fresh reading in the pressure band.
    Elevated,
    /// A fresh reading at the critical band, or an observed refusal.
    Critical,
    /// No usable reading: no pacing change and no invented remainder.
    Unknown,
}

impl Pressure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Calm => "calm",
            Self::Elevated => "elevated",
            Self::Critical => "critical",
            Self::Unknown => "unknown",
        }
    }
}

/// Admission of one new assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    Admit {
        effort: Option<String>,
    },
    Defer {
        eligible_at: u64,
        reset_at: Option<u64>,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentPacing {
    pub id: String,
    pub scope: String,
    pub admission: Admission,
}

/// A deferred assignment and the moment it becomes eligible again, spread so
/// tasks sharing an account do not resume in one synchronized burst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BurstSlot {
    pub assignment: String,
    pub scope: String,
    pub reset_at: Option<u64>,
    pub eligible_at: u64,
    pub stagger_seconds: u64,
}

/// Effective limits for one account scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopePacing {
    pub scope: String,
    pub pressure: Pressure,
    pub configured_concurrency: u32,
    pub concurrency_limit: u32,
    /// `None` keeps whatever the profile or the assignment requests.
    pub effort_ceiling: Option<String>,
    pub configured_feedback_batch_limit: u32,
    pub feedback_batch_limit: u32,
    pub reason: String,
    pub basis: String,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacingPlan {
    pub scopes: Vec<ScopePacing>,
    pub assignments: Vec<AssignmentPacing>,
    pub bursts: Vec<BurstSlot>,
    pub decisions: Vec<PacingDecision>,
    /// Healthy executors the plan leaves untouched.
    pub preserved: Vec<String>,
}

impl PacingPlan {
    pub fn scope(&self, scope: &str) -> Option<&ScopePacing> {
        self.scopes.iter().find(|item| item.scope == scope)
    }
}

/// Computes the pacing plan for the next decision boundary. Nothing here
/// depends on ambient state other than `now`, and no model or provider call is
/// made.
pub fn plan(
    limits: &PacingLimits,
    observations: &[AccountObservation],
    active: &[ActiveExecutor],
    assignments: &[NewAssignment],
    now: u64,
) -> PacingPlan {
    let mut scopes: BTreeSet<String> = BTreeSet::new();
    scopes.extend(assignments.iter().map(|item| item.scope.clone()));
    scopes.extend(active.iter().map(|item| item.scope.clone()));
    scopes.extend(observations.iter().map(|item| item.scope.clone()));

    let mut planned_scopes = Vec::new();
    for scope in &scopes {
        let view = account_view(observations, scope, now);
        let active_here = active.iter().filter(|item| item.scope == *scope).count() as u32;
        let critical = view.refusals > 0
            || view
                .used_percent
                .is_some_and(|percent| percent >= CRITICAL_PERCENT);
        let elevated = !critical
            && view
                .used_percent
                .is_some_and(|percent| percent >= PRESSURE_PERCENT);
        let pressure = if critical {
            Pressure::Critical
        } else if elevated {
            Pressure::Elevated
        } else if view.used_percent.is_some() {
            Pressure::Calm
        } else {
            Pressure::Unknown
        };
        let configured_concurrency = limits.max_concurrent_executors;
        let configured_feedback_batch_limit = limits.feedback_batch_limit;
        let (concurrency_limit, effort_ceiling, feedback_batch_limit) = match pressure {
            Pressure::Calm | Pressure::Unknown => (
                configured_concurrency,
                None,
                configured_feedback_batch_limit,
            ),
            Pressure::Elevated => (
                (configured_concurrency / 2).max(1),
                None,
                (configured_feedback_batch_limit / 2).max(1),
            ),
            Pressure::Critical => (1, Some(REDUCED_EFFORT.to_owned()), 1),
        };
        let mut reason = view.describe();
        reason.push_str(&format!("; pressure={}", pressure.as_str()));
        if active_here > 0 && concurrency_limit < active_here {
            reason.push_str(&format!(
                "; {active_here} healthy executor(s) keep their slots, the limit applies to new assignments"
            ));
        }
        planned_scopes.push(ScopePacing {
            scope: scope.clone(),
            pressure,
            configured_concurrency,
            concurrency_limit,
            effort_ceiling,
            configured_feedback_batch_limit,
            feedback_batch_limit,
            basis: view.describe(),
            reason,
            expires_at: view.expires_at,
        });
    }

    let mut planned_assignments = Vec::new();
    let mut deferred: Vec<(String, Option<u64>, String)> = Vec::new();
    let mut admitted: Vec<String> = Vec::new();
    for assignment in assignments {
        let scope = planned_scopes
            .iter()
            .find(|item| item.scope == assignment.scope)
            .expect("every assignment scope is planned");
        let active_here = active
            .iter()
            .filter(|item| item.scope == assignment.scope)
            .count() as u32;
        let admitted_here = admitted
            .iter()
            .filter(|id| {
                assignments
                    .iter()
                    .any(|item| item.id == **id && item.scope == assignment.scope)
            })
            .count() as u32;
        if scope.pressure == Pressure::Critical {
            let reset_at = account_view(observations, &assignment.scope, now).resets_at;
            deferred.push((
                assignment.id.clone(),
                reset_at,
                format!(
                    "account window critical ({}); new work waits for the reset",
                    scope.basis
                ),
            ));
        } else if admitted_here + active_here >= scope.concurrency_limit {
            deferred.push((
                assignment.id.clone(),
                None,
                format!(
                    "concurrency limit {} reached by healthy executors; new work waits for a slot",
                    scope.concurrency_limit
                ),
            ));
        } else {
            admitted.push(assignment.id.clone());
            planned_assignments.push(AssignmentPacing {
                id: assignment.id.clone(),
                scope: assignment.scope.clone(),
                admission: Admission::Admit {
                    effort: effort_within(
                        scope.effort_ceiling.as_deref(),
                        assignment.requested_effort.as_deref(),
                    ),
                },
            });
        }
    }

    let mut bursts = Vec::new();
    let mut counters: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for (id, reset_at, reason) in &deferred {
        let assignment = assignments
            .iter()
            .find(|item| item.id == *id)
            .expect("deferred assignment exists");
        let index = counters.entry(assignment.scope.clone()).or_insert(0);
        let base = reset_at.unwrap_or(now + UNKNOWN_RESET_BACKOFF_SECONDS);
        let eligible_at = base + *index * BURST_STAGGER_SECONDS;
        *index += 1;
        bursts.push(BurstSlot {
            assignment: id.clone(),
            scope: assignment.scope.clone(),
            reset_at: *reset_at,
            eligible_at,
            stagger_seconds: BURST_STAGGER_SECONDS,
        });
        planned_assignments.push(AssignmentPacing {
            id: id.clone(),
            scope: assignment.scope.clone(),
            admission: Admission::Defer {
                eligible_at,
                reset_at: *reset_at,
                reason: reason.clone(),
            },
        });
    }

    let mut decisions = Vec::new();
    for scope in &planned_scopes {
        let basis = scope.basis.clone();
        let reason = scope.reason.clone();
        let expires_at = scope.expires_at;
        if scope.concurrency_limit != scope.configured_concurrency {
            decisions.push(PacingDecision {
                scope: scope.scope.clone(),
                knob: PacingKnob::Concurrency,
                from: scope.configured_concurrency.to_string(),
                to: scope.concurrency_limit.to_string(),
                reason: reason.clone(),
                basis: basis.clone(),
                expires_at,
            });
        }
        if scope.feedback_batch_limit != scope.configured_feedback_batch_limit {
            decisions.push(PacingDecision {
                scope: scope.scope.clone(),
                knob: PacingKnob::FeedbackCadence,
                from: scope.configured_feedback_batch_limit.to_string(),
                to: scope.feedback_batch_limit.to_string(),
                reason: reason.clone(),
                basis: basis.clone(),
                expires_at,
            });
        }
        if let Some(ceiling) = &scope.effort_ceiling {
            decisions.push(PacingDecision {
                scope: scope.scope.clone(),
                knob: PacingKnob::Effort,
                from: "profile".to_owned(),
                to: ceiling.clone(),
                reason: reason.clone(),
                basis: basis.clone(),
                expires_at,
            });
        }
        let deferred_here = bursts
            .iter()
            .filter(|slot| slot.scope == scope.scope)
            .count();
        if deferred_here > 0 {
            let first = bursts
                .iter()
                .find(|slot| slot.scope == scope.scope)
                .expect("deferred slots exist");
            decisions.push(PacingDecision {
                scope: scope.scope.clone(),
                knob: PacingKnob::NewAssignments,
                from: "admit".to_owned(),
                to: format!("defer:{deferred_here}"),
                reason: format!(
                    "{}; first eligibility at {}",
                    first
                        .reset_at
                        .map(|reset| format!("reset at {reset}"))
                        .unwrap_or_else(|| "no known reset".to_owned()),
                    first.eligible_at
                ),
                basis,
                expires_at,
            });
        }
    }

    let mut order: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (index, assignment) in assignments.iter().enumerate() {
        order.insert(assignment.id.clone(), index);
    }
    planned_assignments.sort_by_key(|item| order.get(&item.id).copied().unwrap_or(usize::MAX));
    let mut preserved: Vec<String> = active.iter().map(|item| item.task.clone()).collect();
    preserved.sort();
    PacingPlan {
        scopes: planned_scopes,
        assignments: planned_assignments,
        bursts,
        decisions,
        preserved,
    }
}

/// Bounds a requested reasoning effort by an optional ceiling. Unrecognized
/// values pass through unchanged: pacing never silently substitutes a model
/// setting it does not understand.
pub fn effort_within(ceiling: Option<&str>, requested: Option<&str>) -> Option<String> {
    fn level(value: &str) -> Option<usize> {
        EFFORT_LEVELS.iter().position(|known| *known == value)
    }
    match (ceiling, requested) {
        (None, requested) => requested.map(str::to_owned),
        (Some(ceiling), None) => Some(ceiling.to_owned()),
        (Some(ceiling), Some(requested)) => match (level(ceiling), level(requested)) {
            (Some(ceiling_level), Some(requested_level)) if requested_level > ceiling_level => {
                Some(ceiling.to_owned())
            }
            _ => Some(requested.to_owned()),
        },
    }
}

/// Recorded decisions and withdrawals read back from a board item.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PacingRecords {
    pub decisions: Vec<PacingDecision>,
    pub revoked: Vec<String>,
}

pub fn parse_pacing_comments(comments: &[String]) -> PacingRecords {
    let mut records = PacingRecords::default();
    for comment in comments {
        if let Some(decision) = PacingDecision::parse(comment) {
            records.decisions.push(decision);
        } else if let Some(id) = comment.strip_prefix(REVOKE_PREFIX).and_then(|rest| {
            rest.split_whitespace()
                .find_map(|part| part.strip_prefix("id="))
        }) {
            records.revoked.push(id.to_owned());
        }
    }
    records
}

pub fn format_revoke_comment(scope: &str, knob: PacingKnob, reason: &str) -> String {
    format!(
        "{REVOKE_PREFIX} id={}:{} reason={}",
        scope,
        knob.as_str(),
        reason.replace(' ', "_")
    )
}

/// Decisions that still apply: not withdrawn and not expired with their basis.
pub fn applicable(records: &PacingRecords, now: u64) -> Vec<&PacingDecision> {
    records
        .decisions
        .iter()
        .filter(|decision| !records.revoked.contains(&decision.id()))
        .filter(|decision| {
            decision
                .expires_at
                .is_none_or(|expires_at| expires_at > now)
        })
        .collect()
}

/// The configured limits a revoked or expired decision returns to.
pub fn configured_scope(limits: &PacingLimits, scope: &str) -> ScopePacing {
    ScopePacing {
        scope: scope.to_owned(),
        pressure: Pressure::Unknown,
        configured_concurrency: limits.max_concurrent_executors,
        concurrency_limit: limits.max_concurrent_executors,
        effort_ceiling: None,
        configured_feedback_batch_limit: limits.feedback_batch_limit,
        feedback_batch_limit: limits.feedback_batch_limit,
        reason: format!("{scope}: configured limits; no active pacing decision"),
        basis: format!("{scope}: configured limits"),
        expires_at: None,
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoped_observations::{
        DEFAULT_MAX_AGE_SECONDS, DashboardSnapshotDraft, dashboard_snapshot, provider_refusal,
    };

    const NOW: u64 = 1_789_853_000;

    fn limits() -> PacingLimits {
        PacingLimits::configured(2, 8).unwrap()
    }

    fn snapshot(scope: &str, used_percent: u8, resets_at: u64) -> AccountObservation {
        dashboard_snapshot(DashboardSnapshotDraft {
            scope: scope.to_owned(),
            used_percent: Some(used_percent),
            resets_at: Some(resets_at),
            window_minutes: Some(300),
            observed_at: NOW - 30,
            max_age_seconds: DEFAULT_MAX_AGE_SECONDS,
        })
        .unwrap()
    }

    fn assignment(id: &str, scope: &str, effort: Option<&str>) -> NewAssignment {
        NewAssignment {
            id: id.to_owned(),
            scope: scope.to_owned(),
            requested_effort: effort.map(str::to_owned),
        }
    }

    #[test]
    fn calm_scope_keeps_configured_limits() {
        let observations = [snapshot("gpt", 20, NOW + 3600)];
        let active = [ActiveExecutor {
            task: "running".into(),
            scope: "gpt".into(),
        }];
        let assignments = [assignment("next", "gpt", Some("xhigh"))];
        let paced = plan(&limits(), &observations, &active, &assignments, NOW);
        let scope = paced.scope("gpt").unwrap();
        assert_eq!(scope.pressure, Pressure::Calm);
        assert_eq!(scope.concurrency_limit, 2);
        assert_eq!(scope.feedback_batch_limit, 8);
        assert_eq!(scope.effort_ceiling, None);
        assert!(paced.decisions.is_empty());
        assert_eq!(
            paced.assignments[0].admission,
            Admission::Admit {
                effort: Some("xhigh".into())
            }
        );
        assert_eq!(paced.preserved, vec!["running".to_owned()]);
    }

    #[test]
    fn pressure_reduces_concurrency_and_cadence_but_not_a_healthy_executor() {
        let observations = [snapshot("gpt", 75, NOW + 3600)];
        let active = [
            ActiveExecutor {
                task: "running-a".into(),
                scope: "gpt".into(),
            },
            ActiveExecutor {
                task: "running-b".into(),
                scope: "gpt".into(),
            },
        ];
        let assignments = [assignment("next", "gpt", Some("xhigh"))];
        let paced = plan(&limits(), &observations, &active, &assignments, NOW);
        let scope = paced.scope("gpt").unwrap();
        assert_eq!(scope.pressure, Pressure::Elevated);
        assert_eq!(scope.concurrency_limit, 1);
        assert_eq!(scope.feedback_batch_limit, 4);
        assert!(scope.reason.contains("75% used"));
        assert!(scope.reason.contains("keep their slots"));
        let knobs: Vec<PacingKnob> = paced.decisions.iter().map(|item| item.knob).collect();
        assert_eq!(
            knobs,
            vec![
                PacingKnob::Concurrency,
                PacingKnob::FeedbackCadence,
                PacingKnob::NewAssignments
            ]
        );
        assert!(
            paced
                .decisions
                .iter()
                .all(|item| item.expires_at == Some(NOW - 30 + DEFAULT_MAX_AGE_SECONDS))
        );
        // Both healthy executors keep their slots and the new assignment waits
        // for one instead of preempting either of them.
        assert_eq!(
            paced.preserved,
            vec!["running-a".to_owned(), "running-b".to_owned()]
        );
        match &paced.assignments[0].admission {
            Admission::Defer { reason, .. } => {
                assert!(reason.contains("concurrency limit 1"));
            }
            other => panic!("expected a queued assignment, got {other:?}"),
        }
    }

    #[test]
    fn critical_window_defers_new_work_and_spreads_the_reset() {
        let reset = NOW + 1800;
        let observations = [snapshot("gpt", 93, reset)];
        let active = [ActiveExecutor {
            task: "healthy".into(),
            scope: "gpt".into(),
        }];
        let assignments = [
            assignment("a", "gpt", Some("xhigh")),
            assignment("b", "gpt", Some("high")),
            assignment("c", "gpt", None),
        ];
        let paced = plan(&limits(), &observations, &active, &assignments, NOW);
        let scope = paced.scope("gpt").unwrap();
        assert_eq!(scope.pressure, Pressure::Critical);
        assert_eq!(scope.concurrency_limit, 1);
        assert_eq!(scope.feedback_batch_limit, 1);
        assert_eq!(scope.effort_ceiling.as_deref(), Some(REDUCED_EFFORT));
        assert_eq!(
            paced
                .bursts
                .iter()
                .map(|slot| (slot.assignment.as_str(), slot.eligible_at))
                .collect::<Vec<_>>(),
            vec![
                ("a", reset),
                ("b", reset + BURST_STAGGER_SECONDS),
                ("c", reset + 2 * BURST_STAGGER_SECONDS)
            ]
        );
        for assignment in &paced.assignments {
            match &assignment.admission {
                Admission::Defer {
                    reset_at, reason, ..
                } => {
                    assert_eq!(*reset_at, Some(reset));
                    assert!(reason.contains("critical"));
                }
                other => panic!("expected a deferral, got {other:?}"),
            }
        }
        assert_eq!(paced.preserved, vec!["healthy".to_owned()]);
        let knobs: Vec<(PacingKnob, String)> = paced
            .decisions
            .iter()
            .map(|item| (item.knob, item.to.clone()))
            .collect();
        assert!(knobs.contains(&(PacingKnob::Concurrency, "1".to_owned())));
        assert!(knobs.contains(&(PacingKnob::FeedbackCadence, "1".to_owned())));
        assert!(knobs.contains(&(PacingKnob::Effort, REDUCED_EFFORT.to_owned())));
        assert!(knobs.contains(&(PacingKnob::NewAssignments, "defer:3".to_owned())));
    }

    #[test]
    fn an_observed_refusal_is_critical_without_an_invented_percentage() {
        let refusal = provider_refusal("xai", 1, NOW - 60, DEFAULT_MAX_AGE_SECONDS).unwrap();
        let assignments = [assignment("next", "xai", None)];
        let paced = plan(&limits(), &[refusal], &[], &assignments, NOW);
        let scope = paced.scope("xai").unwrap();
        assert_eq!(scope.pressure, Pressure::Critical);
        assert_eq!(scope.concurrency_limit, 1);
        assert!(scope.basis.contains("used unknown"));
        assert!(scope.basis.contains("refusals=1"));
        match &paced.assignments[0].admission {
            Admission::Defer {
                eligible_at,
                reset_at,
                ..
            } => {
                assert_eq!(*reset_at, None);
                assert_eq!(*eligible_at, NOW + UNKNOWN_RESET_BACKOFF_SECONDS);
            }
            other => panic!("expected a deferral, got {other:?}"),
        }
    }

    #[test]
    fn unknown_or_stale_telemetry_is_not_zero_and_not_unlimited() {
        let assignments = [assignment("next", "zai", Some("high"))];
        let paced = plan(&limits(), &[], &[], &assignments, NOW);
        let scope = paced.scope("zai").unwrap();
        assert_eq!(scope.pressure, Pressure::Unknown);
        assert_eq!(scope.concurrency_limit, limits().max_concurrent_executors);
        assert_eq!(scope.feedback_batch_limit, limits().feedback_batch_limit);
        assert!(scope.reason.contains("telemetry unknown"));
        assert!(paced.decisions.is_empty());
        assert_eq!(
            paced.assignments[0].admission,
            Admission::Admit {
                effort: Some("high".into())
            }
        );

        let stale = snapshot("zai", 95, NOW + 60);
        let paced = plan(
            &limits(),
            &[stale],
            &[],
            &assignments,
            NOW + DEFAULT_MAX_AGE_SECONDS + 1,
        );
        let scope = paced.scope("zai").unwrap();
        assert_eq!(scope.pressure, Pressure::Unknown);
        assert_eq!(scope.concurrency_limit, limits().max_concurrent_executors);
        assert!(scope.reason.contains("stale_ignored=1"));
        assert!(paced.decisions.is_empty());
    }

    #[test]
    fn concurrency_limit_queues_new_work_without_touching_running_executors() {
        let observations = [snapshot("gpt", 10, NOW + 3600)];
        let active = [ActiveExecutor {
            task: "running".into(),
            scope: "gpt".into(),
        }];
        let assignments = [
            assignment("a", "gpt", None),
            assignment("b", "gpt", None),
            assignment("c", "gpt", None),
        ];
        let paced = plan(&limits(), &observations, &active, &assignments, NOW);
        assert!(matches!(
            paced.assignments[0].admission,
            Admission::Admit { .. }
        ));
        for queued in &paced.assignments[1..] {
            match &queued.admission {
                Admission::Defer {
                    eligible_at,
                    reason,
                    reset_at,
                } => {
                    assert_eq!(*reset_at, None);
                    assert!(reason.contains("concurrency limit 2"));
                    assert!(*eligible_at >= NOW + UNKNOWN_RESET_BACKOFF_SECONDS);
                }
                other => panic!("expected a queue, got {other:?}"),
            }
        }
        assert_eq!(paced.preserved, vec!["running".to_owned()]);
        assert!(
            paced
                .decisions
                .iter()
                .all(|item| item.knob != PacingKnob::Concurrency),
            "a slot queue is not a limit change"
        );
    }

    #[test]
    fn decisions_round_trip_expire_and_can_be_withdrawn() {
        let observations = [snapshot("gpt", 95, NOW + 600)];
        let assignments = [assignment("next", "gpt", None)];
        let paced = plan(&limits(), &observations, &[], &assignments, NOW);
        assert!(!paced.decisions.is_empty());
        let comments: Vec<String> = paced
            .decisions
            .iter()
            .map(|item| item.to_comment())
            .collect();
        let records = parse_pacing_comments(&comments);
        assert_eq!(records.decisions, paced.decisions);
        assert_eq!(applicable(&records, NOW).len(), paced.decisions.len());
        assert!(
            applicable(&records, NOW + DEFAULT_MAX_AGE_SECONDS).is_empty(),
            "an expired basis stops applying"
        );

        let withdrawn = vec![
            comments[0].clone(),
            format_revoke_comment("gpt", paced.decisions[0].knob, "lead restored the limit"),
        ];
        let records = parse_pacing_comments(&withdrawn);
        assert_eq!(records.revoked, vec![paced.decisions[0].id()]);
        assert!(applicable(&records, NOW).is_empty());

        // Reversibility: once the basis expires the scope returns to the
        // configured limits with no hidden state.
        let later = plan(
            &limits(),
            &observations,
            &[],
            &assignments,
            NOW + DEFAULT_MAX_AGE_SECONDS,
        );
        let scope = later.scope("gpt").unwrap();
        assert_eq!(scope.concurrency_limit, limits().max_concurrent_executors);
        assert_eq!(scope.feedback_batch_limit, limits().feedback_batch_limit);
        assert_eq!(scope.pressure, Pressure::Unknown);
        let restored = configured_scope(&limits(), "gpt");
        assert_eq!(
            restored.concurrency_limit,
            limits().max_concurrent_executors
        );
        assert_eq!(restored.effort_ceiling, None);
    }

    #[test]
    fn effort_ceiling_bounds_known_levels_and_passes_unknown_values_through() {
        assert_eq!(effort_within(None, Some("xhigh")), Some("xhigh".into()));
        assert_eq!(
            effort_within(Some("low"), Some("xhigh")),
            Some("low".into())
        );
        assert_eq!(
            effort_within(Some("low"), Some("minimal")),
            Some("minimal".into())
        );
        assert_eq!(effort_within(Some("low"), None), Some("low".into()));
        assert_eq!(effort_within(None, None), None);
        assert_eq!(
            effort_within(Some("low"), Some("provider-custom")),
            Some("provider-custom".into())
        );
    }

    #[test]
    fn limits_reject_degenerate_configuration() {
        assert!(PacingLimits::configured(0, 8).is_err());
        assert!(PacingLimits::configured(33, 8).is_err());
        assert!(PacingLimits::configured(2, 0).is_err());
    }

    #[test]
    fn board_and_lead_skills_document_pacing_and_the_benefit_gate() {
        let board = include_str!("../../../.agents/skills/board-workflow/SKILL.md");
        let lead = include_str!("../../../.agents/skills/team-lead/SKILL.md");
        assert!(board.contains("pacing-observation v1"));
        assert!(board.contains("pacing-decision v1"));
        assert!(board.contains("pacing-revoke v1"));
        assert!(board.contains("benefit-gate v1"));
        assert!(board.contains("accounting=check+coordination+rework"));
        assert!(board.contains("applied to the next boundary") || board.contains("next boundary"));
        assert!(lead.contains("never preempted"));
        assert!(lead.contains("Unknown stays unknown"));
        assert!(lead.contains("synchronized burst"));
        assert!(lead.contains("board-workflow"));
        assert!(lead.contains("unadopted"));
    }
}
